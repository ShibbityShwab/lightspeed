$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.1/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '91ded8f888abbbed35afa70128630f1dfd6323a5391dbbcdebb9b18ba3b37043'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
