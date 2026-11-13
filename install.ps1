# 🍋 LemonPort — installateur Windows (PowerShell).
#
#   irm https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.ps1 | iex
#
# Paramètres : -Version, -InstallDir, -FromSource
# Variables : $env:LEMONPORT_VERSION, $env:LEMONPORT_INSTALL_DIR

[CmdletBinding()]
param(
    [string]$Version = $env:LEMONPORT_VERSION,
    [string]$InstallDir = $env:LEMONPORT_INSTALL_DIR,
    [switch]$FromSource
)

$ErrorActionPreference = "Stop"
$Repo = "LLAStudio/LemonPort"
$Bin = "lemon-port"

function Write-Step([string]$Message) { Write-Host "🍋 $Message" -ForegroundColor Yellow }
function Write-Ok([string]$Message) { Write-Host "✓ $Message" -ForegroundColor Green }
function Write-Err([string]$Message) { Write-Host "✗ $Message" -ForegroundColor Red }

if (-not $FromSource -and $env:LEMONPORT_FROM_SOURCE -match '^(1|true|yes)$') {
    $FromSource = $true
}

if ($FromSource) {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Err "Rust/Cargo est requis : https://rustup.rs"
        exit 1
    }
    Write-Step "Installation de LemonPort depuis les sources…"
    cargo install --git "https://github.com/$Repo" --locked --force
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    Write-Ok "lemon-port et lp installés dans $env:USERPROFILE\.cargo\bin"
    exit 0
}

try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch {
    # PowerShell Core gère TLS automatiquement.
}

if (-not $Version) {
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        $Version = $release.tag_name
    } catch {
        Write-Err "Aucune release binaire disponible pour $Repo."
        Write-Err "Installez depuis les sources : cargo install --git https://github.com/$Repo --locked"
        exit 1
    }
}

if ($Version -notmatch '^v?[0-9]') {
    Write-Err "Version invalide : $Version"
    exit 1
}
if (-not $Version.StartsWith("v")) { $Version = "v$Version" }

$architecture = $env:PROCESSOR_ARCHITEW6432
if (-not $architecture) { $architecture = $env:PROCESSOR_ARCHITECTURE }
if ($architecture -ne "AMD64") {
    Write-Err "Architecture non supportée : $architecture (binaire x64 uniquement)."
    Write-Err "Depuis les sources : cargo install --git https://github.com/$Repo --locked"
    exit 1
}
$Target = "x86_64-pc-windows-msvc"

if (-not $InstallDir) {
    $InstallDir = Join-Path $env:LOCALAPPDATA "LemonPort\bin"
}

$temp = Join-Path ([System.IO.Path]::GetTempPath()) ("lemonport-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $temp -Force | Out-Null

try {
    $asset = "lemon-port-$Version-$Target.zip"
    $base = "https://github.com/$Repo/releases/download/$Version"
    $zip = Join-Path $temp $asset
    $sums = Join-Path $temp "SHA256SUMS"

    Write-Step "Téléchargement de $asset…"
    Invoke-WebRequest -Uri "$base/$asset" -OutFile $zip
    Invoke-WebRequest -Uri "$base/SHA256SUMS" -OutFile $sums

    $line = Select-String -Path $sums -Pattern " $([regex]::Escape($asset))$" | Select-Object -First 1
    if (-not $line) { throw "Checksum introuvable pour $asset" }
    $expected = ($line.Line -split '\s+')[0]
    $actual = (Get-FileHash -Path $zip -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected.ToLower()) { throw "Checksum invalide pour $asset" }

    $extract = Join-Path $temp "extract"
    Expand-Archive -Path $zip -DestinationPath $extract -Force

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item (Join-Path $extract "$Bin.exe") (Join-Path $InstallDir "$Bin.exe") -Force
    Copy-Item (Join-Path $extract "$Bin.exe") (Join-Path $InstallDir "lp.exe") -Force

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $userPath) { $userPath = "" }
    $parts = @($userPath -split ';' | Where-Object { $_ })
    if ($parts -notcontains $InstallDir) {
        $newPath = if ($userPath.TrimEnd(';')) { $userPath.TrimEnd(';') + ";" + $InstallDir } else { $InstallDir }
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        $env:Path = "$env:Path;$InstallDir"
        Write-Host "⚠ PATH utilisateur mis à jour — ouvrez un nouveau terminal." -ForegroundColor Yellow
    }

    Write-Ok "LemonPort $Version installé dans $InstallDir (lemon-port.exe + lp.exe)"
    & (Join-Path $InstallDir "$Bin.exe") --version
} catch {
    Write-Err $_.Exception.Message
    exit 1
} finally {
    Remove-Item -Path $temp -Recurse -Force -ErrorAction SilentlyContinue
}
