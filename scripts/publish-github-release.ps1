param(
    [Parameter(Mandatory=$true)][string]$Bundle,
    [Parameter(Mandatory=$true)][string]$OutputDir,
    [Parameter(Mandatory=$true)][string]$PrivateKeyFile,
    [Parameter(Mandatory=$true)][int]$Version,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{40}$')][string]$Commit,
    [ValidatePattern('^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$')][string]$Repository = 'kimdonghwandeveloper-cmd/self-driving_sim',
    [switch]$Publish,
    [switch]$PrepareOnly
)
$ErrorActionPreference = 'Stop'
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
$tag = "sim-v$Version"
# 업로드가 중단되면 draft를 그대로 남긴다. 모든 자산 업로드 성공 전에는 latest가 바뀌지 않는다.
$body = @{tag_name=$tag;target_commitish=$Commit;name="Simulator v$Version";draft=$true;prerelease=$false;body='Windows simulator. Download Simulator-Windows ZIP, extract the whole folder, and run Simulator.exe.'} | ConvertTo-Json
$release = Invoke-RestMethod -Method Post -Uri "$api/releases" -Headers $headers -ContentType 'application/json' -Body $body
$upload = $release.upload_url -replace '\{.*$', ''
foreach ($file in @(Get-ChildItem -LiteralPath $output -File | Sort-Object Name)) {
    Invoke-RestMethod -Method Post -Uri ($upload + '?name=' + [uri]::EscapeDataString($file.Name)) -Headers $headers -ContentType 'application/octet-stream' -InFile $file.FullName | Out-Null
}
if ($Publish) {
    Invoke-RestMethod -Method Patch -Uri "$api/releases/$($release.id)" -Headers $headers -ContentType 'application/json' -Body '{"draft":false,"make_latest":"true"}' | Out-Null
}
Write-Output "GitHub release created: $($release.html_url); published=$Publish"
