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

## Current sources (verified 2026-11-20)

All seven feeds below are public RFC 8805 CSVs served by the operator (own
domain, or the operator's RIR object declaring the URL). The verification block
per source in [`sources.json`](sources.json) records the HTTP status/hash, the
RIR declaration, the as-of date, row counts and the announced-prefix coverage
spot-check.

| Source | ASN | Public feed URL | RIR declaration | as-of | IPv4 rows | countries | places |
|---|---|---|---|---|---|---|---|
| G-Core | 199524 | <https://geofeed.gcore.lu/IP-Range.csv> | RIPE `geofeed:` on 178.107.0.0/19 (RDAP rel=geofeed) | 2026-11-19 | 679 | 53 | 92 |
| Hetzner | 24940 | <https://www.hetzner.com/geofeed.csv> | RIPE RDAP rel=geofeed on 5.222.0.0/16 (HOA-EU-DC) | 2026-11-17 | 17,038 | 4 | 6 |
| DigitalOcean | 14061 | <https://www.digitalocean.com/geo/google.csv> | none (URL on the operator's own domain) | 2026-11-20 | 1,092 | 8 | 16 |
| Vultr | 20473 | <https://geofeed.constant.com/> | ARIN RDAP remarks on NET-45-32-0-0-1 / NET-108-61-0-0-1 | 2026-11-19 | 441 | 20 | 37 |
| Linode/Akamai | 63949 | <https://ipgeo.akamai.com/linode-geofeed.csv> | RIPE rel=geofeed on 212.71.244.0-255.255; ARIN remarks on LINODE nets | 2026-11-19 | 5,409 | 22 | 40 |
| Contabo | 51167 | <https://geofeed.contabo.de/geofeed.csv> | RIPE rel=geofeed on 213.136.64.0-79.255 (CONTABO) | 2026-11-13 | 540 | 8 | 10 |
| Equinix Metal | 54825 | <https://geofeed.equinixmetal.com/geofeed.csv> | RIPE rel=geofeed on 147.28.128.0/17; ARIN remarks on PACKET nets | 2026-11-20 | 177 | 20 | 36 |

The committed table currently contains **7 sources / 237 places / 11,948
ranges** (119,480-byte blob, up from 603 ranges / 92 places / 6,030 bytes with
G-Core alone). Every source is also named with its attribution line in the
generated Rust header and in the committed
`client/data/server-geofeed.sources.json` manifest.

### G-Core: snapshot versus live feed

The committed table is built from the verified **2026-11-19** snapshot
(`sha256 a5fdac6f…`, header
`#AS199524 Geofeed. Was generated at 2026-11-19 00:00:27 UTC`). The feed URL is
the one declared in the RIPE inetnum for `178.107.0.0/19` (RIPE Stat whois
`geofeed:` attribute; RIPE RDAP `rel=geofeed`). Re-fetching the URL on
2026-11-20 served the next daily regeneration: every data row was byte-identical
to the snapshot, only the `generated at` header line differed. A refresh
therefore changes the label date, not the table; do it only when also rebuilding
the committed table.

### Rejected candidates (2026-11-20)

| Candidate | Why it is not in the table |
|---|---|
| OVH SAS (AS16276) | No operator-owned feed. The only `geofeed:` attributes on OVH-announced space are customer sub-allocations (IPXO, server.pro, goodtec.lv, Acedatacenter); adding them under OVH would misattribute customer space. |
| Leaseweb (AS28753) | Same: `geofeed:` only on customer sub-allocations (IPXO, Rustelekom/Gtelcom, DNS Protection, Aventice). |
| Scaleway SAS (AS12876) | No geofeed attribute on any of its 22 announced IPv4 prefixes (RIPE RDAP), no public feed found. |
| Oracle Cloud (AS31898) | No geofeed attribute/remark on sampled ARIN objects. Oracle publishes the cloud `public_ip_ranges.json` list, which is not an RFC 8805 operator geofeed. |
| Customer feeds observed in RIR records (Hiden Enterprises in AS24940, IPXO in OVH/Leaseweb) | Declared by customers on their own sub-allocations, not by the hosting operator; attribution would be wrong. |

The full machine-readable list with per-candidate URLs and reasons is the
`rejected` array in [`sources.json`](sources.json).

## What "verified" means here

1. **Declaration.** The URL must be declared by the operator: a RIPE
   `inetnum:`/`inet6num:` `geofeed:` attribute (checked through RIPE RDAP
   `rel=geofeed`, `rdapConformance: geofeed1`, and the RIPE Stat whois data
   call) or an ARIN RDAP `Geofeed <url>` remark; for DigitalOcean the URL lives
   on the operator's own domain and no RIR declaration exists (recorded as the
   weakest provenance of the accepted set).
2. **Format.** The file parses as RFC 8805: `prefix,country,region,city,postal`.
   Malformed, cityless and IPv6 rows are counted in the manifest; they never
   reach the table.
3. **Coverage.** The feed's IPv4 ranges are compared with RIPE Stat
   `announced-prefixes` for the operator's ASN (address-weighted). The result is
   recorded per source; an operator feed that misses most of its own space is
   not complete evidence of city coverage.
4. **Publication.** The file is publicly served (HTTP 200, no auth), and the
   fetch used a descriptive User-Agent; `robots.txt` was checked on every feed
   host (none disallows the feed paths).

**RFC 9632 note:** RFC 9632 defines discovery through the RPSL `geofeed:`
attribute and the RDAP `rel=geofeed` link (plus an optional RPKI signed
geofeed). It defines **no DNS record** for geofeeds — its only DNS mention says
other geographic hints (DNS geoloc RRsets etc.) must be ignored when geofeed data
exist. There is therefore no "RFC 9632 DNS record" to query; the RIR attribute /
RDAP link is the authoritative declaration used here.

**As-of rule:** a source's as-of is the feed's own generation or last-update
statement when it has one (G-Core/Vultr/Linode headers, HTTP `Last-Modified`
preserved on the local copy); otherwise it is the local copy's UTC fetch date.
The manifest also records the exact local hash and timestamps, so a reviewer can
tell which feed bytes produced the committed table.

## Download the feeds (gitignored) and build

The CSVs are **not committed** (see `.gitignore` here): feed data stays local.
Download them into this directory under the names `sources.json` uses:

```powershell
$ua = "LightSpeed-geofeed-diligence/1.0 (+https://github.com/ShibbityShwab/lightspeed)"
$feeds = @{
  "gcore-geofeed.csv" = "https://geofeed.gcore.lu/IP-Range.csv"
  "hetzner.csv"       = "https://www.hetzner.com/geofeed.csv"
  "digitalocean.csv"  = "https://www.digitalocean.com/geo/google.csv"
  "vultr.csv"         = "https://geofeed.constant.com/"
  "linode-akamai.csv" = "https://ipgeo.akamai.com/linode-geofeed.csv"
  "contabo.csv"       = "https://geofeed.contabo.de/geofeed.csv"
  "equinix-metal.csv" = "https://geofeed.equinixmetal.com/geofeed.csv"
}
foreach ($feed in $feeds.GetEnumerator()) {
  Invoke-WebRequest -Uri $feed.Value -UserAgent $ua -UseBasicParsing `
    -OutFile (Join-Path "infra\geo\geofeeds" $feed.Key)
}
```

A configured source with no local file is recorded as skipped; at least one
source must resolve. Pass `--geofeed "Name=path\to.csv"` to override a single
source (the name must match `sources.json`), for example
`py -3 infra/scripts/build-server-city.py --geofeed G-Core=target\gcore-geofeed.csv --ship`.

```powershell
# Fixture-based pipeline checks (no licensed data, no network):
py -3 infra/scripts/build-server-city.py --selftest

# Build from the local feeds and write the committed client copy:
py -3 infra/scripts/build-server-city.py --ship
```

Outputs:

| Path | Committed? | What |
|---|---|---|
| `client/data/server-geofeed.bin` | yes | range blob: `start u32 LE \| end u32 LE \| place-index u16 LE` (10 bytes/record), sorted, non-overlapping |
| `client/src/app/location/server_geofeed_tables.rs` | yes | generated place/source table with the per-source attribution header |
| `client/data/server-geofeed.sources.json` | yes | manifest: source URLs, local hashes, as-of/fetch dates, counts, attribution, licence/status, `redistributed` marker, deny list |
| `target/server-location-city/` | no (gitignored) | the same three artifacts as build evidence (`--out` overrides) |

`--ship` refuses any source marked `"redistributed": false`; a build without
`--ship` stays under `target/`. Pass `--out DIR` to write the evidence elsewhere.
The `--ship` build is quiet on network access: it only reads the local copies.

## Adding a source

Only add a source after verifying it the same way (declaration, RFC 8805 parse,
publication, coverage). Append one object to `sources.json`:

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
  "status": "active",
  "verified": {
    "checked": "YYYY-MM-DD",
    "as_of": "YYYY-MM-DD",
    "rir_evidence": [
      {"rir": "RIPE", "rdap": "https://rdap.db.ripe.net/ip/198.51.100.0/24", "object": "198.51.100.0 - 198.51.100.255", "attribute": "geofeed: https://example.net/geofeed.csv"}
    ]
  }
}
```

`attribution` and `license_status` are mandatory: a source without an
attribution line is a hard build error, never silently unattributed output.
`redistributed` must be `true` before the data may be committed and shipped.
Then place the feed at the configured `local` path (gitignored) or pass
`--geofeed "Operator Short Name=path\to.csv"` (the name must match). Regenerate
with `--ship` and run the gates; the committed Rust header and manifest are the
attribution.

## Takedown policy

An operator (or the owner) can require removal of a feed or a prefix at any
time. Add one line per removal to [`deny.txt`](deny.txt) - a prefix (all feed
rows intersecting it are dropped) or an operator name / `AS<number>` (the whole
feed is dropped) - rebuild with `--ship`, and commit the regenerated table. The
attribution header and the committed source manifest record the applied deny
list. The derived city table contains no licensed database data, so there is no
third-party redistribution obligation beyond the operator attribution above.
The deny path was exercised on the expanded table: a scratch list containing
`Hetzner` plus `178.107.0.0/19` dropped 17,046 rows (17,038 Hetzner + 8 G-Core)
and 1 operator, and the build still emitted a valid table.

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
