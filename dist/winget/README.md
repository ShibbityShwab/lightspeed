# LightSpeed winget (Windows Package Manager) publishing

This directory holds the winget manifest for the LightSpeed Windows GUI MSI
(`lightspeed-gui`, produced by cargo-dist), plus notes for publishing it to the
community package repository.

## Manifest layout

The committed manifests stop at `manifests/s/ShibbityShwab/LightSpeed/1.5.0/`;
the `1.4.3/` directory is the version submitted in the bootstrap PR. They follow
the winget multi-file manifest schema (ManifestVersion 1.6.0). Older directories
are kept for history:

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

- A `WINGET_TOKEN` secret: a **classic** personal access token with the single
  `public_repo` scope. Fine-grained PATs do NOT work here: opening the PR
  against `microsoft/winget-pkgs` requires the token's resource owner to match
  the repository owner (Microsoft), which needs Microsoft org membership, so
  the action fails with `403 Resource not accessible by personal access token`
  (vedantmgoyal9/winget-releaser#172). Add it under repo Settings > Secrets and
  variables > Actions (owner `ShibbityShwab`, repo `lightspeed`). Because
  `public_repo` is broad (write to every public repository the account can
  access), set an expiry and rotate it.

  **Status at v1.4.4: this secret IS set** (classic `public_repo` PAT named
  `lightspeed-winget`, 90-day expiry). The `winget-releaser` job in
  `release.yml` reads it. The package must already exist in
  `microsoft/winget-pkgs` before the job can publish an update, so the first
  submission must merge first (see the PRs linked above).
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

## Status at v1.6.5

- **Nothing is published to winget yet.** The package is created only when the
  bootstrap PR merges. `microsoft/winget-pkgs#435790` (1.4.3) is still open:
  the CLA is signed and its validation checks pass, and it is waiting on a
  community moderator. `microsoft/winget-pkgs#437292` (1.4.4) was closed as a
  duplicate, because a package cannot be created by two simultaneous
  first-version PRs.
- The committed manifests stop at `1.5.0/`; no `1.6.5/` manifest exists locally
  because the version manifests are generated by the `winget-releaser`
  automation, which cannot run until the package exists.
- The `winget-releaser` job in `release.yml` pre-checks for
  `manifests/s/ShibbityShwab/LightSpeed` in `microsoft/winget-pkgs` and skips
  when it is absent, so it no longer opens a duplicate first-version PR on every
  release. Once the bootstrap merges, the next release publishes automatically,
  and from then on the automation keeps winget current.
- Do not open further winget PRs by hand while the bootstrap is pending; it
  would be a duplicate and would lose its queue position.
