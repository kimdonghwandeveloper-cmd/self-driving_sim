$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/updater-support.ps1"
$checks = 0
# 기대한 오류가 발생하는지 검사한다.
function Reject($Action) {
    try { & $Action } catch { $script:checks++; return }
    throw 'Expected rejection'
}
# 파일 경로가 Windows 배포 루트 밖으로 나가거나 특수 이름으로 해석되지 않아야 한다.
foreach ($path in @('../x', '/x', 'C:/x', 'a\b', 'a:b', 'a//b', 'a/./b', 'a/../b', 'CON', 'a/NUL.txt', 'a. ', 'a/b.')) {
    Reject { Assert-UpdatePath $path }
}
Assert-UpdatePath 'godot/assets/maps/test.glb'
$checks++
$rsa = New-Object Security.Cryptography.RSACryptoServiceProvider 2048
try {
    $bytes = [Text.Encoding]::UTF8.GetBytes('{"version":1,"files":[]}')
    $signature = [Convert]::ToBase64String($rsa.SignData($bytes, 'SHA256'))
    Assert-UpdateSignature $bytes $signature $rsa.ToXmlString($false)
    $checks++
    Reject { Assert-UpdateSignature ([Text.Encoding]::UTF8.GetBytes('changed')) $signature $rsa.ToXmlString($false) }
    Reject { Assert-UpdateSignature $bytes 'not-base64' $rsa.ToXmlString($false) }
} finally { $rsa.Dispose() }
# 서명이 유효해도 세대 증가, 중복 경로, 필수 파일을 별도로 검사한다.
$required = @('godot.exe','godot/project.godot','godot/tests/smoke_test.gd','godot/sim_godot.gdextension','target/debug/sim_godot.dll','target/release/sim_godot.dll','data/korea/manifest.toml','data/korea/roads.pack','data/korea/places.pack')
$entries = @($required | ForEach-Object { @{path=$_; sha256=('a' * 64); size=1} })
$manifest = @{format='simulator-update-v1'; version=2; files=$entries}
Assert-UpdateManifest $manifest 1
$checks++
Reject { Assert-UpdateManifest $manifest 2 }
$manifest.files = @($entries + $entries[0])
Reject { Assert-UpdateManifest $manifest 1 }
$manifest.files = @($entries | Select-Object -Skip 1)
Reject { Assert-UpdateManifest $manifest 1 }
# 실제 EXE와 세대 디렉터리로 원자 교체·오류 복구·사용자 자료 보존을 시험한다.
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('sim-updater-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
$engine = Join-Path $fixture 'engine.exe'
Add-Type -TypeDefinition 'using System; public static class FakeEngine { /* 시험용 엔진의 종료 규칙을 제공한다. */ public static int Main() { return 0; } }' -OutputAssembly $engine -OutputType ConsoleApplication
$install = Join-Path $fixture 'install'
New-Item -ItemType Directory -Path (Join-Path $install 'config') -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $install 'cache/routes') -Force | Out-Null
[IO.File]::WriteAllText((Join-Path $install 'config/vehicle.toml'), 'user-setting')
[IO.File]::WriteAllText((Join-Path $install 'cache/routes/saved.toml'), 'user-route')
Copy-Item -LiteralPath $engine -Destination (Join-Path $install 'godot.exe')
$payload = Join-Path $fixture 'payload'
$records = @()
foreach ($name in $required) {
    $file = Join-Path $payload $name
    New-Item -ItemType Directory -Path (Split-Path $file -Parent) -Force | Out-Null
    if ($name -eq 'godot.exe') { Copy-Item -LiteralPath $engine -Destination $file } else { [IO.File]::WriteAllText($file, $name) }
    $records += @{path=$name; size=(Get-Item -LiteralPath $file).Length; sha256=(Get-FileHash -LiteralPath $file).Hash.ToLowerInvariant()}
}
$manifest = @{format='simulator-update-v1';version=1;files=$records}
$records += @{path='godot/.gdignore';size=0;sha256='e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855'}
$manifest.files=$records
$script:downloads = 0
$fetch = { param($path, $destination) $script:downloads++; Copy-Item -LiteralPath (Join-Path $payload $path) -Destination $destination }
$current = Install-UpdateGeneration $install @{version=0;path=$install} $manifest $fetch
if ($current.version -ne 1 -or $downloads -ne ($required.Count - 1)) { throw 'Commit/reuse failed' }
if (-not (Test-Path -LiteralPath (Join-Path $current.path 'godot/.gdignore'))) { throw 'Empty file was not generated' }
$checks++
if ([IO.File]::ReadAllText((Join-Path $current.path 'config/vehicle.toml')) -ne 'user-setting' -or [IO.File]::ReadAllText((Join-Path $current.path 'cache/routes/saved.toml')) -ne 'user-route') { throw 'User data lost' }
$checks += 3
$manifest.version = 2
$records[1].sha256 = ('b' * 64)
Reject { Install-UpdateGeneration $install $current $manifest $fetch }
if ((Get-UpdateCurrent $install).version -ne 1) { throw 'Failed update changed pointer' }
$checks++
$records[1].sha256 = (Get-FileHash -LiteralPath (Join-Path $payload $records[1].path)).Hash.ToLowerInvariant()
$script:downloads = 0
$current = Install-UpdateGeneration $install $current $manifest $fetch
if ($current.version -ne 2 -or $downloads -ne 0 -or -not (Test-Path -LiteralPath (Join-Path $install 'previous.json'))) { throw 'Second generation failed' }
$checks++
# 새 엔진 검사 실패 시 현재 세대와 저장 경로는 그대로 남는다.
$badEngine = Join-Path $fixture 'bad-engine.exe'
Add-Type -TypeDefinition 'public static class BadEngine { /* 실패하는 새 엔진을 재현한다. */ public static int Main() { return 1; } }' -OutputAssembly $badEngine -OutputType ConsoleApplication
Copy-Item -LiteralPath $badEngine -Destination (Join-Path $payload 'godot.exe') -Force
$records[0].size = (Get-Item -LiteralPath $badEngine).Length
$records[0].sha256 = (Get-FileHash -LiteralPath $badEngine).Hash.ToLowerInvariant()
$manifest.version = 3
Reject { Install-UpdateGeneration $install $current $manifest $fetch }
if ((Get-UpdateCurrent $install).version -ne 2) { throw 'Bad runtime was activated' }
$checks++
# 실제 Windows 런처를 생성하고 오프라인 실행의 종료 코드를 검사한다.
$launcher = Join-Path $fixture 'Simulator.exe'
& "$PSScriptRoot/build-launcher.ps1" -OutputPath $launcher
New-Item -ItemType Directory -Path (Join-Path $fixture 'updater') | Out-Null
Copy-Item -LiteralPath "$PSScriptRoot/run-simulator.ps1", "$PSScriptRoot/updater-support.ps1" -Destination (Join-Path $fixture 'updater')
Copy-Item -LiteralPath $engine -Destination (Join-Path $fixture 'godot.exe')
$process = Start-Process -FilePath $launcher -PassThru
if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'Offline launcher timed out' }
$process.Refresh()
if ($process.ExitCode -ne 0) { throw 'Offline EXE launcher failed' }
$checks++
# 잘못된 서버 설정도 기존 EXE 실행을 막지 않아야 한다.
[IO.File]::WriteAllText((Join-Path $fixture 'update.json'), '{"enabled":true,"url":"http://invalid/","public_key":"invalid"}')
$process = Start-Process -FilePath $launcher -PassThru
if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'Fallback launch timed out' }
$process.Refresh()
if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath (Join-Path $fixture 'update.log'))) { throw 'Fallback launch failed' }
$checks++
# 게시 도구의 결과 서명을 실제 공개 키로 대조하며 검증되지 않은 번들은 거부한다.
$signer = New-Object Security.Cryptography.RSACryptoServiceProvider 2048
try {
    $key = Join-Path $fixture 'test-only-private-key.xml'
    [IO.File]::WriteAllText($key, $signer.ToXmlString($true))
    [IO.File]::WriteAllText((Join-Path $payload '.simulator-release.json'), '{"format":"simulator-release-bundle-v1","validated":true,"release_version":4}')
    $published = Join-Path $fixture 'published'
    & "$PSScriptRoot/publish-update.ps1" -Bundle $payload -OutputDir $published -PrivateKeyFile $key -Version 4
    Assert-UpdateSignature ([IO.File]::ReadAllBytes((Join-Path $published 'manifest.json'))) ([IO.File]::ReadAllText((Join-Path $published 'manifest.sig'))) $signer.ToXmlString($false)
    $checks++
    $github = Join-Path $fixture 'github-assets'
    & "$PSScriptRoot/publish-update.ps1" -Bundle $payload -OutputDir $github -PrivateKeyFile $key -Version 4 -GitHubAssets
    $githubManifest = Get-Content -Raw -LiteralPath (Join-Path $github 'manifest.json') | ConvertFrom-Json
    foreach ($record in $githubManifest.files) {
        if (-not (Test-UpdateFile (Join-Path $github ($record.sha256 + '.blob')) $record)) { throw 'GitHub asset layout mismatch' }
    }
    $checks++
    Copy-Item -LiteralPath $launcher -Destination (Join-Path $payload 'Simulator.exe')
    Copy-Item -LiteralPath (Join-Path $fixture 'updater') -Destination (Join-Path $payload 'updater') -Recurse
    $prepared = Join-Path $fixture 'github-prepared'
    & "$PSScriptRoot/publish-github-release.ps1" -Bundle $payload -OutputDir $prepared -PrivateKeyFile $key -Version 4 -Commit ('a' * 40) -PrepareOnly
    $settings=Get-Content -Raw -LiteralPath (Join-Path $prepared 'bootstrap/update.json') | ConvertFrom-Json
    if ($settings.layout -ne 'github-assets' -or -not $settings.enabled -or $settings.public_key -ne $signer.ToXmlString($false) -or -not (Test-Path -LiteralPath (Join-Path $prepared 'Simulator-Windows-v4.zip'))) { throw 'GitHub bootstrap mismatch' }
    $checks++
    [IO.File]::WriteAllText((Join-Path $payload '.simulator-release.json'), '{"format":"simulator-release-bundle-v1","validated":false}')
    Reject { & "$PSScriptRoot/publish-update.ps1" -Bundle $payload -OutputDir (Join-Path $fixture 'not-published') -PrivateKeyFile $key -Version 5 }
} finally { $signer.Dispose() }
# 네트워크 경로의 실제 C# 다운로드 클라이언트도 컴파일되는지 검사한다.
$tokens=$null; $errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile("$PSScriptRoot/run-simulator.ps1",[ref]$tokens,[ref]$errors)
if ($errors.Count) { throw $errors[0] }
$source=$ast.Find({param($node) $node -is [Management.Automation.Language.StringConstantExpressionAst] -and $node.Value.Contains('sealed class SimulatorUpdateClient')},$true)
Add-Type -TypeDefinition $source.Value
$client=New-Object SimulatorUpdateClient
$client.Dispose()
$checks++
Write-Output "UPDATER TEST OK: $checks; fixture=$fixture"
