# Sample the game HUD (application-level ping) alongside the LightSpeed relay state.
#
# Windows-only dev tool (PowerShell 5.1). Every -IntervalSec it captures the game
# window and appends one JSON line with the client's current relay state, so the
# player-visible ping (GT-B) can be paired with relay/direct behaviour later.
# Excluding screenshots from the repo is intentional: the output lives under the
# git-ignored evidence tree.
#
# Example:
#   powershell -File infra\scripts\sample-game-hud.ps1 -DurationSec 600 -IntervalSec 30
param(
    [int]$DurationSec = 600,
    [int]$IntervalSec = 30,
    [string]$ProcessName = 'ROB RUN',
    [string]$OutDir = '',
    [string]$LogPath = ''
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class HudWin32 {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

$proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero } |
    Select-Object -First 1
if (-not $proc) { Write-Output "hud-sampler: process '$ProcessName' not running"; exit 1 }
$hwnd = $proc.MainWindowHandle

if (-not $OutDir) {
    $OutDir = Join-Path (Get-Location) ("wat\evidence\" + (Get-Date -Format 'yyyy-MM-dd') + "\hud-samples")
}
New-Item -ItemType Directory -Force $OutDir | Out-Null
$jsonl = Join-Path $OutDir 'index.jsonl'

if (-not $LogPath) {
    $LogPath = Join-Path $env:LOCALAPPDATA 'Lightspeed\gui-trace.log'
}

$deadline = (Get-Date).AddSeconds($DurationSec)
while ((Get-Date) -lt $deadline) {
    $stamp = Get-Date
    $rect = New-Object HudWin32+RECT
    [void][HudWin32]::GetWindowRect($hwnd, [ref]$rect)
    $w = $rect.Right - $rect.Left
    $h = $rect.Bottom - $rect.Top
    $png = $null
    if ($w -gt 0 -and $h -gt 0) {
        try {
            $bmp = New-Object System.Drawing.Bitmap $w, $h
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            $g.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bmp.Size)
            $png = Join-Path $OutDir ('hud-' + $stamp.ToString('HHmmss') + '.png')
            $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
            $g.Dispose(); $bmp.Dispose()
        } catch {
            $png = $null
        }
    }

    $relay = ''; $server = ''; $share = ''; $verdict = ''; $direct = ''; $relayed = ''
    if (Test-Path -LiteralPath $LogPath) {
        $esc = [char]27
        $lines = Get-Content -LiteralPath $LogPath -Tail 200 |
            ForEach-Object { ($_ -replace "$esc\[[0-9;]*m", '') }
        $window = $lines | Where-Object { $_ -match 'Relay reply window judged' } | Select-Object -Last 1
        if ($window -match 'share=([0-9.]+)%.*verdict=(\w+).*server=(\S+)') {
            $share = $matches[1]; $verdict = $matches[2]; $server = $matches[3]
        }
        $latency = $lines | Where-Object { $_ -match 'Latency comparison' } | Select-Object -Last 1
        if ($latency -match 'direct_ms=([0-9.]+)') { $direct = $matches[1] }
        if ($latency -match 'relay_app_ms=([0-9.]+)') { $relayed = $matches[1] }
        if ($latency -match 'relay=(\S+)') { $relay = $matches[1] }
    }

    $row = [ordered]@{
        at           = $stamp.ToString('o')
        png          = if ($png) { Split-Path $png -Leaf } else { $null }
        relay        = $relay
        server       = $server
        reply_share  = $share
        verdict      = $verdict
        direct_ms    = $direct
        relay_app_ms = $relayed
    }
    ($row | ConvertTo-Json -Compress) | Add-Content -LiteralPath $jsonl -Encoding UTF8
    Start-Sleep -Seconds $IntervalSec
}

Write-Output ("hud-sampler: done, index=" + $jsonl)
