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

The `.nupkg` is a zip of the `.nuspec` at the root plus `tools/`. On Windows
this is `choco pack lightspeed.nuspec`; a plain archive with the same layout
works too:

```sh
cd dist/chocolatey && zip -r ../../lightspeed.1.6.3.nupkg lightspeed.nuspec tools/
```

## Publish (maintainer)

Chocolatey Community requires an account and is **moderated** (a human reviews
new packages, usually within a few days).

```sh
# 1. Create an account at https://community.chocolatey.org/account/Register
#    and copy the API key from https://community.chocolatey.org/account
# 2. Push the built package
choco push lightspeed.1.6.3.nupkg --source https://push.chocolatey.org/ --api-key <API_KEY>
```

Or upload the `.nupkg` directly from the "Upload" page while signed in.

## Updating

Bump `version` in `lightspeed.nuspec`, update `url64bit` and `checksum64` in
`tools/chocolateyInstall.ps1` to the new release asset, rebuild, and push.
