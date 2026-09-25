$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.13/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '37ae9f8f4ed3950379061aea6f0142424953b23f8ceebbbef6a4e21ec26d2276'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
