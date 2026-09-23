$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.7/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '98de6ee5563a994aefe7336e78d9fdda5e9730ff3719e5d746fcf973de69997e'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
