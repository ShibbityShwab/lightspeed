$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.0/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '9c847260588f9638b4d1bb69a72763f0f6b503aaeefe46480370442395463e8e'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
