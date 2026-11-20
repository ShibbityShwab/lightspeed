#!/usr/bin/env python3
"""Build the offline server-city table from public operator geofeeds.

WHY THIS EXISTS
  The route card can name a cloud region (exact) or a hosting network whose
  free-geoIP country is wrong, but it could not say which city a server is in.
  The 2026-11-20 panel showed the city-level answer exists offline and for free,
  and the owner then decided the licence route: NO MaxMind/GeoLite2 (the
  CC BY-SA 4.0 terms were rejected). Instead the project consumes **public
  operator geofeeds** (RFC 8805), per-operator, with attribution, shown as-is,
  and a takedown/deny list. Later measured sources can join the same table;
  geofeeds come first. No licensed database is used or redistributed.

OWNER DECISION (2026-11-20, recorded in docs/design/server-location-online-lookup-review.md)
  * No MaxMind/GeoLite2 data anywhere: no .mmdb, no GeoLite CSV, no derived table.
  * Only public operator RFC 8805 geofeeds, from the configured list in
    infra/geo/geofeeds/sources.json, each with an attribution line and a
    licence/status note.
  * The shipped table is display-only: it never feeds `chooser_point`, relay
    ranking or any measured claim, and every row is attributed.
  * Takedown: infra/geo/geofeeds/deny.txt (or --deny-file) removes an operator's
    whole feed or individual prefixes at build time.

INPUTS (build time only)
  1. infra/geo/geofeeds/sources.json - the configured source list. Each source:
       {name, operator, asn, url, local, attribution, license_status,
        redistributed, status}
     `attribution` and `license_status` are required; a source without an
     attribution line is a hard error, never silently unattributed output.
  2. The geofeed CSVs themselves. They are gitignored (see
     infra/geo/geofeeds/.gitignore). Resolve each source's local file in order:
       a. --geofeed NAME=PATH (repeatable; NAME must match a configured source);
       b. the source's `local` path, relative to the repository root;
       c. infra/geo/geofeeds/<slug>.csv (the conventional drop point).
     A configured source with no local file is skipped and recorded in the
     manifest; at least one source must resolve.
  3. infra/geo/geofeeds/deny.txt by default, plus any --deny-file. Each
     non-comment line is either an IPv4 prefix (drop every feed row that
     intersects it) or an operator name / `AS<number>` (drop that whole feed).

PARSING (RFC 8805)
  Five columns: prefix,country,region,city,postal. Full-line `#` comments and
  blank lines are ignored; an optional header row (`prefix,...`) is skipped.
  Extra columns are ignored. Malformed rows are counted and skipped; rows with
  no city are counted as "cityless" and skipped (a city table cannot place
  them). Prefixes are validated strictly and host bits are masked off.

PRECEDENCE
  Longest prefix wins (normal longest-prefix-match semantics). At equal length
  the earlier source in sources.json wins; within one source the earlier row
  wins. Ranges are clipped/merged so the emitted table is sorted and
  non-overlapping.

OUTPUTS
  target/server-location-city/ (gitignored build evidence, default --out):
    * server-geofeed.bin - range records, little-endian:
          start u32 | end u32 | place-index u16   (10 bytes per record)
      sorted by start, non-overlapping; the place index points into
      SERVER_GEOFEED_PLACES in the generated Rust table.
    * server_geofeed_tables.rs - the generated Rust table with the per-source
      attribution header.
    * manifest.json - source URLs, local files, hashes, as-of/fetch dates, row
      counts, attribution strings, licence/status notes, `redistributed` marker
      per source, deny/takedown list and the dropped counts.
  --ship also writes the committed client copy:
    * client-gui/data/server-geofeed.bin
    * client-gui/src/server_geofeed_tables.rs
    * client-gui/data/server-geofeed.sources.json
  --out DIR overrides the build-evidence directory. Shipping is refused for any
  used source marked `"redistributed": false`.

USAGE
  py -3 infra/scripts/build-server-city.py --selftest
  py -3 infra/scripts/build-server-city.py --geofeed G-Core=target/gcore-geofeed.csv --ship
  py -3 infra/scripts/build-server-city.py            # conventional drop point
  py -3 infra/scripts/build-server-city.py --deny-file removals.txt

  --selftest writes fixtures under target/ and asserts parsing, comments,
  malformed rows, deny/takedown handling, attribution enforcement, prefix math,
  merge precedence, the dateline/format and the emitted files. It needs no
  licensed data and no network, and is safe to run on every host.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
import shutil
import struct
import sys
import tempfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GEOFEED_DIR = ROOT / "infra" / "geo" / "geofeeds"
SOURCES_JSON = GEOFEED_DIR / "sources.json"
DENY_FILE = GEOFEED_DIR / "deny.txt"
DEFAULT_OUT = ROOT / "target" / "server-location-city"
SHIP_DATA = ROOT / "client-gui" / "data"
SHIP_TABLE = ROOT / "client-gui" / "src" / "server_geofeed_tables.rs"
SHIP_MANIFEST = SHIP_DATA / "server-geofeed.sources.json"

# 10 bytes per range record: start u32 LE, end u32 LE, place-index u16 LE.
RECORD_BYTES = 10
MAX_PLACE_INDEX = 0xFFFF
PLACE_TABLE_CAP = MAX_PLACE_INDEX + 1

OWNER_DECISION = (
    "OWNER DECISION (2026-11-20): no MaxMind/GeoLite2 data - the CC BY-SA 4.0 "
    "terms were rejected. Consume public operator RFC 8805 geofeeds only, "
    "per-operator entries with attribution, shown as-is, with a takedown/deny "
    "list. No licensed database is used or redistributed. The table is "
    "display-only and never feeds the relay chooser or the ranking."
)

# A header comment such as G-Core's
#   #AS199524 Geofeed. Was generated at 2026-11-19 00:00:27 UTC
# is the feed's own generation date, which the label shows as `as-of`.
_FEED_DATE = re.compile(
    r"generated at\s+(\d{4}-\d{2}-\d{2})(?:[ T](\d{2}:\d{2}:\d{2})\s*UTC)?",
    re.IGNORECASE,
)
_ISO_DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


class BuildError(RuntimeError):
    """A hard input/format error: the build must not emit a partial artifact."""


@dataclass(frozen=True)
class SourceSpec:
    """One configured operator geofeed.

    Every field is required to be present and meaningful: a feed with no
    attribution line is refused rather than shipped unattributed.
    """

    name: str
    operator: str
    asn: int
    url: str
    attribution: str
    license_status: str
    local: str | None
    redistributed: bool


@dataclass(frozen=True)
class Place:
    """One placeable city, indexed by the blob's `place-index` field."""

    country: str
    region: str | None
    city: str | None
    source: int


@dataclass
class InputReport:
    """What one configured source contributed; written into manifest.json."""

    name: str
    operator: str
    asn: int
    url: str
    attribution: str
    license_status: str
    redistributed: bool
    as_of: str
    feed_generated_at: str | None
    file: str | None
    bytes: int
    sha256: str | None
    local_modified: str | None
    skipped: bool
    rows: int = 0
    kept: int = 0
    malformed: int = 0
    no_city: int = 0
    ipv6: int = 0
    denied: int = 0
    denied_operator: bool = False
    header: bool = False

    def as_dict(self) -> dict:
        return {
            "name": self.name,
            "operator": self.operator,
            "asn": self.asn,
            "url": self.url,
            "attribution": self.attribution,
            "license_status": self.license_status,
            "redistributed": self.redistributed,
            "as_of": self.as_of,
            "feed_generated_at": self.feed_generated_at,
            "file": self.file,
            "bytes": self.bytes,
            "sha256": self.sha256,
            "local_modified": self.local_modified,
            "skipped": self.skipped,
            "rows": self.rows,
            "kept": self.kept,
            "malformed": self.malformed,
            "no_city": self.no_city,
            "ipv6": self.ipv6,
            "denied": self.denied,
            "denied_operator": self.denied_operator,
            "header": self.header,
        }


@dataclass
class BuildResult:
    """Everything one build produced, in memory, for callers and the selftest."""

    records: list
    places: list
    sources: list
    reports: list
    skipped: list
    blob: bytes
    rust: str
    manifest: dict
    deny: "DenyList"
    shipped: bool
    out_dir: Path | None = None


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def ipv4_network(text: str) -> tuple[int, int, int] | None:
    """Parse an IPv4 CIDR into (start, end, prefix-length), or None.

    Strict on purpose: four dotted-decimal octets, a `/`, and a length in
    0..=32. Host bits are masked off so ``1.2.3.4/24`` is ``1.2.3.0/24``.
    Returns ``None`` for anything malformed rather than guessing.
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
    return start, start | mask, bits


def ipv4_prefix(text: str) -> tuple[int, int] | None:
    """Parse an IPv4 CIDR into its inclusive (start, end) integer range."""
    network = ipv4_network(text)
    if network is None:
        return None
    return network[0], network[1]


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
        "place",
        "prefix",
    }


def parse_city_row(row: list[str]) -> tuple[str | None, tuple | None]:
    """Parse one five-column geofeed row.

    Returns ``(None, (start, end, prefix_len, country, region, city))`` on
    success, or ``(reason, None)`` where reason is ``"malformed"``,
    ``"no_city"`` or ``"ipv6"``. The shipped range format is IPv4-only, so a
    valid IPv6 geofeed row is counted separately instead of called malformed.
    Extra columns (postal is the fifth, anything beyond it is ignored) are
    accepted; a row without a city is not a place.
    """
    if len(row) < 5:
        return "malformed", None
    if ":" in row[0].strip():
        return "ipv6", None
    network = ipv4_network(row[0])
    if network is None:
        return "malformed", None
    country = row[1].strip().upper()
    if len(country) != 2 or not all("A" <= ch <= "Z" for ch in country):
        return "malformed", None
    region = row[2].strip() or None
    city = row[3].strip() or None
    if city is None:
        return "no_city", None
    # Postal (row[4]) is parsed for format completeness and deliberately not
    # shipped: the route-card label is city/region only.
    return None, (network[0], network[1], network[2], country, region, city)


def detect_dates(
    head: str, path: Path
) -> tuple[str, str | None]:
    """The feed's as-of date (ISO) and generation timestamp, when it states one.

    Falls back to the local file's UTC modification date so every report still
    carries a dateline; the manifest records which one was used.
    """
    match = _FEED_DATE.search(head)
    if match:
        as_of = match.group(1)
        if match.group(2):
            return as_of, f"{as_of}T{match.group(2)}Z"
        return as_of, None
    fallback = datetime.fromtimestamp(path.stat().st_mtime, timezone.utc)
    return fallback.strftime("%Y-%m-%d"), None


def display_path(path: Path) -> str:
    """A path for evidence: repo-relative when possible, else the file name."""
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.name


def slugify(name: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", name.strip().lower()).strip("-")
    return slug or "source"


def require_text(item: dict, key: str, source_name: str, noun: str) -> str:
    value = item.get(key)
    if not isinstance(value, str) or not value.strip():
        raise BuildError(
            f"source {source_name!r} has no {noun}; every configured operator "
            "geofeed must carry one"
        )
    return value.strip()


def validate_source_spec(spec: SourceSpec) -> None:
    """Hard-fail on a source that cannot be attributed or redistributed."""
    if not spec.name.strip():
        raise BuildError("a configured source has an empty name")
    if not spec.attribution.strip():
        raise BuildError(
            f"source {spec.name!r} has no attribution line; refusing to emit "
            "unattributed operator data"
        )
    if not spec.license_status.strip():
        raise BuildError(
            f"source {spec.name!r} has no license/status note; every geofeed "
            "must say what it is and whether it may ship"
        )
    if not spec.url.strip() and not (spec.local or "").strip():
        raise BuildError(
            f"source {spec.name!r} needs a public url or a local path"
        )
    if spec.asn < 0:
        raise BuildError(f"source {spec.name!r} has a negative ASN")


def load_sources(path: Path) -> list[SourceSpec]:
    """Read the configured source list; validates attribution before any build."""
    if not path.is_file():
        raise BuildError(f"source manifest not found: {display_path(path)}")
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise BuildError(f"{display_path(path)}: invalid JSON: {error}") from error
    raw_sources = payload.get("sources")
    if not isinstance(raw_sources, list) or not raw_sources:
        raise BuildError(f"{display_path(path)}: no `sources` list")

    specs: list[SourceSpec] = []
    seen: set[str] = set()
    for item in raw_sources:
        if not isinstance(item, dict):
            raise BuildError(f"{display_path(path)}: a source entry is not an object")
        name = item.get("name")
        if not isinstance(name, str) or not name.strip():
            raise BuildError(f"{display_path(path)}: a source entry has no name")
        name = name.strip()
        key = name.casefold()
        if key in seen:
            raise BuildError(f"configured source {name!r} appears twice")
        seen.add(key)
        operator = item.get("operator")
        if not isinstance(operator, str) or not operator.strip():
            operator = name
        asn = item.get("asn", 0)
        if not isinstance(asn, int):
            raise BuildError(f"source {name!r}: asn must be an integer")
        url = item.get("url")
        url = url.strip() if isinstance(url, str) else ""
        local = item.get("local")
        local = local.strip() if isinstance(local, str) and local.strip() else None
        redistributed = item.get("redistributed")
        if not isinstance(redistributed, bool):
            raise BuildError(
                f"source {name!r}: `redistributed` must be true or false"
            )
        spec = SourceSpec(
            name=name,
            operator=operator.strip(),
            asn=asn,
            url=url,
            attribution=require_text(item, "attribution", name, "attribution line"),
            license_status=require_text(
                item, "license_status", name, "license/status note"
            ),
            local=local,
            redistributed=redistributed,
        )
        validate_source_spec(spec)
        specs.append(spec)
    return specs


def parse_overrides(values: list[str], sources: list[SourceSpec]) -> dict[str, Path]:
    """Parse repeatable `--geofeed NAME=PATH`; unknown names are refused."""
    known = {spec.name.casefold(): spec.name for spec in sources}
    overrides: dict[str, Path] = {}
    for value in values:
        name, separator, path_text = value.partition("=")
        if not separator or not name.strip() or not path_text.strip():
            raise BuildError(
                f"--geofeed expects NAME=PATH, got {value!r}; NAME must match a "
                f"source in {display_path(SOURCES_JSON)}"
            )
        key = name.strip().casefold()
        if key not in known:
            raise BuildError(
                f"--geofeed {name.strip()!r}: unknown source; configured: "
                + ", ".join(sorted(known.values()))
            )
        if key in overrides:
            raise BuildError(f"--geofeed {known[key]!r} given twice")
        path = Path(path_text.strip())
        if not path.is_file():
            raise BuildError(f"local geofeed not found: {path_text.strip()}")
        overrides[key] = path
    return overrides


@dataclass
class DenyFileReport:
    file: str
    sha256: str
    entries: int
    prefixes: list
    operators: list


class DenyList:
    """Takedown/deny list: prefixes to drop and operators to drop entirely."""

    def __init__(self) -> None:
        self.files: list[DenyFileReport] = []
        self.prefixes: list[tuple[int, int, str]] = []
        self.operators: list[str] = []
        self.dropped_rows = 0
        self.dropped_sources: list[str] = []

    def add_file(self, path: Path) -> None:
        if not path.is_file():
            raise BuildError(f"deny file not found: {path}")
        data = path.read_bytes()
        entries = 0
        prefixes = []
        operators = []
        for line in data.decode("utf-8-sig", errors="replace").splitlines():
            text = line.strip()
            if not text or text.startswith("#"):
                continue
            entries += 1
            parsed = ipv4_prefix(text)
            if parsed is not None:
                prefixes.append((parsed[0], parsed[1], text))
            else:
                operators.append(text)
        self.files.append(
            DenyFileReport(
                file=display_path(path),
                sha256=sha256_bytes(data),
                entries=entries,
                prefixes=list(prefixes),
                operators=list(operators),
            )
        )
        for start, end, text in prefixes:
            if (start, end, text) not in self.prefixes:
                self.prefixes.append((start, end, text))
        for operator in operators:
            if operator not in self.operators:
                self.operators.append(operator)

    def matches_operator(self, spec: SourceSpec) -> bool:
        names = {spec.name.casefold(), spec.operator.casefold()}
        if spec.asn:
            names.add(f"as{spec.asn}".casefold())
        return any(entry.casefold() in names for entry in self.operators)

    def prefix_for(self, start: int, end: int) -> str | None:
        for denied_start, denied_end, text in self.prefixes:
            if start <= denied_end and end >= denied_start:
                return text
        return None

    def as_dict(self) -> dict:
        return {
            "files": [
                {
                    "file": report.file,
                    "sha256": report.sha256,
                    "entries": report.entries,
                }
                for report in self.files
            ],
            "prefixes": [text for _, _, text in self.prefixes],
            "operators": list(self.operators),
            "dropped_rows": self.dropped_rows,
            "dropped_sources": list(self.dropped_sources),
        }


def build_deny(explicit_paths: list[str]) -> DenyList:
    """The default deny.txt (when present) plus every explicit --deny-file."""
    deny = DenyList()
    if DENY_FILE.is_file():
        deny.add_file(DENY_FILE)
    for path_text in explicit_paths:
        deny.add_file(Path(path_text))
    return deny


def new_report(spec: SourceSpec, path: Path) -> InputReport:
    data = path.read_bytes()
    head = data[:4096].decode("utf-8-sig", errors="replace")
    as_of, feed_generated_at = detect_dates(head, path)
    if not _ISO_DATE.match(as_of):
        raise BuildError(f"source {spec.name!r}: as-of {as_of!r} is not an ISO date")
    modified = datetime.fromtimestamp(
        path.stat().st_mtime, timezone.utc
    ).strftime("%Y-%m-%dT%H:%M:%SZ")
    return InputReport(
        name=spec.name,
        operator=spec.operator,
        asn=spec.asn,
        url=spec.url,
        attribution=spec.attribution,
        license_status=spec.license_status,
        redistributed=spec.redistributed,
        as_of=as_of,
        feed_generated_at=feed_generated_at,
        file=display_path(path),
        bytes=len(data),
        sha256=sha256_bytes(data),
        local_modified=modified,
        skipped=False,
    )


def skipped_report(spec: SourceSpec) -> InputReport:
    return InputReport(
        name=spec.name,
        operator=spec.operator,
        asn=spec.asn,
        url=spec.url,
        attribution=spec.attribution,
        license_status=spec.license_status,
        redistributed=spec.redistributed,
        as_of="",
        feed_generated_at=None,
        file=None,
        bytes=0,
        sha256=None,
        local_modified=None,
        skipped=True,
    )


class PlaceTable:
    """Deduplicating place table with the blob's u16 index bound.

    A full-planet geofeed extract could exceed 65,535 distinct
    (source, country, region, city) entries; the blob format allows only a u16
    index, so the build fails loudly instead of wrapping or truncating.
    """

    def __init__(self, cap: int = PLACE_TABLE_CAP) -> None:
        self._cap = cap
        self.places: list[Place] = []
        self._index: dict[tuple, int] = {}

    def intern(
        self, source: int, country: str, region: str | None, city: str | None
    ) -> int:
        key = (source, country, region, city)
        existing = self._index.get(key)
        if existing is not None:
            return existing
        if len(self.places) >= self._cap:
            raise BuildError(
                f"place table reached its {self._cap}-entry u16 cap; scope the "
                "inputs instead of emitting a wrapped index"
            )
        index = len(self.places)
        self._index[key] = index
        self.places.append(
            Place(country=country, region=region, city=city, source=source)
        )
        return index


def resolve_ranges(candidates: list) -> list:
    """Resolve overlapping feed rows into sorted, non-overlapping records.

    Candidates are (start, end, place-index, prefix-length, source-rank,
    row-rank). Priority order is: longer prefix first, then the earlier
    configured source, then the earlier row in that source. Each candidate is
    clipped against the ranges already assigned to higher-priority claims, so
    the output is a deterministic longest-prefix-match table.
    """
    ordered = sorted(
        candidates,
        key=lambda candidate: (
            -candidate[3],
            candidate[4],
            candidate[5],
        ),
    )
    assigned: list[tuple[int, int, int]] = []
    for start, end, place, _bits, _source, _row in ordered:
        pieces = [(start, end)]
        for assigned_start, assigned_end, _ in assigned:
            if assigned_end < start or assigned_start > end:
                continue
            remaining = []
            for piece_start, piece_end in pieces:
                if assigned_end < piece_start or assigned_start > piece_end:
                    remaining.append((piece_start, piece_end))
                    continue
                if piece_start < assigned_start:
                    remaining.append((piece_start, assigned_start - 1))
                if piece_end > assigned_end:
                    remaining.append((assigned_end + 1, piece_end))
            pieces = remaining
            if not pieces:
                break
        for piece_start, piece_end in pieces:
            assigned.append((piece_start, piece_end, place))
        assigned.sort()

    # Coalesce adjacent pieces that share one place; the output stays sorted and
    # each range carries one place index.
    merged: list[tuple[int, int, int]] = []
    for start, end, place in assigned:
        if merged and merged[-1][2] == place and merged[-1][1] + 1 == start:
            previous_start, _, previous_place = merged[-1]
            merged[-1] = (previous_start, end, previous_place)
        else:
            merged.append((start, end, place))
    return merged


def validate_ranges(records: list, place_count: int) -> None:
    """Fail closed on anything the shipped reader could not binary-search."""
    if not records:
        raise BuildError("no usable rows: every input row was malformed, cityless or denied")
    previous_end = -1
    for start, end, index in records:
        if start > end:
            raise BuildError(f"inverted range: {start} > {end}")
        if start <= previous_end:
            raise BuildError(
                f"overlapping or unsorted range at {start}..{end} "
                f"(previous end {previous_end})"
            )
        if index >= place_count:
            raise BuildError(f"range {start}..{end} indexes past the place table")
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


def optional_rust_string(text: str | None) -> str:
    return "None" if text is None else f"Some({escape_rust_string(text)})"


def render_rust(
    specs: list[SourceSpec],
    reports: list[InputReport],
    places: list[Place],
    generated_at: str,
    blob: bytes,
    deny: DenyList,
    shipped: bool,
) -> str:
    """The generated Rust table, with the per-source attribution header."""
    destination = (
        "client-gui/src/server_geofeed_tables.rs (committed, display-only)"
        if shipped
        else "target/server-location-city/server_geofeed_tables.rs (build evidence)"
    )
    lines = [
        "// @generated by infra/scripts/build-server-city.py - do not edit by hand.",
        "//",
        "// Offline operator-geofeed server places for the route card. Built at",
        "// build time from the public RFC 8805 feeds below; the shipped client",
        "// reads the committed table and makes no network call.",
        "//",
        "// " + OWNER_DECISION,
        "//",
        "// Sources redistributed in this table:",
    ]
    for spec, report in zip(specs, reports):
        lines.append(
            f"//   * {spec.name} (AS{spec.asn}): {spec.url or spec.local or 'local file'}"
        )
        lines.append(
            f"//     operator: {spec.operator}; as-of {report.as_of}; "
            f"redistributed: {'true' if spec.redistributed else 'false'}"
        )
        lines.append(f"//     licence/status: {spec.license_status}")
        lines.append(f"//     attribution: {spec.attribution}")
    if deny.files:
        entries = sum(report.entries for report in deny.files)
        lines.append(
            f"// Deny/takedown list applied: {entries} entries from "
            + ", ".join(report.file for report in deny.files)
            + f" ({deny.dropped_rows} feed rows dropped, "
            f"{len(deny.dropped_sources)} operator(s) removed)."
        )
    else:
        lines.append("// Deny/takedown list applied: none.")
    lines += [
        "//",
        "// Precedence: longest prefix wins; at equal length the earlier source in",
        "// infra/geo/geofeeds/sources.json wins. Ranges never overlap.",
        f"// Destination: {destination}.",
        f"// Generated: {generated_at}",
        f"// Blob: server-geofeed.bin ({len(blob)} bytes, sha256 {sha256_bytes(blob)})",
        "// Record format: start u32 LE | end u32 LE | place-index u16 LE (10 bytes)",
        "// Display-only: this data never feeds chooser_point, relay ranking or any",
        "// measured claim. The route card shows it as the operator's own statement.",
        "// Regenerate from a local feed: py -3 infra/scripts/build-server-city.py",
        "//   --geofeed NAME=PATH --ship",
        "",
        "/// One configured operator geofeed that contributed rows.",
        "#[derive(Clone, Copy, Debug)]",
        "pub struct GeofeedSource {",
        "    /// Short operator name shown in the route card label.",
        "    pub provider: &'static str,",
        "    /// Public feed URL (also the provenance shown on hover).",
        "    pub url: &'static str,",
        "    /// Feed generation date, UTC (`YYYY-MM-DD`); display as `as-of`.",
        "    pub as_of: &'static str,",
        "}",
        "",
        "/// Sources, indexed by `GeofeedPlace::source`.",
        "#[rustfmt::skip]",
        "pub const SERVER_GEOFEED_SOURCES: &[GeofeedSource] = &[",
    ]
    for spec, report in zip(specs, reports):
        lines.append(
            "    GeofeedSource { "
            f"provider: {escape_rust_string(spec.name)}, "
            f"url: {escape_rust_string(spec.url)}, "
            f"as_of: {escape_rust_string(report.as_of)} }},"
        )
    lines += [
        "];",
        "",
        "/// One place a geofeed range can resolve to, as the operator publishes it.",
        "#[derive(Clone, Copy, Debug)]",
        "pub struct GeofeedPlace {",
        "    /// ISO 3166-1 alpha-2 country code, uppercased.",
        "    pub country: &'static str,",
        "    /// Region/subdivision as published (for example `US-VA`), when given.",
        "    pub region: Option<&'static str>,",
        "    /// City as published, when given.",
        "    pub city: Option<&'static str>,",
        "    /// Index into `SERVER_GEOFEED_SOURCES`.",
        "    pub source: u8,",
        "}",
        "",
        "/// Places, indexed by the place-index field of every range record.",
        "#[rustfmt::skip]",
        "pub const SERVER_GEOFEED_PLACES: &[GeofeedPlace] = &[",
    ]
    for place in places:
        lines.append(
            "    GeofeedPlace { "
            f"country: {escape_rust_string(place.country)}, "
            f"region: {optional_rust_string(place.region)}, "
            f"city: {optional_rust_string(place.city)}, "
            f"source: {place.source} }},"
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


def source_range_counts(records: list, places: list[Place]) -> dict:
    counts: dict[str, int] = {}
    for _, _, index in records:
        source = places[index].source
        counts[source] = counts.get(source, 0) + 1
    return dict(sorted(counts.items()))


def source_place_counts(places: list[Place]) -> dict:
    counts: dict[int, int] = {}
    for place in places:
        counts[place.source] = counts.get(place.source, 0) + 1
    return dict(sorted(counts.items()))


def build(
    inputs: list[tuple[SourceSpec, Path | None]],
    deny: DenyList,
    shipped: bool = False,
) -> BuildResult:
    """Run the whole pipeline in memory; the caller writes the artifacts.

    `inputs` pairs each configured source with its resolved local file, or
    `None` when the source has no local feed (recorded as skipped). Attribution,
    licence/status and redistribution markers are validated on every source.
    """
    table = PlaceTable()
    used_specs: list[SourceSpec] = []
    used_reports: list[InputReport] = []
    skipped: list[InputReport] = []
    candidates: list = []
    seen_names: set[str] = set()

    for spec, path in inputs:
        validate_source_spec(spec)
        if spec.name.casefold() in seen_names:
            raise BuildError(f"configured source {spec.name!r} appears twice")
        seen_names.add(spec.name.casefold())
        if path is None:
            skipped.append(skipped_report(spec))
            continue
        report = new_report(spec, path)
        operator_denied = deny.matches_operator(spec)
        if operator_denied:
            deny.dropped_sources.append(spec.name)
        source_index = len(used_specs)
        used_specs.append(spec)
        used_reports.append(report)

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
            if reason == "ipv6":
                report.ipv6 += 1
                continue
            start, end, bits, country, region, city = payload
            if operator_denied or deny.prefix_for(start, end) is not None:
                report.denied += 1
                deny.dropped_rows += 1
                continue
            place_index = table.intern(source_index, country, region, city)
            candidates.append(
                (start, end, place_index, bits, source_index, report.kept)
            )
            report.kept += 1
        if operator_denied:
            report.denied_operator = True

    if not used_specs:
        raise BuildError(
            "no geofeed inputs: every configured source has no local file; "
            "pass --geofeed NAME=PATH (a configured source name) or place the "
            f"feed at {display_path(GEOFEED_DIR)}/<name>.csv"
        )
    if not candidates:
        raise BuildError(
            "no usable rows: every feed row was malformed, cityless, or denied"
        )

    records = resolve_ranges(candidates)
    validate_ranges(records, len(table.places))
    if shipped:
        for spec in used_specs:
            if not spec.redistributed:
                raise BuildError(
                    f"source {spec.name!r} is marked redistributed: false and "
                    "cannot be shipped; fix the source status or do not --ship"
                )

    generated_at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    blob = write_blob(records)
    rust = render_rust(
        used_specs, used_reports, table.places, generated_at, blob, deny, shipped
    )
    rust_bytes = rust.encode("utf-8")
    source_names = {index: spec.name for index, spec in enumerate(used_specs)}
    manifest = {
        "format": "start-little-endian-u32, end-little-endian-u32, "
        "place-index-little-endian-u16",
        "record_bytes": RECORD_BYTES,
        "generated_at": generated_at,
        "owner_decision": OWNER_DECISION,
        "display_only": True,
        "ranking_input": False,
        "shipped": shipped,
        "precedence": [
            "longest prefix wins",
            "at equal length, earlier source in sources.json wins",
            "within one source, the earlier row wins",
        ],
        "sources": [report.as_dict() for report in used_reports],
        "skipped_sources": [report.as_dict() for report in skipped],
        "deny_list": deny.as_dict(),
        "places": len(table.places),
        "ranges": len(records),
        "source_ranges": {
            source_names[index]: count
            for index, count in source_range_counts(records, table.places).items()
        },
        "source_places": {
            source_names[index]: count
            for index, count in source_place_counts(table.places).items()
        },
        "outputs": {
            "server-geofeed.bin": {
                "bytes": len(blob),
                "sha256": sha256_bytes(blob),
            },
            "server_geofeed_tables.rs": {
                "bytes": len(rust_bytes),
                "sha256": sha256_bytes(rust_bytes),
            },
        },
    }
    return BuildResult(
        records=records,
        places=table.places,
        sources=used_specs,
        reports=used_reports,
        skipped=skipped,
        blob=blob,
        rust=rust,
        manifest=manifest,
        deny=deny,
        shipped=shipped,
    )


def write_outputs(result: BuildResult, out_dir: Path) -> list[Path]:
    """Write the build-evidence artifact set under `out_dir`."""
    out_dir.mkdir(parents=True, exist_ok=True)
    blob_path = out_dir / "server-geofeed.bin"
    rust_path = out_dir / "server_geofeed_tables.rs"
    manifest_path = out_dir / "manifest.json"
    blob_path.write_bytes(result.blob)
    rust_path.write_text(result.rust, encoding="utf-8")
    manifest_path.write_text(
        json.dumps(result.manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return [blob_path, rust_path, manifest_path]


def write_ship_outputs(result: BuildResult) -> list[Path]:
    """Write the committed client copy (bin + Rust table + source manifest)."""
    if not result.shipped:
        raise BuildError("refusing to ship artifacts from a non-ship build")
    return write_outputs_named(
        [
            (SHIP_DATA / "server-geofeed.bin", result.blob),
            (SHIP_TABLE, result.rust.encode("utf-8")),
            (
                SHIP_MANIFEST,
                (
                    json.dumps(result.manifest, indent=2, sort_keys=True) + "\n"
                ).encode("utf-8"),
            ),
        ]
    )


def write_outputs_named(files: list[tuple[Path, bytes]]) -> list[Path]:
    written = []
    for path, data in files:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        written.append(path)
    return written


def resolve_inputs(
    sources: list[SourceSpec], overrides: dict[str, Path]
) -> list[tuple[SourceSpec, Path | None]]:
    """Resolve each configured source's local feed in the documented order."""
    inputs: list[tuple[SourceSpec, Path | None]] = []
    claimed: dict[Path, str] = {}
    for spec in sources:
        path = overrides.get(spec.name.casefold())
        if path is None and spec.local:
            candidate = Path(spec.local)
            if not candidate.is_absolute():
                candidate = ROOT / candidate
            if candidate.is_file():
                path = candidate
        if path is None:
            candidate = GEOFEED_DIR / f"{slugify(spec.name)}.csv"
            if candidate.is_file():
                path = candidate
        if path is not None:
            resolved = path.resolve()
            if resolved in claimed:
                raise BuildError(
                    f"sources {claimed[resolved]!r} and {spec.name!r} both "
                    f"resolve to {display_path(path)}; attribution would be "
                    "ambiguous"
                )
            claimed[resolved] = spec.name
        inputs.append((spec, path))
    return inputs


def refusal(option: str) -> str:
    """Why the licensed-database options are gone, and what replaced them."""
    return (
        f"error: {option} is not supported.\n"
        "\n"
        "The 2026-11-20 owner decision rejected MaxMind/GeoLite2 data (the\n"
        "CC BY-SA 4.0 terms were declined) and chose public operator geofeeds\n"
        "with attribution instead. This pipeline consumes only RFC 8805 feeds\n"
        "listed in infra/geo/geofeeds/sources.json; there is no .mmdb or\n"
        "GeoLite CSV path. Run with --geofeed NAME=PATH for a configured\n"
        "operator feed, or see infra/geo/geofeeds/README.md."
    )


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

    def raises(self, label: str, function, needle: str | None = None) -> None:
        """Assert `function` raises BuildError, optionally mentioning `needle`."""
        self.count += 1
        try:
            function()
        except BuildError as error:
            if needle is not None and needle.casefold() not in str(error).casefold():
                self.failures.append(
                    f"{label}: BuildError did not mention {needle!r}: {error}"
                )
            return
        except Exception as error:  # noqa: BLE001 - report the wrong exception
            self.failures.append(
                f"{label}: expected BuildError, got {type(error).__name__}: {error}"
            )
            return
        self.failures.append(f"{label}: expected BuildError, nothing was raised")


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


def place_at(result: BuildResult, address: str) -> Place | None:
    index = lookup_index(result.records, address)
    if index is None:
        return None
    return result.places[index]


def fixture_source(
    name: str,
    url: str,
    attribution: str,
    *,
    local: str | None = None,
    redistributed: bool = True,
    asn: int = 0,
) -> SourceSpec:
    return SourceSpec(
        name=name,
        operator=name,
        asn=asn,
        url=url,
        attribution=attribution,
        license_status="public operator feed; test fixture",
        local=local,
        redistributed=redistributed,
    )


def selftest() -> int:
    """Fixture-based assertions for everything the pipeline claims to do."""
    checks = Checks()

    # --- prefix math ---------------------------------------------------------
    checks.eq(
        "1.2.3.4/24 masks host bits",
        ipv4_prefix("1.2.3.4/24"),
        (0x01020300, 0x010203FF),
    )
    checks.eq(
        "prefix math keeps the length",
        ipv4_network("1.2.3.4/24"),
        (0x01020300, 0x010203FF, 24),
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
        "1.2.3.4/24/8",
    ):
        checks.eq(f"prefix rejected: {bad!r}", ipv4_prefix(bad), None)

    # --- place table cap -----------------------------------------------------
    capped = PlaceTable(cap=3)
    for number in range(3):
        capped.intern(number, "US", "US-VA", f"City{number}")
    checks.raises(
        "u16 place cap fails loudly instead of wrapping",
        lambda: capped.intern(0, "US", "US-VA", "Overflow"),
        "u16 cap",
    )
    checks.eq("place table stops at the cap", len(capped.places), 3)

    # --- deny list parsing ---------------------------------------------------
    scratch_root = DEFAULT_OUT / "selftest"
    scratch_root.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="case-", dir=scratch_root))
    try:
        takedown_file = work / "takedown.txt"
        takedown_file.write_text(
            "# takedown list fixture; not real data.\n"
            "\n"
            "198.51.100.0/25\n",
            encoding="utf-8",
        )
        operators_file = work / "operators.txt"
        operators_file.write_text(
            "# operator takedown fixture; not real data.\n"
            "Gamma Net\n"
            "AS64501\n",
            encoding="utf-8",
        )
        deny_parsed = DenyList()
        deny_parsed.add_file(takedown_file)
        deny_parsed.add_file(operators_file)
        checks.eq(
            "deny files record their paths",
            [report.file.endswith(".txt") for report in deny_parsed.files],
            [True, True],
        )
        checks.eq(
            "deny file entry counts",
            [report.entries for report in deny_parsed.files],
            [1, 2],
        )
        checks.eq("deny file parses one prefix", len(deny_parsed.prefixes), 1)
        checks.eq("deny file parses two operator names", len(deny_parsed.operators), 2)
        alpha_spec = fixture_source(
            "Alpha Net",
            "https://example.test/alpha.csv",
            "Location data published by Alpha Networks Ltd. "
            "(https://example.test/alpha.csv), shown as-is.",
        )
        beta_spec = fixture_source(
            "Beta Net",
            "https://example.test/beta.csv",
            "Location data published by Beta Networks Ltd. "
            "(https://example.test/beta.csv), shown as-is.",
            asn=64501,
        )
        gamma_spec = fixture_source(
            "Gamma Net",
            "https://example.test/gamma.csv",
            "Location data published by Gamma Networks Ltd., shown as-is.",
        )
        checks.eq(
            "deny matches an operator by name",
            deny_parsed.matches_operator(gamma_spec),
            True,
        )
        checks.eq(
            "deny matches an operator by AS number",
            deny_parsed.matches_operator(beta_spec),
            True,
        )
        checks.eq(
            "deny does not match another operator",
            deny_parsed.matches_operator(alpha_spec),
            False,
        )
        checks.eq(
            "deny prefix intersects a covered row",
            deny_parsed.prefix_for(ipv4_int("198.51.100.0"), ipv4_int("198.51.100.255")),
            "198.51.100.0/25",
        )
        checks.eq(
            "deny prefix does not hit an unrelated row",
            deny_parsed.prefix_for(ipv4_int("203.0.113.0"), ipv4_int("203.0.113.255")),
            None,
        )
        checks.eq(
            "deny manifest shape",
            sorted(deny_parsed.as_dict()),
            ["dropped_rows", "dropped_sources", "files", "operators", "prefixes"],
        )

        # --- source manifest validation -------------------------------------
        sources_json = work / "sources.json"
        sources_json.write_text(
            json.dumps(
                {
                    "sources": [
                        {
                            "name": "Alpha Net",
                            "operator": "Alpha Networks Ltd",
                            "asn": 64500,
                            "url": "https://example.test/alpha.csv",
                            "attribution": "Location data published by Alpha Networks Ltd.",
                            "license_status": "public operator feed",
                            "redistributed": True,
                        }
                    ]
                }
            ),
            encoding="utf-8",
        )
        loaded = load_sources(sources_json)
        checks.eq("source manifest parses one source", len(loaded), 1)
        checks.eq("source manifest keeps the URL", loaded[0].url, "https://example.test/alpha.csv")
        checks.eq("source manifest marks redistribution", loaded[0].redistributed, True)
        checks.eq("source manifest keeps the ASN", loaded[0].asn, 64500)
        checks.raises(
            "unknown --geofeed name is refused",
            lambda: parse_overrides(["Unknown=work/unknown.csv"], loaded),
            "unknown source",
        )
        checks.raises(
            "malformed --geofeed value is refused",
            lambda: parse_overrides(["Alpha"], loaded),
            "NAME=PATH",
        )
        unattributed_json = work / "unattributed.json"
        unattributed_json.write_text(
            json.dumps(
                {
                    "sources": [
                        {
                            "name": "Unattributed",
                            "url": "https://example.test/x.csv",
                            "license_status": "no stated licence",
                            "redistributed": True,
                        }
                    ]
                }
            ),
            encoding="utf-8",
        )
        checks.raises(
            "a source without attribution is refused at load",
            lambda: load_sources(unattributed_json),
            "attribution",
        )
        unlicensed_json = work / "unlicensed.json"
        unlicensed_json.write_text(
            json.dumps(
                {
                    "sources": [
                        {
                            "name": "Unlicensed",
                            "url": "https://example.test/x.csv",
                            "attribution": "Some attribution.",
                            "redistributed": True,
                        }
                    ]
                }
            ),
            encoding="utf-8",
        )
        checks.raises(
            "a source without a licence/status note is refused at load",
            lambda: load_sources(unlicensed_json),
            "license/status",
        )

        # --- fixture feeds ---------------------------------------------------
        alpha_csv = work / "alpha.csv"
        alpha_csv.write_text(
            "# Alpha fixture geofeed; not real data.\n"
            "# generated at 2026-11-19 00:00:27 UTC\n"
            "prefix,country,region,city,postal\n"
            "1.0.0.0/24,US,US-VA,Manassas,20109\n"
            "\n"
            "\n"
            "198.51.100.0/24,US,US-NV,Las Vegas,89101\n"
            "198.51.101.0/24,US,US-DC,\"Washington, D.C.\",20001\n"
            "203.0.113.0/24,PE,,Lima,15023\n",
            encoding="utf-8",
        )
        beta_csv = work / "beta.csv"
        beta_csv.write_text(
            "# Beta fixture geofeed; not real data.\n"
            "network,country,region,city,postal\n"
            "1.0.0.0/25,US,US-CA,San Jose,95112\n"
            "1.0.1.0/24,US,US-CA,Irvine,92612\n"
            "192.0.2.0/24,JP,JP-13,Tokyo,100-0001\n",
            encoding="utf-8",
        )
        gamma_csv = work / "gamma.csv"
        gamma_csv.write_text(
            "not-a-prefix,US,US-VA,Nowhere,00000\n"
            "1.2.3.4/33,US,US-VA,Nowhere,00000\n"
            "1.2.3.0/24,USA,US-VA,Nowhere,00000\n"
            "1.2.3.0/24\n"
            "10.0.0.0/24,US,US-VA,,00000\n"
            "10.1.0.0/24,,US-VA,NoCountry,00000\n"
            "2a03:90c0:100::/44,AU,AU-NSW,Sydney,2015\n",
            encoding="utf-8",
        )
        takedown = DenyList()
        takedown.add_file(takedown_file)
        result = build(
            [
                (alpha_spec, alpha_csv),
                (beta_spec, beta_csv),
                (gamma_spec, gamma_csv),
            ],
            takedown,
        )

        reports = {report.name: report for report in result.reports}
        checks.eq("three used source reports", len(result.reports), 3)
        alpha_report = reports["Alpha Net"]
        beta_report = reports["Beta Net"]
        gamma_report = reports["Gamma Net"]
        checks.eq("alpha data rows", alpha_report.rows, 4)
        checks.eq("alpha header detected", alpha_report.header, True)
        checks.eq("alpha comments and blanks skipped", alpha_report.kept, 3)
        checks.eq("alpha denied row counted", alpha_report.denied, 1)
        checks.eq("beta data rows", beta_report.rows, 3)
        checks.eq("beta kept rows", beta_report.kept, 3)
        checks.eq("beta malformed rows", beta_report.malformed, 0)
        checks.eq("gamma malformed rows", gamma_report.malformed, 5)
        checks.eq("gamma cityless rows", gamma_report.no_city, 1)
        checks.eq("gamma kept rows", gamma_report.kept, 0)
        checks.eq(
            "IPv6 geofeed rows are counted, not called malformed",
            gamma_report.ipv6,
            1,
        )
        checks.eq(
            "row accounting balances for every source",
            all(
                report.rows
                == report.kept
                + report.malformed
                + report.no_city
                + report.ipv6
                + report.denied
                for report in result.reports
            ),
            True,
        )
        checks.eq("deny list dropped rows", takedown.dropped_rows, 1)
        checks.eq("deny list dropped sources", takedown.dropped_sources, [])

        # Parsing: comments, quoted comma, empty region, header.
        checks.eq(
            "quoted comma survives CSV parsing",
            place_at(result, "198.51.101.5").city,
            "Washington, D.C.",
        )
        checks.eq(
            "empty region stays None",
            place_at(result, "203.0.113.5").region,
            None,
        )
        checks.eq("city survives parsing", place_at(result, "203.0.113.5").city, "Lima")
        checks.eq("uncovered address is unknown", place_at(result, "8.8.8.8"), None)

        # Merge precedence: longest prefix wins, then source order.
        checks.eq(
            "a more specific later source wins the overlap",
            (place_at(result, "1.0.0.10").city, place_at(result, "1.0.0.10").source),
            ("San Jose", 1),
        )
        checks.eq(
            "the earlier source keeps an equal-length prefix",
            place_at(result, "1.0.0.200").city,
            "Manassas",
        )
        checks.eq(
            "a later source outside the overlap still places",
            place_at(result, "1.0.1.5").city,
            "Irvine",
        )
        checks.eq(
            "a later source range is not shadowed",
            place_at(result, "192.0.2.10").city,
            "Tokyo",
        )
        checks.eq(
            "denied prefix is not resolvable",
            place_at(result, "198.51.100.5"),
            None,
        )
        checks.true(
            "denied row is counted in the report",
            alpha_report.denied == 1,
        )

        # Dateline: the feed header supplies the as-of date.
        checks.eq("as-of date is the feed's own date", alpha_report.as_of, "2026-11-19")
        checks.eq(
            "feed generation timestamp is parsed",
            alpha_report.feed_generated_at,
            "2026-11-19T00:00:27Z",
        )
        checks.eq(
            "as-of date is ISO 8601 (dateline)",
            bool(_ISO_DATE.match(alpha_report.as_of)),
            True,
        )
        checks.eq(
            "generated timestamp is UTC with Z (dateline)",
            result.manifest["generated_at"].endswith("Z"),
            True,
        )

        # Emitted blob: format, ordering, round-trip.
        blob = result.blob
        checks.eq("blob is whole records", len(blob) % RECORD_BYTES, 0)
        checks.eq("manifest declares the record format", result.manifest["record_bytes"], 10)
        checks.eq("blob matches the in-memory records", read_blob(blob), result.records)
        checks.eq(
            "blob round-trips through the reader",
            BuildResult(
                records=read_blob(blob),
                places=result.places,
                sources=result.sources,
                reports=result.reports,
                skipped=[],
                blob=b"",
                rust="",
                manifest={},
                deny=DenyList(),
                shipped=False,
            ).records,
            result.records,
        )
        checks.eq(
            "manifest records the blob hash",
            result.manifest["outputs"]["server-geofeed.bin"]["sha256"],
            sha256_bytes(blob),
        )
        checks.true(
            "ranges are sorted and non-overlapping",
            all(
                _previous_end < start
                for _previous_end, start in zip(
                    [record[1] for record in result.records],
                    [record[0] for record in result.records][1:],
                )
            ),
        )
        checks.true(
            "place indexes fit the u16 blob field",
            all(index <= MAX_PLACE_INDEX for _, _, index in result.records),
        )
        checks.eq(
            "source range counts add up",
            sum(result.manifest["source_ranges"].values()),
            len(result.records),
        )

        # Attribution in the output header and in the manifest.
        checks.true(
            "generated Rust header carries the Alpha attribution",
            "Location data published by Alpha Networks Ltd." in result.rust,
        )
        checks.true(
            "generated Rust header carries the source URL",
            "https://example.test/alpha.csv" in result.rust,
        )
        checks.true(
            "generated Rust header carries the Beta attribution",
            "Location data published by Beta Networks Ltd." in result.rust,
        )
        checks.true(
            "generated Rust header states the owner decision",
            "no MaxMind/GeoLite2" in result.rust,
        )
        checks.true(
            "generated Rust header names the deny list",
            "Deny/takedown list applied" in result.rust,
        )
        checks.true(
            "generated Rust is the client table module",
            "pub const SERVER_GEOFEED_PLACES: &[GeofeedPlace] = &[" in result.rust,
        )
        checks.true(
            "generated Rust declares the source table",
            "pub const SERVER_GEOFEED_SOURCES: &[GeofeedSource] = &[" in result.rust,
        )
        checks.true(
            "generated Rust is display-only",
            "never feeds chooser_point" in result.rust,
        )
        checks.eq(
            "generated place rows match the table",
            result.rust.count("    GeofeedPlace {"),
            len(result.places),
        )
        manifest_sources = {row["name"]: row for row in result.manifest["sources"]}
        checks.eq(
            "manifest carries the attribution line",
            manifest_sources["Alpha Net"]["attribution"],
            alpha_spec.attribution,
        )
        checks.eq(
            "manifest carries the licence/status note",
            manifest_sources["Alpha Net"]["license_status"],
            "public operator feed; test fixture",
        )
        checks.eq(
            "manifest marks the source redistributable",
            manifest_sources["Alpha Net"]["redistributed"],
            True,
        )
        checks.eq(
            "manifest records the source URL",
            manifest_sources["Alpha Net"]["url"],
            "https://example.test/alpha.csv",
        )
        checks.eq(
            "manifest records the local file hash",
            manifest_sources["Alpha Net"]["sha256"],
            sha256_bytes(alpha_csv.read_bytes()),
        )
        checks.eq(
            "manifest deny list lists the prefix",
            result.manifest["deny_list"]["prefixes"],
            ["198.51.100.0/25"],
        )
        checks.eq(
            "manifest deny list lists the operator names",
            result.manifest["deny_list"]["operators"],
            [],
        )
        checks.eq(
            "manifest deny list counts dropped rows",
            result.manifest["deny_list"]["dropped_rows"],
            1,
        )
        checks.eq(
            "manifest declares display-only data",
            (result.manifest["display_only"], result.manifest["ranking_input"]),
            (True, False),
        )

        # Skipped source is recorded, not fatal when another source resolves.
        skipped_result = build([(beta_spec, None), (alpha_spec, alpha_csv)], DenyList())
        checks.eq(
            "skipped source is recorded",
            [row["name"] for row in skipped_result.manifest["skipped_sources"]],
            ["Beta Net"],
        )
        checks.eq("skipped source contributes no ranges", sorted(skipped_result.manifest["source_ranges"]), ["Alpha Net"])

        # A source without attribution is a hard error even when built directly.
        checks.raises(
            "a source without attribution is refused at build",
            lambda: build(
                [
                    (
                        SourceSpec(
                            name="Anonymous",
                            operator="Anonymous",
                            asn=0,
                            url="https://example.test/a.csv",
                            attribution="",
                            license_status="public",
                            local=None,
                            redistributed=True,
                        ),
                        alpha_csv,
                    )
                ],
                DenyList(),
            ),
            "attribution",
        )

        # A source that may not be redistributed cannot ship.
        non_redistributable = build(
            [
                (fixture_source("Alpha Net", "https://example.test/alpha.csv", "A."), alpha_csv),
            ],
            DenyList(),
        )
        checks.eq("a non-ship build is not marked shipped", non_redistributable.manifest["shipped"], False)
        checks.raises(
            "shipping a redistributed:false source is refused",
            lambda: build(
                [
                    (
                        fixture_source(
                            "Alpha Net",
                            "https://example.test/alpha.csv",
                            "A.",
                            redistributed=False,
                        ),
                        alpha_csv,
                    )
                ],
                DenyList(),
                shipped=True,
            ),
            "redistributed: false",
        )
        shipped_result = build(
            [(fixture_source("Alpha Net", "https://example.test/alpha.csv", "A."), alpha_csv)],
            DenyList(),
            shipped=True,
        )
        checks.eq("a ship build is marked shipped", shipped_result.manifest["shipped"], True)

        # Emitted files: written where the caller asked, hashes match.
        out_dir = work / "artifacts"
        written = write_outputs(result, out_dir)
        checks.eq("three artifacts are written", len(written), 3)
        checks.eq(
            "written blob matches",
            (out_dir / "server-geofeed.bin").read_bytes(),
            result.blob,
        )
        checks.eq(
            "written Rust matches",
            (out_dir / "server_geofeed_tables.rs").read_text(encoding="utf-8"),
            result.rust,
        )
        written_manifest = json.loads(
            (out_dir / "manifest.json").read_text(encoding="utf-8")
        )
        checks.eq(
            "written manifest matches",
            written_manifest["outputs"],
            result.manifest["outputs"],
        )

        # Hard errors: empty input, all-malformed input, no local sources.
        empty_csv = work / "empty.csv"
        empty_csv.write_text("# comments only\n", encoding="utf-8")
        checks.raises(
            "empty input is a hard error",
            lambda: build([(alpha_spec, empty_csv)], DenyList()),
            "no usable rows",
        )
        malformed_only = work / "malformed-only.csv"
        malformed_only.write_text(
            "1.2.3.0/24,USA,US-VA,Nowhere,00000\n1.2.3.0/24\n", encoding="utf-8"
        )
        checks.raises(
            "all-malformed input is a hard error",
            lambda: build([(alpha_spec, malformed_only)], DenyList()),
            "no usable rows",
        )
        checks.raises(
            "all-cityless input is a hard error",
            lambda: build([(alpha_spec, gamma_csv)], DenyList()),
            "no usable rows",
        )
        checks.raises(
            "no local source is a hard error",
            lambda: build([(alpha_spec, None)], DenyList()),
            "no geofeed inputs",
        )
        checks.raises(
            "denied operator leaves no usable rows",
            lambda: build(
                [(alpha_spec, alpha_csv)],
                _deny_operator("Alpha Net"),
            ),
            "no usable rows",
        )

        # Licensed-database options are gone, with guidance.
        mmdb_text = refusal("--mmdb")
        checks.true("--mmdb refusal names the owner decision", "owner decision" in mmdb_text)
        checks.true("--mmdb refusal names --geofeed", "--geofeed" in mmdb_text)
        geolite_text = refusal("--geolite-csv")
        checks.true(
            "--geolite-csv refusal names the sources manifest",
            "sources.json" in geolite_text,
        )

        # Precedence helper directly: the earlier row wins inside one source.
        within = resolve_ranges(
            [
                (ipv4_int("198.18.0.0"), ipv4_int("198.18.0.255"), 0, 24, 0, 0),
                (ipv4_int("198.18.0.0"), ipv4_int("198.18.0.255"), 1, 24, 0, 1),
            ]
        )
        checks.eq(
            "within one source the earlier row wins",
            within,
            [(ipv4_int("198.18.0.0"), ipv4_int("198.18.0.255"), 0)],
        )
        specific = resolve_ranges(
            [
                (ipv4_int("198.18.0.0"), ipv4_int("198.18.0.255"), 0, 24, 1, 0),
                (ipv4_int("198.18.0.128"), ipv4_int("198.18.0.255"), 1, 25, 2, 0),
            ]
        )
        checks.eq(
            "a more specific prefix is split out of a broader one",
            specific,
            [
                (ipv4_int("198.18.0.0"), ipv4_int("198.18.0.127"), 0),
                (ipv4_int("198.18.0.128"), ipv4_int("198.18.0.255"), 1),
            ],
        )
        checks.eq(
            "adjacent same-place pieces coalesce",
            resolve_ranges(
                [
                    (ipv4_int("198.19.0.0"), ipv4_int("198.19.0.127"), 0, 25, 0, 0),
                    (ipv4_int("198.19.0.128"), ipv4_int("198.19.0.255"), 0, 25, 0, 1),
                ]
            ),
            [(ipv4_int("198.19.0.0"), ipv4_int("198.19.0.255"), 0)],
        )
    finally:
        shutil.rmtree(work, ignore_errors=True)

    for failure in checks.failures:
        print(f"SELFTEST FAIL: {failure}")
    if checks.failures:
        print(f"SELFTEST FAILED: {len(checks.failures)} of {checks.count} checks")
        return 1
    print(f"SELFTEST OK: {checks.count} checks passed")
    return 0


def _deny_operator(name: str) -> DenyList:
    """A deny list that drops one operator, for selftest assertions."""
    deny = DenyList()
    deny.operators.append(name)
    return deny


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Build the offline server-city table from public operator RFC 8805 "
            "geofeeds (attribution + takedown list; no licensed database)."
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=OWNER_DECISION,
    )
    parser.add_argument(
        "--selftest",
        action="store_true",
        help="run fixture-based assertions and exit (no licensed data, no network)",
    )
    parser.add_argument(
        "--sources",
        metavar="JSON",
        default=str(SOURCES_JSON),
        help=f"configured source list (default: {display_path(SOURCES_JSON)})",
    )
    parser.add_argument(
        "--geofeed",
        action="append",
        default=[],
        metavar="NAME=PATH",
        help="local CSV for a configured source; repeatable",
    )
    parser.add_argument(
        "--deny-file",
        action="append",
        default=[],
        metavar="FILE",
        help=(
            "takedown/deny list: one IPv4 prefix or operator name per line; "
            f"repeatable (default list {display_path(DENY_FILE)} always applies)"
        ),
    )
    parser.add_argument(
        "--out",
        metavar="DIR",
        default=str(DEFAULT_OUT),
        help=f"build-evidence output directory (default: {display_path(DEFAULT_OUT)})",
    )
    parser.add_argument(
        "--ship",
        action="store_true",
        help=(
            "also write the committed client copy under client-gui/ "
            "(requires every used source to be redistributable)"
        ),
    )
    parser.add_argument(
        "--geolite-csv",
        metavar="CSV",
        help="refused: MaxMind/GeoLite2 data is excluded by the owner decision",
    )
    parser.add_argument(
        "--mmdb",
        metavar="MMDB",
        help="refused: MaxMind/GeoLite2 data is excluded by the owner decision",
    )
    args = parser.parse_args(argv)

    if args.selftest:
        return selftest()
    for option, value in (("--mmdb", args.mmdb), ("--geolite-csv", args.geolite_csv)):
        if value:
            print(refusal(option), file=sys.stderr)
            return 2

    print(OWNER_DECISION)
    try:
        sources = load_sources(Path(args.sources))
        overrides = parse_overrides(args.geofeed, sources)
        deny = build_deny(args.deny_file)
        inputs = resolve_inputs(sources, overrides)
        if not any(path is not None for _, path in inputs):
            raise BuildError(
                "no local geofeed files: pass --geofeed NAME=PATH (a configured "
                f"source name) or place a feed at {display_path(GEOFEED_DIR)}/"
                "<name>.csv"
            )
        result = build(inputs, deny, shipped=args.ship)
        out_dir = Path(args.out)
        write_outputs(result, out_dir)
        shipped_paths = write_ship_outputs(result) if args.ship else []
    except BuildError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    for report in result.reports:
        print(
            f"input {report.name}: {report.file} "
            f"({report.rows} rows, {report.kept} kept, {report.malformed} malformed, "
            f"{report.no_city} cityless, {report.ipv6} IPv6 skipped, "
            f"{report.denied} denied; as-of {report.as_of})"
        )
        print(f"  attribution: {report.attribution}")
        print(f"  licence/status: {report.license_status} ({'redistributed' if report.redistributed else 'NOT redistributable'})")
    for report in result.skipped:
        print(f"skipped {report.name}: no local geofeed file")
    if deny.files:
        print(
            "deny list: "
            + ", ".join(
                f"{report.file} ({report.entries} entries)" for report in deny.files
            )
            + f"; dropped {deny.dropped_rows} rows"
            + (
                f" and {len(deny.dropped_sources)} operator(s)"
                if deny.dropped_sources
                else ""
            )
        )
    else:
        print("deny list: none")
    print(
        f"wrote {display_path(out_dir / 'server-geofeed.bin')} "
        f"({len(result.blob)} bytes, {len(result.records)} ranges, "
        f"{len(result.places)} places) and "
        f"{display_path(out_dir / 'server_geofeed_tables.rs')}"
    )
    for path in shipped_paths:
        print(f"shipped {display_path(path)}")
    print(
        "display-only: this table never feeds chooser_point, relay ranking or "
        "any measured claim."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
