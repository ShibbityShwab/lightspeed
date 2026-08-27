# Bundled WinDivert binaries

The Windows client bundles the official **WinDivert 2.2.2** binaries so the
`windivert-redirect` interceptor backend works out of the box:

- `WinDivert.dll` — userspace library (loaded at runtime).
- `WinDivert64.sys` — signed kernel driver (installed alongside the exe).
- `LICENSE.windivert` — WinDivert license text.

## Provenance

Downloaded from the official WinDivert project: <https://www.reqrypt.org/windivert.html>
(`download/WinDivert-2.2.2-A.zip`, `x64/`).

WinDivert is dual-licensed under the GNU Lesser General Public License (LGPL)
Version 3, or the GNU General Public License (GPL) Version 2, at your choice.
See `LICENSE.windivert`.

These files are copied verbatim next to the compiled `lightspeed.exe` at release
time via the `[package.metadata.dist] include` entry in `client/Cargo.toml`.
The build itself compiles the WinDivert userspace library from source through
the `windivert` crate's `vendored` feature (same 2.2.2 version); the `.sys`
driver is not compiled (it is the official signed binary).
