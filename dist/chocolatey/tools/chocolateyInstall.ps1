$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.2/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '2365d29affe5572f469fedfe8e78d308bf251d201d06a36e1abe59aa8eff4210'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
