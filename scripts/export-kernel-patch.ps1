param(
  [Parameter(Mandatory = $true)][string]$UpstreamDirectory,
  [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{40}$')][string]$BaseRevision,
  [Parameter(Mandatory = $true)][ValidatePattern('^[a-zA-Z0-9][a-zA-Z0-9._-]{0,99}$')][string]$PatchId,
  [Parameter(Mandatory = $true)][string]$Reason,
  [Parameter(Mandatory = $true)][string]$RemovalCondition,
  [Parameter(Mandatory = $true)][int]$ApplyOrder,
  [string[]]$Tests = @(),
  [string]$UpstreamIssueOrCommit = '',
  [string]$OutputDirectory = (Join-Path $PSScriptRoot '..\kernels\patches')
)

# Export from a pinned checkout without modifying it or claiming tests passed.
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $UpstreamDirectory).Path
$head = (& git -C $source rev-parse HEAD)
if ($LASTEXITCODE -ne 0 -or $head -ne $BaseRevision) { throw 'Checkout HEAD must equal the declared base revision.' }
$untracked = @(& git -C $source ls-files --others --exclude-standard)
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect untracked source.' }
if ($untracked.Count -gt 0) { throw 'Untracked files exist; review and mark intended additions before exporting the patch.' }
$names = @(& git -C $source -c core.quotepath=false diff --name-only --no-ext-diff --no-textconv $BaseRevision --)
if ($LASTEXITCODE -ne 0 -or $names.Count -eq 0) { throw 'No tracked changes to export.' }
$diff = @(& git -C $source diff --binary --full-index --no-ext-diff --no-textconv $BaseRevision --)
if ($LASTEXITCODE -ne 0) { throw 'Cannot export patch.' }
$root = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $root | Out-Null
$destination = Join-Path $root $PatchId
if (Test-Path -LiteralPath $destination) { throw "Patch id already exists: $PatchId" }
New-Item -ItemType Directory -Path $destination | Out-Null
$patchPath = Join-Path $destination 'changes.patch'
[IO.File]::WriteAllText($patchPath, (($diff -join "`n") + "`n"), [Text.UTF8Encoding]::new($false))
$record = [ordered]@{
  schema_version = 1; id = $PatchId; upstream_revision = $BaseRevision
  reason = $Reason; files = $names; apply_order = $ApplyOrder
  patch_sha256 = (Get-FileHash -LiteralPath $patchPath -Algorithm SHA256).Hash.ToLowerInvariant()
  tests = $Tests; test_results = 'not-run-by-exporter'
  upstream_issue_or_commit = $UpstreamIssueOrCommit; removal_condition = $RemovalCondition
}
[IO.File]::WriteAllText((Join-Path $destination 'record.json'), ($record | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
Write-Output $destination
