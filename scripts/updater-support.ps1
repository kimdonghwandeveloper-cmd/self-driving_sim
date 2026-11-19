# 배포 루트 안의 일반 파일만 허용하고 Windows 별칭·ADS·경로 탈출을 차단한다.
function Assert-UpdatePath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path) -or $Path.Contains('\') -or $Path -match '[:\x00-\x1f]' -or $Path.StartsWith('/')) { throw "Invalid update path: $Path" }
    foreach ($part in $Path.Split('/')) {
        if ($part -in @('', '.', '..') -or $part -match '[. ]$|[<>"|?*]' -or $part -match '^(?i:CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])(?:\.|$)') { throw "Invalid update path: $Path" }
    }
}
# 고정 공개 키로 원문 바이트의 RSA SHA-256 서명을 검증한다.
function Assert-UpdateSignature([byte[]]$Bytes, [string]$Signature, [string]$PublicKey) {
    $rsa = New-Object Security.Cryptography.RSACryptoServiceProvider
    try {
        $rsa.FromXmlString($PublicKey)
        if ($rsa.KeySize -lt 2048) { throw 'Update signing key must be at least 2048 bits' }
        if (-not $rsa.VerifyData($Bytes, 'SHA256', [Convert]::FromBase64String($Signature.Trim()))) { throw 'Update signature mismatch' }
    } finally { $rsa.Dispose() }
}
# 서명과 별개로 세대·파일 중복·필수 실행 파일 계약을 검사한다.
function Assert-UpdateManifest($Manifest, [long]$CurrentVersion) {
    if ($Manifest.format -ne 'simulator-update-v1' -or $Manifest.version -isnot [ValueType] -or [double]$Manifest.version -ne [long]$Manifest.version -or [long]$Manifest.version -le $CurrentVersion -or [long]$Manifest.version -gt 2147483647) { throw 'Unsupported or non-increasing update version' }
    if (@($Manifest.files).Count -eq 0 -or @($Manifest.files).Count -gt 100000) { throw 'Invalid update file count' }
    $seen = @{}
    [long]$total = 0
    foreach ($file in $Manifest.files) {
        Assert-UpdatePath $file.path
        if ($seen.ContainsKey($file.path) -or $file.sha256 -cnotmatch '^[a-f0-9]{64}$' -or $file.size -isnot [ValueType] -or [double]$file.size -ne [long]$file.size -or $file.size -lt 0 -or $file.size -gt 4294967296) { throw 'Invalid or duplicate file record' }
        if ($file.path -match '^(?i:cache|logs|versions|updater)(/|$)' -or $file.path -match '^(?i:current.json|update.json|Simulator.exe)$') { throw 'Reserved update path' }
        $seen[$file.path] = $true
        $total += [long]$file.size
        if ($total -gt 21474836480) { throw 'Update exceeds 20 GiB' }
    }
    foreach ($required in @('godot.exe','godot/project.godot','godot/tests/smoke_test.gd','godot/sim_godot.gdextension','target/debug/sim_godot.dll','target/release/sim_godot.dll','data/korea/manifest.toml','data/korea/roads.pack','data/korea/places.pack')) {
        if (-not $seen.ContainsKey($required)) { throw "Missing required file: $required" }
    }
}
# 다운로드와 재사용 파일 모두 길이·SHA-256을 동일하게 검사한다.
function Test-UpdateFile([string]$Path, $Record) {
    return (Test-Path -LiteralPath $Path -PathType Leaf) -and (Get-Item -LiteralPath $Path).Length -eq [long]$Record.size -and (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $Record.sha256
}
# 재분석 지점이 포함된 사용자 자료를 따라 외부 폴더를 복사하지 않는다.
function Assert-NoUpdateLinks([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return }
    $items = @((Get-Item -LiteralPath $Path -Force))
    while ($items.Count -gt 0) {
        $item = $items[0]
        $items = @($items | Select-Object -Skip 1)
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Reparse point is not allowed: $($item.FullName)" }
        if ($item.PSIsContainer) { $items += @(Get-ChildItem -LiteralPath $item.FullName -Force) }
    }
}
# 로컬 세대 포인터는 검증한 고정 이름만 허용한다. 손상 시 호출자가 초기 번들로 복구한다.
function Get-UpdateCurrent([string]$Root) {
    $pointer = Join-Path $Root 'current.json'
    if (-not (Test-Path -LiteralPath $pointer)) {
        $version = 0
        $marker = Join-Path $Root '.simulator-release.json'
        if (Test-Path -LiteralPath $marker) {
            $bundle = Get-Content -Raw -LiteralPath $marker | ConvertFrom-Json
            if ($bundle.release_version -ge 0 -and $bundle.release_version -le 2147483647) { $version = [long]$bundle.release_version }
        }
        return @{version=$version; path=$Root}
    }
    $current = Get-Content -Raw -LiteralPath $pointer | ConvertFrom-Json
    if ($current.version -lt 1 -or $current.folder -cne ('v' + $current.version)) { throw 'Invalid current pointer' }
    $path = Join-Path $Root ('versions/' + $current.folder)
    if (-not (Test-Path -LiteralPath (Join-Path $path 'godot.exe'))) { throw 'Current runtime missing' }
    return @{version=[long]$current.version; path=$path}
}
# 검증된 세대 포인터를 원자 교체하고 이전 포인터를 남긴다.
function Set-UpdateCurrent([string]$Root, [long]$Version) {
    $target = Join-Path $Root 'current.json'
    $temporary = Join-Path $Root ('current-' + [guid]::NewGuid().ToString('N') + '.tmp')
    [IO.File]::WriteAllText($temporary, (@{version=$Version; folder="v$Version"} | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
    if (Test-Path -LiteralPath $target) { [IO.File]::Replace($temporary, $target, (Join-Path $Root 'previous.json')) } else { [IO.File]::Move($temporary, $target) }
}

# 파일 검증·실행 검사·사용자 자료 보존 후에만 새 세대를 원자 게시한다. 실패한 stage는 진단용으로 남긴다.
function Install-UpdateGeneration([string]$Root, $Current, $Manifest, [scriptblock]$Download) {
    Assert-UpdateManifest $Manifest $Current.version
    foreach ($path in @($Root, $Current.path)) {
        if ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Install root must not be a reparse point' }
    }
    if (Test-Path -LiteralPath (Join-Path $Root 'versions')) { Assert-NoUpdateLinks (Join-Path $Root 'versions') }
$versions = Join-Path $Root 'versions'
New-Item -ItemType Directory -Path $versions -Force | Out-Null
$stage = Join-Path $versions ('stage-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
foreach ($file in $manifest.files) {
    $destination = Join-Path $stage $file.path
    New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
    $old = Join-Path $current.path $file.path
    # 전국 지도 등 내용이 같은 파일은 다운로드하지 않고 기존 세대에서 복사한다.
    if (Test-UpdateFile $old $file) { Copy-Item -LiteralPath $old -Destination $destination } else {
        $escaped = ($file.path.Split('/') | ForEach-Object { [uri]::EscapeDataString($_) }) -join '/'
        & $Download $escaped $destination
    }
    if (-not (Test-UpdateFile $destination $file)) { throw "Downloaded file mismatch: $($file.path)" }
}
# 새 DLL이 실제로 로드되는지 검사한 뒤에만 세대를 게시한다.
$runtime = Join-Path $stage 'godot.exe'
$probe = Start-Process -FilePath $runtime -ArgumentList @('--headless', '--path', ('"' + (Join-Path $stage 'godot') + '"'), '--script', 'res://tests/smoke_test.gd') -WindowStyle Hidden -PassThru
if (-not $probe.WaitForExit(60000)) { $probe.Kill(); throw 'Updated runtime smoke test timed out' }
$probe.Refresh()
if ($probe.ExitCode -ne 0) { throw 'Updated runtime smoke test failed' }
foreach ($folder in @('config', 'cache', 'logs')) {
    $source = Join-Path $current.path $folder
    if (Test-Path -LiteralPath $source) {
        Assert-NoUpdateLinks $source
        $destination = Join-Path $stage $folder
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        Get-ChildItem -LiteralPath $source -Force | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $destination -Recurse -Force }
    }
}
$final = Join-Path $versions ('v' + $manifest.version)
if (Test-Path -LiteralPath $final) { throw 'Version directory already exists; keeping current version' }
[IO.Directory]::Move($stage, $final)
Set-UpdateCurrent $Root $manifest.version
return (Get-UpdateCurrent $Root)
}
