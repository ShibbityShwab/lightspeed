$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.0/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '52822948c5e6dfe01cd6166c89d402d8cab0313d4f79c7dc42d111d82b049dd5'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
