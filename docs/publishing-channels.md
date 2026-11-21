# Publishing channels

> **Status snapshot: 2026-11-18.** This page is the cross-channel map the owner
> asked for ("why are we not on choco, winget, AUR etc"): what each channel
> serves today, which part is automated, and which step still needs the owner.
> Re-verify a row with the commands at the bottom before quoting it.
>
> **2026-11-21 update (v1.12.0 published):** GitHub Releases 1.12.0 (39 assets,
> published 2026-11-21); Homebrew formula 1.12.0; Chocolatey source/manifests
> 1.12.0 and the `.nupkg` push submitted (community-feed moderation is external);
> Scoop bucket serves 1.12.0; AUR `lightspeed-bin 1.12.0-1`; winget remains
> unpublished pending the owner bootstrap PR
> [microsoft/winget-pkgs#481953](https://github.com/microsoft/winget-pkgs/pull/481953).
> The table rows below are the 2026-11-18 snapshot; the tag's auto-bump jobs were
> blocked by master protection and the same bumps were merged through PR #198,
> after which the release run completed green.

All channels are free community infrastructure; the owner's cost is **$0.00**.

## Channel map

| Channel | Serves today (2026-11-18) | In-repo source | Automation | Required secret / owner step |
|---|---|---|---|---|
| **GitHub Releases** (source of truth) | **1.11.3** (published 2026-11-18, 39 assets) | - | Release workflow: host + `infra/scripts/publish-release.sh`; `release-state` gates everything below on a published release with the six packaging assets | none (optional RELEASE_TOKEN if the Actions token cannot write; see deploy-proxy) |
| **Homebrew** (tap `ShibbityShwab/lightspeed`) | **1.11.3** (formula committed in this repo) | `Formula/lightspeed.rb` | `bump-homebrew` job updates and commits the formula on every tag | none |
| **Chocolatey** (`choco install lightspeed`) | Feed lists **1.10.3**; **1.11.0-1.11.3 pushed 2026-11-17/18** and awaiting moderation | `dist/chocolatey/` at 1.11.3 | `bump-chocolatey` job bumps the source and pushes the `.nupkg`; Choco Push workflow re-pushes by hand for a missed version | `CHOCO_API_KEY` **set** (2026-10-02); account exists. No action beyond the moderation queue |
| **Scoop** (`scoop install lightspeed`) | Bucket serves **1.11.0**; the in-repo manifest is **1.11.3** (bumped 2026-11-18) but the bucket was **not** synced | `dist/scoop/lightspeed.json` | `publish-scoop` job (`infra/scripts/publish-scoop.sh`) syncs the bucket after `bump-scoop` commits to master; no-op when the bucket already matches | **`SCOOP_TOKEN` missing** - owner creates a token with Contents-write on `ShibbityShwab/scoop-bucket` and stores it as the secret |
| **winget** (`winget install ShibbityShwab.LightSpeed`) | **Not published** - bootstrap PR **[microsoft/winget-pkgs#481953](https://github.com/microsoft/winget-pkgs/pull/481953)** is open (1.11.0) awaiting a moderator; `microsoft/winget-pkgs` has no package yet | `dist/winget/manifests/s/ShibbityShwab/LightSpeed/1.11.0/` | `winget-releaser` job is gated on the package existing + `WINGET_TOKEN`; it is a clean no-op today | **First-version PR is open, not merged.** Once #481953 merges, the existing `WINGET_TOKEN` (set 2026-09-18; verify valid) is enough for updates |
| **AUR** (`yay -S lightspeed-bin`) | **Published 1.11.3-1** (2026-11-18; [package page](https://aur.archlinux.org/packages/lightspeed-bin), maintainer `chrissinclair`, 3 sources; the RPC index can lag the page by minutes) | `dist/aur/PKGBUILD` + `.SRCINFO` at **1.11.3-1** | `publish-aur` job (`infra/scripts/publish-aur.sh`) pushes to `ssh://aur@aur.archlinux.org/lightspeed-bin.git` when pkgver/pkgrel changed; no-op when already current | **Done:** AUR account key registered and `AUR_SSH_PRIVATE_KEY` set. The PKGBUILD is still bumped by hand each release (no `bump-aur` automation yet) |
| **Direct downloads** (MSI / zip / tarballs) | **1.11.3** | - | GitHub Releases (same as the first row) | none |

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

### Re-running the bumps for an already-published release

The `release-state` gate reads the release with `gh release view`. Before
2026-11-18 that job had no git checkout, so `gh` failed with "not a git
repository", `ready` stayed false and **every** bump was silently skipped (this
hit the v1.11.1 tag; the release itself published fine). The job now sets
`GH_REPO`, and a `workflow_dispatch` takes `tag` + `run_bumps=true` to run the
same bump jobs for a release that is already published - no build, no publish.
v1.11.1's channels were completed with that mode, and v1.11.2/v1.11.3 repeated
it: the tag's `publish-aur` failed on the stale PKGBUILD as designed, the
PKGBUILD was bumped on master, and re-running the failed job published the new
version. A release that needs its channels re-run no longer needs a new tag.

### Owner-only blockers (exact)

1. **Scoop bucket token.** Create a token (fine-grained preferred, resource
   owner `ShibbityShwab`) with **Contents: write** on
   `ShibbityShwab/scoop-bucket`, then add it as the repository secret
   **`SCOOP_TOKEN`**. Until then the bucket only moves when someone syncs it
   by hand (`gh api --method PUT .../contents/bucket/lightspeed.json`; see
   `dist/scoop/README.md`).
2. ~~**AUR account + SSH key.**~~ **Done 2026-11-17:** the `chrissinclair`
   account now carries the `lightspeed-bin` CI public key
   (`ssh-ed25519 ... lightspeed-aur-ci`), the package was first-pushed
   (`lightspeed-bin 1.11.0-1`, commit `e690531`) and `AUR_SSH_PRIVATE_KEY`
   is stored as the repository secret, so `publish-aur` is armed. Note: the
   AUR profile save also required the Homepage field to be a full URL; it is now
   `https://chrissinclairdesign.com`.
3. **winget first-version PR.** The package has never been created in
   `microsoft/winget-pkgs`, and automation cannot create the first version.
   **[microsoft/winget-pkgs#481953](https://github.com/microsoft/winget-pkgs/pull/481953)**
   (1.11.0) is open and awaiting a moderator; sign the Microsoft CLA if prompted.
   After it merges, the existing `WINGET_TOKEN` (set 2026-09-18; verify it is
   valid and not expired) is enough for updates, starting with 1.11.3. Full
   procedure: `dist/winget/README.md`.
4. **Release publish token (optional).** Only if the Actions token cannot write
   releases in this repository: add `RELEASE_TOKEN` with Contents read/write.
   The 2026-11-16 403 was not reproducible on 2026-11-17; see
   [deploy-proxy](deploy-proxy).

The **AUR PKGBUILD bump** is also manual today: before tagging a release, set
`pkgver` in `dist/aur/PKGBUILD`, refresh the three sha256 values from the
release's `sha256.sum`, update `.SRCINFO`, and commit both. v1.11.1, v1.11.2 and
v1.11.3 were bumped this way after the assets existed; a `bump-aur` automation
would remove that step, and the bump-dispatch mode above is the recovery path
when it is missed.

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

# AUR: the RPC index can lag a freshly created package; check the page too
curl -fsSL 'https://aur.archlinux.org/rpc/v5/info?arg[]=lightspeed-bin' | jq -r '.resultcount'
curl -fsSL -o /dev/null -w '%{http_code}\n' 'https://aur.archlinux.org/packages/lightspeed-bin'
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
