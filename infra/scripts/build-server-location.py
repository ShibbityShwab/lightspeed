#!/usr/bin/env python3
"""Build the offline server-location indexes used by the route card.

Two artifacts are generated from free, key-less public sources:

* ``client-gui/data/server-cloud.bin`` - cloud-provider IPv4 prefixes mapped to
  an exact provider region (AWS ``ip-ranges.json`` and GCP ``cloud.json``).
* ``client-gui/data/server-hosting.bin`` - IPv4 prefixes announced by a curated
  list of game-hosting/CDN ASNs, from the free RIPE Stat API at build time.

Both are ``start u32 | end u32 | index u16`` little-endian records sorted by
start, with the index pointing into the generated tables in
``client-gui/src/server_tables.rs``. They are committed, so the shipped client
never calls a network service to place a server; this script is the only thing
that needs the network, and only when regenerating.

The hosting index exists because free geoIP databases are wrong for hosting
ranges: DB-IP Lite and City both put a G-Core US-Virginia server in JP, and even
DB-IP's free ASN table misses the live ``178.107.14.0/24``. The ASN route
(RIPE Stat) covers it.

Usage:
  py -3 infra/scripts/build-server-location.py [--check]

``--check`` rebuilds into a temporary directory and compares the emitted
artifacts without touching the committed files.
"""

from __future__ import annotations

import hashlib
import io
import json
import os
import struct
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "client-gui" / "data"
CACHE = ROOT / "target" / "server-location-cache"
RUST_TABLES = ROOT / "client-gui" / "src" / "server_tables.rs"
MANIFEST = DATA / "server-location.sources.json"
USER_AGENT = "lightspeed-build/1.0 (+https://github.com/ShibbityShwab/lightspeed)"

CLOUD_FEEDS = {
    "aws": "https://ip-ranges.amazonaws.com/ip-ranges.json",
    "gcp": "https://www.gstatic.com/ipranges/cloud.json",
}

# Hosting / game-infrastructure ASNs. AWS/GCP/Azure are deliberately absent:
# the cloud feeds above already give those an exact region, and a second,
# coarser classification for them would only compete with that.
HOSTING_ASNS = [
    (199524, "G-Core Labs"),
    (16276, "OVH"),
    (24940, "Hetzner"),
    (14061, "DigitalOcean"),
    (20473, "Vultr"),
    (63949, "Linode"),
    (51167, "Contabo"),
    (12876, "Scaleway"),
    (28753, "Leaseweb"),
    (54825, "Equinix Metal"),
    (31898, "Oracle Cloud"),
    (45102, "Alibaba Cloud"),
    (132203, "Tencent Cloud"),
    (13335, "Cloudflare"),
    (20940, "Akamai"),
    (54113, "Fastly"),
    (32590, "Valve/Steam"),
    (36351, "IBM SoftLayer"),
    (9009, "M247"),
    (49505, "Selectel"),
]

# Approximate city coordinates for the provider regions we can place on the
# map. A region absent here is still emitted (the exact region is the honest
# fact); it simply has no dot, which the route card renders as "region known,
# position not".
REGION_COORDS = {
    "aws": {
        "us-east-1": (39.0, -77.5),
        "us-east-2": (40.0, -83.0),
        "us-west-1": (37.8, -122.3),
        "us-west-2": (45.6, -121.2),
        "af-south-1": (-33.9, 18.4),
        "ap-east-1": (22.3, 114.2),
        "ap-south-1": (19.1, 72.9),
        "ap-south-2": (17.4, 78.5),
        "ap-northeast-1": (35.7, 139.7),
        "ap-northeast-2": (37.6, 127.0),
        "ap-northeast-3": (34.7, 135.5),
        "ap-southeast-1": (1.35, 103.8),
        "ap-southeast-2": (-33.9, 151.2),
        "ap-southeast-3": (-6.2, 106.8),
        "ap-southeast-4": (-37.8, 145.0),
        "ca-central-1": (45.5, -73.6),
        "ca-west-1": (51.0, -114.1),
        "eu-central-1": (50.1, 8.7),
        "eu-central-2": (47.4, 8.5),
        "eu-west-1": (53.3, -6.3),
        "eu-west-2": (51.5, -0.1),
        "eu-west-3": (48.9, 2.4),
        "eu-south-1": (45.5, 9.2),
        "eu-south-2": (48.2, 16.4),
        "eu-north-1": (59.3, 18.1),
        "il-central-1": (32.1, 34.8),
        "me-south-1": (26.0, 50.6),
        "me-central-1": (24.4, 54.5),
        "sa-east-1": (-23.5, -46.6),
    },
    "gcp": {
        "us-central1": (41.6, -93.6),
        "us-east1": (33.0, -80.0),
        "us-east4": (39.0, -77.5),
        "us-east5": (39.96, -83.0),
        "us-south1": (32.8, -96.8),
        "us-west1": (45.6, -121.2),
        "us-west2": (34.0, -118.2),
        "us-west3": (40.8, -111.9),
        "us-west4": (36.1, -115.2),
        "northamerica-northeast1": (45.5, -73.6),
        "northamerica-northeast2": (43.7, -79.4),
        "southamerica-east1": (-23.5, -46.6),
        "southamerica-west1": (-33.4, -70.6),
        "europe-central2": (52.2, 21.0),
        "europe-north1": (60.5, 27.0),
        "europe-southwest1": (40.4, -3.7),
        "europe-west1": (50.4, 4.4),
        "europe-west2": (51.5, -0.1),
        "europe-west3": (50.1, 8.7),
        "europe-west4": (52.9, 4.8),
        "europe-west6": (47.4, 8.5),
        "europe-west8": (45.5, 9.2),
        "europe-west9": (48.8, 2.3),
        "europe-west10": (52.5, 13.4),
        "europe-west12": (45.1, 7.7),
        "asia-east1": (24.8, 121.0),
        "asia-east2": (22.3, 114.2),
        "asia-northeast1": (35.7, 139.7),
        "asia-northeast2": (34.7, 135.5),
        "asia-northeast3": (37.6, 127.0),
        "asia-south1": (19.1, 72.9),
        "asia-south2": (28.6, 77.2),
        "asia-southeast1": (1.35, 103.8),
        "asia-southeast2": (-6.2, 106.8),
        "australia-southeast1": (-33.9, 151.2),
        "australia-southeast2": (-37.8, 145.0),
        "me-central1": (25.3, 51.5),
        "me-central2": (24.4, 54.5),
        "africa-south1": (-26.2, 28.0),
    },
}


def fetch(url: str) -> bytes:
    """Download URL, caching by name under target/ so regeneration is cheap."""
    CACHE.mkdir(parents=True, exist_ok=True)
    key = hashlib.sha256(url.encode()).hexdigest()[:16]
    cached = CACHE / f"{key}.body"
    if cached.exists():
        return cached.read_bytes()
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=120) as response:
        body = response.read()
    cached.write_bytes(body)
    return body


def ipv4(cidr: str) -> tuple[int, int] | None:
    address, _, bits = cidr.partition("/")
    try:
        start = struct.unpack(">I", bytes(int(part) for part in address.split(".")))[0]
        length = int(bits)
    except (ValueError, struct.error):
        return None
    if length < 0 or length > 32:
        return None
    end = start | ((1 << (32 - length)) - 1)
    return start, end


def normalize(records: list[tuple[int, int, int]]) -> list[tuple[int, int, int]]:
    """Sort and clip overlapping prefixes, keeping the more specific one."""
    records.sort(key=lambda r: (r[0], -r[1]))
    out: list[tuple[int, int, int]] = []
    for start, end, idx in records:
        if out and start <= out[-1][1]:
            prev_start, prev_end, prev_idx = out[-1]
            if prev_idx == idx or end <= prev_end:
                continue
            # Different owner overlapping from the right: clip the tail.
            if start > prev_start:
                out[-1] = (prev_start, start - 1, prev_idx)
        out.append((start, end, idx))
    return out


def cloud_records(provider: str, payload: dict) -> list[tuple[int, int, str]]:
    key = "prefixes"
    records = []
    for prefix in payload.get(key, []):
        cidr = prefix.get("ip_prefix") or prefix.get("ipv4Prefix")
        region = prefix.get("region") or prefix.get("scope")
        if not cidr or not region:
            continue
        parsed = ipv4(cidr)
        if parsed is None:
            continue
        records.append((parsed[0], parsed[1], region))
    return records


def announced_prefixes(asn: int) -> list[tuple[int, int]]:
    url = f"https://stat.ripe.net/data/announced-prefixes/data.json?resource=AS{asn}"
    payload = json.loads(fetch(url))
    if payload.get("status") != "ok":
        raise RuntimeError(f"RIPE Stat AS{asn}: {payload.get('status')}")
    records = []
    for entry in payload.get("data", {}).get("prefixes", []):
        if ":" in entry.get("prefix", ""):
            continue  # IPv4 index only
        parsed = ipv4(entry["prefix"])
        if parsed is not None:
            records.append(parsed)
    return records


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def write_bin(path: Path, records: list[tuple[int, int, int]]) -> None:
    blob = bytearray()
    for start, end, idx in records:
        blob += struct.pack("<IIH", start, end, idx)
    path.write_bytes(bytes(blob))


def write_tables(
    path: Path,
    regions: list[tuple[str, str, tuple[float, float] | None]],
    providers: list[tuple[int, str]],
) -> None:
    lines = [
        "// @generated by infra/scripts/build-server-location.py - do not edit.",
        "//",
        "// AWS/GCP ranges come from their public IP-range feeds; hosting prefixes",
        "// are derived from the free RIPE Stat API (RIPE NCC) at build time. The",
        "// shipped client only reads the committed .bin files - no network calls.",
        "",
        "/// One cloud-provider region and, when known, a representative point.",
        "pub struct CloudRegion {",
        '    pub provider: &\'static str,',
        '    pub region: &\'static str,',
        '    pub at: Option<(f32, f32)>,',
        "}",
        "",
        "/// Cloud regions, indexed by the cloud blob.",
        "#[rustfmt::skip]",
        "pub const CLOUD_REGIONS: &[CloudRegion] = &[",
    ]
    for provider, region, at in regions:
        if at is None:
            lines.append(
                f'    CloudRegion {{ provider: "{provider}", region: "{region}", at: None }},'
            )
        else:
            lines.append(
                f'    CloudRegion {{ provider: "{provider}", region: "{region}", '
                f"at: Some(({at[0]:.2f}, {at[1]:.2f})) }},"
            )
    lines += [
        "];",
        "",
        "/// One hosting/CDN network whose location geoIP databases get wrong.",
        "pub struct HostingProvider {",
        "    pub asn: u32,",
        "    pub name: &'static str,",
        "}",
        "",
        "/// Hosting providers, indexed by the hosting blob.",
        "#[rustfmt::skip]",
        "pub const HOSTING_PROVIDERS: &[HostingProvider] = &[",
    ]
    for asn, name in providers:
        lines.append(f'    HostingProvider {{ asn: {asn}, name: "{name}" }},')
    lines += ["];", ""]
    path.write_text("\n".join(lines), encoding="utf-8")


def main() -> int:
    check = "--check" in sys.argv[1:]
    DATA.mkdir(parents=True, exist_ok=True)

    sources = {}
    cloud_raw = {}
    for provider, url in CLOUD_FEEDS.items():
        body = fetch(url)
        sources[url] = {"bytes": len(body), "sha256": sha256(body)}
        cloud_raw[provider] = json.loads(body)

    regions: list[tuple[str, str, tuple[float, float] | None]] = []
    region_index: dict[tuple[str, str], int] = {}
    cloud: list[tuple[int, int, int]] = []
    for provider, payload in cloud_raw.items():
        for start, end, region in cloud_records(provider, payload):
            key = (provider, region)
            if key not in region_index:
                region_index[key] = len(regions)
                regions.append((provider, region, REGION_COORDS[provider].get(region)))
            cloud.append((start, end, region_index[key]))
    cloud = normalize(cloud)

    providers: list[tuple[int, str]] = []
    hosting: list[tuple[int, int, int]] = []
    for asn, name in HOSTING_ASNS:
        url = f"https://stat.ripe.net/data/announced-prefixes/data.json?resource=AS{asn}"
        prefixes = announced_prefixes(asn)
        if not prefixes:
            continue
        idx = len(providers)
        providers.append((asn, name))
        for start, end in prefixes:
            hosting.append((start, end, idx))
        sources[url] = {"asn": asn, "name": name, "prefixes": len(prefixes)}
        time.sleep(0.3)
    hosting = normalize(hosting)

    if check:
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            write_bin(tmp_path / "cloud.bin", cloud)
            write_bin(tmp_path / "hosting.bin", hosting)
            write_tables(tmp_path / "tables.rs", regions, providers)
            problems = []
            for name, tmp_file in (
                ("server-cloud.bin", tmp_path / "cloud.bin"),
                ("server-hosting.bin", tmp_path / "hosting.bin"),
            ):
                committed = DATA / name
                if not committed.exists():
                    problems.append(f"{name}: missing")
                elif committed.read_bytes() != tmp_file.read_bytes():
                    problems.append(f"{name}: differs from the committed artifact")
            if RUST_TABLES.exists() and RUST_TABLES.read_text(encoding="utf-8") != (
                tmp_path / "tables.rs"
            ).read_text(encoding="utf-8"):
                problems.append("server_tables.rs: differs from the committed file")
            if problems:
                for problem in problems:
                    print(f"CHECK FAIL: {problem}")
                return 1
            print(
                f"CHECK OK: {len(cloud)} cloud ranges / {len(regions)} regions, "
                f"{len(hosting)} hosting ranges / {len(providers)} providers"
            )
            return 0

    write_bin(DATA / "server-cloud.bin", cloud)
    write_bin(DATA / "server-hosting.bin", hosting)
    write_tables(RUST_TABLES, regions, providers)
    MANIFEST.write_text(
        json.dumps(
            {
                "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "format": "start-little-endian-u32, end-little-endian-u32, index-little-endian-u16",
                "sources": sources,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    def lookup(records: list[tuple[int, int, int]], address: str) -> int | None:
        target = struct.unpack(">I", bytes(int(part) for part in address.split(".")))[0]
        lo, hi = 0, len(records) - 1
        while lo <= hi:
            mid = (lo + hi) // 2
            start, end, idx = records[mid]
            if target < start:
                hi = mid - 1
            elif target > end:
                lo = mid + 1
            else:
                return idx
        return None

    for label, records, address in (
        ("cloud aws us-east-1", cloud, "3.237.221.191"),
        ("cloud gcp europe-west1", cloud, "34.38.12.1"),
        ("hosting gcore", hosting, "178.107.14.201"),
    ):
        idx = lookup(records, address)
        detail = ""
        if idx is not None:
            if records is cloud:
                detail = f"{regions[idx][0]} {regions[idx][1]}"
            else:
                detail = f"AS{providers[idx][0]} {providers[idx][1]}"
        print(f"fixture {label} ({address}): {detail or 'MISS'}")

    print(
        f"wrote {len(cloud)} cloud ranges ({len(regions)} regions), "
        f"{len(hosting)} hosting ranges ({len(providers)} providers)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
