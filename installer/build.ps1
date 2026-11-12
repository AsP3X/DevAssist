<#
.SYNOPSIS
  Builds the release executable and both installers.

.DESCRIPTION
  Writes to target\installer:
    devassist-<version>-x64.msi     Windows Installer package (WiX)
    devassist-<version>-setup.exe   setup wizard (Inno Setup)

  The version comes from Cargo.toml. An installer whose tool is missing is
  skipped with a note on how to get the tool; the script then exits with 1.

  Tools:
    WiX Toolset 4 or 5    dotnet tool install --global wix --version 5.0.2
    Inno Setup 6.3+       winget install JRSoftware.InnoSetup

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File installer\build.ps1
#>
[CmdletBinding()]
param(
    # Use the executable that is already in target\release.
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$manifest = Get-Content (Join-Path $root 'Cargo.toml') -Raw
if ($manifest -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw 'Cargo.toml has no version.'
}
$version = $Matches[1]
$exe = Join-Path $root 'target\release\devassist.exe'
$out = Join-Path $root 'target\installer'

if (-not $SkipBuild) {
    Write-Host "Building devassist $version (release)"
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }
}
if (-not (Test-Path $exe)) { throw "$exe is missing. Run without -SkipBuild." }
New-Item -ItemType Directory -Force $out | Out-Null

$missing = @()

# ---- MSI
$wix = Get-Command wix -ErrorAction SilentlyContinue
if ($wix) {
    $msi = Join-Path $out "devassist-$version-x64.msi"
    Write-Host "Building $msi"
    & $wix.Source build (Join-Path $PSScriptRoot 'devassist.wxs') `
        -arch x64 -pdbtype none -d "Version=$version" -d "ExePath=$exe" -o $msi
    if ($LASTEXITCODE -ne 0) { throw 'wix build failed.' }
} else {
    $missing += 'MSI skipped: the WiX Toolset is not installed. Get it with: dotnet tool install --global wix --version 5.0.2'
}

# ---- setup.exe
$iscc = Get-Command ISCC -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1
if (-not $iscc) {
    $iscc = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if ($iscc) {
    Write-Host "Building the setup wizard with $iscc"
    & $iscc /Qp "/DAppVersion=$version" "/DExePath=$exe" (Join-Path $PSScriptRoot 'devassist.iss')
    if ($LASTEXITCODE -ne 0) { throw 'ISCC failed.' }
} else {
    $missing += 'setup.exe skipped: Inno Setup is not installed. Get it with: winget install JRSoftware.InnoSetup'
}

Write-Host ''
Get-ChildItem $out -File | ForEach-Object {
    Write-Host ('{0,-48} {1,8:N0} KB' -f $_.Name, ($_.Length / 1KB))
}
if ($missing.Count -gt 0) {
    Write-Host ''
    $missing | ForEach-Object { Write-Warning $_ }
    exit 1
}
