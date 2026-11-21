# Synthetic game double for the WF-081b starvation fixture.
#
# Sends a small UDP datagram at a steady rate to a PUBLIC destination literal.
# It is launched through a renamed powershell host (see
# starvation-fixture.ps1) so the GUI's game-presence scanner matches the
# built-in ROB RUN profile by process name, and the WinDivert port-range filter
# (32768-60999) captures the packets.
param(
    [string]$Dest = '8.8.4.4',
    [int]$Port = 40000,
    [int]$IntervalMs = 100
)
$udp = New-Object System.Net.Sockets.UdpClient
$bytes = [byte[]]::new(64)
for ($i = 0; $i -lt $bytes.Length; $i++) { $bytes[$i] = [byte]($i -band 0xff) }
while ($true) {
    try { [void]$udp.Send($bytes, $bytes.Length, $Dest, $Port) } catch { }
    Start-Sleep -Milliseconds $IntervalMs
}
