$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.5/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '005f1a138a4d900635abf843d83fc7c63fd80377da5de94cd7f14e70c22fb77d'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
