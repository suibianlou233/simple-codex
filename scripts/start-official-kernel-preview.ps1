param()
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $root 'desktop-versions/official-283-preview/Simple-preview-process-time.exe'
$manifest = Join-Path $root 'kernels/packages/official-283-windows-candidate-1/kernel.json'
if (!(Test-Path -LiteralPath $binary -PathType Leaf) -or !(Test-Path -LiteralPath $manifest -PathType Leaf)) {
    throw 'Preview desktop or pinned kernel package is missing; see kernels/desktop-preview.md.'
}
# Only the child receives this selection. Never change the machine/user environment.
$previous = [Environment]::GetEnvironmentVariable('SIMPLE_CODEX_KERNEL_MANIFEST', 'Process')
try {
    $env:SIMPLE_CODEX_KERNEL_MANIFEST = (Resolve-Path -LiteralPath $manifest).Path
    Start-Process -FilePath $binary -WorkingDirectory $root -PassThru |
        Select-Object Id, ProcessName
} finally {
    [Environment]::SetEnvironmentVariable('SIMPLE_CODEX_KERNEL_MANIFEST', $previous, 'Process')
}
