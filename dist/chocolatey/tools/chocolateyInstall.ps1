$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.10/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '82ee0342eaf672a8985e88944d80de2085e17eb55b47c4687fd89452ced9f9ab'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
