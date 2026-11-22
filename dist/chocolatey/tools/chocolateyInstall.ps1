$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.3/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '966524eda8c49c652fe4cc2f064c13ad82eae2d4f5ae9373a92d281d081967dc'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
