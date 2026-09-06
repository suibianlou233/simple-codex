param(
  [Parameter(Mandatory)][ValidatePattern('^[a-zA-Z0-9][a-zA-Z0-9._-]{0,99}$')][string]$Id,
  [Parameter(Mandatory)][string]$SourceDirectory,
  [Parameter(Mandatory)][string]$BinaryDirectory,
  [Parameter(Mandatory)][string]$PatchDirectory,
  [string]$RegistryDirectory = (Join-Path $PSScriptRoot '..\kernels\packages')
)
# Windows candidate only. Never activate, mutate the source or overwrite a package.
$ErrorActionPreference = 'Stop'
$revision = '28327355b861ab6cc76b01c7248663eb1be440cf'
$source = (Resolve-Path -LiteralPath $SourceDirectory).Path
$binaries = (Resolve-Path -LiteralPath $BinaryDirectory).Path
$patch = (Resolve-Path -LiteralPath $PatchDirectory).Path
if ((& git -C $source rev-parse HEAD) -ne $revision -or $LASTEXITCODE -ne 0) { throw 'Wrong upstream pin' }
if (@(& git -C $source ls-files --others --exclude-standard).Count -ne 0) { throw 'Untracked source must be reviewed' }
$record = Get-Content -LiteralPath (Join-Path $patch 'record.json') -Raw | ConvertFrom-Json
if ($record.upstream_revision -ne $revision) { throw 'Wrong patch base' }
$patchPath = Join-Path $patch 'changes.patch'
if ((Get-FileHash -LiteralPath $patchPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $record.patch_sha256) { throw 'Patch checksum mismatch' }
$actual = ((@(& git -C $source diff --binary --full-index --no-ext-diff --no-textconv $revision --) -join "`n") + "`n")
$recorded = [IO.File]::ReadAllText($patchPath).Replace("`r`n", "`n")
if ($LASTEXITCODE -ne 0 -or $actual -cne $recorded) { throw 'Working source does not match recorded patch exactly' }
$registry = [IO.Path]::GetFullPath($RegistryDirectory)
New-Item -ItemType Directory -Force -Path $registry | Out-Null
$destination = Join-Path $registry $Id
if (Test-Path -LiteralPath $destination) { throw 'Version already exists' }
$staging = Join-Path $registry ('.staging-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging | Out-Null
foreach ($name in @('codex-app-server.exe','codex-code-mode-host.exe','apply_patch.exe')) {
  Copy-Item -LiteralPath (Join-Path $binaries $name) -Destination (Join-Path $staging $name)
}
foreach ($name in @('LICENSE','NOTICE')) {
  Copy-Item -LiteralPath (Join-Path $source $name) -Destination (Join-Path $staging $name)
}
Copy-Item -LiteralPath $patchPath -Destination (Join-Path $staging 'changes.patch')
Copy-Item -LiteralPath (Join-Path $patch 'record.json') -Destination (Join-Path $staging 'PATCHES.json')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\kernels\upstream-283-candidate.md') -Destination (Join-Path $staging 'MODIFICATIONS.md')
$files = [ordered]@{}
Get-ChildItem -LiteralPath $staging -File | Sort-Object Name | ForEach-Object {
  $files[$_.Name] = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
}
$manifest = [ordered]@{
  schema_version = 1; id = $Id; upstream_revision = $revision
  adapter = 'official-283-candidate-v1'; data_contract = 'simple-upstream-283-isolated-v1'
  target = 'x86_64-pc-windows-msvc'; provenance = 'pinned-upstream-with-recorded-patches'
  capabilities = @('stdio-thread-turn','approval-interrupt','paginated-history','code-mode',
    'official-provider-config','strict-config','isolated-candidate-only')
  files = $files
}
[IO.File]::WriteAllText((Join-Path $staging 'kernel.json'), ($manifest | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
if ([IO.Path]::GetDirectoryName($destination) -ne $registry -or [IO.Path]::GetDirectoryName($staging) -ne $registry) { throw 'Invalid package destination' }
Move-Item -LiteralPath $staging -Destination $destination
Write-Output (Join-Path $destination 'kernel.json')
Write-Output 'Candidate only; no activation. Source/patch correspondence checked, reproducible binary build not claimed.'
