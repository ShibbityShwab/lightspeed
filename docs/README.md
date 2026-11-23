# <picture><source media="(prefers-color-scheme: dark)" srcset="../web/assets/brand/lightspeed-mark-inverse.svg"><img src="../web/assets/brand/lightspeed-mark.svg" width="26" height="26" align="absmiddle" alt=""></picture> LightSpeed Documentation

Welcome to the LightSpeed documentation wiki - synced from the [source repo](https://github.com/ShibbityShwab/lightspeed/tree/master/docs).

---

## 🚀 Getting Started

| Document | Description |
|----------|-------------|
| [Install on Windows](install-windows) | GUI-first Windows setup (MSI + zip) |
| [Install on macOS](install-macos) | CLI-first macOS setup (Intel + Apple Silicon) |
| [Install on Linux](install-linux) | CLI-first Linux setup (x86_64 + ARM64) |
| [User Guide](user-guide) | Step-by-step setup and usage |
| [CLI Reference](CLI-REFERENCE) | All commands and flags |
| [FAQ](faq) | Common questions and answers |
| [Supported Games](supported-games) | Game profiles and compatibility |

## 🔧 Operations

| Document | Description |
|----------|-------------|
| [Deploy Proxy](deploy-proxy) | Self-host a proxy on any VPS |
| [Community Relay Network](community-network) | Run + publish a relay for others |
| [Troubleshooting](troubleshooting) | Diagnose and fix issues |
| [Game Requests](game-requests) | How a game request becomes a profile (or an honest "cannot") |
| [Control-Handshake Trend](m1-handshake-trend) | M1 (alert-120) trend in the published history and the `m1_reject_drain` detector |
| [Agent Operations](agent-operations) | How AI agents drive this repo (DeepSeek Harness, personas, team mode) |

## 🏗️ Architecture & Protocol

| Document | Description |
|----------|-------------|
| [Architecture](architecture) | System design and data flow |
| [Protocol](protocol) | Wire protocol specification |
| [QUIC ALPN Rollout](quic-alpn-rollout) | Versioned control-plane ALPN: compatibility matrix and staged rollout plan |
| [Security Audit](security-audit-mvp) | MVP security review |

## 📋 Reference

| Document | Description |
|----------|-------------|
| [Glossary](glossary) | Terminology definitions |
| [Accessibility](accessibility) | The Windows UI Automation tree, automation ids, live regions and how to verify them |
| [Privacy](privacy) | Privacy policy and telemetry |
| [Agent-System Audit](agent-system-audit-2026-11-16) | The 2026-11-16 DeepSeek Harness migration record |
| [GitHub Feedback Audit](github-feedback-audit-2026-11-16) | Triage of the open issues, discussions and PR at 1.10.3 |
| [Project Audit](project-audit-2026-11-18) | Code, CI, docs, GitHub and fleet audit at 1.11.0, with the prioritized fix list |
| [Security Advisory Review](security-advisory-review-2026-11-18) | The 2026-11-18 advisory allowlist review and re-review cadence |
| [Fleet ALPN Options](fleet-alpn-options-2026-11-18) | Quantified fleet `control_alpn` state and the owner decision options |
| [App Refactor Plan](design/app-refactor-plan) | Measured split plan for `client-gui/src/app.rs` |
| [Contributing](../CONTRIBUTING.md) | Build, test, and submit changes on Windows, macOS or Linux |
