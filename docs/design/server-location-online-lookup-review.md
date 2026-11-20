# Online geolocation fallback: adversarial review (2026-11-20)

> Three independent reviews (`sec-ops`, `net-eng-2`, `architect-2`) + live tests.
> Read-only panel; decision below. Applies to the proposal to call
> `get.geojs.io` from the client for hosting-network servers, and to the
> alternatives.

## Verdict

**Do not ship the runtime GeoJS lookup.** It contradicts our published privacy
policy ("only three network connections", no third-party services, game-server
IP explicitly not collected), has no SLA or durable grant, and is not
free-at-scale by the filter adopted on 2026-11-19.

**But the panel also removed the excuse for staying wrong.** GeoJS is a thin
mirror of **MaxMind GeoLite2 City**: an offline GeoLite2-City build
(2026-11-17) reproduced GeoJS *exactly* on 12 probed IPs, including the
Manassas/20109 answer and its `accuracy` radii. MaxMind got it right because it
ingested G-Core's RFC 8805 geofeed. So the city-level answer is obtainable
offline at build time from free data — it is a **licence/terms decision**, not a
paid-data problem:

| Path | Cost | Accuracy | Gate |
|---|---|---|---|
| G-Core operator geofeed (build-time, display-only) | $0 | exact `US-VA, Manassas, 20109` | written permission; feed states no licence |
| MaxMind GeoLite2 City (build-time, offline table) | $0 | Manassas/20109; wrong on some /24s (e.g. 178.107.18.0/24 says LU vs NL) | accept GeoLite EULA + CC BY-SA attribution/ShareAlike, refresh cadence (≤30 downloads/day) |
| Runtime GeoJS call | $0 cash | same as GeoLite2 | **rejected** (policy, no SLA, privacy, licence ambiguity) |
| Client / relay traceroute | $0 | hop/lower-bound only, no city | ICMP/TTL filtering observed; weak value |
| Stay "hosting network — unknown" | $0 | none | no gate |

Provider tests: ipwho.is and ip-api returned **Tokyo/JP** for the G-Core IP
(wrong); ipinfo keyless matched every G-Core /24 probed but is an
account/terms decision; DB-IP City Lite remained wrong. Only MaxMind-derived
data (GeoLite2/geojs) and the operator feed got Manassas right.

## Decision

1. **No runtime third-party geolocation in the client.** Keep the offline
   ladder and the honest "hosting network — unknown" label.
2. **Keep GeoJS in the route visualizer only** (browser, user-initiated, one
   page) and add a "location data by GeoJS / MaxMind GeoLite" note there.
3. **Owner gate O1:** approve a build-time offline city table from either
   (a) the G-Core operator geofeed after written permission, or (b) **MaxMind
   GeoLite2 City** (free account; GeoLite EULA + CC BY-SA 4.0 — attribution
   and ShareAlike on the derived table, keep-current obligation). Either ships
   display-only, never feeding `chooser_point` or the ranking.
4. **No-gate work now:** add the caption "free IP databases are unreliable for
   hosting ranges" under the location line; and fix the pre-existing privacy
   policy gap — the GUI's update check (`api.github.com`) is a third-party
   connection not listed in `docs/privacy.md`.
5. **If (3) is declined:** stay "unknown" with the caption; no online fallback.

## Implementation status (2026-11-20)

- **Done - honesty caption (no gate):** the route card shows a second caption
  line, "Free IP databases are unreliable for hosting ranges.", under the
  location line whenever the server's place is `Hosting` or `Unknown` (the two
  cases where the client could not place it). New locale key
  `route.location_hosting_hint` in all 9 `client-gui/locales/*.toml`; `ja`,
  `ko` and `zh-Hans` keep the English string because the bundled CJK font
  subsets are glyph-tested against them. Covered by a GUI test that also
  asserts a cloud region and a country estimate carry no hint.
- **Done - privacy honesty fix (no gate):** `docs/privacy.md` no longer claims
  the client's only connection is the relay pair. The GUI's update check
  (`https://api.github.com/repos/ShibbityShwab/lightspeed/releases/latest`) is
  now listed as a user-initiated, non-blocking request that sends no
  identifiers beyond the HTTP request itself (app User-Agent; GitHub sees the
  client IP), with the optional release download following only a second
  click. `docs/data-dictionary.md` states that this call is not part of the
  telemetry report. No new online geolocation is added.
- **DECIDED - owner gate O1 (2026-11-20): no MaxMind/GeoLite2.** The CC BY-SA
  4.0 route was rejected. The accepted route is **public operator RFC 8805
  geofeeds only**, per-operator, with attribution, shown as-is, and a
  takedown/deny list. No licensed database is used or redistributed.
- **Done - build-time operator-geofeed pipeline (2026-11-20):**
  `infra/scripts/build-server-city.py` was rewritten for that decision. It
  ingests the configured sources in `infra/geo/geofeeds/sources.json` (name,
  operator, ASN, public URL, attribution, licence/status note, `redistributed`
  marker), each local feed resolved with `--geofeed NAME=PATH` or the
  conventional drop point, validates RFC 8805 rows (comments, optional header,
  malformed/cityless accounting; valid IPv6 rows counted separately because the
  blob is IPv4-only), applies `deny.txt`/`--deny-file` (prefixes or operators),
  and resolves overlaps by longest prefix then source order. It emits
  `start u32 | end u32 | place-index u16` + a generated Rust table + a manifest
  with source URLs, hashes, as-of/fetch dates, row counts, attribution strings,
  `redistributed` flags and the applied deny list. A source without an
  attribution line is a hard error. `--selftest` covers parsing/comments/
  malformed, deny handling, attribution in the output header, prefix math,
  merge precedence, the as-of dateline and the emitted files (109 checks).
- **Done - the G-Core table now ships, display-only (2026-11-20):** the
  committed artifacts are `client-gui/data/server-geofeed.bin` (603 ranges),
  `client-gui/src/server_geofeed_tables.rs` (92 places, 1 source: G-Core
  AS199524, as-of 2026-11-19) and `client-gui/data/server-geofeed.sources.json`.
  The client gained `Place::Geofeed { provider, country, region, city, source,
  as_of }`, resolved in the ladder after cloud and before hosting. The route
  card shows "Server location: operator geofeed — Manassas, US-VA" with
  "(G-Core, as-of 2026-11-19)" and a "Shown as-is from the operator's published
  geofeed." line (feed URL on hover); the free-database caption is unchanged
  and only appears for `Hosting`/`Unknown`. `display_point` and
  `chooser_point` both stay `None` for a geofeed place until a measurement
  validates it, so ranking is untouched. Locale keys exist in all 9 catalogs
  (`ja`, `ko`, `zh-Hans` keep English), and NOTICE/`docs/privacy.md` carry the
  per-operator attribution, the "shown as-is" statement and the takedown
  policy. The table covers the G-Core US-Virginia server whose DB-IP country
  was JP.
- **Done - dataset expanded to 7 verified operator feeds (2026-11-20):** a
  due-diligence pass added Hetzner (AS24940), DigitalOcean (AS14061), Vultr
  (AS20473), Linode/Akamai (AS63949), Contabo (AS51167) and Equinix Metal
  (AS54825) next to G-Core. Every feed is declared in the operator's RIR object
  (RIPE RDAP `rel=geofeed` / ARIN `Geofeed` remark) or served on the operator's
  own domain (DigitalOcean, no RIR declaration), parses as RFC 8805, and is
  recorded in `sources.json` with URL, as-of, row counts and announced-prefix
  coverage; OVH, Leaseweb, Scaleway and Oracle were rejected (no operator-owned
  feed) with reasons. The committed table is now **7 sources / 237 places /
  11,948 ranges** (119,480-byte blob) and its generated Rust header and manifest
  carry every attribution line and per-source as-of date.
- **Still open:** measured sources (relay/client traceroute, hop geolocation)
  joining the same table once one is validated; more operators' geofeeds need a
  `sources.json` entry plus a local feed; and the table remains IPv4-only until
  a wire format/consumer for IPv6 rows exists. There is still no online
  geolocation fallback.

## Required labels (the shipped geofeed label)

- Geofeed (implemented): caption "Server location: operator geofeed —
  Manassas, US-VA", detail "(G-Core, as-of 2026-11-19)", note "Shown as-is
  from the operator's published geofeed." (source URL on hover). Never a map
  dot, never a ranking input.
- GeoLite2: **not shipped** - rejected by the 2026-11-20 owner decision.

## Failure/quality notes

- False precision: GeoJS reports `accuracy` radii (20/200/1000 km) but no data
  version; its own GCP answer was ~45 km off; 8.8.8.8 resolves to the US
  centroid. Any city label must carry the estimate qualifier + radius.
- Coverage: GeoLite2 was wrong for an adjacent G-Core /24 (LU vs Amsterdam),
  so it improves but does not guarantee hosting accuracy.
- The route visualizer's hop timeline is simulated (random IPs + hardcoded city
  graph); its endpoints' geo comes from GeoJS — cite it only as a UI precedent,
  not as evidence of a live sensor.
