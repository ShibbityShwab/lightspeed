# Publishing channels

> **Status snapshot: 2026-11-17.** This page is the cross-channel map the owner
> asked for ("why are we not on choco, winget, AUR etc"): what each channel
> serves today, which part is automated, and which step still needs the owner.
> Re-verify a row with the commands at the bottom before quoting it.

All channels are free community infrastructure; the owner's cost is **$0.00**.

## Channel map

| Channel | Serves today (2026-11-17) | In-repo source | Automation | Required secret / owner step |
|---|---|---|---|---|
| **GitHub Releases** (source of truth) | **1.11.0** (published 2026-11-16, 39 assets) | - | Release workflow: host + `infra/scripts/publish-release.sh`; release-state gates everything below on a published release with the six packaging assets | none (optional RELEASE_TOKEN if the Actions token cannot write; see deploy-proxy) |
| **Homebrew** (tap `ShibbityShwab/lightspeed`) | **1.11.0** (formula committed in this repo) | `Formula/lightspeed.rb` | `bump-homebrew` job updates and commits the formula on every tag | none |
| **Chocolatey** (`choco install lightspeed`) | Feed lists **1.10.3**; **1.11.0 pushed 2026-11-17** and awaits moderation | `dist/chocolatey/` | `bump-chocolatey` job bumps the source and pushes the `.nupkg`; Choco Push workflow re-pushes by hand for a missed version | `CHOCO_API_KEY` **set** (2026-10-02); account exists. No action beyond the moderation queue |
| **Scoop** (`scoop install lightspeed`) | Bucket serves **1.11.0** (synced 2026-11-17 from the Lead's manual API write; in-repo manifest was already 1.11.0) | `dist/scoop/lightspeed.json` | **new** `publish-scoop` job (`infra/scripts/publish-scoop.sh`) syncs the bucket after `bump-scoop` commits to master; no-op when the bucket already matches | **`SCOOP_TOKEN` missing** - owner creates a token with Contents-write on `ShibbityShwab/scoop-bucket` and stores it as the secret |
| **winget** (`winget install ShibbityShwab.LightSpeed`) | **Not published** - `microsoft/winget-pkgs` has no `manifests/s/ShibbityShwab/LightSpeed` (HTTP 404) | `dist/winget/manifests/s/ShibbityShwab/LightSpeed/1.11.0/` (added 2026-11-17; older versions kept) | `winget-releaser` job exists and is gated on the package existing + `WINGET_TOKEN`; it is a clean no-op today | **First-version PR missing.** PRs #435790 (1.4.3) and #437292 (1.4.4) were closed unmerged; a new 1.11.0 bootstrap PR + Microsoft CLA merge is an owner/manual step. `WINGET_TOKEN` **already exists** (set 2026-09-18) - verify/rotate it, but the token is not the blocker |
| **AUR** (`yay -S lightspeed-bin`) | **No package** - AUR RPC returns `resultcount: 0` for `lightspeed-bin`; the name is free | `dist/aur/PKGBUILD` + `.SRCINFO` at **1.11.0-1** | **new** `publish-aur` job (`infra/scripts/publish-aur.sh`) pushes to `ssh://aur@aur.archlinux.org/lightspeed-bin.git` when pkgver/pkgrel changed; no-op when already current | **`AUR_SSH_PRIVATE_KEY` missing and no AUR account.** Owner creates an AUR account, registers an SSH key, stores the private key as the secret. The PKGBUILD is still bumped by hand each release (no bump automation yet) |
| **Direct downloads** (MSI / zip / tarballs) | **1.11.0** | - | GitHub Releases (same as the first row) | none |

### What the next tag does automatically

On a `vX.Y.Z` tag push, after `release-state` reports the release is
published with its assets:

1. `bump-homebrew` -> commits `Formula/lightspeed.rb`.
2. `bump-chocolatey` -> bumps `dist/chocolatey`, pushes the `.nupkg` when
   `CHOCO_API_KEY` is set.
3. `bump-scoop` -> bumps `dist/scoop/lightspeed.json`.
4. `publish-scoop` -> syncs the bucket when `SCOOP_TOKEN` is set; **no-op
   otherwise with a `::notice::` naming the secret**. The job is red, not
   skipped, if the manifest does not match the tag.
5. `publish-aur` -> pushes `dist/aur` when `AUR_SSH_PRIVATE_KEY` is set;
   no-op with a `::notice::` otherwise. Red if the PKGBUILD was not bumped to
   the tag version (a deliberate signal - bump `dist/aur`, then re-run the job).
6. `winget-releaser` -> opens a winget-pkgs update PR once the package exists
   and `WINGET_TOKEN` is valid.
7. `verify-bumps` -> fails the release if the committed package manifests
   did not move to the tag.

`publishing-self-test` runs the hermetic Scoop/AUR self-tests on **every**
workflow event (PR included, no token, no network).

## Owner-only blockers (exact)

1. **Scoop bucket token.** Create a token (fine-grained preferred, resource
   owner `ShibbityShwab`) with **Contents: write** on
   `ShibbityShwab/scoop-bucket`, then add it as the repository secret
   **`SCOOP_TOKEN`**. Until then the bucket only moves when someone syncs it
   by hand (`gh api --method PUT .../contents/bucket/lightspeed.json`; see
   `dist/scoop/README.md`).
2. **AUR account + SSH key.** Create an AUR account and register an SSH public
   key on it at <https://aur.archlinux.org/register/> (browser step; the page is
   behind an Anubis bot challenge, so automation cannot complete or even check
   it). The package name `lightspeed-bin` is free - the `lightspeed` name is an
   unrelated package. Then add the (passphrase-less) private key as the
   repository secret **`AUR_SSH_PRIVATE_KEY`** and re-run a release, or run
   the publish script by hand from a Linux host.
3. **winget first-version PR.** The package has never been created in
   `microsoft/winget-pkgs`, and automation cannot create the first version.
   Open a fresh PR with the `1.11.0` manifest (the prior 1.4.3/1.4.4 bootstrap
   PRs are closed, not merged), sign the Microsoft CLA if prompted, and let a
   winget-pkgs moderator merge it. After that the existing `WINGET_TOKEN`
   (set 2026-09-18; verify it is valid and not expired) is enough for updates.
   Full procedure: `dist/winget/README.md`.
4. **Release publish token (optional).** Only if the Actions token cannot write
   releases in this repository: add `RELEASE_TOKEN` with Contents read/write.
   The 2026-11-16 403 was not reproducible on 2026-11-17; see
   [deploy-proxy](deploy-proxy).

The **AUR PKGBUILD bump** is also manual today: before tagging a release, set
`pkgver` in `dist/aur/PKGBUILD`, refresh the three sha256 values from the
release's `sha256.sum`, update `.SRCINFO`, and commit both. A `bump-aur`
automation would remove that step; it does not exist yet.

## Verification commands (2026-11-17 state)

These are the exact commands used for the table above; run them from a machine
with `gh` authenticated and `jq` (CI has both). The expected output is the
"serves today" column.

```sh
# GitHub Releases: latest published release and its assets
gh release view v1.11.0 --repo ShibbityShwab/lightspeed --json isDraft,isPrerelease,assets

# Chocolatey community feed (OData v2): newest versions first
curl -fsSL "https://community.chocolatey.org/api/v2/FindPackagesById()?id='lightspeed'" \
  | grep -o '<d:Version>[^<]*' | head -3
# 2026-11-17 -> <d:Version>1.10.3 (1.11.0 push accepted 2026-11-17, awaiting moderation)

# Scoop bucket (published copy) and the in-repo source
curl -fsSL https://raw.githubusercontent.com/ShibbityShwab/scoop-bucket/master/bucket/lightspeed.json | jq -r .version
jq -r .version dist/scoop/lightspeed.json
# 2026-11-17 -> 1.11.0 / 1.11.0

# AUR RPC: resultcount 0 means the package does not exist yet
curl -fsSL 'https://aur.archlinux.org/rpc/v5/info?arg[]=lightspeed-bin' | jq -r '.resultcount'
# 2026-11-17 -> 0

# winget-pkgs: 404 until the first manifest PR merges
gh api repos/microsoft/winget-pkgs/contents/manifests/s/ShibbityShwab/LightSpeed
# 2026-11-17 -> HTTP 404

# Homebrew formula (tap is this repository, so the raw file is the source)
curl -fsSL https://raw.githubusercontent.com/ShibbityShwab/lightspeed/master/Formula/lightspeed.rb | grep 'version "'
# 2026-11-17 -> version "1.11.0"

# Local manifests: winget schema validation (Windows host) and AUR checksum view
winget validate --manifest dist\winget\manifests\s\ShibbityShwab\LightSpeed\1.11.0
grep -A2 'sha256sums_x86_64' dist/aur/PKGBUILD
```

PowerShell equivalents for the Windows host where a POSIX pipe is inconvenient:
`gh` and `winget` run fine; `curl` maps to `Invoke-WebRequest` and `jq` may
not be installed. Prefer the Linux/CI form above when recording evidence.

## Related files

- `dist/scoop/README.md` - manifest source of truth + sync procedure.
- `infra/scripts/publish-scoop.sh`, `infra/scripts/test_publish_scoop.sh`.
- `dist/winget/README.md` - the one-time bootstrap PR and token procedure.
- `dist/aur/README.md`, `infra/scripts/publish-aur.sh`,
  `infra/scripts/test_publish_aur.sh`.
- `docs/deploy-proxy.md` - the release/publish operator contract.
