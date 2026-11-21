#!/usr/bin/env bash
# LightSpeed GUI headless smoke (CI-only; Linux runner).
#
# Proves the GUI creates a real window on a display-less runner and, with the
# G0.2 launch hook present, exits cleanly. The caller supplies `xvfb-run` and a
# wall-clock timeout; this script launches the debug binary, proves the window
# through the OS (xdotool: title + PID + geometry), then requires the hook's own
# log markers, and reports one verdict.
#
# Invoked by the `gui-headless-smoke` job in .github/workflows/ci.yml; the
# `--self-check` mode runs the same decision function over synthetic inputs (no
# X server needed) and is wired as an advisory CI step so the negative controls
# are printed on every run.
#
# Advisory until it has passed on ~10 consecutive runs
# (docs/design/cross-platform-2026.md §6.1-§6.2). Promotion = deleting
# `continue-on-error`; never delete the step silently.
#
# G0.2 hook contract (owned by `desktop-platform`, task-61; fixed in task-81):
#   1. `GUI_SMOKE window_created title="LightSpeed" pixels_per_point=<f32> backend=<OS> renderer=<Debug>`
#   2. `GUI_SMOKE exit=0 frames=<n>` (n >= 1)
# The hook logs the title the app sets (`APP_TITLE`), so the marker is exact on
# every backend (X11 used to log the runtime viewport title, which is empty on
# the first frame there). The window is held open for a bounded moment and then
# closes itself; the process exits 0. It still takes the single-instance guard,
# so a stray instance exits 2 - a real second instance, not a retryable flake.
#
# Fail-closed rules (WF-081b task-81, QA C3-C7):
#   * the OS-title proof is mandatory: xdotool must exist, the matched window's
#     PID must be THIS run's PID, and its geometry must be plausible; any
#     missing/ambiguous/failed check FAILS the probe (there is no "logs only"
#     pass any more);
#   * the hook markers must both appear, `frames >= 1`, the process must exit 0,
#     and the probe must not have had to kill it;
#   * the pre-hook fallback is unreachable in smoke mode (this script always
#     sets LIGHTSPEED_GUI_SMOKE=1) and would still require a real OS window;
#   * on macOS the workflow step applies the same marker + clean-exit rules and
#     fails a killed run instead of masking exit 143.
#
# NOT a Windows dev-host script: it needs Xvfb + xdotool and is never invoked by
# the PowerShell gates or by tools/Check-LightSpeed.ps1.
set -euo pipefail

BIN="${1:-./target/debug/lightspeed-gui}"
TITLE='^LightSpeed$'
WINDOW_TITLE='LightSpeed'
LOG="${HOME}/.local/share/Lightspeed/gui-trace.log"
FALLBACK_LOG="${TMPDIR:-/tmp}/lightspeed-gui-trace.log"
PROBE_STEPS=300          # x 0.2 s = up to 60 s for the OS window + markers
HOOK_GRACE_STEPS=75      # x 0.2 s = up to 15 s for the hook's own clean exit
FRAMES_MIN=1
WINDOW_MARKER="GUI_SMOKE window_created title=\"${WINDOW_TITLE}\""
EXIT_MARKER='GUI_SMOKE exit=0'
ALLOW_PRE_HOOK="${SMOKE_ALLOW_PRE_HOOK:-0}"

fail() {
  echo "smoke: FAIL - $*" >&2
  exit 1
}

# The decision function. Every probe path (live run and --self-check) uses it, so
# the truth table below is the real pass/fail logic.
#   $1 window_marker 0|1   $2 exit_marker 0|1   $3 frames (integer)
#   $4 app_exit status     $5 terminated 0|1 (probe had to kill the app)
#   $6 os_window_ok 0|1    (title + PID + geometry proven through xdotool)
verdict() {
  local window_marker=$1 exit_marker=$2 frames=$3 status=$4 terminated=$5 window_ok=$6
  if [ "$window_ok" -ne 1 ]; then echo FAIL; return; fi
  if [ "$window_marker" -ne 1 ] || [ "$exit_marker" -ne 1 ]; then echo FAIL; return; fi
  if [ "$frames" -lt "$FRAMES_MIN" ]; then echo FAIL; return; fi
  if [ "$status" -ne 0 ]; then echo FAIL; return; fi
  if [ "$terminated" -ne 0 ]; then echo FAIL; return; fi
  echo PASS
}

# Deterministic negative/positive controls for the decision logic (QA C7). Runs
# without Xvfb and without the GUI binary.
self_check() {
  local failures=0
  run_case() {
    local name=$1 expected=$2
    shift 2
    local got
    got=$(verdict "$@")
    if [ "$got" = "$expected" ]; then
      echo "smoke self-check: PASS  ${name} -> ${got}"
    else
      echo "smoke self-check: FAIL  ${name} -> ${got} (expected ${expected})" >&2
      failures=$((failures + 1))
    fi
  }
  echo 'smoke self-check: decision matrix (window_marker exit_marker frames status terminated os_window)'
  run_case 'new hook, all proof present' PASS 1 1 3 0 0 1
  run_case 'old hook, title marker empty/absent' FAIL 0 0 0 0 0 1
  run_case 'window-less run (no OS-title proof)' FAIL 0 0 0 0 0 0
  run_case 'killed run (probe had to kill it)' FAIL 1 1 3 143 1 1
  run_case 'markers but nonzero app exit' FAIL 1 1 3 1 0 1
  run_case 'markers but no frames recorded' FAIL 1 1 0 0 0 1
  if [ "$failures" -ne 0 ]; then
    echo "smoke self-check: FAILED (${failures} row(s) disagreed)" >&2
    exit 1
  fi
  echo 'smoke self-check: PASS (6/6 decision rows)'
}

if [ "${1:-}" = "--self-check" ]; then
  self_check
  exit 0
fi

if [ ! -x "$BIN" ]; then
  echo "smoke: no executable at ${BIN} (build the GUI first)" >&2
  exit 1
fi
if ! command -v xdotool >/dev/null 2>&1; then
  # Fail closed (QA C3): without xdotool the OS-title/PID/geometry proof cannot
  # be made, and a log-only pass is exactly the false positive this task kills.
  fail 'xdotool is missing; the OS-title proof cannot be made (install xdotool)'
fi

# A software GL stack is what Xvfb can provide. Keep the renderer on glow so the
# run does not depend on a Vulkan adapter (client-gui/src/main.rs honors
# LIGHTSPEED_GUI_RENDERER).
export LIGHTSPEED_GUI_SMOKE=1
export LIGHTSPEED_GUI_RENDERER=glow
export LIBGL_ALWAYS_SOFTWARE=1

rm -f "$LOG" "$FALLBACK_LOG"

"$BIN" &
app=$!

has_marker() {
  grep -qF "$1" "$2" 2>/dev/null
}

# Phase 1: prove the window through the OS and wait for both hook markers.
window=""
os_window_ok=0
i=0
while [ "$i" -lt "$PROBE_STEPS" ]; do
  i=$((i + 1))
  if [ "$os_window_ok" -eq 0 ]; then
    window=$(xdotool search --name "$TITLE" 2>/dev/null | head -n 1 || true)
    if [ -n "$window" ]; then
      win_pid=$(xdotool getwindowpid "$window" 2>/dev/null || true)
      if [ "$win_pid" != "$app" ]; then
        fail "OS-title window ${window} belongs to PID ${win_pid:-unknown}, not this smoke run (${app})"
      fi
      geom=$(xdotool getwindowgeometry --shell "$window" 2>/dev/null || true)
      width=$(printf '%s\n' "$geom" | sed -n 's/^WIDTH=//p' | head -n 1)
      height=$(printf '%s\n' "$geom" | sed -n 's/^HEIGHT=//p' | head -n 1)
      case "${width:-x}" in *[!0-9]*) width="" ;; esac
      case "${height:-x}" in *[!0-9]*) height="" ;; esac
      if [ -z "$width" ] || [ -z "$height" ] || [ "$width" -lt 200 ] || [ "$height" -lt 200 ]; then
        fail "OS-title window ${window} has implausible geometry (${width:-?}x${height:-?})"
      fi
      os_window_ok=1
      echo "smoke: OS window ${window} title=${WINDOW_TITLE} pid=${win_pid} geometry=${width}x${height}"
    fi
  fi
  window_marker=0
  exit_marker=0
  for candidate in "$LOG" "$FALLBACK_LOG"; do
    if has_marker "$WINDOW_MARKER" "$candidate"; then
      window_marker=1
    fi
    if has_marker "$EXIT_MARKER" "$candidate"; then
      exit_marker=1
    fi
  done
  if [ "$window_marker" -eq 1 ] && [ "$exit_marker" -eq 1 ]; then
    break
  fi
  if ! kill -0 "$app" 2>/dev/null; then
    break
  fi
  sleep 0.2
done

# Phase 2: the hook is expected to close the window and exit by itself; wait a
# bounded grace period instead of killing a clean run.
i=0
while [ "$i" -lt "$HOOK_GRACE_STEPS" ] && kill -0 "$app" 2>/dev/null; do
  i=$((i + 1))
  sleep 0.2
done

terminated=0
if kill -0 "$app" 2>/dev/null; then
  terminated=1
  kill "$app" 2>/dev/null || true
fi
status=0
wait "$app" 2>/dev/null || status=$?

# Phase 3: read the markers and the frame count out of whichever sink the app used.
window_marker=0
exit_marker=0
frames=0
legacy_created=0
log_path=""
for candidate in "$LOG" "$FALLBACK_LOG"; do
  [ -f "$candidate" ] || continue
  if has_marker "$WINDOW_MARKER" "$candidate"; then
    window_marker=1
    log_path="$candidate"
  fi
  if has_marker "$EXIT_MARKER" "$candidate"; then
    exit_marker=1
    log_path="$candidate"
  fi
  if has_marker 'app created' "$candidate"; then
    legacy_created=1
  fi
  count=$(sed -n 's/.*GUI_SMOKE exit=0 frames=\([0-9][0-9]*\).*/\1/p' "$candidate" 2>/dev/null | tail -n 1)
  if [ -n "$count" ]; then
    frames=$count
  fi
done

result=$(verdict "$window_marker" "$exit_marker" "$frames" "$status" "$terminated" "$os_window_ok")

echo "smoke: window_marker=${window_marker} exit_marker=${exit_marker} frames=${frames} os_window=${os_window_ok} terminated=${terminated} app_exit=${status} legacy_created=${legacy_created} verdict=${result}"
if [ -n "$window" ]; then
  echo "smoke: window id ${window} matched title ${TITLE}"
fi
if [ -n "$log_path" ]; then
  echo "smoke: probe log ${log_path}"
fi

if [ "$result" = "PASS" ]; then
  echo "smoke: PASS - G0.2 hook: window title '${WINDOW_TITLE}' proven by PID ${app} + geometry, process exited 0 (frames=${frames})"
  exit 0
fi

# Pre-hook fallback: unreachable in smoke mode (this script always sets
# LIGHTSPEED_GUI_SMOKE=1). Kept for documentation only: even when explicitly
# allowed it would still require the real OS window proof, never a log-only pass.
if [ "$ALLOW_PRE_HOOK" = "1" ]; then
  if [ "${LIGHTSPEED_GUI_SMOKE:-}" = "1" ]; then
    fail 'pre-hook fallback is unreachable in smoke mode (the G0.2 hook must be used)'
  fi
  if [ "$legacy_created" -eq 1 ] && [ "$os_window_ok" -eq 1 ]; then
    echo "smoke: PASS - pre-hook fallback: real window proven through the OS; G0.2 markers absent (hook not in this tree)"
    exit 0
  fi
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
