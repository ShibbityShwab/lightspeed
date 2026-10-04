# Fleet Load Re-balancing: Registry Weighting and Second Nodes

> Ops runbook for the `load_skew` detector in
> [`infra/scripts/health-anomaly.sh`](../infra/scripts/health-anomaly.sh).
> Read it when the monitor reports `load_skew`, or when a region's traffic
> keeps landing on one relay.

---

## 1. What the monitor reports

`load_skew` fires when **one relay carries a dominant share of the fleet's
relayed traffic** over the sustained window (default 3 snapshots). Two gates
must hold at once:

| Gate | Default | Meaning |
|------|---------|---------|
| Share | `SKEW_SHARE=0.5` | the top relay relayed **at least half** of all the packets the fleet relayed in the window |
| Floor | `SKEW_MIN=1000` per hour | the top relay's own window delta is above 1000 packets **per hour of window**, scaled by the window's real duration exactly like `abuse_flood`'s floor |

The share is measured on the window **delta** of `packets_relayed`, not the
lifetime counter: a relay that dominated months ago but is quiet now must not
be reported as dominating today. The floor keeps a near-idle fleet from
tripping the share test on a rounding artifact, and the window must be
sustained (`--window >= 2`).

The alert is a `warning` and is listed in `NON_FATAL_TYPES`: it is printed and
posted to Discord but **does not fail the run** - a relay carrying too much
traffic is an imbalance to act on, not an outage. `LIGHTSPEED_ANOMALY_STRICT=1`
restores fail-on-any.

Reproduce the finding by hand:

```bash
# human-readable
bash infra/scripts/health-anomaly.sh \
  --history web/network-history.json --no-probe
# the evidence document (relay, share, second_relay, fleet_packets, ...)
bash infra/scripts/health-anomaly.sh \
  --history web/network-history.json --no-probe --json
```

The live fleet has been in this state: `relay-fra` accumulated ~20.9M lifetime
packets against `relay-nrt` ~4.2M and `relay-bom-1` ~1.6M - one node taking the
majority of the fleet's traffic while every liveness check stayed green.

---

## 2. Why concentration is a fault, not a statistic

- **Capacity ceiling.** A load test measured a ~500 KB RSS footprint and
  near-zero CPU at 50 clients, estimating **500-1000+ concurrent clients per
  node** before any limit
  ([docs/archive/load-test-report-wf005.md](archive/load-test-report-wf005.md)).
  A relay at the dominant share is a fleet-wide latency event waiting to happen.
- **Single fault domain.** That node's saturation, network blip, or provider
  action degrades most of the fleet at once, not just one region.
- **It is self-reinforcing.** Clients drop revoked nodes, probe the survivors,
  and connect to the **fastest**
  ([docs/community-network.md](community-network.md)). The best-peered node
  keeps winning, so an imbalance grows by itself until the published set changes.
- **Cost is not the problem.** The network is sponsor-funded and zero-cost; the
  concern is headroom and blast radius, not a bill. Keep it that way (see §4.0).

---

## 3. How relay selection works today (there is no numeric weight)

The registry is a signed list. Clients verify the operator's Ed25519 signature
against a key compiled into the binary, drop every node whose pubkey is in the
`revoked` list, **probe the survivors, and connect to the fastest**. The node
schema is `node_id`, `region`, `data_addr`, `health_url`, `pubkey`,
`cert_fingerprint` (optional), `note` - there is **no weight / priority /
capacity field**, and no code path reads one.

So "re-weighting the registry" today means changing the **set** and the
**geography** of what the registry publishes, not editing a number:

1. **Add a node in the busy region** - the fastest-probe winner is then split
   between two candidates.
2. **Move an idle or underused relay into the busy region** - same effect from
   the other direction (it stops serving a region that no longer needs it).
3. **Retire/revoke a node that has no remaining role** - drops it from
   discovery on every client.

A literal per-node weight would be a **client change** (selection lives in
`client/src/registry.rs` plus the probe/route pipeline). If one is ever added,
document the field and its semantics here.

---

## 4. The re-balancing playbook

### 4.0 Budget first: $0.00

The project runs on Always Free tier, sponsor-funded, and donated community
hosts. **Do not add a node with an ongoing bill.** Prefer a donated host or an
Always Free instance with a public IPv4.

### 4.1 Confirm the skew is real

- Read the window **deltas**, not the lifetime counters.
- A frozen collector also shows unchanging counters - if `stale_history` is
  alongside, fix collection first; a stopped pipeline is not load skew.
- Confirm the top relay is actually serving: `curl http://<ip>:8080/health`.

### 4.2 Ask the placement recommender

`infra/scripts/recommend-regions.sh` reconstructs a demand matrix from the
collector history and emits `ADD`, `ADD_REDUNDANT`, `MOVE`, or `NONE` with a
stability gate. It is **advisory** - it never provisions or moves anything -
and always exits 0. It already runs on every collector pass (`pages.yml`) and
the result persists on the stats branch as `web/placement.json`.

```bash
bash infra/scripts/recommend-regions.sh \
  --history web/network-history.json \
  --previous /path/to/previous/placement.json \
  --registry web/registry.json \
  --geo-dir infra/geo \
  --out /tmp/placement.json
```

Read `recommendation.action`, `ranking[]`, and `relay_necessity[]`:

- `ADD` - the leader is an **unserved** region.
- `ADD_REDUNDANT` - the leader is in an **already-served** region and wins on
  redundancy (a genuinely distinct second path, at least
  `distinct_min_ms` from the incumbent). **This is the second-node signal for a
  busy region.**
- `MOVE` - relocate an existing relay; check that `move_coverage_keep` is
  satisfied so the source region is not starved.
- `NONE` - no stable case yet: do nothing.

The recommender needs a trailing streak (`stability_runs`, default 3) of the
same top candidate. **Never act on a single run.** `relay_necessity[]` and
`prune_candidates[]` tell you which relays are idle *and* redundant - the
candidates for a move or retirement.

### 4.3 Re-weight (change the published set)

Cheapest and least disruptive first:

1. **Do nothing** while the top relay is far from its ceiling. Record the
   observation and revisit when the share or the absolute rate grows.
2. **Add a second node in the busy region** (§4.4) - the `ADD_REDUNDANT` path.
3. **Move** an idle/underused relay into the busy region (a `MOVE`
   recommendation that keeps `move_coverage_keep`).
4. **Retire** a node that carries no traffic: remove it from the registry node
   list, and - only if it is compromised or abusive - add its pubkey to
   `revoked`. Revocation is explicit and drops the node from discovery on every
   client, so it is the last resort.

Never hand-edit `web/registry.json`: it is generated and signed. Edit the
source node list and re-sign (§4.4 step 4).

### 4.4 Recruit and publish a second node in a busy region

1. **Pick the location** *inside* the busy region but **not co-located** with
   the incumbent. The recommender's redundancy test needs `distinct_min_ms`
   (5 ms default) of separation - a clone in the same datacenter scores no
   redundancy. A different path from the same metro is fine.
2. **Provision** (see "Choosing a Region" in
   [`infra/README.md`](../infra/README.md#choosing-a-region-network-position)):
   create the instance, then
   `./infra/scripts/setup-new-node.sh <ip> <node-id> <region>`.
   `provision-oci.sh` is one provider's optional automation;
   `setup-new-node.sh` works on any host.
3. **Get the node identity.** The installer writes an Ed25519 key at
   `/etc/lightspeed/node.key` (public key in `/etc/lightspeed/identity`); on a
   BYO host generate one with `ssh-keygen -t ed25519 -N "" -f node.key`.
4. **Register** by adding the node to the registry JSON
   (`node_id`, `region`, `data_addr`, `health_url`, `pubkey`, and optionally
   `cert_fingerprint` to pre-pin the relay's certificate) and re-signing offline:

   ```bash
   cargo run -p lightspeed-client --example sign_registry -- operator.pk8 registry.json
   ```

   Keep `region` consistent with `region_aliases` in
   [`infra/geo/regions.json`](../infra/geo/regions.json) so the recommender can
   place the node. (The optional Cloudflare Worker accepts the same fields over
   HTTP with an `x-registry-token` invite token.)
5. **Publish** the signed registry; it is served from GitHub Pages.
6. **Verify**: `curl http://<ip>:8080/health` is healthy, and
   `lightspeed --probe-proxies` lists the new node.

### 4.5 Verify the re-balance worked

- After the window turns over (at least the detector's 3 snapshots), re-run
  `health-anomaly.sh`. `load_skew` clears once the top relay's share falls
  below `SKEW_SHARE` (or the fleet total outgrows it).
- The recommender's stability streak should stop recommending the same `ADD`.
- Record what changed in `wat/state/decisions.md`.

---

## 5. Tuning the detector

Both constants live at the top of `infra/scripts/health-anomaly.sh`
(`SKEW_MIN`, `SKEW_SHARE`), with the calibration rationale in the comments.
Re-derive them from live data - the way `abuse_flood`'s floor was re-derived -
and keep **both** gates plus the sustained-window requirement: lowering the
share alone will fire on a healthy fleet that simply has one strong region.

Regression coverage: `bash infra/scripts/test_health_anomaly.sh` includes a
firing case on a dominant relay and a no-fire case on an evenly spread fleet.

---

## Related

- [docs/community-network.md](community-network.md) - deploy -> register ->
  discover, end to end
- [infra/geo/README.md](../infra/geo/README.md) - the recommender's catalogs
- [docs/archive/load-test-report-wf005.md](archive/load-test-report-wf005.md) -
  per-node capacity numbers
- [infra/registry/README.md](../infra/registry/README.md) - signed registry and
  the optional Worker
- [infra/scripts/health-anomaly.sh](../infra/scripts/health-anomaly.sh) - the
  detector itself
