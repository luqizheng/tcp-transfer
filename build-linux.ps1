# build-linux.ps1
#
# Build the static Linux binary via Docker and copy it to .\dist\tcp-transfer.
#
# Usage (from repo root, in PowerShell):
#   .\build-linux.ps1
#
# Requirements:
#   - Docker Desktop running
#   - The base image zhcoder-docker-registry.com:8000/builder/rust:1.96-slim-musl
#     already pulled locally (the build uses --pull=false intentionally;
#     the registry only serves plain HTTP, see AGENTS.md 7.1).

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $scriptDir

$imageName = "tcp-transfer:linux"
$distDir   = Join-Path $scriptDir "dist"
$binInImg  = "/usr/local/bin/tcp-transfer"
$tmpCtr    = "tcp-transfer-tmp"

# 1) Build the image.
Write-Host "[build-linux] docker build ..."
docker build --pull=false -f Dockerfile-linux -t $imageName .
if ($LASTEXITCODE -ne 0) { throw "docker build failed ($LASTEXITCODE)" }

# 2) Prepare dist/.
if (-not (Test-Path $distDir)) {
    New-Item -ItemType Directory -Path $distDir | Out-Null
}
$outBin = Join-Path $distDir "tcp-transfer"

# 3) Extract the static binary from the image.
#    Cleanup any leftover container from a previous run.
docker rm -f $tmpCtr 2>$null | Out-Null

Write-Host "[build-linux] extracting $binInImg -> $outBin"
docker create --name $tmpCtr $imageName | Out-Null
try {
    docker cp "${tmpCtr}:${binInImg}" $outBin
    if ($LASTEXITCODE -ne 0) { throw "docker cp failed ($LASTEXITCODE)" }
}
finally {
    docker rm $tmpCtr | Out-Null
}

# 4) Sanity check.
if (-not (Test-Path $outBin)) {
    throw "binary not found at $outBin after docker cp"
}
$size = (Get-Item $outBin).Length
Write-Host ("[build-linux] OK: {0} ({1:N0} bytes)" -f $outBin, $size)
