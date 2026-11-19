param(
    [Parameter(Mandatory=$true)][string]$Bundle,
    [Parameter(Mandatory=$true)][string]$OutputDir,
    [string]$PrivateKeyFile = '',
    [Parameter(Mandatory=$true)][int]$Version,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{40}$')][string]$Commit,
    [ValidatePattern('^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$')][string]$Repository = 'kimdonghwandeveloper-cmd/self-driving_sim',
    [switch]$Publish,
    [switch]$PrepareOnly,
    [switch]$ResumeDraft,
    [string]$EnvFile = ''
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/release-env.ps1"
if (-not $EnvFile) { $EnvFile = Join-Path (Split-Path $PSScriptRoot -Parent) '.env' }
$localSettings = Read-ReleaseEnv $EnvFile
if ([string]::IsNullOrWhiteSpace($env:GITHUB_TOKEN) -and $localSettings.GITHUB_TOKEN) { $env:GITHUB_TOKEN = $localSettings.GITHUB_TOKEN }
if (-not $PrivateKeyFile) { $PrivateKeyFile = $localSettings.SIMULATOR_SIGNING_KEY_FILE }
if (-not $PrivateKeyFile) { throw 'Signing key path is required in .env or -PrivateKeyFile' }
if (-not $PSBoundParameters.ContainsKey('Repository') -and $localSettings.SIMULATOR_GITHUB_REPOSITORY) { $Repository = $localSettings.SIMULATOR_GITHUB_REPOSITORY }
if ($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { throw 'Invalid release repository' }
if (-not $PrepareOnly -and [string]::IsNullOrWhiteSpace($env:GITHUB_TOKEN)) { throw 'GITHUB_TOKEN is required for upload; it is never embedded in the simulator' }
& "$PSScriptRoot/publish-update.ps1" -Bundle $Bundle -OutputDir $OutputDir -PrivateKeyFile $PrivateKeyFile -Version $Version -GitHubAssets
$output = (Resolve-Path -LiteralPath $OutputDir).Path
$bundlePath = (Resolve-Path -LiteralPath $Bundle).Path
$bootstrap = Join-Path $output 'bootstrap'
New-Item -ItemType Directory -Path $bootstrap | Out-Null
# 최초 설치 ZIP에도 같은 공개 키와 채널을 넣되 원본 검증 번들은 수정하지 않는다.
Get-ChildItem -LiteralPath $bundlePath -Force | Where-Object { $_.Name -notin @('versions','cache','logs','current.json','previous.json','.simulator-launch.lock') } | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $bootstrap -Recurse
}
$rsa = New-Object Security.Cryptography.RSACryptoServiceProvider
try {
    $rsa.FromXmlString([IO.File]::ReadAllText((Resolve-Path -LiteralPath $PrivateKeyFile)))
    $settings = @{enabled=$true; layout='github-assets'; url="https://github.com/$Repository/releases/latest/download/"; public_key=$rsa.ToXmlString($false)}
    [IO.File]::WriteAllText((Join-Path $bootstrap 'update.json'), ($settings | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
} finally { $rsa.Dispose() }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = Join-Path $output "Simulator-Windows-v$Version.zip"
[IO.Compression.ZipFile]::CreateFromDirectory($bootstrap, $zip)
if ((Get-Item -LiteralPath $zip).Length -ge 2147483648) { throw 'Bootstrap ZIP exceeds GitHub 2 GiB limit' }
if ($PrepareOnly) { Write-Output "GitHub upload prepared, nothing published: $output"; return }
$headers = @{Authorization="Bearer $env:GITHUB_TOKEN"; 'User-Agent'='self-driving-sim-publisher'; Accept='application/vnd.github+json'; 'X-GitHub-Api-Version'='2022-11-28'}
$api = "https://api.github.com/repos/$Repository"
$repo = Invoke-RestMethod -Uri $api -Headers $headers
if ($repo.private) { throw 'Public updater requires a public release repository; tokens must not be shipped to users' }
# 아직 올라가지 않은 커밋을 릴리스 태그가 가리키지 않도록 원격 커밋 존재를 확인한다.
try { $null = Invoke-RestMethod -Uri "$api/commits/$Commit" -Headers $headers } catch { throw 'Release commit must already be available on GitHub' }
$tag = "sim-v$Version"
# 업로드가 중단되면 draft를 그대로 남긴다. 모든 자산 업로드 성공 전에는 latest가 바뀌지 않는다.
$body = @{tag_name=$tag;target_commitish=$Commit;name="Simulator v$Version";draft=$true;prerelease=$false;body='Windows simulator. Download Simulator-Windows ZIP, extract the whole folder, and run Simulator.exe.'} | ConvertTo-Json
if ($ResumeDraft) {
    $allReleases = Invoke-RestMethod -Uri "$api/releases?per_page=100" -Headers $headers
    $drafts = @($allReleases | Where-Object { $_.tag_name -eq $tag -and $_.draft })
    if ($drafts.Count -ne 1) { throw 'Exactly one matching draft is required for resume' }
    $release = $drafts[0]
    if (-not $release.draft) { throw 'Only an unpublished draft may be resumed' }
    $release = Invoke-RestMethod -Method Patch -Uri "$api/releases/$($release.id)" -Headers $headers -ContentType 'application/json' -Body $body
} else {
    $release = Invoke-RestMethod -Method Post -Uri "$api/releases" -Headers $headers -ContentType 'application/json' -Body $body
}
$upload = $release.upload_url -replace '\{.*$', ''
$existing = @()
for ($pageNumber=1; $pageNumber -le 10; $pageNumber++) {
    $page = Invoke-RestMethod -Uri "$api/releases/$($release.id)/assets?per_page=100&page=$pageNumber" -Headers $headers
    $existing += @($page)
    if (@($page).Count -lt 100) { break }
}
$done=0
foreach ($file in @(Get-ChildItem -LiteralPath $output -File | Sort-Object Name)) {
    if ($file.Length -eq 0) { throw 'GitHub does not accept empty assets' }
    $old=@($existing | Where-Object { $_.name -eq $file.Name })
    $digest='sha256:'+(Get-FileHash -LiteralPath $file.FullName).Hash.ToLowerInvariant()
    if ($old.Count -eq 1 -and $old[0].digest -eq $digest -and $old[0].size -eq $file.Length -and $old[0].state -eq 'uploaded') { $done++; continue }
    foreach ($asset in $old) { Invoke-RestMethod -Method Delete -Uri "$api/releases/assets/$($asset.id)" -Headers $headers | Out-Null }
    Invoke-RestMethod -Method Post -Uri ($upload + '?name=' + [uri]::EscapeDataString($file.Name)) -Headers $headers -ContentType 'application/octet-stream' -InFile $file.FullName | Out-Null
    $done++
    if ($done % 10 -eq 0) { Write-Output "Release assets completed: $done" }
}
if ($Publish) {
    $release = Invoke-RestMethod -Method Patch -Uri "$api/releases/$($release.id)" -Headers $headers -ContentType 'application/json' -Body '{"draft":false,"make_latest":"true"}'
}
Write-Output "GitHub release created: $($release.html_url); published=$Publish"
