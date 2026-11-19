# 임시 저장소에서 경로 거부와 필수 파일 누락을 시험한다. 정상 출력 교체·빌드·프로그램 실행은 하지 않는다.
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'package_release_support.ps1')
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('sim-release-safety-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path (Join-Path $fixture 'scripts') -Force | Out-Null
foreach ($name in @('package_release.ps1', 'package_release_support.ps1')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $fixture 'scripts')
}
$script = Join-Path $fixture 'scripts/package_release.ps1'
$script:cases = 0

# 예상한 이유로 거부되는지 확인하며 다른 오류를 통과로 세지 않는다.
function Assert-Refused([scriptblock]$Action, [string]$Expected) {
    $message = ''
    try { & $Action *> $null } catch { $message = $_.Exception.Message }
    if (-not $message.Contains($Expected)) { throw "Expected refusal '$Expected', received '$message'" }
    $script:cases++
}

# 임시 시험 파일만 만들고 상위 폴더를 자동으로 준비한다.
function Write-FixtureFile([string]$Relative, [string]$Text = 'fixture') {
    $path = Join-Path $fixture $Relative
    New-Item -ItemType Directory -Path (Split-Path $path -Parent) -Force | Out-Null
    [IO.File]::WriteAllText($path, $Text)
}

foreach ($path in @('', '.', '..', 'config', 'godot', 'data', 'dist', 'dist/', 'dist/../config', 'dist/../../', 'dist/a/b', 'dist/.hidden', 'dist/a/../b', 'C:/', $repo, $env:USERPROFILE)) {
    Assert-Refused { & $script -SkipBuild -OutputDir $path } 'Safety Error:'
}
$safe = Get-ReleaseOutputPath $fixture 'dist/sim-review'
if ($safe -ne (Join-Path $fixture 'dist/sim-review')) { throw 'Valid dist path rejected' }
$script:cases++

Write-FixtureFile 'dist/preserved/sentinel.txt' 'preserve me'
Assert-Refused { & $script -SkipBuild -OutputDir 'dist/preserved' } 'ownership marker'
if ((Get-Content (Join-Path $fixture 'dist/preserved/sentinel.txt')) -ne 'preserve me') { throw 'Existing output changed' }
Write-FixtureFile 'dist/preserved/.simulator-release.json' '{"format":"wrong","repository":"wrong"}'
Assert-Refused { & $script -SkipBuild -OutputDir 'dist/preserved' } 'ownership'
Write-FixtureFile 'dist/preserved/.simulator-release.json' (@{ format = 'simulator-release-bundle-v1'; repository = [IO.Path]::GetFullPath($fixture) } | ConvertTo-Json)
Assert-Refused { & $script -SkipBuild -OutputDir 'dist/preserved' -GodotPath (Join-Path $fixture 'missing.exe') } 'Godot runtime not found'
if ((Get-Content (Join-Path $fixture 'dist/preserved/sentinel.txt')) -ne 'preserve me') { throw 'Missing input removed a previous bundle' }

Assert-Refused { & $script -SkipBuild -OutputDir 'dist/new' -GodotPath (Join-Path $fixture 'missing.exe') } 'Godot runtime not found'
Assert-Refused { & $script -SkipBuild -Validate:$false -OutputDir 'dist/new' -GodotPath (Join-Path $fixture 'missing.exe') } 'Godot runtime not found'
Write-FixtureFile 'runtime/godot.exe'
Assert-Refused { & $script -SkipBuild -OutputDir 'dist/new' -GodotPath (Join-Path $fixture 'runtime/godot.exe') } 'Godot runtime missing'
Write-FixtureFile 'runtime/godot_console.exe'

# 실제 스크립트의 필수 파일 목록을 안전한 리터럴 AST로 읽어 누락 시험 목록과의 불일치를 방지한다.
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw $parseErrors[0] }
$assignment = $ast.Find({ param($node) $node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -eq '$requiredFiles' }, $true)
$required = @($assignment.Right.FindAll({ param($node) $node -is [Management.Automation.Language.StringConstantExpressionAst] }, $true) | ForEach-Object { $_.Value })
$required += @('scripts/build-launcher.ps1', 'scripts/run-simulator.ps1', 'scripts/updater-support.ps1')
foreach ($file in $required) { Write-FixtureFile $file }
New-Item -ItemType Directory -Path (Join-Path $fixture 'godot/.godot/imported') -Force | Out-Null
foreach ($file in $required) {
    $path = Join-Path $fixture $file
    # 빈 파일로 바꿔 누락·손상 상태를 재현한다. 실제 저장소 파일은 건드리지 않는다.
    [IO.File]::WriteAllText($path, '')
    Assert-Refused { & $script -SkipBuild -OutputDir 'dist/new' -GodotPath (Join-Path $fixture 'runtime/godot.exe') } 'Required file missing or empty:'
    Write-FixtureFile $file
    Move-Item -LiteralPath $path -Destination ($path + '.absent')
    Assert-Refused { & $script -SkipBuild -OutputDir 'dist/new' -GodotPath (Join-Path $fixture 'runtime/godot.exe') } 'Required file missing or empty:'
    Move-Item -LiteralPath ($path + '.absent') -Destination $path
}
Assert-Refused { & $script -SkipBuild -OutputDir 'dist/new' -GodotPath (Join-Path $fixture 'runtime/godot.exe') } 'sim_godot.dll not found'
Write-FixtureFile 'target/release/sim_godot.dll'
Assert-Refused { & $script -SkipBuild -OutputDir 'dist/new' -GodotPath (Join-Path $fixture 'runtime/godot.exe') } 'sim-tools.exe not found'

# 기존 dist junction이 저장소 밖을 가리켜도 경로 검사에서 먼저 거부한다.
$linkRoot = Join-Path $fixture 'junction-repo'
New-Item -ItemType Directory -Path $linkRoot | Out-Null
New-Item -ItemType Junction -Path (Join-Path $linkRoot 'dist') -Target (Join-Path $fixture 'runtime') | Out-Null
Assert-Refused { Get-ReleaseOutputPath $linkRoot 'dist/new' } 'Safety Error:'
if (Test-Path -LiteralPath (Join-Path $fixture 'dist/new')) { throw 'Negative tests created an output folder' }
Write-Output "PACKAGE SAFETY OK: $script:cases cases; no output removed; fixtures: $fixture"
