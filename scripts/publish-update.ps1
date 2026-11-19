param(
    [Parameter(Mandatory=$true)][string]$Bundle,
    [Parameter(Mandatory=$true)][string]$OutputDir,
    [Parameter(Mandatory=$true)][string]$PrivateKeyFile,
    [Parameter(Mandatory=$true)][int]$Version
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/updater-support.ps1"
$Bundle = (Resolve-Path -LiteralPath $Bundle).Path
$marker = Get-Content -Raw -LiteralPath (Join-Path $Bundle '.simulator-release.json') | ConvertFrom-Json
if (-not $marker.validated) { throw 'Only validated bundles can be published' }
if (Test-Path -LiteralPath $OutputDir) { throw 'Output directory must be new; existing releases are never overwritten' }
$output = [IO.Path]::GetFullPath($OutputDir)
if ($output.StartsWith($Bundle + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Output must be outside bundle' }
Assert-NoUpdateLinks $Bundle
$files = @()
# 사용자의 기록과 런처 자체는 원격 프로그램 업데이트 대상에서 제외한다.
foreach ($item in @(Get-ChildItem -LiteralPath $Bundle -File -Recurse -Force | Sort-Object FullName)) {
    $path = $item.FullName.Substring($Bundle.Length + 1).Replace('\','/')
    if ($path -match '^(?i:cache|logs|versions|updater)(/|$)' -or $path -match '^(?i:current.json|previous.json|update.json|update.log|Simulator.exe|\.simulator-launch.lock)$') { continue }
    $files += @{path=$path; size=$item.Length; sha256=(Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
}
$manifest = @{format='simulator-update-v1'; version=$Version; files=$files}
Assert-UpdateManifest $manifest 0
New-Item -ItemType Directory -Path (Join-Path $output 'files') | Out-Null
foreach ($file in $files) {
    $target = Join-Path (Join-Path $output 'files') $file.path
    New-Item -ItemType Directory -Path (Split-Path $target -Parent) -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $Bundle $file.path) -Destination $target
    if (-not (Test-UpdateFile $target $file)) { throw 'Bundle changed while publishing' }
}
$bytes = (New-Object Text.UTF8Encoding($false)).GetBytes(($manifest | ConvertTo-Json -Depth 6))
$rsa = New-Object Security.Cryptography.RSACryptoServiceProvider
try {
    $rsa.FromXmlString([IO.File]::ReadAllText((Resolve-Path -LiteralPath $PrivateKeyFile)))
    [IO.File]::WriteAllBytes((Join-Path $output 'manifest.json'), $bytes)
    [IO.File]::WriteAllText((Join-Path $output 'manifest.sig'), [Convert]::ToBase64String($rsa.SignData($bytes, 'SHA256')))
    Assert-UpdateSignature $bytes ([IO.File]::ReadAllText((Join-Path $output 'manifest.sig'))) $rsa.ToXmlString($false)
} finally { $rsa.Dispose() }
Write-Output "Signed update ready: $output"
