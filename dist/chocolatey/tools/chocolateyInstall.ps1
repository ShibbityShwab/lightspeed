$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.3/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '45d7110e96d6b5353e9a5925bd90c48c47c63a7c98fa613448dbc56b8eb45f0a'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
