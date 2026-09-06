param(
  [ValidateSet('debug', 'release')]
  [string]$Profile = 'debug',
  [switch]$BuildOnly
)

$ErrorActionPreference = 'Stop'
$simpleRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
$slimRoot = Resolve-Path (Join-Path $simpleRoot '..\simple-codex-slim')
$manifest = Join-Path $slimRoot 'codex-rs\Cargo.toml'

# rusty_v8 creates a directory symlink when Cargo's target directory and the
# crate registry are on different Windows drives. Creating symlinks requires a
# machine policy that ordinary Simple contributors may not have. A directory
# junction has the same path effect here and does not require elevation, so
# prepare it before Cargo starts the v8 build script.
if ($IsWindows -or $env:OS -eq 'Windows_NT') {
  $metadataJson = (& cargo metadata --manifest-path $manifest --format-version 1 --locked) -join "`n"
  if ($LASTEXITCODE -ne 0) {
    throw "Unable to inspect slim Codex Cargo metadata (exit code $LASTEXITCODE)"
  }
  $metadata = $metadataJson | ConvertFrom-Json
  $v8Package = $metadata.packages | Where-Object { $_.name -eq 'v8' } | Select-Object -First 1
  if ($null -eq $v8Package) {
    throw 'Unable to locate the rusty_v8 package in slim Codex metadata'
  }
  $v8Root = Split-Path -Parent $v8Package.manifest_path
  $targetProfile = Join-Path $metadata.target_directory $Profile
  $v8Drive = [System.IO.Path]::GetPathRoot($v8Root)
  $targetDrive = [System.IO.Path]::GetPathRoot($targetProfile)
  if (-not [string]::Equals($v8Drive, $targetDrive, [System.StringComparison]::OrdinalIgnoreCase)) {
    New-Item -ItemType Directory -Force -Path $targetProfile | Out-Null
    $gnRoot = Join-Path $targetProfile 'gn_root'
    if (Test-Path -LiteralPath $gnRoot) {
      $existing = Get-Item -LiteralPath $gnRoot
      $existingTarget = @($existing.Target) | Select-Object -First 1
      if ($existing.LinkType -ne 'Junction' -or
          -not [string]::Equals($existingTarget, $v8Root, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Cannot prepare rusty_v8 build junction because $gnRoot already exists with another target"
      }
    } else {
      New-Item -ItemType Junction -Path $gnRoot -Target $v8Root | Out-Null
    }
  }

  # GitHub's normal release URL can be unreachable on otherwise working
  # networks. Keep the verified public archive inside the handoff tree so an
  # offline development bundle can rebuild Code Mode without fetching it.
  $v8Checksums = @{
    '150.4.0' = '571bf6a028576ac1413c8a942383f637f91e94b0c964bbeefff8a098637aaa40'
  }
  $expectedV8Hash = $v8Checksums[$v8Package.version]
  if ([string]::IsNullOrWhiteSpace($expectedV8Hash)) {
    throw "No reviewed Windows rusty_v8 checksum is pinned for version $($v8Package.version)"
  }
  $v8AssetName = 'rusty_v8_release_x86_64-pc-windows-msvc.lib.gz'
  $v8ArchiveDirectory = Join-Path $slimRoot 'vendor\rusty_v8'
  $v8Archive = Join-Path $v8ArchiveDirectory "$($v8Package.version)-$v8AssetName"
  if (-not (Test-Path -LiteralPath $v8Archive -PathType Leaf)) {
    New-Item -ItemType Directory -Force -Path $v8ArchiveDirectory | Out-Null
    $release = Invoke-RestMethod -Headers @{ Accept = 'application/vnd.github+json' } `
      -Uri "https://api.github.com/repos/denoland/rusty_v8/releases/tags/v$($v8Package.version)"
    $asset = $release.assets | Where-Object { $_.name -eq $v8AssetName } | Select-Object -First 1
    if ($null -eq $asset) {
      throw "Official rusty_v8 release v$($v8Package.version) does not contain $v8AssetName"
    }
    Invoke-WebRequest -Headers @{ Accept = 'application/octet-stream' } `
      -Uri $asset.url -OutFile $v8Archive
  }
  $actualV8Hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $v8Archive).Hash.ToLowerInvariant()
  if ($actualV8Hash -ne $expectedV8Hash) {
    throw "rusty_v8 archive checksum mismatch at $v8Archive"
  }
  $env:RUSTY_V8_ARCHIVE = $v8Archive
}

$cargoArguments = @(
  'build',
  '--manifest-path', $manifest,
  '--package', 'codex-app-server',
  '--bin', 'codex-app-server',
  '--package', 'codex-code-mode-host',
  '--bin', 'codex-code-mode-host',
  '--package', 'codex-apply-patch',
  '--bin', 'apply_patch'
)
if ($Profile -eq 'release') {
  $cargoArguments += '--release'
}

& cargo @cargoArguments
if ($LASTEXITCODE -ne 0) {
  throw "slim Codex app-server build failed with exit code $LASTEXITCODE"
}

if ($BuildOnly) {
  Write-Output 'Built pinned runtime components; running desktop files were not replaced.'
  exit 0
}

$binaryNames = if ($IsWindows -or $env:OS -eq 'Windows_NT') {
  @('codex-app-server.exe', 'codex-code-mode-host.exe', 'apply_patch.exe')
} else {
  @('codex-app-server', 'codex-code-mode-host', 'apply_patch')
}
foreach ($binaryName in $binaryNames) {
  $sourceBinary = Join-Path $slimRoot "codex-rs\target\$Profile\$binaryName"
  if (-not (Test-Path -LiteralPath $sourceBinary -PathType Leaf)) {
    throw "Built slim Codex runtime was not found at $sourceBinary"
  }
}

if ($Profile -eq 'debug') {
  $destination = Join-Path $simpleRoot 'target\debug'
  New-Item -ItemType Directory -Force -Path $destination | Out-Null
  foreach ($binaryName in $binaryNames) {
    $sourceBinary = Join-Path $slimRoot "codex-rs\target\$Profile\$binaryName"
    Copy-Item -Force -LiteralPath $sourceBinary -Destination (Join-Path $destination $binaryName)
  }
  Write-Output "Synced debug slim Codex kernel and Code Mode host to $destination"
  exit 0
}

$destination = Join-Path $simpleRoot 'apps\desktop\src-tauri\resources\simple-resources'
New-Item -ItemType Directory -Force -Path $destination | Out-Null
foreach ($binaryName in $binaryNames) {
  $sourceBinary = Join-Path $slimRoot "codex-rs\target\$Profile\$binaryName"
  Copy-Item -Force -LiteralPath $sourceBinary -Destination (Join-Path $destination $binaryName)
}
Copy-Item -Force -LiteralPath (Join-Path $slimRoot 'LICENSE') -Destination (Join-Path $destination 'codex-LICENSE')
Copy-Item -Force -LiteralPath (Join-Path $slimRoot 'NOTICE') -Destination (Join-Path $destination 'codex-NOTICE')
Copy-Item -Force -LiteralPath (Join-Path $slimRoot 'SIMPLE_SLIM.md') -Destination (Join-Path $destination 'codex-MODIFICATIONS.md')

Write-Output "Synced release slim Codex kernel, Code Mode host, and notices to $destination"
