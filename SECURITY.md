# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.5.x   | ✅ Active support  |
| 0.4.x   | ❌ End of life     |
| < 0.4   | ❌ End of life     |

## Reporting a Vulnerability

**Do not open a public issue.** Instead, report vulnerabilities privately:

1. **GitHub Security Advisory** — Go to the [Security tab](https://github.com/ShibbityShwab/lightspeed/security/advisories) and click "Report a vulnerability"
2. **Email** — If you prefer, contact the maintainer directly

You should receive a response within **48 hours**. We take all security reports seriously.

### What to Include

- Description of the vulnerability
- Steps to reproduce
- Affected versions
- Any potential mitigations you've identified

### Process

1. Acknowledge receipt within 48 hours
2. Validate and assess severity
3. Develop and test a fix
4. Release a patch
5. Publish the advisory (credit given unless you prefer to remain anonymous)

## Security Design

LightSpeed is built with security in mind:

- **No encryption on data plane** — game traffic remains inspectable by anti-cheat systems
- **Token-based authentication** — session tokens validated per-packet
- **Rate limiting** — per-client PPS and BPS limits prevent abuse
- **Destination validation** — blocks forwarding to private/internal IPs
- **Anti-amplification** — inbound/outbound byte ratio tracking
- **No secrets in source** — all credentials via environment variables

See the [Security Audit](docs/security-audit-mvp.md) for the full MVP review.
