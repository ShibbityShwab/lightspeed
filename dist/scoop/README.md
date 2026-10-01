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
the repo now owns its own installer instead of depending on a file nobody
looked at.

## What is automated

On every tag, the `bump-scoop` job in `.github/workflows/release.yml` runs
`infra/scripts/bump-scoop.sh <version>`, which rewrites `version`, the download
URL, and the sha256 from the released Windows asset, then commits the result to
`master`. `infra/scripts/test_bump_scoop.sh` covers that script in CI,
including the case where a release is missing the Windows zip (the manifest
must be left byte-identical rather than half-updated).

## What is not automated

**Publishing the copy to the bucket.** That requires write access to a second
repository, so it needs a token this repo does not hold by default. Until that
is wired up, sync the copy by hand after a release:

```sh
gh api --method PUT repos/ShibbityShwab/scoop-bucket/contents/bucket/lightspeed.json \
  -f message="chore(scoop): bump to vX.Y.Z" \
  -f content="$(base64 -w0 dist/scoop/lightspeed.json)" \
  -f sha="$(gh api repos/ShibbityShwab/scoop-bucket/contents/bucket/lightspeed.json --jq .sha)"
```

To automate it later, add a `SCOOP_TOKEN` secret (a fine-grained PAT with
"Contents: write" on `ShibbityShwab/scoop-bucket`) and gate a publish step on
it, the same idiom the `winget-releaser` job uses for `WINGET_TOKEN`.

## Autoupdate

The manifest keeps `checkver` and `autoupdate`, so `scoop update` can also
refresh a user's install from the published GitHub release without the bucket
being re-synced first. That is a safety net, not a substitute for the sync
above: a stale `version` still misreports what the user has.
