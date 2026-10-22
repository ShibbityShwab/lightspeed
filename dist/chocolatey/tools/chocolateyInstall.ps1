$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.8.4/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '60cc2379e6f919b8a224aadb3c14d994f57c5c08de5ec0abe915cd9624efbad3'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
