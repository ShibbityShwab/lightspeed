$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.8/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '9fd57d4b2407a1a054cd54844ca026952b4d341f9656895a299c76086c9de791'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
