# AUR package: lightspeed-bin

Prebuilt (`-bin`) AUR packaging for **LightSpeed**, the zero-cost global
network optimizer for multiplayer games:
<https://github.com/ShibbityShwab/lightspeed>.

## Status on 2026-11-17

- The files in this directory are the **source of truth** for the AUR package,
  at **1.11.0-1**.
- **Nothing is published to the AUR yet.** The AUR RPC reports
  `resultcount: 0` for `lightspeed-bin` (checked 2026-11-17), i.e. the package
  name is free.
- The plain `lightspeed` name is **taken** by an unrelated special-relativity
  app (maintainer `PhotonX`, package base `lightspeed`), hence the `-bin`
  suffix.
- **The first push is owner-gated** - it needs an AUR account with an SSH key
  registered on it. Account registration is a browser step and the register
  page is behind an Anubis bot challenge, so it cannot be completed (or even
  checked for availability) from automation:
  <https://aur.archlinux.org/register/>. Do not store the account password in
  this repo.
- Once the account exists, the release workflow's `publish-aur` job
  (infra/scripts/publish-aur.sh) publishes on tag pushes when the
  `AUR_SSH_PRIVATE_KEY` repository secret is set. See
  [`docs/publishing-channels.md`](../../docs/publishing-channels.md).

## What ships

| File        | Arch          | Source asset (GitHub release v1.11.0) |
|-------------|---------------|--------------------------------------|
| `/usr/bin/lightspeed`     | x86_64, aarch64 | `lightspeed-client-<triple>.tar.xz`   |
| `/usr/bin/lightspeed-gui` | x86_64 only     | `lightspeed-gui-x86_64-unknown-linux-gnu.tar.xz` |
| `/usr/share/licenses/lightspeed-bin/LICENSE` | both | bundled `LICENSE` |

Upstream publishes **no** `lightspeed-gui` build for
`aarch64-unknown-linux-gnu`, so the GUI is only installed on x86_64
(handled by per-arch `source_*` arrays and a `$CARCH` guard in
`package()`). The `lightspeed-proxy` relay-server tarballs also exist on the
release; a separate `lightspeed-proxy-bin` package would be the natural
follow-up if a relay operator wants one.

Licensing is the custom noncommercial **LightSpeed-NC-1.1**
(`license=('custom:LightSpeed-NC-1.1')`). The prebuilt binaries are installed
byte-identical to the release artifacts (`options=('!strip')`) so the Sigstore
attestations stay valid.

## First-time submission (owner steps, then automation)

1. Create an Arch Linux / AUR account and register an **SSH public key** with
   it: <https://aur.archlinux.org/register/> (browser; the page is protected by
   an Anubis bot challenge).
2. Create an SSH key for publishing (ed25519, **no passphrase** - the workflow
   cannot unlock one), store the **private** key as the `AUR_SSH_PRIVATE_KEY`
   repository secret, and never commit it.
3. Publish. Either wait for the next tag push (the `publish-aur` job then runs
   `infra/scripts/publish-aur.sh`), or push by hand from a Linux host:

   ```sh
   # On a Linux host/CI runner with the AUR SSH key loaded (this host is
   # Windows; the script is POSIX and cannot run here):
   bash infra/scripts/publish-aur.sh --dry-run   # show the plan
   bash infra/scripts/publish-aur.sh             # clone AUR repo, push if changed
   ```

   The manual pre-automation path is still valid:

   ```sh
   git clone ssh://aur@aur.archlinux.org/lightspeed-bin.git
   cd lightspeed-bin
   cp /path/to/dist/aur/PKGBUILD /path/to/dist/aur/.SRCINFO .
   git add PKGBUILD .SRCINFO
   git commit -m "lightspeed-bin 1.11.0-1"
   git push
   ```

The AUR repo is **empty until the first push**; pushing the initial commit
creates the package page.

## Updating to a new upstream release

The PKGBUILD is bumped **by hand** (there is no bump-aur automation yet; the
AUR publish job only pushes what the repo already says):

1. `pkgver=X.Y.Z` in `PKGBUILD` and `pkgrel=1` (bump `pkgrel` instead for a
   packaging-only change).
2. Refresh the sha256 sums from the new release:
   ```sh
   gh release view vX.Y.Z --repo ShibbityShwab/lightspeed --json assets \
     --jq '.assets[] | select(.name | endswith(".tar.xz")) | "\(.name)  \(.digest | sub("sha256:"; ""))"'
   ```
   The released `sha256.sum` asset lists the same digests.
3. Regenerate `.SRCINFO` **on a Linux host with pacman**:
   `makepkg --printsrcinfo > .SRCINFO` (or hand-edit it to match exactly;
   `infra/scripts/publish-aur.sh` refuses a PKGBUILD/.SRCINFO mismatch).
4. Validate with `makepkg --nobuild` (checksums), or `makepkg -f` for a full
   local build, then commit both files.
5. The next tag push runs the `publish-aur` job; it pushes only when
   `pkgver`/`pkgrel` differ from the AUR copy and is a clean no-op otherwise.
   A version mismatch between the tag and the PKGBUILD is a red job on
   purpose - it means this step was skipped.

## Local verification

- **1.11.0 (2026-11-17):** the three sha256 values in `PKGBUILD` were checked
  two ways: against the release asset digests from
  `gh release view v1.11.0 --json assets`, and by downloading the three
  `linux-gnu` tarballs and hashing them locally (values match exactly). A
  `tar -tf` listing confirms each tarball extracts to a
  `lightspeed-{client,gui}-<triple>/` directory containing the binary and
  `LICENSE` paths used by `package()`; the aarch64 release has no GUI tarball.
- **makepkg is not run here.** This development host is Windows and has no
  pacman/Arch userland; the build is verified by CI's packaging self-test only
  for the publish script, and by any Arch contributor running
  `makepkg -f`. The last full `makepkg -f` build on x86_64 was 1.4.3 (see
  git history); the packaging logic is unchanged since then.
- **Publish script:** `infra/scripts/test_publish_aur.sh` is a hermetic
  self-test (a `file://` bare repo standing in for the AUR remote, no network,
  no credentials) run by the Release workflow's *publishing self-test* job on
  every event. It covers first push, same-version no-op, version bump,
  `pkgrel`-only change, PKGBUILD/.SRCINFO mismatch refusal, downgrade refusal,
  and `--dry-run`.
