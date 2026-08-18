param(
    [ValidateSet("release", "debug")]
    [string]$Profile = "release",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot

function Read-WorkspaceVersion {
    $cargoToml = Get-Content -Raw (Join-Path $ProjectRoot "Cargo.toml")
    $match = [regex]::Match($cargoToml, '(?m)^version = "([^"]+)"')
    if (-not $match.Success) {
        throw "Unable to read the workspace version from Cargo.toml"
    }
    return $match.Groups[1].Value
}

$Version = Read-WorkspaceVersion
$IconPath = Join-Path $ProjectRoot "assets\branding\synapse-app-icon.ico"
$LicensePath = Join-Path $ProjectRoot "LICENSE-MIT"
$IssPath = Join-Path $ProjectRoot "scripts\windows\synapse.iss"
$TargetDir = Join-Path $ProjectRoot "target\$Profile"
$BuiltExe = Join-Path $TargetDir "synapse.exe"
$OutputDir = Join-Path $ProjectRoot "target\$Profile\bundle\windows"
$StagedExe = Join-Path $OutputDir "Synapse.exe"

if (-not (Test-Path $IconPath)) {
    throw "Application icon is missing: $IconPath"
}
if (-not (Test-Path $IssPath)) {
    throw "Inno Setup script is missing: $IssPath"
}

if (-not $SkipBuild) {
    if ($Profile -eq "release") {
        cargo build -p synapse --release
    } else {
        cargo build -p synapse
    }
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed"
    }
}

if (-not (Test-Path $BuiltExe)) {
    throw "Built executable is missing: $BuiltExe"
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
Copy-Item -Force $BuiltExe $StagedExe

$rcedit = Get-Command rcedit -ErrorAction SilentlyContinue
if ($rcedit) {
    & $rcedit.Source $StagedExe `
        --set-icon $IconPath `
        --set-version-string FileDescription "Synapse" `
        --set-version-string ProductName "Synapse" `
        --set-version-string CompanyName "xuyi" `
        --set-version-string LegalCopyright "Copyright (c) 2026 xuyi" `
        --set-version-string OriginalFilename "Synapse.exe" `
        --set-file-version "$Version.0" `
        --set-product-version "$Version.0"
    if ($LASTEXITCODE -ne 0) {
        throw "rcedit failed"
    }
} else {
    Write-Host "rcedit not found; installer will still use SetupIconFile"
}

$iscc = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles}\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $iscc) {
    $isccCommand = Get-Command iscc -ErrorAction SilentlyContinue
    if ($isccCommand) {
        $iscc = $isccCommand.Source
    }
}

if (-not $iscc) {
    throw "Inno Setup compiler (ISCC.exe) is not installed"
}

& $iscc $IssPath `
    "/DAppVersion=$Version" `
    "/DSourceExe=$StagedExe" `
    "/DOutputDir=$OutputDir" `
    "/DSetupIcon=$IconPath" `
    "/DLicenseFile=$LicensePath"
if ($LASTEXITCODE -ne 0) {
    throw "Inno Setup compilation failed"
}

$Installer = Join-Path $OutputDir "Synapse-$Version-windows-x64.exe"
if (-not (Test-Path $Installer)) {
    throw "Installer was not created: $Installer"
}

Write-Host "Created Windows installer: $Installer"
