$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.4/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '83fe49c183d4553a18250ed594f2d64a93d9dab4e3ed4f9b089c6f9aba843b5d'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
