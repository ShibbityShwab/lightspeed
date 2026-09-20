# Chocolatey package: lightspeed

Package source for the Chocolatey Community feed
(<https://community.chocolatey.org>), the standard Windows `choco install`
channel.

## What it installs

`choco install lightspeed` installs the Windows CLI client (`lightspeed.exe`)
and its bundled WinDivert driver files from the GitHub release zip. The tray
GUI is shipped as an MSI on the
[releases page](https://github.com/ShibbityShwab/lightspeed/releases).

## Layout

| File | Purpose |
|------|---------|
| `lightspeed.nuspec` | Package metadata (`id`, `version`, `authors`, ...) |
| `tools/chocolateyInstall.ps1` | Downloads + verifies the release zip and installs it |

## Build

A `.nupkg` is an OPC zip, not a plain archive: it needs the `.nuspec` plus
`[Content_Types].xml` and `_rels/.rels` at the root, or NuGet rejects the push
with "Package does not contain a manifest". On Windows use `choco pack`; on any
platform use the bundled script:

```sh
cd dist/chocolatey && ./build.sh
```

## Publish (maintainer)

Chocolatey Community requires an account and is **moderated** (a human reviews
new packages, usually within a few days; the package shows as unlisted until it
is approved). Web upload of `.nupkg` files is disabled, so push with an API key.

With Chocolatey installed (Windows):

```sh
choco push lightspeed.1.6.3.nupkg --source https://push.chocolatey.org/ --api-key <API_KEY>
```

Or from any platform with curl, against the NuGet v2 push endpoint:

```sh
curl -X PUT -H "X-NuGet-ApiKey: <API_KEY>" \
  --data-binary @lightspeed.1.6.3.nupkg \
  https://push.chocolatey.org/api/v2/package/
```

## Updating

Bump `version` in `lightspeed.nuspec`, update `url64bit` and `checksum64` in
`tools/chocolateyInstall.ps1` to the new release asset, rebuild, and push.
