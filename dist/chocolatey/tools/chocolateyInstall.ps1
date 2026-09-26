$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.14/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '1b8e969feb84b2ac17b2732fcb569c8c91926800cd425d75f80b96e53d234d25'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
