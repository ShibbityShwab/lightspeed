$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.7.0/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = 'c208ddd141e2880c3a01dd794c8b9f8619d81cd52c99d07e46d1a1c494f52711'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
