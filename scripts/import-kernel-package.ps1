param(
  [Parameter(Mandatory = $true)][ValidatePattern('^[a-zA-Z0-9][a-zA-Z0-9._-]{0,99}$')][string]$Id,
  [Parameter(Mandatory = $true)][string]$BinaryDirectory,
  [Parameter(Mandatory = $true)][string]$SourceDirectory,
  [string]$RegistryDirectory = (Join-Path $PSScriptRoot '..\kernels\packages')
)

# Import an existing trusted legacy build, never replace an installed version.
# Hashes prove package consistency, not correspondence between source and binary.
$ErrorActionPreference = 'Stop'
$binaryRoot = (Resolve-Path -LiteralPath $BinaryDirectory).Path
$sourceRoot = (Resolve-Path -LiteralPath $SourceDirectory).Path
$registryRoot = [IO.Path]::GetFullPath($RegistryDirectory)
New-Item -ItemType Directory -Force -Path $registryRoot | Out-Null
$destination = Join-Path $registryRoot $Id
if (Test-Path -LiteralPath $destination) { throw "Version already exists: $destination" }
$sourceRecord = Join-Path $sourceRoot 'SIMPLE_SLIM.md'
if ((Get-Content -LiteralPath $sourceRecord -Raw) -notmatch '2b7c279735d0d096cf7b34fe98938f46792f4d4f') {
  throw 'This importer supports only the recorded legacy Simple derivative.'
}
$staging = Join-Path $registryRoot ('.staging-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging | Out-Null
$isWindowsPlatform = ($env:OS -eq 'Windows_NT')
$names = @('codex-app-server', 'codex-code-mode-host', 'apply_patch')
foreach ($name in $names) {
  $filename = if ($isWindowsPlatform) { "$name.exe" } else { $name }
  Copy-Item -LiteralPath (Join-Path $binaryRoot $filename) -Destination (Join-Path $staging $filename)
}
foreach ($name in @('LICENSE', 'NOTICE')) {
  Copy-Item -LiteralPath (Join-Path $sourceRoot $name) -Destination (Join-Path $staging $name)
}
Copy-Item -LiteralPath $sourceRecord -Destination (Join-Path $staging 'MODIFICATIONS.md')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\kernels\patches\legacy-import.json') -Destination (Join-Path $staging 'PATCHES.json')
$files = [ordered]@{}
Get-ChildItem -LiteralPath $staging -File | Sort-Object Name | ForEach-Object {
  $files[$_.Name] = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
}
$manifest = [ordered]@{
  schema_version = 1
  id = $Id
  upstream_revision = '2b7c279735d0d096cf7b34fe98938f46792f4d4f'
  adapter = 'simple-slim-v1'
  data_contract = 'simple-slim-project-memory-v1'
  target = $(if ($isWindowsPlatform) { 'x86_64-pc-windows-msvc' } else { 'x86_64-unknown-linux-gnu' })
  provenance = 'legacy-modified-import'
  capabilities = @('stdio-thread-turn', 'approval-interrupt', 'paginated-history', 'code-mode',
    'simple-gateway-env', 'strict-config', 'project-memory-scope', 'memory-forget')
  files = $files
}
[IO.File]::WriteAllText((Join-Path $staging 'kernel.json'), ($manifest | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
# Both explicit absolute targets are immediate children of the same registry.
if ([IO.Path]::GetDirectoryName($destination) -ne $registryRoot -or
    [IO.Path]::GetDirectoryName($staging) -ne $registryRoot) { throw 'Invalid registry destination' }
Move-Item -LiteralPath $staging -Destination $destination
Write-Output (Join-Path $destination 'kernel.json')
Write-Output 'Imported without activation. Capability declarations still require contract acceptance; source/binary correspondence is not proven.'
