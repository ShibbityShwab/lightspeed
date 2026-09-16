# LightSpeed winget (Windows Package Manager) publishing

This directory holds the winget manifest for the LightSpeed Windows GUI MSI
(`lightspeed-gui`, produced by cargo-dist), plus notes for publishing it to the
community package repository.

## Manifest layout

`manifests/s/ShibbityShwab/LightSpeed/1.4.3/` is the current manifest and follows
the winget multi-file manifest schema (ManifestVersion 1.6.0). The `1.4.2` and
`1.3.2` directories are kept for history:

| File | ManifestType | Purpose |
| --- | --- | --- |
| `ShibbityShwab.LightSpeed.yaml` | `version` | Binds the version dir to its default locale |
| `ShibbityShwab.LightSpeed.installer.yaml` | `installer` | x64 MSI, `Scope: machine`, silent switches `/qn` / `/norestart`, `UpgradeBehavior: install` |
| `ShibbityShwab.LightSpeed.locale.en-US.yaml` | `defaultLocale` | Publisher, description, tags, license |

`PackageIdentifier` is `ShibbityShwab.LightSpeed` and always matches the folder
path under `manifests/s/`.

The installer URL points at the GitHub release asset:
`https://github.com/ShibbityShwab/lightspeed/releases/download/v1.4.3/lightspeed-gui-x86_64-pc-windows-msvc.msi`
with the real `InstallerSha256`
`091BC682B1BB23DE4A10FC24CCD6CC5870F60936613D1B0EF328B57FD2E72792`
(verified against the `.msi.sha256` sidecar cargo-dist uploads with the
release).

Validation caveat: `winget validate` only runs on Windows and is not available
in this repo's Linux toolchain. The manifests are validated by schema
conformance against the winget-cli v1.6.0 JSON schemas plus manual review, and
the real gate is the microsoft/winget-pkgs PR pipeline described below.

## How a release reaches winget (handoff)

**First submission is a manual PR to `microsoft/winget-pkgs` (external, not done
from this repo).** There is no "create" permission on that repository from the
LightSpeed repo, so the very first manifest cannot be opened by automation:

1. Fork `https://github.com/microsoft/winget-pkgs`.
2. Copy this version directory into the fork at
   `manifests/s/ShibbityShwab/LightSpeed/1.4.3/` (create the folder if absent).

   **Done for 1.4.3:** the first manual submission was opened from this repo's
   manifest via the GitHub contents API:
   https://github.com/microsoft/winget-pkgs/pull/435790
   (branch `ShibbityShwab.LightSpeed-1.4.3` in the `ShibbityShwab/winget-pkgs`
   fork). Once that PR merges, later versions can be automated by adding the
   `WINGET_TOKEN` secret described below.
3. Open a pull request. The winget-pkgs bot runs `winget validate` and the
   Microsoft.Winget.Create pipeline; fix anything it flags and keep the PR
   updated until it merges.
4. Do NOT delete this local `dist/winget` copy: winget-releaser (below) reads
   its manifest from the repo, and winget-pkgs updates are diffed against the
   currently published version.

**After the first PR merges, add the `winget-releaser` GitHub Action** to this
repo (see https://github.com/vedantmgoyal2009/winget-releaser). On every
LightSpeed release it reads this manifest directory, generates the new-version
manifest for the MSI asset, and opens an update PR to `microsoft/winget-pkgs`
automatically.

Required setup for the action:

- A `WINGET_TOKEN` secret: a fine-grained PAT scoped to
  `microsoft/winget-pkgs` with **Pull requests: write** and **Contents: read**
  on that repository only. Add it under repo Settings > Secrets and variables >
  Actions (owner `ShibbityShwab`, repo `lightspeed`). The token lets the action
  open PRs as the repo, not as a personal account, so it keeps working
  unattended.

  **Status at v1.4.2: this secret is NOT set.** The `winget-releaser` job in
  `release.yml` gates on `env.WINGET_TOKEN != ''`, so it resolves to a clean
  skip (the job reports success without publishing anything). Until the secret
  is added, new versions must be submitted manually using the steps above.
- Pin `uses: vedantmgoyal2009/winget-releaser@v2` (or the current major) and
  set `with: identifier: ShibbityShwab.LightSpeed`.

Notes that apply to every future release:

- The manifest must reference the exact GitHub release asset name cargo-dist
  produces (`lightspeed-gui-x86_64-pc-windows-msvc.msi`), or winget-pkgs
  validation fails on the 404 / hash mismatch.
- `InstallerSha256` must be refreshed for every new version. winget-releaser
  recomputes it from the asset automatically; for manual updates, download the
  MSI and run `sha256sum`, then transcribe the hash (uppercase) into the
  installer manifest.
- The MSI is per-machine and bundles the WinDivert kernel driver; those facts
  are encoded as `Scope: machine` and a machine-scope product. Keep them when
  bumping versions.
