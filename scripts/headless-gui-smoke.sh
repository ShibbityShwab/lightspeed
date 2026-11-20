#!/usr/bin/env bash
# LightSpeed GUI headless smoke (CI-only; Linux runner).
#
# Proves the GUI creates a real window on a display-less runner and, when the
# G0.2 launch hook is present, exits cleanly. The caller supplies `xvfb-run`
# and a wall-clock timeout; this script launches the debug binary, waits for the
# hook's log markers (or, before the hook lands, the documented xdotool title +
# `app created` fallback), shuts the process down, and reports one verdict.
#
# Invoked by the `gui-headless-smoke` job in .github/workflows/ci.yml.
# Advisory until it has passed on ~10 consecutive runs
# (docs/design/cross-platform-2026.md §6.1-§6.2).
#
# G0.2 hook contract (owned by `desktop-platform`, task-61; both lines land in
# the normal trace log ~/.local/share/Lightspeed/gui-trace.log or the temp
# fallback):
#   1. `GUI_SMOKE window_created title="LightSpeed" pixels_per_point=<f32> backend=<OS> renderer=<Debug>`
#   2. `GUI_SMOKE exit=0 frames=<n>`
# The hook is deterministic: the window is titled `LightSpeed`, closes itself
# after ~2 frames and the process exits 0; a `run_native` error exits 1. The
# hook still takes the single-instance guard, so a stray instance exits 2 -
# that is a real second instance, not a retryable smoke flake.
#
# Before the hook exists (HEAD before task-61) the script falls back to the
# documented `xdotool search --name '^LightSpeed$'` title probe (§6.2) plus the
# existing `app created` startup line, then terminates the process itself.
#
# NOT a Windows dev-host script: it needs Xvfb + xdotool and is never invoked by
# the PowerShell gates or by tools/Check-LightSpeed.ps1.
set -euo pipefail

BIN="${1:-./target/debug/lightspeed-gui}"
TITLE='^LightSpeed$'
LOG="${HOME}/.local/share/Lightspeed/gui-trace.log"
FALLBACK_LOG="${TMPDIR:-/tmp}/lightspeed-gui-trace.log"
PROBE_SECONDS=30
HOOK_GRACE_SECONDS=10
WINDOW_MARKER='GUI_SMOKE window_created title="LightSpeed"'
EXIT_MARKER='GUI_SMOKE exit=0'

if [ ! -x "$BIN" ]; then
  echo "smoke: no executable at ${BIN} (build the GUI first)" >&2
  exit 1
fi

# A software GL stack is what Xvfb can provide. Keep the renderer on glow so
# the run does not depend on a Vulkan adapter (client-gui/src/main.rs honors
# LIGHTSPEED_GUI_RENDERER).
export LIGHTSPEED_GUI_SMOKE=1
export LIGHTSPEED_GUI_RENDERER=glow
export LIBGL_ALWAYS_SOFTWARE=1

rm -f "$LOG" "$FALLBACK_LOG"

"$BIN" &
app=$!

# Wait for the deterministic hook marker, or for the window in the pre-hook
# fallback. The hook exits on its own, so poll the log as well as the process.
window=""
hook_seen=0
i=0
while [ "$i" -lt "$PROBE_SECONDS" ]; do
  i=$((i + 1))
  sleep 1
  for candidate in "$LOG" "$FALLBACK_LOG"; do
    if grep -q "$EXIT_MARKER" "$candidate" 2>/dev/null; then
      hook_seen=1
      break
    fi
  done
  if [ "$hook_seen" -eq 1 ]; then
    break
  fi
  window=$(xdotool search --name "$TITLE" 2>/dev/null | head -n 1 || true)
  if [ -n "$window" ]; then
    break
  fi
  if ! kill -0 "$app" 2>/dev/null; then
    break
  fi
done

# The hook is expected to close the window and exit by itself; give it a bounded
# grace period instead of killing a clean run.
i=0
while [ "$i" -lt "$HOOK_GRACE_SECONDS" ] && kill -0 "$app" 2>/dev/null; do
  i=$((i + 1))
  sleep 1
done

terminated=0
status=0
if kill -0 "$app" 2>/dev/null; then
  terminated=1
  kill "$app" 2>/dev/null || true
fi
wait "$app" 2>/dev/null || status=$?

title_ok=0
if [ -n "$window" ]; then
  title_ok=1
fi

# Read the markers out of whichever log sink the app used.
window_marker=0
exit_marker=0
legacy_created=0
log_path=""
for candidate in "$LOG" "$FALLBACK_LOG"; do
  [ -f "$candidate" ] || continue
  if [ -z "$log_path" ]; then
    log_path="$candidate"
  fi
  if grep -q "$WINDOW_MARKER" "$candidate" 2>/dev/null; then
    window_marker=1
    log_path="$candidate"
  fi
  if grep -q "$EXIT_MARKER" "$candidate" 2>/dev/null; then
    exit_marker=1
    log_path="$candidate"
  fi
  if grep -q 'app created' "$candidate" 2>/dev/null; then
    legacy_created=1
  fi
done

echo "smoke: window_marker=${window_marker} exit_marker=${exit_marker} legacy_created=${legacy_created} title_probe=${title_ok} terminated=${terminated} app_exit=${status}"
if [ -n "$window" ]; then
  echo "smoke: window id ${window} matched title ${TITLE}"
fi
if [ -n "$log_path" ]; then
  echo "smoke: probe log ${log_path}"
fi

if [ "$window_marker" -eq 1 ]; then
  # Deterministic G0.2 path: a window titled `LightSpeed` was created and the
  # process must have logged its clean exit 0.
  if [ "$exit_marker" -ne 1 ] || [ "$status" -ne 0 ]; then
    echo "smoke: FAIL - G0.2 hook logged the window but not a clean exit (exit_marker=${exit_marker}, app_exit=${status})" >&2
    exit 1
  fi
  echo "smoke: PASS - G0.2 hook: window title 'LightSpeed' created, process exited 0"
  exit 0
fi

# Pre-hook fallback (the documented §6.2 probe): window creation is proven by
# the `app created` line plus either the title probe or a clean self-exit 0.
if [ "$legacy_created" -eq 1 ] && { [ "$title_ok" -eq 1 ] || { [ "$terminated" -eq 0 ] && [ "$status" -eq 0 ]; }; }; then
  echo "smoke: PASS - pre-hook fallback: window creation observed; G0.2 markers absent (hook not in this tree yet) - advisory until the hook lands"
  exit 0
fi

echo "smoke: FAIL - window/log probe did not pass under Xvfb" >&2
if [ "$status" -eq 2 ]; then
  echo "smoke: exit 2 is the single-instance guard (a stray lightspeed-gui instance on this runner), not a GL failure" >&2
fi
for candidate in "$LOG" "$FALLBACK_LOG"; do
  if [ -f "$candidate" ]; then
    echo "--- tail of ${candidate} ---" >&2
    tail -n 40 "$candidate" >&2 || true
  fi
done
exit 1
