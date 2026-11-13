$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.9.0/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = 'b1078aaf00594b6834569b51d60d21a4ac1b74a474a1319ba747541a0a5be95d'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
