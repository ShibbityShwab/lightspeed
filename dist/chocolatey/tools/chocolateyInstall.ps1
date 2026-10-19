$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.1/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '7d04d74d391b4e2d4aad62824ef7a8405604d1a051fac1191f4a9e2517ca96ed'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
