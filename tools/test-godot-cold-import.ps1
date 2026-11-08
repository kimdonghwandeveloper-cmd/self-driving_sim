# 캐시 없는 별도 프로젝트에서 첫 import와 Rust 연결을 검증한다.
param(
    [Parameter(Mandatory = $true)][string]$Godot,
    [string]$ScratchRoot = [IO.Path]::GetTempPath(),
    [ValidateRange(1, 10)][int]$Runs = 3,
    [ValidateRange(0, 10000)][int]$FrameDelay = 1000
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$engine = (Resolve-Path -LiteralPath $Godot).Path
$dll = Join-Path $repo 'target/debug/sim_godot.dll'
if (-not (Test-Path -LiteralPath $dll)) {
    throw '먼저 cargo.exe build --locked -p sim-godot 으로 확장을 빌드하세요.'
}

# 프로세스가 멈추거나 비정상 종료하면 로그 위치와 함께 실패시킨다.
function Invoke-GodotChecked {
    param([string[]]$EngineArguments, [string]$Log)
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $engine
    $quoted = foreach ($arg in $EngineArguments) {
        if ($arg.Contains('"')) { throw '인수에 큰따옴표를 사용할 수 없습니다.' }
        '"' + $arg + '"'
    }
    $info.Arguments = $quoted -join ' '
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    $finished = $process.WaitForExit(120000)
    if (-not $finished) { $process.Kill(); $process.WaitForExit() }
    $output = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
    [IO.File]::WriteAllText($Log, $output)
    $exitCode = $process.ExitCode
    $process.Dispose()
    if (-not $finished) { throw "Godot timeout: $Log" }
    if ($exitCode -ne 0) { throw "Godot exit ${exitCode}: $Log" }
    return $output
}

$session = Join-Path $ScratchRoot ('godot-cold-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $session -Force | Out-Null
Write-Output "검증 기록: $session"
for ($i = 1; $i -le $Runs; $i++) {
    $trial = Join-Path $session "run-$i"
    $project = Join-Path $trial 'godot'
    New-Item -ItemType Directory -Path $project, "$trial/target/debug" -Force | Out-Null
    Get-ChildItem -LiteralPath (Join-Path $repo 'godot') -Force |
        Where-Object { $_.Name -ne '.godot' } |
        ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $project -Recurse }
    Copy-Item -LiteralPath $dll -Destination "$trial/target/debug/sim_godot.dll"
    if (Test-Path -LiteralPath "$project/.godot") { throw '검증 시작 전 캐시가 존재합니다.' }
    $import = Invoke-GodotChecked -EngineArguments @('--headless', '--path', $project, '--import', '--frame-delay', "$FrameDelay") -Log "$trial/import.log"
    if (-not (Test-Path -LiteralPath "$project/.godot/extension_list.cfg")) {
        throw "확장 목록이 생성되지 않았습니다: $trial"
    }
    $smoke = Invoke-GodotChecked -EngineArguments @('--headless', '--path', $project, '--script', 'res://tests/smoke_test.gd') -Log "$trial/smoke.log"
    if ($smoke -notmatch 'SMOKE OK:') { throw "연결 성공 메시지가 없습니다: $trial" }
    Write-Output "PASS $i/$Runs : 첫 import exit 0, Rust 연결 exit 0"
}
# 실패 기록도 분석할 수 있도록 복사본과 로그를 자동 삭제하지 않는다.
