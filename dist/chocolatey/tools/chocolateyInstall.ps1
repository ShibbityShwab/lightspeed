$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.6/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '849c30c7802afa307cc54c6b9aa3c2f0467771430f24fc13593c91d331e3faf5'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
