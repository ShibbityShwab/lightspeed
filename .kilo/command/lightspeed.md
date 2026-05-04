# /lightspeed — Run the WAT Autonomy Loop

> Execute the LightSpeed WAT autonomy loop.

## Usage

```
/lightspeed [start|continue|status]
```

## Arguments

- `start` — Begin the autonomy loop from current phase
- `continue` — Continue from the current step
- `status` — Show current phase status

## Behavior

1. Reads `wat/state/current-phase.md` to identify the active agent and next action
2. Reads `wat/state/decisions.md` for context
3. Executes the next action
4. Runs verification (cargo test, cargo clippy)
5. Updates state files