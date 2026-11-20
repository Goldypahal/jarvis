# Build and package script for Jarvis & Oblivion (Windows x64)
# Produces:
#  1. Portable distribution directory and zip in dist/
#  2. WiX MSI installer in dist/JarvisSetup.msi (if WiX toolset is installed)
#  3. Code signing verification (marks BLOCKED if certificate is absent)

param(
    [switch]$SkipBuild = $false,
    [string]$CertificatePath = $env:JARVIS_SIGNING_CERT,
    [string]$CertificatePassword = $env:JARVIS_SIGNING_PASSWORD
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir
$DistDir = Join-Path $RepoRoot "dist"
$BinDir = Join-Path $DistDir "bin"
$ReleaseExe = Join-Path $RepoRoot "oblivion\target\release\oblivion.exe"

Write-Host "=== Jarvis & Oblivion Packaging Pipeline ===" -ForegroundColor Cyan

# 1. Build release binary if needed
if (-not $SkipBuild) {
    Write-Host "[1/4] Building release binary..." -ForegroundColor Yellow
    & cargo build --release --manifest-path (Join-Path $RepoRoot "oblivion\Cargo.toml") -p oblivion-cli
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build release failed with exit code $LASTEXITCODE"
    }
}

if (-not (Test-Path $ReleaseExe)) {
    throw "Release binary not found at $ReleaseExe"
}

# 2. Stage distribution files
Write-Host "[2/4] Staging distribution files..." -ForegroundColor Yellow
if (-not (Test-Path $DistDir)) {
    New-Item -ItemType Directory -Path $DistDir -Force | Out-Null
}
if (-not (Test-Path $BinDir)) {
    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
}

$JarvisExe = Join-Path $BinDir "jarvis.exe"
Copy-Item $ReleaseExe $JarvisExe -Force
Copy-Item (Join-Path $RepoRoot "README.md") (Join-Path $DistDir "README.md") -Force -ErrorAction SilentlyContinue

# Create portable zip archive
$ZipPath = Join-Path $DistDir "Jarvis-windows-x64.zip"
if (Test-Path $ZipPath) {
    Remove-Item $ZipPath -Force
}
Compress-Archive -Path "$BinDir\*" -DestinationPath $ZipPath -Force
Write-Host "  -> Portable archive created: $ZipPath" -ForegroundColor Green

# 3. Check code signing certificate
Write-Host "[3/4] Checking code signing certificate..." -ForegroundColor Yellow
$SignTool = (Get-Command signtool.exe -ErrorAction SilentlyContinue)
if ($CertificatePath -and (Test-Path $CertificatePath) -and $SignTool) {
    Write-Host "Signing binary with certificate $CertificatePath..." -ForegroundColor Cyan
    & $SignTool.Source sign /f $CertificatePath /p $CertificatePassword /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $JarvisExe
    Write-Host "  -> Binary successfully signed!" -ForegroundColor Green
} else {
    Write-Host "  -> Code signing: BLOCKED (needs user) for a certificate" -ForegroundColor Yellow
    Write-Host "     (To sign, provide -CertificatePath or set `$env:JARVIS_SIGNING_CERT)" -ForegroundColor DarkGray
}

# 4. WiX MSI Packaging
Write-Host "[4/4] Checking WiX toolset for MSI installer..." -ForegroundColor Yellow
$WixExe = (Get-Command wix.exe -ErrorAction SilentlyContinue)
$CandleExe = (Get-Command candle.exe -ErrorAction SilentlyContinue)
$LightExe = (Get-Command light.exe -ErrorAction SilentlyContinue)

$MsiOutput = Join-Path $DistDir "JarvisSetup.msi"
$WxsFile = Join-Path $ScriptDir "jarvis.wxs"

if ($CandleExe -and $LightExe) {
    Write-Host "Compiling MSI using WiX v3 toolset..." -ForegroundColor Cyan
    $WixObj = Join-Path $DistDir "jarvis.wixobj"
    & $CandleExe.Source -dSourceDir="$BinDir" -out $WixObj $WxsFile
    & $LightExe.Source -ext WixUIExtension -out $MsiOutput $WixObj
    Write-Host "  -> MSI installer created: $MsiOutput" -ForegroundColor Green
} elseif ($WixExe) {
    Write-Host "Compiling MSI using WiX v4+ toolset..." -ForegroundColor Cyan
    & $WixExe.Source build -d SourceDir="$BinDir" -o $MsiOutput $WxsFile
    Write-Host "  -> MSI installer created: $MsiOutput" -ForegroundColor Green
} else {
    Write-Host "  -> WiX toolset not found on system PATH." -ForegroundColor Yellow
    Write-Host "     WiX can be installed via: 'dotnet tool install --global wix' or 'winget install WiX'" -ForegroundColor DarkGray
    Write-Host "     Standalone portable package is ready in $ZipPath" -ForegroundColor Green
}

Write-Host "Packaging complete!" -ForegroundColor Cyan
