$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.3/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '752e315165364100bf7fd487412dff56e58f3dbbf46c96020ba864fb58d369ee'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
