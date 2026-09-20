$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.3/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '4002291f35838c2fc4964c3afd188ac1ca119cdc5af25bcb083b47fdf19e5216'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
