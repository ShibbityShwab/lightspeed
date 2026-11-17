# LightSpeed winget (Windows Package Manager) publishing

This directory holds the winget manifests for the LightSpeed Windows GUI MSI
(`lightspeed-gui`, produced by cargo-dist), plus the exact procedure and
current blockers for publishing them to the community package repository
`microsoft/winget-pkgs`.

```
winget install --id ShibbityShwab.LightSpeed --exact
```

## Manifest layout

```
manifests/s/ShibbityShwab/LightSpeed/<version>/
  ShibbityShwab.LightSpeed.yaml                 # version manifest
  ShibbityShwab.LightSpeed.installer.yaml       # installer manifest
  ShibbityShwab.LightSpeed.locale.en-US.yaml    # default locale
```

The committed versions are `1.3.2`, `1.4.2`, `1.4.3`, `1.4.4`, `1.5.0`,
`1.6.14` and **`1.11.0`** (new on 2026-11-17). Older directories are kept for
history. `PackageIdentifier` is `ShibbityShwab.LightSpeed` and must always
match the folder path under `manifests/s/`.

The `1.11.0` `installer.yaml` points at the published release asset:

- URL: `https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.0/lightspeed-gui-x86_64-pc-windows-msvc.msi`
- `InstallerSha256`: `C4CA86749CD542E4BBC760397D84F74100D304CA0ABE80218353021C11097436`
  (matches the release asset digest and the `.msi.sha256` sidecar on
  `v1.11.0`; verified 2026-11-17)
- `InstallerType: wix`, `Architecture: x64`, `Scope: machine`,
  `UpgradeBehavior: install`, MSI silent switches `/qn` / `/norestart`.

Validation on this Windows host (winget 1.29.380):

```powershell
winget validate --manifest dist\winget\manifests\s\ShibbityShwab\LightSpeed\1.11.0
# Manifest validation succeeded.
```

The real gate stays the `microsoft/winget-pkgs` PR pipeline below; the
winget-pkgs bot re-runs `winget validate` plus its own policy checks.

## Status on 2026-11-17 (why `winget install` does not work yet)

**The package has never been published.** `microsoft/winget-pkgs` has no
`manifests/s/ShibbityShwab/LightSpeed` path (contents API returns `404`);
`winget install ShibbityShwab.LightSpeed` fails with "No package found".

The earlier bootstrap PRs used manifest schema 1.6.0 and carried the
`Manifest-Version-Deprecated` label, which is one reason they were not
actionable. The 1.11.0 manifests were refreshed to the schema in current use
(the same one recent merges use) and both `winget validate` and the
winget-pkgs Validation Pipeline accept them; on 2026-11-17 the manifests were
bumped again to **1.28.0** so the submission carries no deprecated schema
version.

On 2026-11-17 the replacement first-version PR was opened:
**[microsoft/winget-pkgs#481953](https://github.com/microsoft/winget-pkgs/pull/481953)**
(`ShibbityShwab.LightSpeed` 1.11.0, manifest schema **1.28.0**, `winget
validate` clean, CLA already signed on the account). It is waiting on a
winget-pkgs moderator; once it merges, the `winget-releaser` job publishes
later versions automatically because `WINGET_TOKEN` already exists.

The two earlier bootstrap attempts were **closed without merging**, so they do
not bootstrap the package:

| PR | Version | State at 2026-11-17 |
|----|---------|---------------------|
| [microsoft/winget-pkgs#435790](https://github.com/microsoft/winget-pkgs/pull/435790) | 1.4.3 | `closed`, not merged (updated 2026-11-16) |
| [microsoft/winget-pkgs#437292](https://github.com/microsoft/winget-pkgs/pull/437292) | 1.4.4 | `closed`, not merged |

A **new first-version PR for 1.11.0** is required. The repository secret
`WINGET_TOKEN` **does exist** (set 2026-09-18), so the missing piece is the
bootstrap PR - not the token. The `winget-releaser` job in
`.github/workflows/release.yml` is already wired: it pre-checks whether the
package exists and skips cleanly until it does, so it opens no duplicate
first-version PR.

## One-time bootstrap procedure (owner actions)

Automatic publishing cannot start until the first manifest merges. For 1.11.0,
steps 1 and 2 are **done** (CLA signed 2026-09-18; PR #481953 open from this
repository's fork); the remaining action is a winget-pkgs moderator merge. The
steps below are the repeatable procedure if that PR is closed instead. Steps 1,
2 and 4 require the owner's GitHub account; do not automate the first PR.

1. **Sign the Microsoft CLA** if not already signed for the account - the
   winget-pkgs bot prompts on the first PR. (The 1.4.3 submission reported it
   as signed; the prompt re-appears only if that signature is invalid.)
2. **Open the first-version PR** from a fork of `microsoft/winget-pkgs`:
   - fork `https://github.com/microsoft/winget-pkgs`;
   - copy this directory's `1.11.0/` to
     `manifests/s/ShibbityShwab/LightSpeed/1.11.0/` in the fork;
   - open a PR titled
     `New version: ShibbityShwab.LightSpeed version 1.11.0`;
   - do **not** delete `dist/winget` (the automation reads release assets and
     re-generates manifests itself; this directory is the reviewed source and
     the bootstrap copy).
3. **Let the winget-pkgs pipeline validate and a moderator merge it.** A
   package cannot be created by two simultaneous first-version PRs - only one
   bootstrap PR at a time.
4. **Verify the token** `WINGET_TOKEN` (set 2026-09-18) can open PRs on
   `microsoft/winget-pkgs`; rotate/replace it when it expires. The documented
   token shape for the publisher is a **fine-grained PAT** with repository
   access to `microsoft/winget-pkgs` and permissions **Pull requests: read and
   write** and **Contents: read** (store it under Settings -> Secrets and
   variables -> Actions). If GitHub refuses to grant `Pull requests: write` on
   a repository you do not administer, the winget-releaser fallback is a
   classic PAT with the `public_repo` scope (broad - set an expiry and rotate).
   Never commit the token.

After the bootstrap merges, the next tag release publishes automatically: the
`winget-releaser` job runs
[`vedantmgoyal9/winget-releaser@v2`](https://github.com/vedantmgoyal9/winget-releaser)
with `identifier: ShibbityShwab.LightSpeed`, which opens an update PR to
`microsoft/winget-pkgs` on every release.

## Notes for every future release

- The installer asset name must stay
  `lightspeed-gui-x86_64-pc-windows-msvc.msi`; winget-pkgs validation fails on
  a 404 or hash mismatch.
- `InstallerSha256` must be refreshed for every version. `winget-releaser`
  recomputes it from the release asset; for a manual bump download the MSI,
  hash it, and transcribe the uppercase hash.
- The MSI is per-machine and bundles the WinDivert driver; those facts stay
  encoded as `Scope: machine` and `InstallerType: wix`.

## Verification commands

```powershell
# Is the package bootstrapped?
gh api repos/microsoft/winget-pkgs/contents/manifests/s/ShibbityShwab/LightSpeed
# -> 404 (HTTP 404: Not Found) until the first PR merges

# Is the release asset hash in the manifest correct?
gh release view v1.11.0 --repo ShibbityShwab/lightspeed --json assets --jq '.assets[] | select(.name == "lightspeed-gui-x86_64-pc-windows-msvc.msi")'
# -> "digest": "sha256:c4ca86749cd542e4bbc760397d84f74100d304ca0abe80218353021c11097436"

# Validate the local manifests (Windows host)
winget validate --manifest dist\winget\manifests\s\ShibbityShwab\LightSpeed\1.11.0
```

See [`docs/publishing-channels.md`](../../docs/publishing-channels.md) for the
cross-channel state table.
