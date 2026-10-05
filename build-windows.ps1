# build-windows.ps1
#
# Build the native Windows binary with the local Rust toolchain and copy it
# to .\dist\tcp-transfer.exe.
#
# Usage (from repo root, in PowerShell):
#   .\build-windows.ps1
#
# Requirements:
#   - Rust toolchain installed locally (cargo on PATH), see AGENTS.md section 2.

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $scriptDir

$distDir  = Join-Path $scriptDir "dist"
$relBin   = Join-Path $scriptDir "target\release\tcp-transfer.exe"
$outBin   = Join-Path $distDir "tcp-transfer.exe"

# 1) Make sure cargo is available.
$cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargoCmd) {
    throw "cargo not found on PATH. Install the Rust toolchain first (https://rustup.rs)."
}

# 2) Release build (must pass without warnings).
Write-Host "[build-windows] cargo build --release ..."
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }

# 3) Prepare dist/ and copy the binary.
if (-not (Test-Path $distDir)) {
    New-Item -ItemType Directory -Path $distDir | Out-Null
}

Write-Host "[build-windows] copying $relBin -> $outBin"
Copy-Item -Force $relBin $outBin

# 4) Sanity check: the produced executable must run and answer --help.
if (-not (Test-Path $outBin)) {
    throw "binary not found at $outBin after copy"
}
& $outBin --help | Out-Null
if ($LASTEXITCODE -ne 0) { throw "smoke test failed: tcp-transfer --help exited $LASTEXITCODE" }

$size = (Get-Item $outBin).Length
Write-Host ("[build-windows] OK: {0} ({1:N0} bytes)" -f $outBin, $size)
