$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.4/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '99f49afee060175ab606d38abeb9ed9b48d572feec8edaa53ba7e52da36568fd'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
