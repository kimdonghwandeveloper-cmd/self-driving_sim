$ErrorActionPreference='Stop'
. "$PSScriptRoot/release-env.ps1"
$path=Join-Path ([IO.Path]::GetTempPath()) ('sim-env-test-'+[guid]::NewGuid().ToString('N'))
# 테스트 내용은 실제 자격 증명과 무관하며 비밀 값이 오류에 섞이지 않는지 검사한다.
[IO.File]::WriteAllText($path, "# comment`nGITHUB_TOKEN=`"test=value`"`nSIMULATOR_SIGNING_KEY_FILE='C:/path with spaces/key.xml'`nSIMULATOR_GITHUB_REPOSITORY=owner/repo`n")
$settings=Read-ReleaseEnv $path
if($settings.GITHUB_TOKEN -ne 'test=value' -or $settings.SIMULATOR_SIGNING_KEY_FILE -ne 'C:/path with spaces/key.xml'){throw 'Quoted value parsing failed'}
$checks=1
foreach($bad in @("GITHUB_TOKEN=secret-marker`nGITHUB_TOKEN=duplicate",'GITHUB_TOKEN="secret-marker','invalid secret-marker','OTHER=secret-marker')) {
 [IO.File]::WriteAllText($path,$bad)
 $failed=$false
 try{$null=Read-ReleaseEnv $path}catch{$failed=$true;if($_.Exception.Message.Contains('secret-marker')){throw 'Value exposed in diagnostic'}}
 if(-not $failed){throw 'Invalid input accepted'}
 $checks++
}
[IO.File]::WriteAllText($path,'GITHUB_TOKEN=$(throw ''must-not-execute'')')
$settings=Read-ReleaseEnv $path
if($settings.GITHUB_TOKEN -cne '$(throw ''must-not-execute'')'){throw 'Literal value changed'}
$checks++
Remove-Item -LiteralPath $path
if((Read-ReleaseEnv $path).Count -ne 0){throw 'Missing optional env failed'}
$checks++
Write-Output "RELEASE ENV OK: $checks"
