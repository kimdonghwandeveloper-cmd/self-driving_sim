# 기존 전국 OSM 자료를 검증하고 지정한 워크트리에 배치한다. 다운로드나 기존 자료 덮어쓰기는 하지 않는다.
param(
    [Parameter(Mandatory = $true)][string]$SourceData,
    [string]$DestinationRepo = ''
)
$ErrorActionPreference = 'Stop'
if (-not $DestinationRepo) { $DestinationRepo = Split-Path $PSScriptRoot -Parent }

# manifest의 묶음별 SHA-256을 읽고 실제 파일을 대조해 잘못된 자료 복사를 막는다.
function Test-KoreaPack([string]$Directory, [string]$Manifest, [string]$Pack) {
    $section = [regex]::Match($Manifest, "(?ms)^\[$Pack\]\r?\n(.*?)(?=^\[|\z)").Groups[1].Value
    $file = [regex]::Match($section, '(?m)^file\s*=\s*"([a-z]+\.pack)"').Groups[1].Value
    $expected = [regex]::Match($section, '(?m)^sha256\s*=\s*"([a-f0-9]{64})"').Groups[1].Value
    if ($file -ne "$Pack.pack" -or -not $expected) { throw "Invalid $Pack manifest" }
    $actual = (Get-FileHash -LiteralPath (Join-Path $Directory $file) -Algorithm SHA256).Hash
    if ($actual -ne $expected) { throw "SHA-256 mismatch: $Directory/$file" }
}

# 자료 형식과 두 묶음을 모두 확인하고 manifest 원문을 반환한다.
function Read-VerifiedKoreaData([string]$Directory) {
    $manifest = Get-Content -LiteralPath (Join-Path $Directory 'manifest.toml') -Raw -Encoding UTF8
    if ($manifest -notmatch '(?m)^format\s*=\s*"korea-data-v1"') { throw 'Unsupported Korea data format' }
    foreach ($pack in @('roads', 'places')) { Test-KoreaPack $Directory $manifest $pack }
    return $manifest
}

$source = (Resolve-Path -LiteralPath $SourceData).Path
$repo = (Resolve-Path -LiteralPath $DestinationRepo).Path
if (-not (Test-Path -LiteralPath (Join-Path $repo 'Cargo.toml'))) { throw 'Destination is not a simulator repository' }
$manifest = Read-VerifiedKoreaData $source
$dataRoot = Join-Path $repo 'data'
$destination = Join-Path $dataRoot 'korea'
if (Test-Path -LiteralPath $destination) {
    $existing = Read-VerifiedKoreaData $destination
    if ($existing -ne $manifest) { throw 'Destination already has a different dataset; nothing was overwritten' }
    Write-Output "Verified existing Korea data: $destination"
    exit 0
}
New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
$staging = Join-Path $dataRoot ('.korea-staging-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging | Out-Null
foreach ($file in @('manifest.toml', 'roads.pack', 'places.pack')) {
    Copy-Item -LiteralPath (Join-Path $source $file) -Destination $staging
}
$null = Read-VerifiedKoreaData $staging
# 최종 절대 경로가 지정한 저장소 data 폴더 안인지 확인한 뒤 검증된 자료만 이동한다.
$resolvedStaging = (Resolve-Path -LiteralPath $staging).Path
$resolvedData = (Resolve-Path -LiteralPath $dataRoot).Path
if (-not $resolvedStaging.StartsWith($resolvedData + [IO.Path]::DirectorySeparatorChar) -or
    [IO.Path]::GetFullPath($destination) -ne (Join-Path $resolvedData 'korea')) { throw 'Unsafe destination path' }
Move-Item -LiteralPath $resolvedStaging -Destination $destination
Write-Output "Prepared verified Korea OSM data: $destination"
