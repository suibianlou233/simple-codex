param(
  [ValidateSet('debug', 'release')]
  [string]$Profile = 'release'
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
$cargoArguments = @('build', '-p', 'simple-windows-sandbox', '--bins')
if ($Profile -eq 'release') {
  $cargoArguments += '--release'
}

& cargo @cargoArguments
if ($LASTEXITCODE -ne 0) {
  throw "Simple sandbox helper build failed with exit code $LASTEXITCODE"
}

$source = Join-Path $repositoryRoot "target\$Profile"
$destination = Join-Path $repositoryRoot 'apps\desktop\src-tauri\resources\simple-resources'
$helpers = @(
  'simple-windows-sandbox-setup.exe',
  'simple-command-runner.exe'
)

New-Item -ItemType Directory -Force -Path $destination | Out-Null
foreach ($helper in $helpers) {
  $sourcePath = Join-Path $source $helper
  if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
    throw "Built Simple sandbox helper was not found at $sourcePath"
  }
  Copy-Item -Force -LiteralPath $sourcePath -Destination (Join-Path $destination $helper)
}

Write-Output "Synced Simple Windows sandbox helpers ($Profile) to $destination"
