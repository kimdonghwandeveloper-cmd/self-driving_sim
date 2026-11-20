# 삭제 없는 경로·입력 검사를 분리해 패키징 전에 회귀 시험한다.

# 저장소 dist 바로 아래의 전용 폴더 이름만 허용하고 junction/symlink 경유를 거부한다.
function Get-ReleaseOutputPath([string]$RepoRoot, [string]$OutputDir, [string]$DistributionRoot = '') {
    $relative = $OutputDir.Replace('\', '/').TrimEnd('/')
    if ($relative -notmatch '^dist/[A-Za-z0-9][A-Za-z0-9_-]*$') {
        throw 'Safety Error: OutputDir must be dist/<dedicated-folder> (letters, digits, _ or -)'
    }
    $root = [IO.Path]::GetFullPath($RepoRoot)
    $dist = Join-Path $root 'dist'
    if ($DistributionRoot) {
        if (-not [IO.Path]::IsPathRooted($DistributionRoot)) { throw 'Safety Error: distribution root must be absolute' }
        $dist = [IO.Path]::GetFullPath($DistributionRoot).TrimEnd('\')
        if ([IO.Path]::GetFileName($dist) -ne 'dist' -or $dist -eq [IO.Path]::GetPathRoot($dist)) { throw 'Safety Error: distribution root must be a dedicated dist directory' }
        $ancestor = $dist
        while ($ancestor) {
            if (Test-Path -LiteralPath $ancestor) {
                $item = Get-Item -LiteralPath $ancestor -Force
                if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Safety Error: distribution root crosses a link' }
            }
            $ancestor = [IO.Path]::GetDirectoryName($ancestor)
        }
    }
    $output = [IO.Path]::GetFullPath((Join-Path $dist ($relative.Substring(5))))
    if ([IO.Path]::GetDirectoryName($output) -ne $dist) { throw 'Safety Error: output escaped dist' }
    foreach ($path in @($dist, $output)) {
        if (Test-Path -LiteralPath $path) {
            $item = Get-Item -LiteralPath $path -Force
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw "Safety Error: output is not an ordinary directory: $path"
            }
        }
    }
    return $output
}

# 기존 폴더는 같은 저장소의 패키저가 남긴 소유 표식이 있을 때만 교체한다.
function Assert-ReleaseOwnership([string]$OutputPath, [string]$RepoRoot) {
    if (-not (Test-Path -LiteralPath $OutputPath)) { return }
    $marker = Join-Path $OutputPath '.simulator-release.json'
    if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) {
        throw 'Safety Error: existing output has no ownership marker; choose a new folder'
    }
    $record = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
    if ($record.format -ne 'simulator-release-bundle-v1' -or $record.repository -ne [IO.Path]::GetFullPath($RepoRoot)) {
        throw 'Safety Error: output ownership does not match this repository'
    }
}

# 필수 파일이 없거나 비어 있으면 검사를 건너뛰지 않고 즉시 실패한다.
function Assert-ReleaseFiles([string]$Root, [string[]]$Files) {
    foreach ($relative in $Files) {
        $path = Join-Path $Root $relative
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or (Get-Item -LiteralPath $path).Length -eq 0) {
            throw "Required file missing or empty: $path"
        }
    }
}

# 잘못 지정한 Godot 경로는 자동 탐색으로 숨기지 않으며 GUI·콘솔 런타임을 모두 요구한다.
function Get-ReleaseGodot([string]$GodotPath) {
    if ([string]::IsNullOrWhiteSpace($GodotPath)) {
        $GodotPath = Join-Path $env:USERPROFILE 'Downloads/Godot_v4.7.2-stable_win64.exe/Godot_v4.7.2-stable_win64.exe'
    }
    if (-not (Test-Path -LiteralPath $GodotPath -PathType Leaf)) { throw "Godot runtime not found: $GodotPath" }
    $path = (Resolve-Path -LiteralPath $GodotPath).Path
    if ($path.EndsWith('_console.exe', [StringComparison]::OrdinalIgnoreCase)) {
        $gui = $path.Substring(0, $path.Length - 12) + '.exe'
        $console = $path
    } else {
        if (-not $path.EndsWith('.exe', [StringComparison]::OrdinalIgnoreCase)) { throw 'Godot runtime must be an exe' }
        $gui = $path
        $console = $path.Substring(0, $path.Length - 4) + '_console.exe'
    }
    foreach ($file in @($gui, $console)) {
        if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or (Get-Item -LiteralPath $file).Length -eq 0) {
            throw "Godot runtime missing or empty: $file (supply -GodotPath)"
        }
    }
    return @{ Gui = $gui; Console = $console }
}
