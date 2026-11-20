# Operator geofeed build inputs (server city table)

This directory configures the **offline server-city table** built by
`infra/scripts/build-server-city.py`. The table ships inside the Windows GUI as
a committed data file plus a generated Rust table; the shipped client only reads
those committed artifacts and never calls a network service.

## Owner decision: no licensed database, operator geofeeds only

On **2026-11-20** the owner decided the licence route for city-level server
location:

- **No MaxMind/GeoLite2 data.** The CC BY-SA 4.0 terms were declined; no
  `.mmdb`, no GeoLite CSV, and no table derived from them is used or shipped.
- **Public operator geofeeds (RFC 8805) only**, from the configured list in
  [`sources.json`](sources.json), each with an attribution line and a
  licence/status note.
- Everything is **shown as-is** as the operator publishes it, with the operator
  named in the route card's source line.
- A **takedown/deny list** ([`deny.txt`](deny.txt)) removes an operator's whole
  feed or individual prefixes at build time.
- The table is **display-only**: it never feeds `chooser_point`, relay ranking
  or any measured claim.

Later measured sources can join the same table; geofeeds come first.

## First source: G-Core Labs (AS199524)

- Public feed URL: <https://geofeed.gcore.lu/IP-Range.csv>
- Operator: G-Core Labs S.A. (AS199524)
- Attribution: "Server city data published by G-Core Labs S.A. in its public
  RFC 8805 geofeed (https://geofeed.gcore.lu/IP-Range.csv); shown as-is."
- The feed header states its own generation date
  (`#AS199524 Geofeed. Was generated at ... UTC`); the build records that as the
  label's **as-of** date (2026-11-19 for the committed table).
- The feed states no licence. It is consumed under the owner decision above and
  can be removed from the next build with a deny entry.

The CSV is **not committed** (see `.gitignore` here): feed data stays local.
Download it at build time or drop your local copy in this directory:

```powershell
Invoke-WebRequest https://geofeed.gcore.lu/IP-Range.csv -OutFile target\gcore-geofeed.csv
```

## Build and ship the table

```powershell
# Fixture-based pipeline checks (no licensed data, no network):
py -3 infra/scripts/build-server-city.py --selftest

# Generate from the local G-Core CSV and write the committed client copy:
py -3 infra/scripts/build-server-city.py --geofeed G-Core=target\gcore-geofeed.csv --ship
```

Outputs:

| Path | Committed? | What |
|---|---|---|
| `client-gui/data/server-geofeed.bin` | yes | range blob: `start u32 LE \| end u32 LE \| place-index u16 LE` (10 bytes/record), sorted, non-overlapping |
| `client-gui/src/server_geofeed_tables.rs` | yes | generated place/source table with the per-source attribution header |
| `client-gui/data/server-geofeed.sources.json` | yes | manifest: source URLs, local hashes, as-of/fetch dates, counts, attribution, licence/status, `redistributed` marker, deny list |
| `target/server-location-city/` | no (gitignored) | the same three artifacts as build evidence (`--out` overrides) |

`--ship` refuses any source marked `"redistributed": false`; a build without
`--ship` stays under `target/`. Pass `--out DIR` to write the evidence elsewhere.
Source manifests and feeds that are missing a local file are recorded as skipped;
at least one source must resolve.

## Adding a source

Append one object to `sources.json`:

```json
{
  "name": "Operator Short Name",
  "operator": "Legal Operator Name",
  "asn": 64500,
  "url": "https://example.net/geofeed.csv",
  "local": "infra/geo/geofeeds/operator-short-name.csv",
  "attribution": "Server city data published by Legal Operator Name in its public RFC 8805 geofeed (https://example.net/geofeed.csv); shown as-is.",
  "license_status": "public operator geofeed; <licence or 'no licence stated'>",
  "redistributed": true,
  "status": "active"
}
```

`attribution` and `license_status` are mandatory: a source without an
attribution line is a hard build error, never silently unattributed output.
`redistributed` must be `true` before the data may be committed and shipped.
Then either place the feed at the conventional path or pass
`--geofeed "Operator Short Name=path\to.csv"` (the name must match).

## Takedown policy

An operator (or the owner) can require removal of a feed or a prefix at any
time. Add one line per removal to [`deny.txt`](deny.txt) - a prefix (all feed
rows intersecting it are dropped) or an operator name / `AS<number>` (the whole
feed is dropped) - rebuild with `--ship`, and commit the regenerated table. The
attribution header and the committed source manifest record the applied deny
list. The derived city table contains no licensed database data, so there is no
third-party redistribution obligation beyond the operator attribution above.

## Parser contract (RFC 8805)

Five columns: `prefix,country,region,city,postal`.

```csv
prefix,country,region,city,postal
203.0.113.0/24,US,US-VA,Manassas,20109
```

- A line starting with `#` is a comment (the header line of a feed is one);
  blank lines are ignored.
- An optional header row (`prefix,network,...`) is skipped.
- Malformed rows are counted and skipped; rows without a city are counted as
  "cityless" and skipped (a city table cannot place them); valid IPv6 rows are
  counted separately and skipped because the shipped range format is IPv4-only.
  Every count lands in the manifest so a bad or unsupported feed is visible
  instead of silently lost.
- Prefixes are validated strictly and host bits are masked off. The postal
  column is parsed for format completeness and not shipped.
- Precedence: longest prefix wins; at equal length the earlier source in
  `sources.json` wins; within one source the earlier row wins.

## Verify without licensed data

```powershell
py -3 infra/scripts/build-server-city.py --selftest
```

`--selftest` writes fixtures under `target/` and asserts parsing, comments,
malformed rows, deny/takedown handling, attribution enforcement, prefix math,
merge precedence, the as-of dateline and the emitted files. It needs no feed
data and no network.
