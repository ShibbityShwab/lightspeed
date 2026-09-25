$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.12/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = 'ba618a94be8c16c980a54216370bf596b5240005d5bd4c22e6a52fd2ab3271c1'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
