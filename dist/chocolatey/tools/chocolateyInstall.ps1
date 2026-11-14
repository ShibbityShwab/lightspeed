$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.2/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '9ebf31f13eaf0d11dc9337223eb0961893154f065d86bee8032e0a3cf90f0ca4'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
