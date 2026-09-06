# AUR package: lightspeed-bin

Prebuilt (`-bin`) AUR packaging for **LightSpeed**, the zero-cost global
network optimizer for multiplayer games:
<https://github.com/ShibbityShwab/lightspeed>.

This directory is a **prepared + handoff** state. Publishing to
`aur.archlinux.org` is external and intentionally **not** performed here.

## What ships

| File        | Arch          | Source asset (GitHub release v1.3.2) |
|-------------|---------------|--------------------------------------|
| `/usr/bin/lightspeed`     | x86_64, aarch64 | `lightspeed-client-<triple>.tar.xz`   |
| `/usr/bin/lightspeed-gui` | x86_64 only     | `lightspeed-gui-x86_64-unknown-linux-gnu.tar.xz` |
| `/usr/share/licenses/lightspeed-bin/LICENSE` | both | bundled `LICENSE` |

Upstream publishes **no** `lightspeed-gui` build for
`aarch64-unknown-linux-gnu`, so the GUI is only installed on x86_64
(handled by per-arch `source_*` arrays and a `$CARCH` guard in `package()`).
The `lightspeed-proxy` relay-server tarballs also exist on the release; a
separate `lightspeed-proxy-bin` package would be the natural follow-up if a
relay operator wants one.

Licensing is the custom noncommercial **LightSpeed-NC-1.0**
(`license=('custom:LightSpeed-NC-1.0')`). The `lightspeed` package name is
already taken on the AUR by an unrelated special-relativity app, hence the
`-bin` suffixed name; there is currently **no** `lightspeed-bin` package.

## First-time submission

Requires an AUR account (SSH key registered with your account) and a
real `# Maintainer: YourName <you@example.com>` line in `PKGBUILD`
(the placeholder must be replaced first).

```sh
# 1. Fill in the real Maintainer line in PKGBUILD, then regenerate .SRCINFO:
makepkg --printsrcinfo > .SRCINFO

# 2. Clone the (empty) AUR package repo via SSH and copy the files in:
git clone ssh://aur@aur.archlinux.org/lightspeed-bin.git
cd lightspeed-bin
cp /path/to/dist/aur/PKGBUILD /path/to/dist/aur/.SRCINFO .

# 3. Commit and push. Pushing the initial commit publishes the package:
git add PKGBUILD .SRCINFO
git commit -m "lightspeed-bin 1.3.2-1"
git push
```

## Updating to a new upstream release

When LightSpeed tags a new version (e.g. `v1.4.0`):

1. `pkgver=1.4.0` in `PKGBUILD` (keep `pkgrel=1` on a version bump; bump
   `pkgrel` for packaging-only changes).
2. Refresh the sha256 sums against the new release's assets:
   ```sh
   gh release view v1.4.0 --repo ShibbityShwab/lightspeed --json assets \
     -q '.assets[] | select(.name | endswith(".tar.xz")) | "\(.name)  \(.digest | sub("sha256:"; ""))"'
   ```
   Asset names follow `lightspeed-{client,gui}-<target-triple>.tar.xz`;
   confirm upstream did not add an aarch64 GUI tarball before enabling it.
3. `makepkg --printsrcinfo > .SRCINFO` and sanity-build with
   `makepkg -f` (also run once per arch change).
4. Commit both files and `git push`.

Do **not** update sha256sums of unchanged assets unnecessarily: real sums
are already in place for 1.3.2 (verified by downloading each tarball and by
a full `makepkg` build that reported `Passed` on all checksums).

## Local verification (already performed for 1.3.2)

- `makepkg --printsrcinfo` -> exit 0, `.SRCINFO` consistent with `PKGBUILD`.
- Full `makepkg -f` build on x86_64 -> success; checksum verification
  `Passed`; package contains `/usr/bin/lightspeed`,
  `/usr/bin/lightspeed-gui` and the LICENSE file.
- `/usr/bin/lightspeed --version` -> `lightspeed 1.3.2`.
- `ldd` on both binaries shows glibc/libgcc only, hence `depends=()`.
  The GUI dlopens X11/Wayland/EGL at runtime (winit/glutin); install
  `mesa`, `libxkbcommon`, and the wayland/X11 libs on a bare system.
