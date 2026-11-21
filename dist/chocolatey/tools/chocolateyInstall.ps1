$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.1/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = 'b530723791d31a95dab6396339ef9efa2742c5ef66b4ce878bb11527fafcd52e'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
