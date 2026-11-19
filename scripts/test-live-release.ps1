param(
    [Parameter(Mandatory=$true)][string]$Bundle,
    [Parameter(Mandatory=$true)][string]$SettingsFile
)
$ErrorActionPreference='Stop'
. "$PSScriptRoot/updater-support.ps1"
# 실제 런처의 HTTPS 클라이언트 구현을 그대로 사용하고 자격 증명은 전달하지 않는다.
$tokens=$null; $errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile("$PSScriptRoot/run-simulator.ps1",[ref]$tokens,[ref]$errors)
if($errors.Count){throw $errors[0]}
$source=$ast.Find({param($node) $node -is [Management.Automation.Language.StringConstantExpressionAst] -and $node.Value.Contains('sealed class SimulatorUpdateClient')},$true)
Add-Type -TypeDefinition $source.Value
$settings=Get-Content -Raw -LiteralPath $SettingsFile | ConvertFrom-Json
if($settings.layout -ne 'github-assets' -or ([uri]$settings.url).Scheme -ne 'https'){throw 'Expected public HTTPS GitHub channel'}
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12
$client=New-Object SimulatorUpdateClient
$client.Headers.Add('User-Agent','SelfDrivingSimulator/1')
try {
 $base=[uri]$settings.url
 $bytes=$client.DownloadData([uri]::new($base,'manifest.json'))
 Assert-UpdateSignature $bytes ($client.DownloadString([uri]::new($base,'manifest.sig'))) $settings.public_key
 $manifest=[Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json
 Assert-UpdateManifest $manifest 0
 # 원본 번들은 그대로 두고 독립 임시 설치를 이전 세대로 구성한다.
 $fixture=Join-Path ([IO.Path]::GetTempPath()) ('sim-live-update-'+[guid]::NewGuid().ToString('N'))
 New-Item -ItemType Directory -Path $fixture | Out-Null
 Get-ChildItem -LiteralPath $Bundle -Force | ForEach-Object {Copy-Item -LiteralPath $_.FullName -Destination $fixture -Recurse}
 $markerPath=Join-Path $fixture '.simulator-release.json'
 $marker=Get-Content -Raw -LiteralPath $markerPath | ConvertFrom-Json
 $marker.release_version=0
 [IO.File]::WriteAllText($markerPath,($marker | ConvertTo-Json))
 $small=@($manifest.files | Where-Object {$_.size -gt 0 -and $_.size -lt 4096 -and $_.path -notmatch '^config/|^\.simulator-release.json$'})[0]
 if(-not $small){throw 'No small asset for download test'}
 Assert-UpdatePath $small.path
 $missing=Join-Path $fixture $small.path
 if(Test-Path -LiteralPath $missing){Remove-Item -LiteralPath $missing}
 New-Item -ItemType Directory -Path (Join-Path $fixture 'cache/routes') -Force | Out-Null
 [IO.File]::WriteAllText((Join-Path $fixture 'cache/routes/live-update-preserved.txt'),'preserve-route')
 [IO.File]::WriteAllText((Join-Path $fixture 'config/live-update-preserved.txt'),'preserve-setting')
 $script:downloads=0
 $updated=Install-UpdateGeneration $fixture (Get-UpdateCurrent $fixture) $manifest {
    param($path,$destination,$hash)
    $script:downloads++
    $client.DownloadFile([uri]::new($base,($hash+'.blob')),$destination)
 }
 if($updated.version -ne $manifest.version -or $script:downloads -lt 1 -or $script:downloads -ge $manifest.files.Count){throw 'Live install/reuse failed'}
 foreach($item in @(@('cache/routes/live-update-preserved.txt','preserve-route'),@('config/live-update-preserved.txt','preserve-setting'))){
    if([IO.File]::ReadAllText((Join-Path $updated.path $item[0])) -ne $item[1]){throw 'User data preservation failed'}
 }
 Write-Output "LIVE RELEASE OK: version=$($updated.version), anonymous HTTPS/signature verified, downloads=$script:downloads, files=$($manifest.files.Count), actual Godot smoke passed, user data preserved; fixture=$fixture"
} finally {$client.Dispose()}
