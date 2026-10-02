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
An API key is created per-account at
<https://community.chocolatey.org/account> and is the only missing piece here;
set it as a `CHOCO_API_KEY` repository secret to let a workflow push.

**Status: version 1.6.14 is BUILT and READY TO PUSH; the only blocker is the
missing API key.** The package source in this directory is already correct
(`nuspec` version 1.6.14, install script pinned to the v1.6.14 asset with its
verified sha256), and `target/lightspeed.1.6.14.nupkg` builds clean, but no
Chocolatey API key exists on this machine or in the repo's secrets, so the
push cannot be automated from here.

The community feed's newest published version is **1.6.3** - eleven releases
behind - because the 1.6.5 submission has been pending moderation since
2026-10-01 and no later version was ever pushed. Note the feed only ever
listed `1.6.3`; a `1.6.14` package does not exist there.

With Chocolatey installed (Windows):

```sh
choco push lightspeed.1.6.14.nupkg --source https://push.chocolatey.org/ --api-key <API_KEY>
```

Or from any platform with curl, against the NuGet v2 push endpoint:

```sh
curl -X PUT -H "X-NuGet-ApiKey: <API_KEY>" \
  --data-binary @lightspeed.1.6.14.nupkg \
  https://push.chocolatey.org/api/v2/package/
```

Rebuild the package after any change to the source with `cd dist/chocolatey
&& ./build.sh` (it needs no Chocolatey install; it assembles the OPC zip
directly).

## Updating

Bump `version` in `lightspeed.nuspec`, update `url64bit` and `checksum64` in
`tools/chocolateyInstall.ps1` to the new release asset, rebuild, and push.
The release workflow's `bump-chocolatey` job already does the bump on every
tag (`infra/scripts/bump-chocolatey.sh`), so the source stays current - only
the push is manual.
