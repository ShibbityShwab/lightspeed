$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.4/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = 'fabc25242c4a4084f23df5fa9722c84eab1d7ad2d0214a7eeeac8281d24be0b9'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
