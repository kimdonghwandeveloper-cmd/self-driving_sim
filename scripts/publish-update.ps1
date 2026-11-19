param(
    [Parameter(Mandatory=$true)][string]$Bundle,
    [Parameter(Mandatory=$true)][string]$OutputDir,
    [Parameter(Mandatory=$true)][string]$PrivateKeyFile,
    [Parameter(Mandatory=$true)][int]$Version,
    [switch]$GitHubAssets
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/updater-support.ps1"
$Bundle = (Resolve-Path -LiteralPath $Bundle).Path
$keyPath = (Resolve-Path -LiteralPath $PrivateKeyFile).Path
if ($keyPath.StartsWith($Bundle + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Private key must be outside the bundle' }
$marker = Get-Content -Raw -LiteralPath (Join-Path $Bundle '.simulator-release.json') | ConvertFrom-Json
if ($marker.format -ne 'simulator-release-bundle-v1' -or $marker.validated -ne $true) { throw 'Only validated bundles can be published' }
if ($Version -lt 1 -or $marker.release_version -ne $Version) { throw 'Build bundle with -ReleaseVersion matching published Version' }
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
if ($GitHubAssets) {
    if (@($files.sha256 | Select-Object -Unique).Count + 3 -gt 1000 -or @($files | Where-Object { $_.size -ge 2147483648 }).Count -gt 0) { throw 'GitHub Releases asset limits exceeded' }
}
New-Item -ItemType Directory -Path (Join-Path $output 'files') | Out-Null
foreach ($file in $files) {
    $target = Join-Path (Join-Path $output 'files') $file.path
    if ($GitHubAssets) { $target = Join-Path $output ($file.sha256 + '.blob') }
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
