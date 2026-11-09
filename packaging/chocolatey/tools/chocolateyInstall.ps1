$ErrorActionPreference = 'Stop'

if (-not [Environment]::Is64BitOperatingSystem) {
  throw 'OmniBuild requires 64-bit Windows.'
}

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$packageName = 'omnibuild'
$url64 = 'https://github.com/mukesh-craft/omnibuild/releases/download/v0.1.1/omnibuild-0.1.1-windows-x86_64.zip'
$checksum64 = 'FC20B2030E9D08AD3576DE2138E6EE147FD875E59FF1CBED52D68CF4DFD2632B'
$urlArm64 = 'https://github.com/mukesh-craft/omnibuild/releases/download/v0.1.1/omnibuild-0.1.1-windows-arm64.zip'
$checksumArm64 = 'REPLACE_WITH_WINDOWS_ARM64_SHA256'
$isArm64 = $env:PROCESSOR_ARCHITECTURE -eq 'ARM64'
if ($isArm64 -and $checksumArm64 -notmatch '^REPLACE_') {
  $url64 = $urlArm64
  $checksum64 = $checksumArm64
} elseif ($isArm64) {
  throw 'Windows ARM64 asset is not published for this version yet.'
}

Install-ChocolateyZipPackage `
  -PackageName $packageName `
  -Url $url64 `
  -UnzipLocation $toolsDir `
  -Checksum $checksum64 `
  -ChecksumType 'sha256' `
  -Url64bit $url64 `
  -Checksum64 $checksum64 `
  -ChecksumType64 'sha256'

Install-BinFile -Name 'omnibuild' -Path (Join-Path $toolsDir 'omnibuild.exe')
