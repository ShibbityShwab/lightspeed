---
name: Bug Report
about: Something isn't working correctly
title: "[Bug] "
labels: bug
assignees: ''
---

**Describe the bug**
A clear and concise description of what went wrong.

**To Reproduce**
Steps to reproduce the behavior:
1. Ran `lightspeed ...`
2. Connected to game server at `...`
3. Observed `...`

**Expected behavior**
What should have happened instead.

**Environment:**
- OS: [e.g. Windows 11, Ubuntu 24.04, macOS 15]
- LightSpeed version: [e.g. v0.5.1 — run `lightspeed --version`]
- Game: [e.g. Rust, CS2]
- Proxy region: [e.g. US-West, Singapore]

**Logs**
Run with debug logging and attach the output:
```bash
RUST_LOG=debug lightspeed [your command] 2>&1 | tee lightspeed.log
```

**Additional context**
Any other information that might help.
