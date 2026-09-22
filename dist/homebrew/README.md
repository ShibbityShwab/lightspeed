# LightSpeed Homebrew (macOS and Linux)

`Formula/lightspeed.rb` makes the prebuilt CLI client installable with
Homebrew. It covers the four cargo-dist client targets: macOS `x86_64` and
`aarch64`, and Linux `x86_64` and `aarch64`. The GUI is not covered: upstream
publishes no macOS GUI build.

## Install

The tap `ShibbityShwab/lightspeed` is **live**. Tap it and install the
formula:

```sh
brew tap ShibbityShwab/lightspeed
brew install ShibbityShwab/lightspeed/lightspeed
```

The fully qualified `install` trusts just this formula (Homebrew's tap trust
model). Afterwards the fully qualified name works anywhere:

```sh
brew upgrade ShibbityShwab/lightspeed/lightspeed
```

The formula source of truth lives in this repository at
`Formula/lightspeed.rb`; the tap repository mirrors it.

## Updating for a new release

This is automatic. On every tag, the `bump-homebrew` job in
`.github/workflows/release.yml` runs `infra/scripts/bump-homebrew.sh <version>`,
which rewrites `version` and the four `sha256` values from the release assets
and commits the result to `master`.

To repair the formula by hand:

```sh
bash infra/scripts/bump-homebrew.sh X.Y.Z
git diff -- Formula/lightspeed.rb
```

Sanity check on a machine with Homebrew:

```sh
brew install --build-from-source --verbose ./Formula/lightspeed.rb
brew test lightspeed
```

A binary-only formula like this is also checked by `brew audit --strict` and
`brew style`.

## Why not a cask

A cask would install the GUI, but there is no macOS GUI build to point it at
(the GUI ships for Linux and Windows only), so the CLI formula is the whole
macOS story today.
