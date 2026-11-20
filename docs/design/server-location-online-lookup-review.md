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

## Required labels (if a build-time table ships)

- Geofeed: "Server location: operator geofeed — US-VA, Manassas (published by
  the network operator, shown as-is)".
- GeoLite2: "Server location: city estimate ±20 km (MaxMind GeoLite2, build
  DATE)". Never a bare city; never on the map as a verified dot.

## Failure/quality notes

- False precision: GeoJS reports `accuracy` radii (20/200/1000 km) but no data
  version; its own GCP answer was ~45 km off; 8.8.8.8 resolves to the US
  centroid. Any city label must carry the estimate qualifier + radius.
- Coverage: GeoLite2 was wrong for an adjacent G-Core /24 (LU vs Amsterdam),
  so it improves but does not guarantee hosting accuracy.
- The route visualizer's hop timeline is simulated (random IPs + hardcoded city
  graph); its endpoints' geo comes from GeoJS — cite it only as a UI precedent,
  not as evidence of a live sensor.
