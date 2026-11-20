# Server city build inputs (operator-placed, never committed)

This directory is the build-time drop point for
`infra/scripts/build-server-city.py`. Nothing here is executed at runtime or
read by the shipped client; the script emits only under
`target/server-location-city/` (gitignored).

## What goes here

RFC 8805 geofeed CSV files with five columns:

```csv
prefix,country,region,city,postal
203.0.113.0/24,US,US-VA,Manassas,20109
```

- A line starting with `#` is a comment; blank lines are ignored.
- An optional header row (`network,country,region,city,postal`) is skipped.
- Malformed rows are counted and skipped; the run's `manifest.json` records the
  counts so an operator can see a bad feed instead of silently losing it.

Small fixture files are fine here; real feeds stay local. `.mmdb` files are
**not** accepted: the script deliberately does not decode MaxMind's binary
format (no reader dependency, no shell-out), so `--mmdb` is refused with
conversion guidance.

## Owner gate (O1) — do not ship derived data without it

Shipping a city table derived from either input requires an explicit owner
decision, recorded in `docs/design/server-location-online-lookup-review.md`:

- **MaxMind GeoLite2 City:** accept the GeoLite2 End User Licence Agreement and
  CC BY-SA 4.0 — attribution, ShareAlike on the derived table, and a
  keep-current refresh cadence (≤ 30 downloads/day).
- **Operator geofeed** (for example G-Core's RFC 8805 feed): the feed states no
  licence, so shipping it needs the network operator's written permission.

Until that decision exists, the pipeline is a scaffold only: the route card
stays "hosting network — unknown" with the free-database caption, and no online
geolocation is added.

## Converting GeoLite2-City to the accepted CSV

The script ingests a **pre-extracted** CSV, not the `.mmdb`. The owner converts
MaxMind's Blocks and Locations files with MaxMind's own tooling, flattening them
into one row per network:

```
network,country,region,city,postal
```

Then build:

```powershell
py -3 infra/scripts/build-server-city.py --geolite-csv GeoLite2-City-flat.csv
```

## Verify without licensed data

```powershell
py -3 infra/scripts/build-server-city.py --selftest
```

`--selftest` writes its own fixtures under `target/` and asserts parsing,
comments, malformed rows, prefix math, merge precedence (geofeed beats GeoLite)
and the emitted artifacts. It needs no licensed data and no network.
