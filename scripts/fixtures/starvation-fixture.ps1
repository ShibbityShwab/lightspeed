# WF-081b synthetic starvation fixture (loopback only) - Windows host.
#
# Drives the shipped GUI binary to the real `NO RETURN PATH`/RelayStarved state
# without touching product code:
#   synthethic game (renamed powershell host, ROB RUN.exe)
#     -> real WinDivert interceptor (client, elevated)
#     -> real shipped relay binary (lightspeed-proxy) on 127.0.0.1
#     -> destination deliberately blocked by the relay allowlist (no egress)
#     -> zero replies -> real no-reply watchdog -> warning + tray fifth state
#
# Network posture: loopback only. The fake game server literal is public (the
# client refuses to intercept non-public destinations), so the script installs
# a /32 loopback blackhole route for it and blocks the GUI's HTTPS (registry)
# egress with a program-scoped firewall rule. Both are removed in Stop-Fx.
# Port isolation: control 127.0.0.1:4433 (the client's registration port is a
# compile-time constant), data 127.0.0.1:24434, health 127.0.0.1:18080 - port
# 8080 is deliberately NOT bound (telemetry tests elsewhere expect it free).
#
# Requirements: elevated shell (WinDivert driver), built binaries in this
# worktree's target/debug, ~120 MB free disk for captures.
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File starvation-fixture.ps1 -Action start -Language en
#   ... -Action capture -Language en -Tag en
#   ... -Action stop
# Evidence dir defaults to the current directory's run/.
param(
    [ValidateSet('start', 'capture', 'stop', 'probe')]
    [string]$Action = 'start',
    [string]$Language = 'en',
    [int]$Height = 760,
    [string]$EvidenceDir = '',
    [string]$GameServer = '8.8.4.4',
    [int]$GamePort = 40000
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not (Test-Path (Join-Path $repo 'Cargo.toml'))) { throw "repo root not found from $PSScriptRoot" }
if (-not $EvidenceDir) { $EvidenceDir = Join-Path (Get-Location) 'run' }
New-Item -ItemType Directory -Force $EvidenceDir | Out-Null
$run = $EvidenceDir
$fixtureDir = $PSScriptRoot
$guiExe = Join-Path $repo 'target\debug\lightspeed-gui.exe'
$proxyExe = Join-Path $repo 'target\debug\lightspeed-proxy.exe'
$cfgDir = Join-Path $env:APPDATA 'light-speed'
$log = Join-Path (Join-Path $env:LOCALAPPDATA 'Lightspeed') 'gui-trace.log'
$guardNameRoute = 'LightSpeedEvidence-Blackhole'
$guardNameHttps = 'LightSpeedEvidence-GuiHttps'

function Assert-Elevated {
    $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if (-not $isAdmin) { throw 'this fixture needs an elevated shell (WinDivert driver + loopback route)' }
}

function Add-EgressGuards {
    Assert-Elevated
    # (1) fake server IP goes to loopback: ping/traceroute cannot leave the host
    if (-not (Get-NetRoute -DestinationPrefix "$GameServer/32" -ErrorAction SilentlyContinue)) {
        $lo = Get-NetIPInterface -AddressFamily IPv4 | Where-Object { $_.InterfaceAlias -match 'Loopback' } | Select-Object -First 1
        New-NetRoute -DestinationPrefix "$GameServer/32" -InterfaceIndex $lo.InterfaceIndex -NextHop '0.0.0.0' -ErrorAction Stop | Out-Null
    }
    # (2) registry discovery HTTPS from the GUI cannot leave the host
    if (-not (Get-NetFirewallRule -DisplayName $guardNameHttps -ErrorAction SilentlyContinue)) {
        New-NetFirewallRule -DisplayName $guardNameHttps -Direction Outbound -Program $guiExe -Protocol TCP -RemotePort 443 -Action Block -Profile Any | Out-Null
    }
}

function Remove-EgressGuards {
    Remove-NetFirewallRule -DisplayName $guardNameHttps -ErrorAction SilentlyContinue | Out-Null
    Remove-NetFirewallRule -DisplayName $guardNameRoute -ErrorAction SilentlyContinue | Out-Null
    Remove-NetRoute -DestinationPrefix "$GameServer/32" -Confirm:$false -ErrorAction SilentlyContinue | Out-Null
}

function Backup-UserState {
    $bak = Join-Path $run 'backup'
    New-Item -ItemType Directory -Force $bak | Out-Null
    foreach ($f in 'config.toml', 'config.toml.bak', 'ui-window-state.toml', 'lightspeed.toml') {
        $p = Join-Path $cfgDir $f
        if (Test-Path $p) { Copy-Item $p (Join-Path $bak "cfg-$f") -Force }
    }
    $history = Join-Path (Join-Path $env:LOCALAPPDATA 'Lightspeed') 'relay-history.json'
    if (Test-Path $history) { Copy-Item $history (Join-Path $bak 'relay-history.json') -Force }
}

function Restore-UserState {
    $bak = Join-Path $run 'backup'
    foreach ($f in 'config.toml', 'config.toml.bak', 'ui-window-state.toml', 'lightspeed.toml') {
        $saved = Join-Path $bak "cfg-$f"
        $dest = Join-Path $cfgDir $f
        if (Test-Path $saved) { Copy-Item $saved $dest -Force } else { Remove-Item $dest -ErrorAction SilentlyContinue }
    }
    $savedHistory = Join-Path $bak 'relay-history.json'
    if (Test-Path $savedHistory) { Copy-Item $savedHistory (Join-Path (Join-Path $env:LOCALAPPDATA 'Lightspeed') 'relay-history.json') -Force }
}

function Write-FixtureConfig([string]$lang, [int]$height) {
    # canonical lightspeed.toml + the explicit-defaults marker keeps the run
    # deterministic even when the user's legacy config.toml exists.
    $canon = "selected = `"127.0.0.1:24434`"`nauto_select = true`ngame_auto = true`nmanual_mode = false`nlanguage = `"$lang`"`nshare_latency_stats = true`n`n[meta]`nlegacy_import = false`n"
    [System.IO.File]::WriteAllText((Join-Path $cfgDir 'lightspeed.toml'), $canon, (New-Object System.Text.UTF8Encoding($false)))
    [System.IO.File]::WriteAllText((Join-Path $cfgDir 'ui-window-state.toml'), "height = $height`n", (New-Object System.Text.UTF8Encoding($false)))
}

function Start-Proxy {
    $tls = Join-Path $run 'tls'
    New-Item -ItemType Directory -Force $tls | Out-Null
    $env:LIGHTSPEED_TLS_DIR = $tls
    $p = Start-Process -FilePath $proxyExe -WindowStyle Hidden -PassThru `
        -ArgumentList @('--config', (Join-Path $fixtureDir 'proxy-loopback.toml'),
            '--data-bind', '127.0.0.1:24434', '--control-bind', '127.0.0.1:4433', '--health-bind', '127.0.0.1:18080') `
        -RedirectStandardOutput (Join-Path $run 'proxy.out.log') -RedirectStandardError (Join-Path $run 'proxy.err.log')
    Start-Sleep -Seconds 2
    if ($p.HasExited) { throw "proxy exited: see $(Join-Path $run 'proxy.err.log')" }
    [System.IO.File]::WriteAllText((Join-Path $run 'proxy.pid'), [string]$p.Id)
    return $p
}

function Start-Gui([string]$tag) {
    $before = if (Test-Path $log) { (Get-Item $log).Length } else { 0 }
    $env:LIGHTSPEED_PROXIES = '127.0.0.1:24434'
    $env:LIGHTSPEED_GUI_FORCE = '1'
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $guiExe; $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
    $gui = [System.Diagnostics.Process]::Start($psi)
    [System.IO.File]::WriteAllText((Join-Path $run "gui-$tag.pid"), [string]$gui.Id)
    $deadline = (Get-Date).AddSeconds(40); $text = ''
    while ((Get-Date) -lt $deadline -and -not $gui.HasExited) {
        Start-Sleep -Milliseconds 400
        $fs = [System.IO.File]::Open($log, 'Open', 'Read', 'ReadWrite')
        $fs.Seek($before, 'Begin') | Out-Null
        $sr = New-Object System.IO.StreamReader($fs); $text = $sr.ReadToEnd(); $sr.Close(); $fs.Close()
        if ($text -match 'control-plane registered relay=127\.0\.0\.1:24434') { break }
    }
    [System.IO.File]::WriteAllText((Join-Path $run "gui-$tag-startup.log"), $text, (New-Object System.Text.UTF8Encoding($false)))
    if ($text -notmatch 'control-plane registered relay=127\.0\.0\.1:24434') { throw "GUI did not register with the loopback relay (see gui-$tag-startup.log)" }
    return $gui
}

function Start-GameDouble([string]$tag) {
    # A renamed powershell host so the presence scanner matches ROB RUN.exe.
    $hostCopy = Join-Path $run 'ROB RUN.exe'
    if (-not (Test-Path $hostCopy)) { Copy-Item 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' $hostCopy -Force }
    $gpsi = New-Object System.Diagnostics.ProcessStartInfo
    $gpsi.FileName = $hostCopy
    $gpsi.Arguments = '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + (Join-Path $fixtureDir 'game-double.ps1') + '" -Dest ' + $GameServer + ' -Port ' + $GamePort + ' -IntervalMs 100'
    $gpsi.UseShellExecute = $false; $gpsi.CreateNoWindow = $true
    $game = [System.Diagnostics.Process]::Start($gpsi)
    [System.IO.File]::WriteAllText((Join-Path $run "game-$tag.pid"), [string]$game.Id)
    return $game
}

# --- Win32 helpers (client-area capture, live resize, tray observation) ------
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class FxWin32 {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [StructLayout(LayoutKind.Sequential)] public struct NII { public uint cbSize; public IntPtr hWnd; public uint uID; public Guid guid; }
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder t, int n);
  [DllImport("shell32.dll")] public static extern int Shell_NotifyIconGetRect(ref NII id, out RECT r);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc p, IntPtr l);
  public static List<IntPtr> ForPid(uint pid) { var o = new List<IntPtr>(); EnumWindows(delegate(IntPtr h, IntPtr l) { uint p; GetWindowThreadProcessId(h, out p); if (p == pid) o.Add(h); return true; }, IntPtr.Zero); return o; }
  public static IntPtr MainWindow(uint pid) { foreach (var h in ForPid(pid)) { if (!IsWindowVisible(h)) continue; var sb = new StringBuilder(256); GetWindowText(h, sb, 256); if (sb.ToString() == "LightSpeed") return h; } return IntPtr.Zero; }
  public static int[] TrayRect(uint pid) { foreach (var h in ForPid(pid)) { for (uint uid = 0; uid < 64; uid++) { var id = new NII(); id.cbSize = (uint)Marshal.SizeOf(typeof(NII)); id.hWnd = h; id.uID = uid; RECT r; if (Shell_NotifyIconGetRect(ref id, out r) == 0) return new int[] { r.Left, r.Top, r.Right, r.Bottom }; } } return null; }
}
'@ -ReferencedAssemblies 'System.Drawing', 'System.Windows.Forms'

function Get-ClientSize([int]$ProcessId) {
    $h = [FxWin32]::MainWindow([uint32]$ProcessId); $cr = New-Object FxWin32+RECT
    [void][FxWin32]::GetClientRect($h, [ref]$cr)
    return @{ Width = $cr.Right - $cr.Left; Height = $cr.Bottom - $cr.Top }
}

function Set-ClientHeight([int]$ProcessId, [int]$Wanted) {
    $h = [FxWin32]::MainWindow([uint32]$ProcessId)
    [void][FxWin32]::ShowWindow($h, 9)
    for ($i = 0; $i -lt 4; $i++) {
        $wr = New-Object FxWin32+RECT; $cr = New-Object FxWin32+RECT
        [void][FxWin32]::GetWindowRect($h, [ref]$wr); [void][FxWin32]::GetClientRect($h, [ref]$cr)
        $fw = ($wr.Right - $wr.Left) - ($cr.Right - $cr.Left); $fh = ($wr.Bottom - $wr.Top) - ($cr.Bottom - $cr.Top)
        [void][FxWin32]::SetWindowPos($h, [IntPtr]::Zero, $wr.Left, $wr.Top, 320 + $fw, $Wanted + $fh, 0x0004)
        Start-Sleep -Milliseconds 700
        [void][FxWin32]::GetClientRect($h, [ref]$cr)
        if (($cr.Bottom - $cr.Top) -eq $Wanted) { return }
    }
    throw "could not resize the client to 320x$Wanted"
}

function Capture-ClientArea([int]$ProcessId, [string]$Path, [int]$ExpectHeight) {
    $h = [FxWin32]::MainWindow([uint32]$ProcessId)
    # z-order lift so a concurrently running GUI (other agents share this
    # desktop) cannot pollute CopyFromScreen; restored immediately after.
    [void][FxWin32]::SetWindowPos($h, [IntPtr](-1), 0, 0, 0, 0, 0x0013)
    Start-Sleep -Milliseconds 350
    $cr = New-Object FxWin32+RECT; [void][FxWin32]::GetClientRect($h, [ref]$cr)
    $origin = New-Object FxWin32+POINT; [void][FxWin32]::ClientToScreen($h, [ref]$origin)
    $w = $cr.Right - $cr.Left; $hh = $cr.Bottom - $cr.Top
    if ($w -ne 320 -or ($ExpectHeight -gt 0 -and $hh -ne $ExpectHeight)) { throw "unexpected client size ${w}x${hh}" }
    $bmp = New-Object System.Drawing.Bitmap($w, $hh); $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($origin.X, $origin.Y, 0, 0, (New-Object System.Drawing.Size($w, $hh))); $g.Dispose()
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    [void][FxWin32]::SetWindowPos($h, [IntPtr](-2), 0, 0, 0, 0, 0x0013)
    return @{ Path = $Path; Width = $w; Height = $hh }
}

function Scroll-ToTop([int]$ProcessId) {
    $h = [FxWin32]::MainWindow([uint32]$ProcessId); [void][FxWin32]::ShowWindow($h, 9)
    $cr = New-Object FxWin32+RECT; [void][FxWin32]::GetClientRect($h, [ref]$cr)
    $o = New-Object FxWin32+POINT; [void][FxWin32]::ClientToScreen($h, [ref]$o)
    [void][FxWin32]::SetCursorPos($o.X + [int](($cr.Right - $cr.Left) / 2), $o.Y + [int](($cr.Bottom - $cr.Top) / 2))
    for ($i = 0; $i -lt 8; $i++) { [FxWin32]::mouse_event(0x0800, 0, 0, 120, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60 }
    Start-Sleep -Milliseconds 400
}

function Capture-Screen([string]$Path) {
    $s = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap($s.Width, $s.Height); $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($s.X, $s.Y, 0, 0, $bmp.Size); $g.Dispose(); $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
# -----------------------------------------------------------------------------

# The warning line is English in every shipped catalog today (Q5 translation
# debt for the W2-2b rows), so one ASCII needle identifies the state on all
# three capture languages without encoding hazards in this ANSI-read script.
$starvedNeedle = 'No game traffic is coming back from this relay'

function Wait-Starved([int]$ProcessId, [string]$lang, [int]$TimeoutSeconds = 75) {
    Add-Type -AssemblyName UIAutomationClient
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 700
        $h = [FxWin32]::MainWindow([uint32]$ProcessId)
        if ($h -eq [IntPtr]::Zero) { continue }
        $root = [System.Windows.Automation.AutomationElement]::FromHandle($h)
        $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($el in $all) {
            try {
                if ($el.Current.ControlType.ProgrammaticName -eq 'ControlType.Text' -and $el.Current.Name -like ("*" + $starvedNeedle + "*")) {
                    Start-Sleep -Seconds 2
                    return $true
                }
            } catch { }
        }
    }
    return $false
}

function Capture-StarvedSet([int]$ProcessId, [string]$tag, [string]$lang) {
    Set-ClientHeight $ProcessId 760
    Start-Sleep -Milliseconds 500
    $c760 = Capture-ClientArea $ProcessId (Join-Path $run "window-320x760-$tag-starved.png") 760
    Set-ClientHeight $ProcessId 420
    Scroll-ToTop $ProcessId
    Start-Sleep -Milliseconds 400
    $c420 = Capture-ClientArea $ProcessId (Join-Path $run "window-320x420-$tag-starved.png") 420
    return @{ c760 = $c760; c420 = $c420 }
}

switch ($Action) {
    'start' {
        Assert-Elevated
        Backup-UserState
        Add-EgressGuards
        Write-FixtureConfig $Language $Height
        Copy-Item (Join-Path $repo 'client\windivert\WinDivert.dll'), (Join-Path $repo 'client\windivert\WinDivert64.sys') (Join-Path $repo 'target\debug') -Force
        $proxy = Start-Proxy
        $gui = Start-Gui $Language
        $game = Start-GameDouble $Language
        Write-Host "fixture up: proxy=$($proxy.Id) gui=$($gui.Id) game=$($game.Id) lang=$Language"
    }
    'capture' {
        $guiPid = [int](Get-Content (Join-Path $run "gui-$Language.pid"))
        $c = Capture-StarvedSet $guiPid $Language $Language
        Write-Host ($c | ConvertTo-Json -Compress)
    }
    'probe' {
        # Live probes: Esc closes a sheet; Ctrl+Q / Ctrl+, / Ctrl+H are NOT app
        # handlers in the shipped shell (this is the recorded U evidence).
        $guiPid = [int](Get-Content (Join-Path $run "gui-$Language.pid"))
        $h = [FxWin32]::MainWindow([uint32]$guiPid)
        Add-Type -AssemblyName UIAutomationClient
        $root = [System.Windows.Automation.AutomationElement]::FromHandle($h)
        $langRow = { param($needle) $els = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition); foreach ($e in $els) { try { if ($e.Current.Name -eq $needle) { return $true } } catch {} }; return $false }
        $settings = $null
        foreach ($e in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)) { try { if ($e.Current.Name -eq 'Settings' -and $e.Current.ControlType.ProgrammaticName -eq 'ControlType.Button') { $settings = $e; break } } catch {} }
        $result = @{}
        if ($settings) { $settings.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke(); Start-Sleep -Milliseconds 900; $result.opened = $true }
        $result.language_before_esc = (& $langRow 'Language')
        [FxWin32]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); [FxWin32]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 900
        $result.language_after_esc = (& $langRow 'Language')
        $result.esc_closed_sheet = ($result.language_before_esc -and -not $result.language_after_esc)
        [FxWin32]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero); [FxWin32]::keybd_event(0xBC, 0, 0, [UIntPtr]::Zero); [FxWin32]::keybd_event(0xBC, 0, 2, [UIntPtr]::Zero); [FxWin32]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 800
        $result.ctrl_comma_opened_settings = (& $langRow 'Language')
        [FxWin32]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero); [FxWin32]::keybd_event(0x48, 0, 0, [UIntPtr]::Zero); [FxWin32]::keybd_event(0x48, 0, 2, [UIntPtr]::Zero); [FxWin32]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 800
        $result.window_visible_after_ctrl_h = [FxWin32]::IsWindowVisible($h)
        [FxWin32]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero); [FxWin32]::keybd_event(0x51, 0, 0, [UIntPtr]::Zero); [FxWin32]::keybd_event(0x51, 0, 2, [UIntPtr]::Zero); [FxWin32]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
        Start-Sleep -Seconds 2
        $result.process_exited_after_ctrl_q = ((Get-Process -Id $guiPid -ErrorAction SilentlyContinue) -eq $null)
        [System.IO.File]::WriteAllText((Join-Path $run 'probe-evidence.json'), ($result | ConvertTo-Json), (New-Object System.Text.UTF8Encoding($false)))
        Write-Host ($result | ConvertTo-Json -Compress)
    }
    'stop' {
        Get-Process | Where-Object { $_.ProcessName -eq 'ROB RUN' } | Stop-Process -Force -ErrorAction SilentlyContinue
        foreach ($f in Get-ChildItem $run -Filter '*.pid') {
            $id = [int](Get-Content $f.FullName); $p = Get-Process -Id $id -ErrorAction SilentlyContinue
            if ($p -and $p.ProcessName -match 'lightspeed') { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
        }
        Remove-EgressGuards
        Restore-UserState
        Remove-Item (Join-Path $repo 'target\debug\WinDivert.dll'), (Join-Path $repo 'target\debug\WinDivert64.sys') -ErrorAction SilentlyContinue
        Write-Host 'fixture stopped; user state restored; egress guards and listeners removed'
    }
}
