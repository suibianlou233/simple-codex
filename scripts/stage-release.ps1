param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [string]$OutputRoot
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $OutputRoot) { $OutputRoot = Join-Path $projectRoot 'releases' }
$installerFile = Get-Item -LiteralPath $Installer
if ($installerFile.Extension -ne '.exe') { throw 'Expected a built NSIS installer.' }
$releaseName = 'Simple-0.1.0-rc.20260906-windows-x64-' + (Get-Date -Format 'HHmmss')
$stage = Join-Path $OutputRoot $releaseName
if (Test-Path -LiteralPath $stage) { throw "Output already exists: $stage" }
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item -LiteralPath $installerFile.FullName -Destination (Join-Path $stage 'Simple-0.1.0-rc.20260906-windows-x64-setup.exe')
Copy-Item -LiteralPath (Join-Path $projectRoot 'packaging/README-release.md') -Destination (Join-Path $stage 'README.md')
foreach ($name in @('LICENSE', 'THIRD_PARTY_NOTICES.md')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot $name) -Destination $stage
}
foreach ($name in @('DEPENDENCY_LICENSES.md', 'dependency-inventory.json')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot "target/release-licenses/$name") -Destination $stage
}
$provenance = Join-Path $stage 'kernel-provenance'
New-Item -ItemType Directory -Path $provenance | Out-Null
$kernel = Join-Path $projectRoot 'kernels/packages/official-283-windows-candidate-1'
foreach ($name in @('kernel.json', 'LICENSE', 'NOTICE', 'MODIFICATIONS.md', 'PATCHES.json', 'changes.patch')) {
    Copy-Item -LiteralPath (Join-Path $kernel $name) -Destination $provenance
}
$sourceFiles = @(
    'Cargo.lock', 'apps/desktop/package.json', 'apps/desktop/pnpm-lock.yaml',
    'apps/desktop/src-tauri/Cargo.toml', 'apps/desktop/src-tauri/tauri.conf.json',
    'apps/desktop/src-tauri/tauri.release.json', 'apps/desktop/src-tauri/src/runtime.rs',
    'apps/desktop/src-tauri/src/kernel_desktop.rs', 'apps/desktop/src/items/TaskTimeline.tsx',
    'apps/desktop/src-tauri/src/legacy_execution.rs',
    'apps/desktop/src-tauri/src/lib.rs',
    'apps/desktop/src/app/WorkbenchApp.tsx',
    'apps/desktop/src/app/onboarding.ts',
    'apps/desktop/src/components/FirstRunGuide.tsx',
    'apps/desktop/src/settings/ModelSettings.tsx',
    'crates/model/src/kernel_compat/codex_session.rs',
    'crates/model/src/kernel_compat/history_layout.rs',
    'crates/model/src/kernel_compat/package.rs',
    'crates/storage/src/native_history.rs',
    'apps/desktop/src/styles.css', 'apps/desktop/src/design/workbench.css',
    'packaging/README-release.md'
)
$fingerprints = @($sourceFiles | ForEach-Object {
    $path = Join-Path $projectRoot $_
    if (Test-Path -LiteralPath $path -PathType Leaf) {
        [ordered]@{path = $_; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
    }
})
$manifest = [ordered]@{
    release = $releaseName
    builtAtUtc = [DateTime]::UtcNow.ToString('o')
    internalVersion = '0.1.0'
    status = 'unsigned-release-candidate'
    architecture = 'windows-x86_64'
    kernelCommit = '28327355b861ab6cc76b01c7248663eb1be440cf'
    kernelPackage = 'official-283-windows-candidate-1'
    kernelManifestSha256 = (Get-FileHash -LiteralPath (Join-Path $kernel 'kernel.json') -Algorithm SHA256).Hash.ToLowerInvariant()
    desktopExeSha256 = (Get-FileHash -LiteralPath (Join-Path $projectRoot 'target/release/local-agent-desktop.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
    buildFeature = 'bundled-official-kernel'
    sourceFingerprints = $fingerprints
    prerequisites = @('Windows 10/11 x64', 'Microsoft Edge WebView2 Evergreen Runtime')
    cleanMachineAcceptance = 'pending-user-manual-test'
}
$manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $stage 'release-manifest.json') -Encoding UTF8
$checksums = @(Get-ChildItem -LiteralPath $stage -Recurse -File | Sort-Object FullName | ForEach-Object {
    $relative = $_.FullName.Substring($stage.Length + 1).Replace('\', '/')
    '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $relative
})
$checksums | Set-Content -LiteralPath (Join-Path $stage 'SHA256SUMS.txt') -Encoding UTF8
$archive = "$stage.zip"
Compress-Archive -LiteralPath $stage -DestinationPath $archive -CompressionLevel Optimal
$archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
('{0}  {1}' -f $archiveHash, [IO.Path]::GetFileName($archive)) | Set-Content -LiteralPath "$archive.sha256.txt" -Encoding UTF8
Get-Item -LiteralPath $archive | Select-Object FullName, Length
Write-Host "SHA256: $archiveHash"
