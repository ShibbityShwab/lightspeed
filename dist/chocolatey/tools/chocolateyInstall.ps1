$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.2/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = 'e52e8fcd745048569097bcb951e1120076c6d200f7ff8013eb40706e4ae9d94d'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
