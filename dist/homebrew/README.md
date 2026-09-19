# LightSpeed Homebrew (macOS and Linux)

`Formula/lightspeed.rb` makes the prebuilt CLI client installable with
Homebrew. It covers the four cargo-dist client targets: macOS `x86_64` and
`aarch64`, and Linux `x86_64` and `aarch64`. The GUI is not covered: upstream
publishes no macOS GUI build.

## Install (no separate tap repository required)

Homebrew's one-argument `brew tap user/name` assumes a repository named
`homebrew-name`. Rather than maintain a second repository, this formula lives
in the main `lightspeed` repository and is tapped with the explicit URL form,
which works with any Git repository:

```sh
brew tap ShibbityShwab/lightspeed https://github.com/ShibbityShwab/lightspeed
brew install ShibbityShwab/lightspeed/lightspeed
```

The fully qualified `install` trusts just this formula (Homebrew's tap trust
model). Afterwards the fully qualified name works anywhere:

```sh
brew upgrade ShibbityShwab/lightspeed/lightspeed
```

If you would rather have the shorter `brew install ShibbityShwab/tap/lightspeed`,
create a repository named `homebrew-tap` and move `Formula/lightspeed.rb` into
it; nothing else changes.

## Updating for a new release

1. Set `version` to the new tag and update the four `url`/`sha256` values to the
   matching `lightspeed-client-<triple>.tar.xz` assets.
2. Refresh the sums straight from the release:

   ```sh
   gh release view vX.Y.Z --repo ShibbityShwab/lightspeed --json assets \
     -q '.assets[] | select(.name | test("lightspeed-client-.*\\.tar\\.xz$")) | "\(.name)  \(.digest | sub("sha256:"; ""))"'
   ```

3. Sanity check on a machine with Homebrew:

   ```sh
   brew install --build-from-source --verbose ./Formula/lightspeed.rb
   brew test lightspeed
   ```

   A binary-only formula like this is also checked by `brew audit --strict`
   and `brew style`.

## Why not a cask

A cask would install the GUI, but there is no macOS GUI build to point it at
(the GUI ships for Linux and Windows only), so the CLI formula is the whole
macOS story today.
