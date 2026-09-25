$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.11/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = 'ba8a160680baaef402e3f95b473d7757cc647759064c9d618692630b9fa92399'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
