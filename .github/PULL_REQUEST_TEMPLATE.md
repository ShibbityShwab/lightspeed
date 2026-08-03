## Description

<!-- Briefly describe what this PR does and why. -->

## Type of Change

- [ ] 🐛 Bug fix
- [ ] ✨ New feature
- [ ] 🎮 New game support
- [ ] 📚 Documentation
- [ ] 🧪 Tests
- [ ] ⚡ Performance
- [ ] 🔒 Security
- [ ] 🧹 Refactor / cleanup

## Checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --workspace --all-targets --exclude lightspeed-gui` — zero warnings
- [ ] `cargo test --workspace --exclude lightspeed-gui` — all tests pass
- [ ] `cargo build --release --workspace --exclude lightspeed-gui` — compiles
- [ ] Public API changes documented with doc comments (`///`)
- [ ] New code has tests covering the primary paths
- [ ] No new `unwrap()` in production code (use `?` or proper error types)
- [ ] `cargo audit` passes (no new advisories)

## Related Issues

<!-- Link to issues this PR fixes or relates to. -->
Fixes #

## Screenshots / Logs

<!-- If applicable, add screenshots or log output. -->
