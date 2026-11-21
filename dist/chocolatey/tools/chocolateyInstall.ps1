$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = 'https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.0/lightspeed-client-x86_64-pc-windows-msvc.zip'
  checksum64     = '0c8722cab87555895efaf66c48bc54fd51b434b490e74f7fd1c875ffdbceb431'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
