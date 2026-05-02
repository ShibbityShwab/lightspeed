# LightSpeed capture/inject E2E tester for Rust (Facepunch)
# Sends UDP packets to proxy-lax:28015 (Rust default port) and listens for
# echoed replies injected back by the GUI's pcap injector.
#
# Run in a normal PowerShell window AFTER:
#   1. lightspeed-gui.exe is open (as Admin), Rust selected, Auto-capture ACTIVE
#   2. echo server is running on proxy-lax (handled by the agent via SSH)

$target = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Parse("149.28.84.139"), 28015)
$udp    = New-Object System.Net.Sockets.UdpClient
$udp.Client.ReceiveTimeout = 150   # ms – non-blocking poll
$remote = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)

$n       = 0
$sent    = 0
$received = 0

Write-Host "=== LightSpeed Rust E2E Test ===" -ForegroundColor Cyan
Write-Host "Sending UDP to 149.28.84.139:28015 (Rust port) every 100ms" -ForegroundColor Cyan
Write-Host "Watch GUI: Captured / From proxy / Injected should all increment" -ForegroundColor Cyan
Write-Host "RX lines here prove the injector wrote frames back to your NIC" -ForegroundColor Green
Write-Host "Press Ctrl+C to stop`n"

try {
    while ($true) {
        # Send a fake "Rust" UDP packet on port 28015
        $payload = [Text.Encoding]::UTF8.GetBytes("RUST_PKT $n $(Get-Date -Format 'HH:mm:ss.fff')")
        $udp.Send($payload, $payload.Length, $target) | Out-Null
        $sent++
        $n++

        # Try to receive injected echo (non-blocking via timeout)
        try {
            $reply = $udp.Receive([ref]$remote)
            $received++
            $text  = [Text.Encoding]::UTF8.GetString($reply)
            Write-Host ("  [RX] from {0}:{1}  => {2}" -f $remote.Address, $remote.Port, $text) -ForegroundColor Green
        } catch [System.Net.Sockets.SocketException] {
            # Timeout – no reply yet
        }

        if ($n % 20 -eq 0) {
            Write-Host ("  [stats] sent={0}  received={1}  loss={2:P0}" -f `
                $sent, $received, (1 - ($received / [Math]::Max(1, $sent)))) -ForegroundColor Yellow
        }

        Start-Sleep -Milliseconds 100
    }
} finally {
    $udp.Close()
    Write-Host ("`nFinal: sent={0}  received={1}" -f $sent, $received)
}
