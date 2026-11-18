$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.1/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '65350ca0ae419683d1935f7cc3873b08b2ae311dc7e344c08dba9fb14a91c9b1'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
