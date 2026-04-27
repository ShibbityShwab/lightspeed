> **ARCHIVED 2026-04-27** — Launch initiative paused indefinitely per maintainer request.
> Do not reference, suggest, or surface this content unless explicitly asked by the maintainer.

---

# LightSpeed Discord Server — Setup Guide

This is a ready-to-execute template for standing up a Discord community server. When ready to launch, create the server and replicate the channel/role structure below. Estimated time: ~10 minutes.

---

## Server Name & Icon

- **Name:** LightSpeed
- **Icon:** Upload the landing-page logo or a simple lightning bolt ⚡ PNG
- **Description:** Free open-source game network optimizer. Cut ping, keep $0.

---

## Roles

| Role | Colour | Who gets it | Permissions |
|---|---|---|---|
| `@Owner` | Gold | You | All |
| `@Moderator` | Blue | Trusted contributors | Manage messages, kick, ban |
| `@Community Node Operator` | Teal | Anyone running a self-hosted proxy node | Standard + pin messages in #node-operators |
| `@Beta Tester` | Green | Anyone who has tried the client | Standard |
| `@Member` | Default | Automatic on join | Read + send |

### Auto-role on join
Use a bot (Carl-bot or MEE6 free tier) to assign `@Member` automatically on server join.

---

## Category & Channel Structure

### 📢 ANNOUNCEMENTS
| Channel | Type | Purpose |
|---|---|---|
| `#announcements` | Text, read-only for @Member | Release notes, major updates |
| `#changelog` | Text, read-only for @Member | Auto-post from GitHub releases (use a GitHub webhook) |

### 👋 START HERE
| Channel | Type | Purpose |
|---|---|---|
| `#rules` | Text, read-only | Server rules (see below) |
| `#welcome` | Text, read-only | Welcome message + quick links |
| `#roles` | Text | React-to-role for Beta Tester / Node Operator |

### 💬 GENERAL
| Channel | Type | Purpose |
|---|---|---|
| `#general` | Text | Off-topic, introductions |
| `#showcase` | Text | Share your ping improvements / screenshots |
| `#ping-results` | Text | Before/after ping comparisons — great signal for marketing |

### 🐛 SUPPORT & FEEDBACK
| Channel | Type | Purpose |
|---|---|---|
| `#help` | Text | Installation & usage questions |
| `#bug-reports` | Text | Client bugs (redirect to GitHub Issues for tracking) |
| `#feature-requests` | Text | Region requests, game profile requests |
| `#anti-cheat-questions` | Text | Dedicated channel — will be high-traffic |

### 🌐 COMMUNITY NODES
| Channel | Type | Purpose |
|---|---|---|
| `#node-operators` | Text | Self-hosters coordinating community proxy nodes |
| `#mesh-status` | Text | Node operators post health updates |

### 🔧 DEVELOPMENT
| Channel | Type | Purpose |
|---|---|---|
| `#dev-discussion` | Text | Technical discussion, PR reviews |
| `#ci-status` | Text | GitHub Actions webhook for build pass/fail |

---

## Welcome Message (post in #welcome)

```
⚡ Welcome to LightSpeed!

LightSpeed is a free, open-source game network optimizer — like ExitLag, but $0 forever.

**Quick links:**
• GitHub: https://github.com/ShibbityShwab/lightspeed
• Latest release (v0.3.0): https://github.com/ShibbityShwab/lightspeed/releases/tag/v0.3.0
• Landing page: https://shibbityshwab.github.io/lightspeed/

**Get started:**
1. Grab the binary from the release page (Windows/Linux/ARM64)
2. Follow the README to configure your game profile
3. Post your before/after ping in #ping-results!

**Want to run a community proxy node?** Head to #node-operators.

**Found a bug?** Post in #bug-reports or open a GitHub Issue.
```

---

## Server Rules (post in #rules)

```
1. Be respectful — this is a technical community, not a support battleground.
2. No spam, self-promotion, or advertising.
3. Bug reports with reproduction steps only — vague "it doesn't work" posts will be redirected to #help.
4. Anti-cheat questions are welcome. Discussions about actual cheating are not and will result in a ban.
5. English preferred in main channels; bilingual is fine.
6. GitHub Issues are the source of truth for bugs/features. Discord is for discussion and support.
```

---

## GitHub Webhooks to Wire Up

| Webhook target | GitHub event | What it posts |
|---|---|---|
| `#changelog` | Release published | New release announcement with changelog |
| `#ci-status` | Workflow run (CI/deploy) | Build pass/fail status |

To add a webhook: **Discord channel Settings → Integrations → Webhooks → New Webhook → Copy URL** → paste into GitHub repo Settings → Webhooks.

---

## Recommended Bots (all free tier)

| Bot | Purpose |
|---|---|
| **Carl-bot** | Auto-role on join, reaction roles in #roles |
| **MEE6** | Welcome DM, basic moderation commands |
| *(optional)* **GH Actions Bot** | Native GitHub status notifications |

---

## Day-1 Seed Posts

Post these immediately after creating the server to make it look active:

1. **#announcements** — "LightSpeed v0.3.0 is live! Windows + Linux + ARM64 pre-built binaries: [release link]"
2. **#ping-results** — Post a screenshot of your own before/after ping improvement (or the load-test results from the docs)
3. **#node-operators** — "Community nodes: proxy-lax (LA) and relay-sgp (Singapore) are live. Looking for volunteers in EU, India, OCE."
4. **#dev-discussion** — "First priorities for v0.4.0: in-app opt-in telemetry for real-world latency data, additional game profiles. What should come first?"

---

## Invite Link

Once the server is set up, create a permanent invite link with no expiry:

**Server Settings → Invites → Create Invite → Never expire → Copy link**

Add this link to:
- [ ] `README.md` badge / community section
- [ ] `web/index.html` landing page
- [ ] GitHub Discussions bio
- [ ] Reddit post footers (future posts)
