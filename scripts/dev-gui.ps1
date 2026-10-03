param([switch]$Elevate)

# Run the GUI straight from target\debug, optionally elevated.
#
# This exists so verifying a UI change does not require installing first. The
# installed copy lives in Program Files, which needs admin to write, and the
# running app locks its own exe - so the old loop was: quit the app, trigger a
# UAC prompt, copy, relaunch, and do it again for the next tweak. Running the
# debug build elevated gives the same window with none of that.
#
# It also refuses to start a second instance: the app's single-instance guard
# shares one named mutex across every build, so a stray copy from a preview
# directory silently wins and the new launch exits - which is exactly how a
# stale build once looked "installed" while the old process kept running.

$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\debug\lightspeed-gui.exe'

if (-not (Test-Path $exe)) {
  Write-Output "no debug build at $exe - run: cargo build -p lightspeed-gui"
  exit 1
}

$running = Get-Process lightspeed-gui -ErrorAction SilentlyContinue
if ($running) {
  $info = $running | Select-Object -First 1
  Write-Output ("already running: pid {0} from {1}" -f $info.Id, $info.Path)
  Write-Output 'quit it from the tray first (its own mutex blocks a second window)'
  exit 0
}

if ($Elevate) {
  Start-Process -FilePath $exe -Verb RunAs
  Write-Output "launched elevated: $exe"
} else {
  Start-Process -FilePath $exe
  Write-Output "launched: $exe"
}
