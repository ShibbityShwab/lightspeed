# LightSpeed Scoop bucket publishing

`lightspeed.json` in this directory is the **source of truth** for the Scoop
manifest. The public bucket,
[`ShibbityShwab/scoop-bucket`](https://github.com/ShibbityShwab/scoop-bucket),
holds a **published copy** that `scoop install lightspeed` reads.

## Why this directory exists

The bucket used to hold the only copy. Nothing in this repository updated it,
so it sat at **1.6.3** while the project shipped eleven later releases - users
running `scoop install lightspeed` silently received a binary many releases
old. Keeping the manifest here, and bumping it from the release workflow, means
the repo owns its own installer instead of depending on a file nobody looked
at.

## Status on 2026-11-17

- In-repo manifest: **1.11.0**.
- Public bucket: **1.11.0** (synced from this manifest on 2026-11-17).
- The bucket's next update is automated but needs one owner step: a
  `SCOOP_TOKEN` repository secret (see below). Until it exists, new tags
  bump the in-repo manifest and the workflow logs a `::notice::` that the
  bucket was not updated.

## What is automated

On every tag, the `bump-scoop` job in `.github/workflows/release.yml` runs
`infra/scripts/bump-scoop.sh <version>`, which rewrites `version`, the download
URL, and the sha256 from the released Windows asset, then commits the result to
`master`. `infra/scripts/test_bump_scoop.sh` covers that script in CI,
including the case where a release is missing the Windows zip (the manifest
must be left byte-identical rather than half-updated).

The new `publish-scoop` job then runs `infra/scripts/publish-scoop.sh`,
which copies `dist/scoop/lightspeed.json` into the bucket through the GitHub
contents API:

- **No-op** when the bucket already parses to the same JSON as this manifest
  (different indentation, key order, or CRLF line endings are ignored, so there
  is no churn commit on every run).
- **Creates** `bucket/lightspeed.json` when the bucket does not have it yet,
  **updates** it with the current blob `sha` otherwise, and retries once if
  another writer changed the bucket in between.
- **Refuses** (red job) when `--version <tag>` does not match this manifest,
  so a skipped/failed `bump-scoop` cannot silently publish the previous
  version.
- `false`/missing secret: the job logs a `::notice::` naming `SCOOP_TOKEN` and
  does nothing; the release stays green.

Hermetic self-test: `infra/scripts/test_publish_scoop.sh` (stateful `gh` stub,
no network, no token) runs in the `publishing-self-test` job on every
workflow event.

## The one owner step: `SCOOP_TOKEN`

Writing to a second repository needs a credential this repo does not hold by
default. Create a token (fine-grained preferred) owned by `ShibbityShwab`
with **Contents: write** on `ShibbityShwab/scoop-bucket`, then store it as
the repository secret `SCOOP_TOKEN` in
`ShibbityShwab/lightspeed` (Settings -> Secrets and variables -> Actions).
Never commit a token.

Until the secret exists, sync the bucket by hand after a release:

```sh
gh api --method PUT repos/ShibbityShwab/scoop-bucket/contents/bucket/lightspeed.json \
  -f message="chore(scoop): bump to vX.Y.Z" \
  -f content="$(base64 -w0 dist/scoop/lightspeed.json)" \
  -f sha="$(gh api repos/ShibbityShwab/scoop-bucket/contents/bucket/lightspeed.json --jq .sha)"
```

## Autoupdate

The manifest keeps `checkver` and `autoupdate`, so `scoop update` can also
refresh a user's install from the published GitHub release without the bucket
being re-synced first. That is a safety net, not a substitute for the sync
above: a stale `version` still misreports what the user has.

## Verify

```sh
curl -fsSL https://raw.githubusercontent.com/ShibbityShwab/scoop-bucket/master/bucket/lightspeed.json | jq -r .version
jq -r .version dist/scoop/lightspeed.json
```

Both should print the released version. Full cross-channel map:
`docs/publishing-channels.md`.
