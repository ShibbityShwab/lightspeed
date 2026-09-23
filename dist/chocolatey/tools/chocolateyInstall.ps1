$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.9/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '6b45daced561ef87545d487e923b92c6a933f5c3ffea80291205d85f1c1f7ebe'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
