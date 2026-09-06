param([switch]$Offline, [string]$KernelSource, [ValidateRange(1, 64)][int]$Jobs = 2)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$desktopPath = Join-Path $projectRoot 'apps/desktop'
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
$originalOffline = $env:CARGO_NET_OFFLINE
$originalJobs = $env:CARGO_BUILD_JOBS
try {
    $buildTemp = Join-Path $projectRoot 'target/tmp'
    New-Item -ItemType Directory -Path $buildTemp -Force | Out-Null
    $env:TEMP = $buildTemp
    $env:TMP = $buildTemp
    $env:CARGO_BUILD_JOBS = [string]$Jobs
    if ($Offline) { $env:CARGO_NET_OFFLINE = 'true' }
    & node (Join-Path $PSScriptRoot 'collect-release-licenses.mjs') $KernelSource
    if ($LASTEXITCODE -ne 0) { throw 'Failed to collect dependency notices.' }
    Push-Location $desktopPath
    try {
        # Dependencies must already be installed with the repository's lockfiles.
        # Offline only covers Cargo; Tauri may need cached NSIS tools on first build.
        & pnpm exec tauri build --ci --no-sign --features bundled-official-kernel --bundles nsis --config src-tauri/tauri.release.json -- --locked
        if ($LASTEXITCODE -ne 0) { throw "Release build failed: $LASTEXITCODE" }
    } finally { Pop-Location }
    Write-Host "Installer output: $(Join-Path $projectRoot 'target/release/bundle/nsis')"
    Write-Host 'Unsigned release candidate; see packaging/README-release.md before redistribution.'
} finally {
    $env:TEMP = $originalTemp
    $env:TMP = $originalTmp
    $env:CARGO_NET_OFFLINE = $originalOffline
    $env:CARGO_BUILD_JOBS = $originalJobs
}
