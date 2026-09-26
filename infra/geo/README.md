# LightSpeed Placement Geography Catalogs

Operator-maintained reference data used only by the relay placement recommender.
Nothing in this directory is executed at runtime, and no source code reads it
except the placement recommender when an operator runs it. These are static
catalogs that map session geography to a coarse region, and list candidate relay
locations with their current cost basis.

## Files

| File | Purpose |
|------|---------|
| `regions.json` | Region definitions, country to region mapping, relay region aliases, and the coordinates of the live fleet. |
| `candidates.json` | Recommender parameters and the candidate relay locations with coordinates and cost basis. |
| `README.md` | This document. |

## Units and conventions

- All `lat` and `lon` values are decimal degrees in WGS 84. Latitude is positive
  north of the equator and negative south. Longitude is positive east of the
  prime meridian and negative west, in the range -180 to 180.
- Region keys are short lowercase slugs. Region keys are stable identifiers,
  not display names. The human readable name lives in `regions.<key>.label`.
- Country keys are uppercase ISO 3166-1 alpha-2 codes.
- Latency knobs in `candidates.json` `params`: `proximity_ms_floor` and
  `distinct_min_ms` are milliseconds of one-way latency, `redundancy_band` is a
  dimensionless fraction, and `redundancy_min_gain` is in the same
  demand-weighted units as the gains it floors.
- `window_secs` is a duration in seconds.

## regions.json schema

```json
{
  "schema_version": 1,
  "regions": { "<key>": {"label": "<human label>", "lat": <num>, "lon": <num>} },
  "countries": { "<ISO2 uppercase>": "<region key>" },
  "region_aliases": { "<existing relay region string>": "<region key>" },
  "relays": { "<node_id>": {"lat": <num>, "lon": <num>} }
}
```

- `regions` is the coarse region set. The `lat` and `lon` are approximate
  centroids used only for coarse grouping and sanity checks, not server
  positions.
- `countries` maps a session or operator country to one region key. Every value
  must exist as a key in `regions`.
- `region_aliases` maps the free-form region strings already present in the
  signed relay registry to a canonical region key. The five strings currently in
  the fleet registry are covered: `us-west` and `us-east` map to `na`,
  `eu-central` maps to `eu`, and `ap-southeast` and `ap-northeast` map to
  `apac`. Common cloud region codes such as `us-east-1` and `eu-west-1` are also
  included so future registrations resolve without a catalog change.
- `relays` carries the real fleet node ids and their city coordinates. This
  mirrors the live registry, which currently lists `relay-lax-1`,
  `relay-ewr-1`, `relay-sgp-1`, `relay-fra`, and `relay-nrt`.

Every region key referenced anywhere in this file, and every region key
referenced by `candidates.json`, must exist as a key in `regions`. This is the
one hard integrity rule of the catalog.

## candidates.json schema

```json
{
  "schema_version": 1,
  "params": { "...": "see below" },
  "candidates": [
    {"id": "<slug>", "provider": "<name>", "region": "<region key>", "lat": <num>, "lon": <num>, "free_tier": true, "viable": true, "note": "<short>"}
  ]
}
```

- `params` holds the tuning knobs for the recommender. They are recorded here so
  a recommendation can be reproduced from the same data the operator used.
  `alpha` is the prior strength that shrinks sparse cells toward the region
  mean. `window_secs` is the observation window (604800 is seven days).
  `min_window_sessions` and `min_cell_sessions` gate which cells are allowed to
  influence a decision. `stability_runs` is how many consecutive runs must agree
  before a recommendation is acted on. `add_margin` and `move_margin` are the
  improvement thresholds for adding a node and for moving one. `redundancy_weight`
  down-weights a candidate that duplicates existing coverage.
  `move_coverage_keep` is the fraction of coverage that must survive a move.
  `proximity_ms_floor` is the two-leg latency improvement a candidate must
  reach before a cell counts as new coverage. `redundancy_band` and
  `distinct_min_ms` define a distinct second path: a candidate must sit within
  the band of the single in-band provider and at least `distinct_min_ms` away
  from it. `redundancy_min_gain` floors the demand-weighted redundancy a
  served-region candidate needs before an `ADD_REDUNDANT` is emitted.
- `candidates[].id` is a stable slug unique within the file.
- `candidates[].free_tier` is `true` only when there is a genuine ongoing $0
  basis as of 2026, meaning an Always Free allowance or an active renewable
  open-source credit program. It is `false` for paid locations that only make
  sense with sponsorship.
- `candidates[].viable` is `true` when the location is real and currently
  obtainable, and `false` when availability or cost basis is genuinely uncertain.
  `free_tier` describes cost. `viable` describes existence and obtainability. A
  candidate can be viable and not free, but a candidate that is neither free nor
  sponsor-viable should not be listed at all.
- `note` records the honest caveat for that candidate, including whether the $0
  basis is guaranteed or gated.

## Region set choice and rationale

The catalog uses eight regions: `na`, `latam`, `eu`, `mena`, `africa`, `sasia`,
`apac`, and `oceania`.

The governing constraint is statistical stability with sparse data. Early
placement decisions rest on a small number of sessions, often a handful per city
or ISP. A fine-grained region set (metro level, or country level) would give
each cell too few samples to estimate latency reliably, and the recommender would
chase noise. Pooling observations into continent-scale buckets raises the
samples per bucket, and the `alpha` prior then shrinks whatever remains thin
toward the region mean. Eight buckets is the coarsest set that still separates
the physically and operationally distinct latency basins.

The set also has to line up with three external facts:

1. The five relay region strings already in the signed registry resolve cleanly
   onto `na`, `eu`, and `apac` through `region_aliases`.
2. Ocean and continental boundaries are the dominant latency boundaries, so
   grouping by them keeps within-region latency far below cross-region latency
   in almost all cases.
3. The required country coverage spans all eight buckets, so no required country
   maps to a region that does not exist.

Judgement calls worth recording:

- Turkey (`TR`), Iran (`IR`), and the Levant are grouped into `mena`, matching
  the common game-server peering practice for that corridor.
- Russia (`RU`) and Ukraine (`UA`) are grouped into `eu`. Most population and
  the major carrier exchanges sit in European Russia, and a Moscow or
  St Petersburg placement serves that traffic far better than an Asia-Pacific
  bucket would.
- Egypt (`EG`) and Morocco (`MA`) are grouped into `africa` rather than `mena`.
  Both are North African and both have Mediterranean and Red Sea cable paths
  that make a `mena` placement plausible, but the current candidate set has no
  North African location, so they are grouped with the African candidates that
  do exist. This is a deliberate simplification and can be revisited if a North
  African candidate is added.

## Candidate viability caveats

The zero-cost mandate in `wat/rules.md` means only candidates with `free_tier:
true` can actually be provisioned tonight. Everything else is recorded as a
sponsor-viable option and is filtered out of a strict zero-cost recommendation.
The claims below were checked against provider pages in September 2026. They are
operator-maintained and will drift as providers change terms.

### Oracle Cloud Infrastructure Always Free (`free_tier: true`)

OCI Always Free is a genuine ongoing $0 tier, but with two hard limits. First,
Always Free compute can be provisioned only in the tenancy home region, and the
home region is immutable after tenancy provisioning. A single tenancy therefore
yields free compute in exactly one region, so each OCI candidate region in this
catalog would need its own tenancy if more than one were wanted. Second, the
Ampere A1 allowance was cut in June 2026 from 4 OCPU and 24 GB to an equivalent
of 2 OCPU and 12 GB. The two AMD micro instances are unchanged. Out-of-host
capacity for A1 is common and is documented by Oracle.

Sources:

- Always Free resources: https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm
- Free tier overview and home-region restriction: https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier.htm
- Immutable home region: https://docs.oracle.com/en-us/iaas/Content/Identity/Tasks/managingregions.htm
- Public region list: https://www.oracle.com/cloud/public-cloud-regions/
- A1 reduction reporting, 2026-06/07: https://www.infoq.com/news/2026/07/oracle-cloud-free-tier-limits/ and https://www.heise.de/en/news/Oracle-halves-free-cloud-resources-11334516.html

### Google Cloud Always Free (`free_tier: true` for `na` only)

GCP Always Free compute is a single e2-micro VM in one of `us-west1`,
`us-central1`, or `us-east1`. There is no always-free compute outside the United
States, so every non-US GCP entry here is `free_tier: false`. The Premium tier
egress line is 1 GB per month from North America. A separate Standard tier
allowance of 200 GB per month exists, and the two are distinct: Always Free
usage limits do not apply to Standard tier. Google's own pages describe the
Standard tier allowance both as per region and as per account across regions, so
treat the exact scope as unsettled.

Sources:

- Free cloud features, retrieved 2026-09-19: https://docs.cloud.google.com/free/docs/free-cloud-features
- Standard tier 200 GB announcement, 2023-09-20: https://cloud.google.com/blog/products/networking/standard-tier-network-now-includes-200-gb-data-transfer-per-month

### Amazon Web Services (`free_tier: false`)

There is no ongoing free EC2 instance. AWS Always Free covers services such as
Lambda, not EC2. For accounts created on or after 2025-07-15, the EC2 offer is a
6-month credit plan ($100 on signup plus up to $100 from activities, expiring at
6 months or when credits run out). Accounts created before that date keep the
legacy 12-month t2.micro or t3.micro offer. This expires, so AWS locations are
recorded as sponsor-viable only.

Sources:

- Free tier update, 2025-07-15: https://aws.amazon.com/blogs/aws/aws-free-tier-update-new-customers-can-get-started-and-explore-aws-with-up-to-200-in-credits/
- EC2 free tier usage: https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/ec2-free-tier-usage.html

### Microsoft Azure (`free_tier: false`, not listed)

Azure's free offer is a 12-month B1s or B2pts v2 or B2ats v2 allowance plus a
30-day credit and always-free services that do not include a VM. Because it
expires after 12 months and has no ongoing free VM, Azure is not listed as a
candidate. This is recorded here so the absence is deliberate rather than an
oversight.

Source: https://azure.microsoft.com/en-us/pricing/free-services

### Fly.io (`free_tier: false`, not listed)

Fly.io removed its free allowance for new organizations on 2024-10-07, and its
discontinued-plans page was still current as of 2026-08-14. New organizations
are pay-as-you-go with no monthly credit, the smallest always-on machine is
roughly $2 per month, and UDP additionally requires a dedicated IPv4 at about
$2 per month. Fly.io is therefore not a $0 candidate. It is noted because the
task specifically called out this change.

Source: https://fly.io/docs/about/discontinued-plans/

### Vultr Free Tier (`free_tier: true`)

Vultr runs an application-gated Free Tier: a free 1 vCPU, 512 MB, 10 GB instance
in Miami, Seattle, or Frankfurt. It is real and ongoing, but access is limited,
randomized, and capacity-bound, so it is not guaranteed to be granted. The
candidate is listed as viable with that caveat in the note.

Source: https://www.vultr.com/free-tier-program/

### Sponsorship-backed candidates (`free_tier: false`, `viable: true`)

Where no ongoing $0 tier exists, provider-neutral sponsor paths are the fallback.
The most relevant open-source program is DigitalOcean Credits for Projects,
which grants renewable credits to individual open-source projects (roughly $60
per year at 100 or more GitHub stars, up to $20k per year for large projects)
and is renewed annually. AWS Cloud Credits for Open Source and Cloudflare Project
Alexandria are comparable but are not raw-VM relays. Hetzner offers only a
one-time 50 EUR integration credit. Oracle for Startups, Google Cloud for
Startups, Microsoft for Startups, and AWS Activate are company-oriented and not
available to an individual open-source project, so they are not counted here.

Sources:

- DigitalOcean Credits for Projects: https://www.digitalocean.com/open-source/credits-for-projects
- AWS Cloud Credits for Open Source: https://aws.amazon.com/blogs/opensource/aws-cloud-credits-for-open-source-projects-affirming-our-commitment/
- Cloudflare Project Alexandria: https://www.cloudflare.com/lp/project-alexandria/
- Hetzner open-source credit: https://foropensource.com/company/hetzner/

### Platform limits that rule out other free tiers

Serverless and platform-as-a-service free tiers were considered and rejected for
this use case because a relay needs a persistent public raw UDP socket.
Cloudflare Workers and Durable Objects, Google Cloud Run, Render, Railway,
Northflank, and Koyeb either expose only HTTP or WebSocket ingress, scale to zero
when idle, or cannot publish a UDP port without extra infrastructure and cost.
None of them can host the relay as specified, so none are listed as candidates.

## Operator maintenance

This catalog is plain operator-maintained data. It is edited by hand when the
fleet grows, when a provider changes its free-tier terms, or when a country
mapping needs correction. It is not generated, it is not fetched at runtime, and
it is not a source of truth for billing. Before adding or moving a candidate,
re-check the provider page and update both the `note` and the sources above.
