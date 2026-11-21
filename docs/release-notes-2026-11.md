# LightSpeed release notes — November 2026 (DRAFT)

> **Status: draft, not a release.** Prepared 2026-11-21; updated after the
> owner-selected **Direction C "Broadcast console"** redesign was implemented and
> merged (PR #196). The version **1.12.0** was selected by the owner on
> 2026-11-21 (SemVer minor for the new desktop interface), but **no tag, publish,
> package bump or dispatch has happened** — the release remains an owner gate.
> The GitHub release body is the `CHANGELOG.md` version section extracted verbatim
> by `infra/scripts/release-notes.sh`; this page is the announcement bullet list
> plus the honest verification statement, not the release body itself.

## Highlights

- **A new desktop app — "Broadcast console".** LightSpeed's status window is now
  a dark telemetry console: a 320 px column whose state word always fits, a mono
  data rail, a dot-matrix fleet globe, one lime action control, a clear
  `NO RETURN PATH` warning, a Details ledger, and a restyled Settings/update/
  editor set across the nine shipped locales — with the same honest route data,
  warn-only behavior and settings as before.
- **Honest information, never invented.** Server location is a labelled ladder —
  cloud-provider region, operator geofeed, hosting network, country estimate,
  otherwise unknown — and the route trace names the last responding hop's
  provider or hosting network. Display-only: it never steers relay choice, and
  the client makes no network calls to place a server.
- **No silent route changes.** If a relay stops returning your game traffic,
  LightSpeed keeps relaying and warns (amber tray, `NO RETURN PATH`, danger
  banner) instead of quietly switching you to a direct path; STOP OPTIMIZING is
  the manual change. Relays also log their own per-session starvation counters.
- **One config file for both front ends.** The GUI and CLI share
  `lightspeed.toml`; an existing GUI `config.toml` is imported read-only and
  never modified, and Reset to defaults now really resets.
- **Native behaviour per OS.** Tray/menu including the starvation state,
  shortcut keymaps, an explicit elevation handshake, autostart, install-channel-
  honest updater copy, and OS reduced-motion respect. Linux and macOS GUI
  builds, lint and tests (and a headless Xvfb window smoke) now run on PRs.

## What you need to do

- **Nothing.** Your settings are preserved — the legacy GUI `config.toml` is
  imported read-only — and the retired `retry_after_fallback` key is ignored
  while old config files still load.
- **CLI users:** no command, flag, output or exit-code change; the CLI stays
  golden-tested.
- **One behaviour change to know about:** automatic relay fallback/retry is
  replaced by the warn-only policy above.

## Verification (three tiers — never collapsed into "parity")

| Tier | Windows | Linux | macOS |
|---|---|---|---|
| Build | verified | verified (CI `linux-gui`) | verified (CI `macos-gui`) |
| Headless | verified (dev-host captures) | window smoke in CI, **advisory** (probe hardening in progress) | window probe in CI, **advisory and unproven** |
| Runtime | verified on the dev host (live states below) | **unverified — needs hardware** | **unverified — needs hardware; the README lists the GUI as untested** |

CI note for the release announcement: the headless Xvfb smoke is
`continue-on-error` advisory while its title probe is hardened, so it must not
be described as a passing gate in the release copy. Linux/macOS runtime
behaviour must stay labelled unverified until the owner names hardware.

## Known unverified (not claims)

- Linux/macOS GUI runtime behaviour: tray/menu interaction, 200 % scaling,
  elevation → boost, autostart login, updater click-through.
- Interactive Windows UAC accept/decline path (the dev harness is elevated).
- Live tray/menu capture of the starving-relay state (needs a starving relay).
- Translation debt (Q5: English-identical keys in some locales) and the CJK
  font-subset gap (Q19).

Full register: [gui-refresh-closeout.md](design/gui-refresh-closeout.md) §4;
parity tiers: [cross-platform-2026.md](design/cross-platform-2026.md) §6.3.

## Release mechanics (for the owner; not a release)

- Publishing is a **tag push** `vX.Y.Z`: the Release workflow builds the artifact
  matrix, publishes the GitHub Release and bumps the packaging manifests. The
  release body is the `[X.Y.Z]` CHANGELOG section verbatim, so the CHANGELOG
  entry must land before the tag.
- Channels: Homebrew/Chocolatey/Scoop manifests bump automatically; the Scoop
  bucket publish needs the `SCOOP_TOKEN` secret (currently missing), AUR needs a
  manual `pkgver`/sha256 bump, and winget needs the open bootstrap PR
  (#481953) merged first — see [publishing-channels.md](publishing-channels.md).
- Owner gates: version choice + release approval, tag push, Announcements/
  Discord post, `SCOOP_TOKEN`, winget/AUR/Chocolatey follow-through. $0.00, no
  paid services or signing.

## Version note

The owner decides between `1.11.5` (matches the recent patch-release cadence)
and `1.12.0` (SemVer minor for the new desktop interface). This draft names
neither; the CHANGELOG stays under `[Unreleased]` until that decision.
