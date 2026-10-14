<#
.SYNOPSIS
  Build OrgInventory Agent ban Rust cho Windows: cargo build + MSI (WiX).

.DESCRIPTION
  1. cargo build --release -p orginventory-agent (target x86_64-pc-windows-msvc).
  2. Copy exe ra dist\win-x64\OrgInventoryAgent.exe + ghi .sha256.
  3. -Msi: goi build-msi.ps1 -AgentBin de dong goi MSI (rust khong can e_sqlite3.dll).

.EXAMPLE
  .\build-windows-rust.ps1
  .\build-windows-rust.ps1 -Msi
  .\build-windows-rust.ps1 -Target aarch64-pc-windows-msvc -OutDir dist\win-arm64
#>
[CmdletBinding()]
param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$OutDir = "",
    [switch]$Msi,
    [switch]$Sign,
    [string]$CertificateThumbprint = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path $PSScriptRoot -Parent
$RustDir = Join-Path $Root "rust"
if ([string]::IsNullOrWhiteSpace($OutDir)) {
    $Rid = if ($Target -like "aarch64*") { "win-arm64" } else { "win-x64" }
    $OutDir = Join-Path $Root "dist\$Rid"
}

Write-Host "== OrgInventory Agent (Rust) Windows build ==" -ForegroundColor Cyan
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Thieu cargo - cai Rust tu https://rustup.rs"
}

Push-Location $RustDir
try {
    cargo build --release --locked -p orginventory-agent --target $Target
    if ($LASTEXITCODE -ne 0) { throw "cargo build that bai" }
} finally {
    Pop-Location
}

$Exe = Join-Path $RustDir "target\$Target\release\OrgInventoryAgent.exe"
if (-not (Test-Path $Exe)) { throw "Khong thay $Exe" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$Out = Join-Path $OutDir "OrgInventoryAgent.exe"
Copy-Item $Exe $Out -Force
$hash = (Get-FileHash $Out -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content -Path "$Out.sha256" -Value $hash -NoNewline
$sizeKb = [math]::Round((Get-Item $Out).Length / 1KB)
Write-Host "Binary: $Out ($sizeKb KB)" -ForegroundColor Green
Write-Host "SHA256: $hash"

if ($Msi) {
    $msiArgs = @{ AgentBin = $Out }
    if ($Sign) { $msiArgs["Sign"] = $true }
    if ($CertificateThumbprint) { $msiArgs["CertificateThumbprint"] = $CertificateThumbprint }
    & (Join-Path $PSScriptRoot "build-msi.ps1") @msiArgs
    if ($LASTEXITCODE -ne 0) { throw "build-msi.ps1 that bai" }
}
