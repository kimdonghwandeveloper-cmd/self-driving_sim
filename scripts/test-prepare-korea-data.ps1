# 자료 준비 스크립트의 v1/v2 허용, 기존 자료 보존, 미지원 형식·해시 오류 거부를 임시 파일로 검사한다.
$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ('sim-korea-prepare-' + [guid]::NewGuid().ToString('N'))
$prepare = Join-Path $PSScriptRoot 'prepare-korea-data.ps1'
New-Item -ItemType Directory -Path $root | Out-Null
$cases = 0
foreach ($version in @(1, 2)) {
    $source = Join-Path $root "source-v$version"
    $target = Join-Path $root "target-v$version"
    New-Item -ItemType Directory -Path $source, $target | Out-Null
    [IO.File]::WriteAllText((Join-Path $target 'Cargo.toml'), '[workspace]')
    foreach ($pack in @('roads', 'places')) { [IO.File]::WriteAllText((Join-Path $source "$pack.pack"), "fixture-$pack-v$version") }
    $roadHash = (Get-FileHash (Join-Path $source 'roads.pack')).Hash.ToLowerInvariant()
    $placeHash = (Get-FileHash (Join-Path $source 'places.pack')).Hash.ToLowerInvariant()
    $manifest = "format = `"korea-data-v$version`"`n[roads]`nfile = `"roads.pack`"`nsha256 = `"$roadHash`"`n[places]`nfile = `"places.pack`"`nsha256 = `"$placeHash`"`n"
    [IO.File]::WriteAllText((Join-Path $source 'manifest.toml'), $manifest)
    & $prepare -SourceData $source -DestinationRepo $target
    if ((Get-FileHash (Join-Path $target 'data/korea/places.pack')).Hash.ToLowerInvariant() -ne $placeHash) { throw 'Copied pack mismatch' }
    $cases++
    [IO.File]::WriteAllText((Join-Path $target 'data/korea/sentinel.txt'), 'preserved')
    & $prepare -SourceData $source -DestinationRepo $target
    if ((Get-Content (Join-Path $target 'data/korea/sentinel.txt')) -ne 'preserved') { throw 'Existing data changed' }
    $cases++
}
# 未지원 판본과 해시 불일치는 대상 폴더를 생성하기 전에 거부해야 한다.
foreach ($mode in @('format', 'hash')) {
    $target = Join-Path $root "rejected-$mode"
    New-Item -ItemType Directory -Path $target | Out-Null
    [IO.File]::WriteAllText((Join-Path $target 'Cargo.toml'), '[workspace]')
    $bad = if ($mode -eq 'format') { $manifest.Replace('korea-data-v2', 'korea-data-v3') } else { $manifest.Replace($placeHash, ('0' * 64)) }
    [IO.File]::WriteAllText((Join-Path $source 'manifest.toml'), $bad)
    $message = ''
    try { & $prepare -SourceData $source -DestinationRepo $target } catch { $message = $_.Exception.Message }
    $expected = if ($mode -eq 'format') { 'Unsupported Korea data format' } else { 'SHA-256 mismatch' }
    if (-not $message.Contains($expected) -or (Test-Path (Join-Path $target 'data'))) { throw "Incorrect refusal: $message" }
    $cases++
}
Write-Output "PREPARE KOREA DATA OK: $cases cases; fixtures: $root"
