$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.0/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = 'a394f4c24051657d347d1d58be0f7cafbee01f7a70b73263877d5f60355da7e5'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
