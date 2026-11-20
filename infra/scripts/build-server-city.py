#!/usr/bin/env python3
"""BUILD-TIME-ONLY scaffold for an offline server-city lookup (not shipped).

WHY THIS EXISTS
  The route card can place a server in a cloud region (exact) or name a hosting
  network whose free-geoIP country is wrong, but it cannot say which city the
  server is in. The 2026-11-20 panel found the city-level answer is obtainable
  offline at build time from free data (an offline MaxMind GeoLite2 City build
  reproduced GeoJS exactly on every probed IP), and that the blocker is a
  licence/terms decision, not a data or cost problem.

OWNER GATE - O1 (do not bypass)
  Shipping data derived from either source requires an explicit owner decision:
  * MaxMind GeoLite2 City: accept the GeoLite2 End User Licence Agreement and
    the CC BY-SA 4.0 terms - attribution, ShareAlike on the derived table, and
    a keep-current refresh cadence (<= 30 downloads/day).
  * Operator geofeed (RFC 8805): the feed states no licence; shipping it needs
    the operator's written permission.
  Nothing from this script is committed or wired into the client. Outputs land
  under target/server-location-city/ (gitignored); no build.rs includes the
  generated Rust and no client code reads the blob. See
  docs/design/server-location-online-lookup-review.md.

INPUTS (build time only)
  1. RFC 8805 geofeed CSV files with five columns:
       prefix,country,region,city,postal
     Full-line `#` comments and blank lines are ignored. Place them under
     infra/geo/geofeeds/ (auto-discovered) or pass --geofeed.
  2. A pre-extracted MaxMind GeoLite2-City CSV with the same five columns,
     passed with --geolite-csv. This scaffold deliberately does NOT decode
     .mmdb: that needs a reader dependency or a shell-out, and no licensed
     data may ship yet. --mmdb is refused with conversion guidance; the owner
     flattens GeoLite2-City's Blocks + Locations files into the five-column
     CSV with MaxMind's own tooling before running this.

PRECEDENCE
  geofeed beats geolite: a geofeed row overrides any GeoLite row covering the
  same address, even when the GeoLite prefix is more specific.

OUTPUTS (target/server-location-city/, never committed)
  * server-city.bin - range records, little-endian:
        start u32 | end u32 | city-index u16   (10 bytes per record)
    sorted by start and non-overlapping; city-index points into SERVER_CITIES.
  * server_city.rs - generated Rust city table with the licence/gate header.
  * manifest.json - input hashes, row counts, output hashes and the gate text.

USAGE
  py -3 infra/scripts/build-server-city.py --selftest
  py -3 infra/scripts/build-server-city.py --geolite-csv path/to/geolite.csv
  py -3 infra/scripts/build-server-city.py --geofeed path/to/operator.csv
  py -3 infra/scripts/build-server-city.py            # auto-discover
                                                      # infra/geo/geofeeds/*.csv

  --selftest writes fixtures under target/ and asserts parsing, comments,
  malformed rows, prefix math, merge precedence and the emitted files. It
  needs no licensed data and no network, and is safe to run on every host.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import shutil
import struct
import sys
import tempfile
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GEOFEED_DIR = ROOT / "infra" / "geo" / "geofeeds"
DEFAULT_OUT = ROOT / "target" / "server-location-city"

# 10 bytes per range record: start u32 LE, end u32 LE, city-index u16 LE.
RECORD_BYTES = 10
MAX_CITY_INDEX = 0xFFFF
CITY_TABLE_CAP = MAX_CITY_INDEX + 1

GATE = (
    "OWNER GATE (O1): shipping data derived from these sources requires an "
    "explicit owner decision - either accept the MaxMind GeoLite2 End User "
    "Licence Agreement plus CC BY-SA 4.0 (attribution and ShareAlike on the "
    "derived table, keep-current refresh cadence) or obtain the operator's "
    "written permission to ship the geofeed. This build-time scaffold writes "
    "under target/ only: it ships nothing, is not committed, and is not wired "
    "into the client. See docs/design/server-location-online-lookup-review.md."
)

# A row is parsed into (start, end, (country, region, city, postal-or-None)).
# A city table entry carries the source so a future consumer can pick the label
# the licence requires ("operator geofeed" vs "GeoLite2 city estimate").


class BuildError(RuntimeError):
    """A hard input/format error: the build must not emit a partial artifact."""


@dataclass(frozen=True)
class City:
    """One placeable city, indexed by the blob's `city-index` field."""

    country: str
    region: str
    city: str
    postal: str | None
    source: str


@dataclass
class InputReport:
    """What one input file contributed; written into manifest.json."""

    kind: str
    file: str
    bytes: int
    sha256: str
    rows: int = 0
    kept: int = 0
    malformed: int = 0
    no_city: int = 0
    header: bool = False

    def as_dict(self) -> dict:
        return {
            "kind": self.kind,
            "file": self.file,
            "bytes": self.bytes,
            "sha256": self.sha256,
            "rows": self.rows,
            "kept": self.kept,
            "malformed": self.malformed,
            "no_city": self.no_city,
            "header": self.header,
        }


@dataclass
class BuildResult:
    """Everything one build produced, in-memory, for callers and the selftest."""

    records: list = field(default_factory=list)
    cities: list = field(default_factory=list)
    inputs: list = field(default_factory=list)
    out_dir: Path | None = None
    blob: bytes = b""
    rust: str = ""
    manifest: dict = field(default_factory=dict)


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def ipv4_prefix(text: str) -> tuple[int, int] | None:
    """Parse an IPv4 CIDR into its inclusive (start, end) integer range.

    Strict on purpose: four dotted decimal octets, a `/`, and a length in
    0..=32. Host bits are masked off so ``1.2.3.4/24`` is the ``1.2.3.0/24``
    network. Returns ``None`` for anything malformed rather than guessing.
    """
    address, separator, bits_text = text.strip().partition("/")
    if not separator:
        return None
    octets = address.split(".")
    if len(octets) != 4:
        return None
    values = []
    for octet in octets:
        if not octet or any(ch not in "0123456789" for ch in octet) or len(octet) > 3:
            return None
        value = int(octet)
        if value > 255:
            return None
        values.append(value)
    if not bits_text or any(ch not in "0123456789" for ch in bits_text):
        return None
    bits = int(bits_text)
    if bits > 32:
        return None
    start = (values[0] << 24) | (values[1] << 16) | (values[2] << 8) | values[3]
    mask = (1 << (32 - bits)) - 1
    start &= ~mask & 0xFFFFFFFF
    return start, start | mask


def ipv4_int(address: str) -> int:
    """The integer value of one bare IPv4 address (used by the selftest)."""
    parsed = ipv4_prefix(address.strip() + "/32")
    if parsed is None:
        raise BuildError(f"not an IPv4 address: {address!r}")
    return parsed[0]


def iter_csv_rows(path: Path):
    """Yield CSV rows, skipping RFC 8805 full-line `#` comments and blanks."""
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        data_lines = (
            line
            for line in handle
            if line.strip() and not line.lstrip().startswith("#")
        )
        try:
            for row in csv.reader(data_lines):
                yield row
        except csv.Error as error:
            raise BuildError(f"{path}: malformed CSV: {error}") from error


def is_header_row(first_cell: str) -> bool:
    return ipv4_prefix(first_cell) is None and first_cell.strip().lower() in {
        "cidr",
        "ip",
        "ip_prefix",
        "ipv4",
        "ipv4_prefix",
        "network",
        "prefix",
    }


def parse_city_row(row: list[str]) -> tuple[str | None, tuple | None]:
    """Parse one five-column row.

    Returns ``(None, (start, end, (country, region, city, postal-or-None)))``
    on success, or ``(reason, None)`` where reason is ``"malformed"`` or
    ``"no_city"``. Extra columns are ignored; a missing city is not a location.
    """
    if len(row) < 5:
        return "malformed", None
    parsed = ipv4_prefix(row[0])
    if parsed is None:
        return "malformed", None
    country = row[1].strip().upper()
    if len(country) != 2 or not all("A" <= ch <= "Z" for ch in country):
        return "malformed", None
    region = row[2].strip()
    city = row[3].strip()
    postal = row[4].strip()
    if not city:
        return "no_city", None
    return None, (parsed[0], parsed[1], (country, region, city, postal or None))


def load_input_rows(path: Path, report: InputReport) -> list:
    """Read one input file into parsed rows, updating its report counters."""
    rows = []
    first_row = True
    for row in iter_csv_rows(path):
        if not row or not any(field.strip() for field in row):
            continue
        if first_row:
            first_row = False
            if is_header_row(row[0]):
                report.header = True
                continue
        reason, payload = parse_city_row(row)
        report.rows += 1
        if reason == "malformed":
            report.malformed += 1
            continue
        if reason == "no_city":
            report.no_city += 1
            continue
        report.kept += 1
        rows.append(payload)
    return rows


class CityTable:
    """Deduplicating city table with the blob's u16 index bound.

    A full-planet GeoLite2 extract can exceed 65,535 distinct
    (country, region, city, postal, source) entries; the blob format allows
    only a u16 index, so the build fails loudly instead of wrapping or
    truncating. Hitting that cap is the signal to scope the table (this
    scaffold exists for hosting ranges, not the whole planet).
    """

    def __init__(self, cap: int = CITY_TABLE_CAP) -> None:
        self._cap = cap
        self.cities: list[City] = []
        self._index: dict[tuple, int] = {}

    def intern(
        self, country: str, region: str, city: str, postal: str | None, source: str
    ) -> int:
        key = (country, region, city, postal, source)
        existing = self._index.get(key)
        if existing is not None:
            return existing
        if len(self.cities) >= self._cap:
            raise BuildError(
                f"city table reached its {self._cap}-entry u16 cap; scope the "
                "inputs (hosting ranges only) instead of emitting a wrapped "
                "index"
            )
        index = len(self.cities)
        self._index[key] = index
        self.cities.append(
            City(country=country, region=region, city=city, postal=postal, source=source)
        )
        return index


def normalize(records: list) -> list:
    """Sort and clip overlapping prefixes within one source (first wins).

    Same contract as infra/scripts/build-server-location.py: sort by start,
    keep the longer of two same-start prefixes, and clip a later record's tail
    so the emitted blob is sorted and non-overlapping.
    """
    ordered = sorted(records, key=lambda record: (record[0], -record[1]))
    out = []
    for start, end, index in ordered:
        if out and start <= out[-1][1]:
            previous_start, previous_end, previous_index = out[-1]
            if previous_index == index or end <= previous_end:
                continue
            if start > previous_start:
                out[-1] = (previous_start, start - 1, previous_index)
        out.append((start, end, index))
    return out


def merge_geofeed_over_geolite(geolite: list, geofeed: list) -> list:
    """Return sorted, non-overlapping records where geofeed always wins.

    Both inputs must already be normalized. Each GeoLite range is split around
    every overlapping geofeed range; whole or partial overlaps therefore leave
    a GeoLite piece only where the geofeed does not claim the address.
    """
    if not geolite:
        return list(geofeed)
    if not geofeed:
        return list(geolite)

    merged = []
    cursor = 0
    for start, end, index in geolite:
        while cursor < len(geofeed) and geofeed[cursor][1] < start:
            cursor += 1
        pieces = [(start, end)]
        position = cursor
        while position < len(geofeed) and geofeed[position][0] <= end:
            geofeed_start, geofeed_end, _ = geofeed[position]
            position += 1
            if geofeed_end < start:
                continue
            remaining = []
            for piece_start, piece_end in pieces:
                if geofeed_end < piece_start or geofeed_start > piece_end:
                    remaining.append((piece_start, piece_end))
                    continue
                if piece_start < geofeed_start:
                    remaining.append((piece_start, geofeed_start - 1))
                if piece_end > geofeed_end:
                    remaining.append((geofeed_end + 1, piece_end))
            pieces = remaining
            if not pieces:
                break
        merged.extend((piece_start, piece_end, index) for piece_start, piece_end in pieces)

    merged.extend(geofeed)
    merged.sort(key=lambda record: (record[0], record[1]))
    return merged


def validate_records(records: list, city_count: int) -> None:
    """Fail closed on anything the shipped reader could not binary-search."""
    if not records:
        raise BuildError("no usable rows: every input row was malformed or cityless")
    previous_end = -1
    for start, end, index in records:
        if start > end:
            raise BuildError(f"inverted range: {start} > {end}")
        if start <= previous_end:
            raise BuildError(
                f"overlapping or unsorted range at {start}..{end} "
                f"(previous end {previous_end})"
            )
        if index >= city_count:
            raise BuildError(f"range {start}..{end} indexes past the city table")
        previous_end = end


def escape_rust_string(text: str) -> str:
    """Render a Rust string literal for the generated table."""
    out = []
    for character in text:
        if character == "\\":
            out.append("\\\\")
        elif character == '"':
            out.append('\\"')
        elif character == "\n":
            out.append("\\n")
        elif character == "\r":
            out.append("\\r")
        elif character == "\t":
            out.append("\\t")
        else:
            out.append(character)
    return '"' + "".join(out) + '"'


def display_path(path: Path) -> str:
    """A path for evidence: repo-relative when possible, else the file name."""
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.name


def new_report(kind: str, path: Path) -> InputReport:
    if not path.is_file():
        raise BuildError(f"input not found: {path}")
    data = path.read_bytes()
    return InputReport(
        kind=kind,
        file=display_path(path),
        bytes=len(data),
        sha256=sha256_bytes(data),
    )


def source_counts(records: list, cities: list[City]) -> dict:
    counts: dict[str, int] = {}
    for _, _, index in records:
        source = cities[index].source
        counts[source] = counts.get(source, 0) + 1
    return dict(sorted(counts.items()))


def render_rust(
    cities: list[City],
    reports: list[InputReport],
    generated_at: str,
    blob: bytes,
) -> str:
    """The generated city table Rust file, gate header included."""
    kinds = list(dict.fromkeys(report.kind for report in reports))
    sources = "+".join(kinds)
    attribution = (
        "Derived from operator geofeed and/or MaxMind GeoLite2 City. "
        "GeoLite2 terms require attribution and CC BY-SA 4.0 ShareAlike on this "
        "derived table; free IP databases are unreliable for hosting ranges."
    )
    lines = [
        "// @generated by infra/scripts/build-server-city.py - DO NOT EDIT, DO NOT COMMIT.",
        "//",
        "// BUILD-TIME ONLY. This file and `server-city.bin` are emitted under",
        "// target/server-location-city/ and are NOT part of the shipped client: no",
        "// build.rs includes this file and no client code reads the blob.",
        "//",
        "// OWNER GATE (O1) - shipping data derived from these sources requires an",
        "// explicit owner decision:",
        "//   * MaxMind GeoLite2 City: GeoLite2 End User Licence Agreement + CC BY-SA",
        "//     4.0 - attribution, ShareAlike on the derived table, keep-current",
        "//     refresh cadence.",
        "//   * Operator geofeed (RFC 8805): the feed states no licence, so shipping",
        "//     it needs the operator's written permission.",
        "// Until that decision is recorded in",
        "// docs/design/server-location-online-lookup-review.md, this output stays",
        "// under target/ and nothing may copy it into the client.",
        "//",
        "// Required labels if this ever ships:",
        "//   * geofeed: \"Server location: operator geofeed - <REGION>, <CITY>",
        "//     (<POSTAL>), published by the network operator, shown as-is\".",
        "//   * geolite: \"Server location: city estimate +/-20 km (MaxMind",
        "//     GeoLite2, build DATE)\"; never a bare city, never a verified dot.",
        "//",
        f"// Generated: {generated_at}",
        f"// Blob: server-city.bin ({len(blob)} bytes, sha256 {sha256_bytes(blob)})",
        "// Record: start u32 LE | end u32 LE | city index u16 LE (10 bytes each)",
        "// Inputs:",
    ]
    for report in reports:
        lines.append(
            f"//   {report.kind}: {report.file} ({report.rows} rows, "
            f"{report.kept} kept, sha256 {report.sha256})"
        )
    lines += [
        "",
        "/// Bytes per range record in `server-city.bin`.",
        "pub const SERVER_CITY_RECORD_BYTES: usize = 10;",
        "",
        "/// Build inputs that produced this table (`geofeed`, `geolite`, or both).",
        f"pub const SERVER_CITY_SOURCES: &str = {escape_rust_string(sources)};",
        "",
        "/// UTC build date, for the \"MaxMind GeoLite2, build DATE\" label.",
        f"pub const SERVER_CITY_BUILT: &str = {escape_rust_string(generated_at[:10])};",
        "",
        "/// Attribution/label text any consumer of this table must carry.",
        f"pub const SERVER_CITY_ATTRIBUTION: &str = {escape_rust_string(attribution)};",
        "",
        "/// One city the build could place, indexed by the blob's city index.",
        "pub struct ServerCity {",
        "    pub country: &'static str,",
        "    pub region: &'static str,",
        "    pub city: &'static str,",
        "    pub postal: Option<&'static str>,",
        "    pub source: &'static str,",
        "}",
        "",
        "/// Cities, indexed by the city-index field of every `server-city.bin` record.",
        "#[rustfmt::skip]",
        "pub const SERVER_CITIES: &[ServerCity] = &[",
    ]
    for city in cities:
        postal = (
            "None" if city.postal is None else f"Some({escape_rust_string(city.postal)})"
        )
        lines.append(
            "    ServerCity { "
            f"country: {escape_rust_string(city.country)}, "
            f"region: {escape_rust_string(city.region)}, "
            f"city: {escape_rust_string(city.city)}, "
            f"postal: {postal}, "
            f"source: {escape_rust_string(city.source)} }},"
        )
    lines += ["];", ""]
    return "\n".join(lines)


def write_blob(records: list) -> bytes:
    blob = bytearray()
    for start, end, index in records:
        blob += struct.pack("<IIH", start, end, index)
    return bytes(blob)


def read_blob(blob: bytes) -> list:
    """Inverse of write_blob; the selftest round-trips through it."""
    if len(blob) % RECORD_BYTES:
        raise BuildError(f"blob length {len(blob)} is not whole 10-byte records")
    records = []
    for offset in range(0, len(blob), RECORD_BYTES):
        start, end, index = struct.unpack_from("<IIH", blob, offset)
        records.append((start, end, index))
    return records


def build(
    geofeed_paths: list[Path],
    geolite_csv: Path | None,
    out_dir: Path,
) -> BuildResult:
    """Run the whole pipeline and emit the three artifacts under `out_dir`."""
    table = CityTable()
    reports: list[InputReport] = []
    geofeed_records: list = []
    geolite_records: list = []

    for path in geofeed_paths:
        report = new_report("geofeed", path)
        for start, end, (country, region, city, postal) in load_input_rows(path, report):
            index = table.intern(country, region, city, postal, "geofeed")
            geofeed_records.append((start, end, index))
        reports.append(report)

    if geolite_csv is not None:
        report = new_report("geolite", geolite_csv)
        for start, end, (country, region, city, postal) in load_input_rows(
            geolite_csv, report
        ):
            index = table.intern(country, region, city, postal, "geolite")
            geolite_records.append((start, end, index))
        reports.append(report)

    if not reports:
        raise BuildError(
            "no inputs: pass --geofeed and/or --geolite-csv, or place RFC 8805 "
            "CSVs under infra/geo/geofeeds/"
        )

    total_rows = sum(report.rows for report in reports)
    if total_rows == 0:
        raise BuildError("no data rows: every input was empty or comments only")
    if not table.cities:
        raise BuildError(
            "no usable city rows: every data row was malformed or carried no city"
        )

    geofeed_records = normalize(geofeed_records)
    geolite_records = normalize(geolite_records)
    records = merge_geofeed_over_geolite(geolite_records, geofeed_records)
    validate_records(records, len(table.cities))

    generated_at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    blob = write_blob(records)
    rust = render_rust(table.cities, reports, generated_at, blob)
    rust_bytes = rust.encode("utf-8")

    manifest = {
        "generated_at": generated_at,
        "format": "start-little-endian-u32, end-little-endian-u32, "
        "city-index-little-endian-u16",
        "record_bytes": RECORD_BYTES,
        "precedence": ["geofeed", "geolite"],
        "owner_gate": GATE,
        "wired_into_client": False,
        "inputs": [report.as_dict() for report in reports],
        "cities": len(table.cities),
        "ranges": len(records),
        "source_ranges": source_counts(records, table.cities),
        "outputs": {
            "server-city.bin": {
                "bytes": len(blob),
                "sha256": sha256_bytes(blob),
            },
            "server_city.rs": {
                "bytes": len(rust_bytes),
                "sha256": sha256_bytes(rust_bytes),
            },
        },
    }

    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "server-city.bin").write_bytes(blob)
    (out_dir / "server_city.rs").write_text(rust, encoding="utf-8")
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return BuildResult(
        records=records,
        cities=table.cities,
        inputs=reports,
        out_dir=out_dir,
        blob=blob,
        rust=rust,
        manifest=manifest,
    )


def mmdb_refusal() -> str:
    """Why --mmdb is refused, and the supported route to the same data."""
    return (
        "error: --mmdb is not supported by this scaffold.\n"
        "\n"
        "Decoding MaxMind's .mmdb format here would need a reader dependency or "
        "a shell-out to a converter, and no licensed data may ship before the "
        "owner decision O1. Convert GeoLite2-City to the five-column CSV first "
        "- flatten MaxMind's Blocks and Locations files with MaxMind's own "
        "tooling so each row is\n"
        "  network,country,region,city,postal\n"
        "then pass it with --geolite-csv. See the script docstring for the gate."
    )


def unique_paths(paths: list[Path]) -> list[Path]:
    seen = set()
    unique = []
    for path in paths:
        try:
            key = path.resolve()
        except OSError:
            key = path
        if key in seen:
            continue
        seen.add(key)
        unique.append(path)
    return unique


class Checks:
    """Tiny assertion recorder so --selftest reports every failure, not the first."""

    def __init__(self) -> None:
        self.count = 0
        self.failures: list[str] = []

    def eq(self, label: str, actual: object, expected: object) -> None:
        self.count += 1
        if actual != expected:
            self.failures.append(f"{label}: expected {expected!r}, got {actual!r}")

    def true(self, label: str, condition: bool) -> None:
        self.count += 1
        if not condition:
            self.failures.append(f"{label}: expected true")


def lookup_index(records: list, address: str) -> int | None:
    """The shipped-reader algorithm, in Python, for selftest assertions."""
    target = ipv4_int(address)
    low, high = 0, len(records) - 1
    while low <= high:
        middle = (low + high) // 2
        start, end, index = records[middle]
        if target < start:
            high = middle - 1
        elif target > end:
            low = middle + 1
        else:
            return index
    return None


def city_at(result: BuildResult, address: str) -> City | None:
    index = lookup_index(result.records, address)
    if index is None:
        return None
    return result.cities[index]


def selftest() -> int:
    """Fixture-based assertions for everything the pipeline claims to do."""
    checks = Checks()

    # --- prefix math ---------------------------------------------------------
    checks.eq(
        "1.2.3.4/24 masks host bits",
        ipv4_prefix("1.2.3.4/24"),
        (0x01020300, 0x010203FF),
    )
    checks.eq("0.0.0.0/0 covers the address space", ipv4_prefix("0.0.0.0/0"), (0, 0xFFFFFFFF))
    checks.eq(
        "255.255.255.255/32 is one address",
        ipv4_prefix("255.255.255.255/32"),
        (0xFFFFFFFF, 0xFFFFFFFF),
    )
    for bad in (
        "",
        "1.2.3.4",
        "1.2.3.4/33",
        "1.2.3.256/24",
        "1.2.3/24",
        "1.2.3.4/-1",
        "::1/128",
        "1.2.3.4/2a",
    ):
        checks.eq(f"prefix rejected: {bad!r}", ipv4_prefix(bad), None)

    # --- city table cap ------------------------------------------------------
    capped = CityTable(cap=3)
    for number in range(3):
        capped.intern("US", "VA", f"City{number}", None, "geofeed")
    cap_raised = False
    try:
        capped.intern("US", "VA", "Overflow", None, "geofeed")
    except BuildError:
        cap_raised = True
    checks.true("u16 city cap fails loudly instead of wrapping", cap_raised)
    checks.eq("city table stops at the cap", len(capped.cities), 3)

    # --- fixture pipeline ----------------------------------------------------
    scratch_root = DEFAULT_OUT / "selftest"
    scratch_root.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="case-", dir=scratch_root))
    try:
        geofeed_csv = work / "geofeed.csv"
        geofeed_csv.write_text(
            "# RFC 8805 fixture for --selftest; not real data.\n"
            "# prefix,country,region,city,postal\n"
            "1.0.0.0/25,US,US-VA,Manassas,20109\n"
            "\n"
            "198.51.100.0/24,US,US-NV,Las Vegas,89101\n"
            "198.51.101.0/24,US,US-DC,\"Washington, D.C.\",20001\n",
            encoding="utf-8",
        )
        malformed_csv = work / "malformed.csv"
        malformed_csv.write_text(
            "not-a-prefix,US,US-VA,Nowhere,00000\n"
            "1.2.3.4/33,US,US-VA,Nowhere,00000\n"
            "1.2.3.0/24,USA,US-VA,Nowhere,00000\n"
            "1.2.3.0/24\n"
            "10.0.0.0/24,US,US-VA,,00000\n"
            "10.1.0.0/24,,US-VA,NoCountry,00000\n",
            encoding="utf-8",
        )
        geolite_csv = work / "geolite.csv"
        geolite_csv.write_text(
            "network,country,region,city,postal\n"
            "1.0.0.0/24,US,VA,Manassas,20109\n"
            "1.0.1.0/24,US,VA,Manassas,20109\n"
            "203.0.113.0/24,JP,13,Tokyo,100-0001\n"
            "198.51.100.0/24,US,NV,Las Vegas,89101\n",
            encoding="utf-8",
        )
        out_dir = work / "artifacts"
        result = build([geofeed_csv, malformed_csv], geolite_csv, out_dir)

        # Report counters: comments/blank lines, header, malformed rows.
        reports = {report.file.split("/")[-1]: report for report in result.inputs}
        checks.eq("three input reports", len(result.inputs), 3)
        geofeed_report = reports["geofeed.csv"]
        checks.eq("geofeed data rows", geofeed_report.rows, 3)
        checks.eq("geofeed kept rows", geofeed_report.kept, 3)
        checks.eq("geofeed has no header", geofeed_report.header, False)
        malformed_report = reports["malformed.csv"]
        checks.eq("malformed rows are counted", malformed_report.malformed, 5)
        checks.eq("cityless rows are counted", malformed_report.no_city, 1)
        checks.eq("malformed rows are not kept", malformed_report.kept, 0)
        geolite_report = reports["geolite.csv"]
        checks.eq("geolite header is skipped", geolite_report.header, True)
        checks.eq("geolite data rows", geolite_report.kept, 4)

        # Merge precedence: geofeed beats geolite, including a partial overlap.
        checks.eq("range count after merge", len(result.records), 6)
        checks.eq("city count after dedupe", len(result.cities), 6)
        checks.eq("geofeed range count", result.manifest["source_ranges"]["geofeed"], 3)
        checks.eq("geolite range count", result.manifest["source_ranges"]["geolite"], 3)

        checks.eq(
            "quoted comma survives CSV parsing",
            city_at(result, "198.51.101.5").city,
            "Washington, D.C.",
        )
        checks.eq("geofeed claims its /25", city_at(result, "1.0.0.10").source, "geofeed")
        checks.eq("geofeed region is shown as-is", city_at(result, "1.0.0.10").region, "US-VA")
        checks.eq(
            "geolite keeps the address the geofeed does not claim",
            city_at(result, "1.0.0.200").source,
            "geolite",
        )
        checks.eq("geolite Manassas region", city_at(result, "1.0.0.200").region, "VA")
        checks.eq("geolite-only address", city_at(result, "203.0.113.5").city, "Tokyo")
        checks.eq(
            "geofeed overrides a whole geolite prefix",
            city_at(result, "198.51.100.5").source,
            "geofeed",
        )
        checks.eq("uncovered address is unknown", city_at(result, "8.8.8.8"), None)

        # Dedupe: the same GeoLite place shares one index across networks.
        checks.eq(
            "identical cities share one index",
            lookup_index(result.records, "1.0.0.200")
            == lookup_index(result.records, "1.0.1.5"),
            True,
        )
        checks.eq(
            "different sources do not share an index",
            lookup_index(result.records, "1.0.0.10")
            != lookup_index(result.records, "1.0.0.200"),
            True,
        )

        # Emitted blob: format, ordering and round-trip.
        blob_path = out_dir / "server-city.bin"
        blob = blob_path.read_bytes()
        checks.eq("blob is whole records", len(blob) % RECORD_BYTES, 0)
        checks.eq("blob matches the in-memory records", read_blob(blob), result.records)
        checks.eq("blob round-trips through the reader", city_at(
            BuildResult(records=read_blob(blob), cities=result.cities), "203.0.113.5"
        ).city, "Tokyo")
        checks.eq(
            "manifest records the blob hash",
            result.manifest["outputs"]["server-city.bin"]["sha256"],
            sha256_bytes(blob),
        )

        # Generated Rust file: gate header, table shape, no wiring claim.
        rust_text = (out_dir / "server_city.rs").read_text(encoding="utf-8")
        for needle in (
            "DO NOT EDIT, DO NOT COMMIT",
            "GeoLite2 End User Licence Agreement",
            "CC BY-SA",
            "ShareAlike",
            "operator's written permission",
            "NOT part of the shipped client",
            "pub const SERVER_CITIES: &[ServerCity] = &[",
            "pub const SERVER_CITY_RECORD_BYTES: usize = 10;",
            "pub const SERVER_CITY_SOURCES: &str = \"geofeed+geolite\";",
        ):
            checks.true(f"generated Rust contains {needle!r}", needle in rust_text)
        checks.eq(
            "generated table row count",
            rust_text.count("    ServerCity {"),
            len(result.cities),
        )

        # Manifest: gate text, no-wiring flag, input hashes, city index bound.
        checks.eq("manifest carries the gate", result.manifest["owner_gate"], GATE)
        checks.eq(
            "manifest refuses to claim wiring", result.manifest["wired_into_client"], False
        )
        checks.eq(
            "manifest hashes every input",
            all(row["sha256"] == sha256_bytes((work / row["file"].split("/")[-1]).read_bytes())
                for row in result.manifest["inputs"]),
            True,
        )
        checks.true(
            "city indexes fit the u16 blob field",
            all(index <= MAX_CITY_INDEX for _, _, index in result.records),
        )

        # A geofeed-only build is a valid pipeline too.
        only_geofeed = build([geofeed_csv], None, work / "artifacts-geofeed-only")
        checks.eq("geofeed-only sources", only_geofeed.manifest["source_ranges"], {"geofeed": 3})
        checks.true(
            "geofeed-only Rust source label",
            "pub const SERVER_CITY_SOURCES: &str = \"geofeed\";" in only_geofeed.rust,
        )

        # Hard errors: no cityless output, no empty output.
        empty_csv = work / "empty.csv"
        empty_csv.write_text(
            "network,country,region,city,postal\n# no data rows\n", encoding="utf-8"
        )
        empty_raised = False
        try:
            build([empty_csv], None, work / "artifacts-empty")
        except BuildError:
            empty_raised = True
        checks.true("empty input is a hard error", empty_raised)

        malformed_only = work / "malformed-only.csv"
        malformed_only.write_text(
            "1.2.3.0/24,USA,US-VA,Nowhere,00000\n1.2.3.0/24\n", encoding="utf-8"
        )
        malformed_raised = False
        try:
            build([malformed_only], None, work / "artifacts-malformed")
        except BuildError:
            malformed_raised = True
        checks.true("all-malformed input is a hard error", malformed_raised)

        # MMDB refusal points the owner at the supported conversion route.
        refusal = mmdb_refusal()
        checks.true("--mmdb refusal names --geolite-csv", "--geolite-csv" in refusal)
        checks.true("--mmdb refusal names the gate", "O1" in refusal)
    finally:
        shutil.rmtree(work, ignore_errors=True)

    for failure in checks.failures:
        print(f"SELFTEST FAIL: {failure}")
    if checks.failures:
        print(f"SELFTEST FAILED: {len(checks.failures)} of {checks.count} checks")
        return 1
    print(f"SELFTEST OK: {checks.count} checks passed")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Build-time-only offline server-city index scaffold (not shipped).",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=GATE,
    )
    parser.add_argument(
        "--selftest",
        action="store_true",
        help="run fixture-based assertions and exit (no licensed data, no network)",
    )
    parser.add_argument(
        "--geofeed",
        action="append",
        default=[],
        metavar="CSV",
        help="RFC 8805 geofeed CSV (prefix,country,region,city,postal); repeatable",
    )
    parser.add_argument(
        "--geolite-csv",
        metavar="CSV",
        help="pre-extracted GeoLite2-City CSV with the same five columns",
    )
    parser.add_argument(
        "--mmdb",
        metavar="MMDB",
        help="refused: convert the .mmdb to CSV and pass --geolite-csv",
    )
    parser.add_argument(
        "--out",
        metavar="DIR",
        default=str(DEFAULT_OUT),
        help=f"output directory (default: {display_path(DEFAULT_OUT)})",
    )
    args = parser.parse_args(argv)

    if args.selftest:
        return selftest()
    if args.mmdb:
        print(mmdb_refusal(), file=sys.stderr)
        return 2

    geofeeds = [Path(path) for path in args.geofeed]
    if GEOFEED_DIR.is_dir():
        geofeeds.extend(sorted(GEOFEED_DIR.glob("*.csv")))
    geofeeds = unique_paths(geofeeds)
    geolite_csv = Path(args.geolite_csv) if args.geolite_csv else None

    if not geofeeds and geolite_csv is None:
        print(GATE, file=sys.stderr)
        print(
            "error: no inputs. Pass --geofeed/--geolite-csv, or place RFC 8805 "
            f"CSVs under {display_path(GEOFEED_DIR)}/.",
            file=sys.stderr,
        )
        return 2

    print(GATE)
    try:
        result = build(geofeeds, geolite_csv, Path(args.out))
    except BuildError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    for report in result.inputs:
        print(
            f"input {report.kind}: {report.file} "
            f"({report.rows} rows, {report.kept} kept, "
            f"{report.malformed} malformed, {report.no_city} cityless)"
        )
    print(
        f"wrote {display_path(result.out_dir / 'server-city.bin')} "
        f"({len(result.blob)} bytes, {len(result.records)} ranges) and "
        f"{display_path(result.out_dir / 'server_city.rs')} "
        f"({len(result.cities)} cities); geofeed beats geolite"
    )
    print(
        "NOT WIRED IN: this output is build-time evidence only. Do not commit it "
        "or copy it into client-gui/ before the owner decision O1."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
