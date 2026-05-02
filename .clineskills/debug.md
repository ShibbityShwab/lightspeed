# @debug — Agentic Test-Fix Loop

> Tight iterative debug loop for the LightSpeed WAT system.
> Invoke with "@debug" after making a change, or after a CI failure.

## Behavior

When invoked, execute this strict test-fix loop:

### 1. Read Context
- Read `wat/state/current-phase.md` for current task state
- Read `wat/TASK.md` (if it exists) for success criteria and test commands
- Read `wat/state/decisions.md` (last 5 entries) for recent context

### 2. Run Tests
Run the full workspace test suite:
```bash
cargo test --workspace --all --exclude lightspeed-gui 2>&1
```

If TASK.md specifies custom test commands, run those as well:
```bash
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui 2>&1
```

### 3. Analyze Failures
- Parse test output for FAILED tests
- Group failures by: compilation errors / test assertions / clippy lints
- Identify the root cause for each failure group

### 4. Fix
- Fix the root cause (never the symptom)
- After each fix attempt, return to Step 2

### 5. Escalate
If the same failure persists after **3 fix attempts**:
- Log the failure to `wat/state/decisions.md` with full error output
- Update `wat/state/current-phase.md` — set blocked = true, describe the blocker
- Output: "Blocked on [description]. Need human guidance."

### 6. Report Success
When all tests pass:
- Update `wat/state/current-phase.md` — mark step as verified
- Log success to `wat/state/decisions.md` with a summary of what was fixed
- Output: "All tests passing. [N] fixes applied: [summary]."