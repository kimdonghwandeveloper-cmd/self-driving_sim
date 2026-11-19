param([string]$Root = (Split-Path $PSScriptRoot -Parent))
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/updater-support.ps1"
$Root = [IO.Path]::GetFullPath($Root)
$lock = $null
try {
    # 실행 전체에 독점 잠금을 유지해 주행 중인 세대의 변경과 중복 실행을 막는다.
    $lock = [IO.File]::Open((Join-Path $Root '.simulator-launch.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
    try { $current = Get-UpdateCurrent $Root } catch { $current = @{version=0; path=$Root} }
    try {
        $settingsPath = Join-Path $Root 'update.json'
        if (Test-Path -LiteralPath $settingsPath) {
            $settings = Get-Content -Raw -LiteralPath $settingsPath | ConvertFrom-Json
            if ($settings.enabled) {
                $base = [uri]$settings.url
                if ($base.Scheme -ne 'https' -or -not $base.AbsoluteUri.EndsWith('/') -or $base.UserInfo -or $base.Query -or $base.Fragment) { throw 'Update URL must be HTTPS directory URL' }
                [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
                # 리다이렉트로 HTTPS를 벗어나지 않으며 응답 지연으로 실행이 무기한 막히지 않게 한다.
                Add-Type -TypeDefinition @'
using System;
using System.Net;
public sealed class SimulatorUpdateClient : WebClient {
    // 업데이트 요청에 연결·읽기 제한과 리다이렉트 금지를 적용한다.
    protected override WebRequest GetWebRequest(Uri address) {
        var request = (HttpWebRequest)base.GetWebRequest(address);
        request.Timeout = 15000;
        request.ReadWriteTimeout = 15000;
        request.AllowAutoRedirect = false;
        return request;
    }
    // GitHub 자산 리다이렉트는 최대 다섯 번, 인증 정보 없는 HTTPS만 따라간다.
    protected override WebResponse GetWebResponse(WebRequest request) {
        for (int hop = 0; hop <= 5; hop++) {
            var response = (HttpWebResponse)request.GetResponse();
            int code = (int)response.StatusCode;
            if (code < 300 || code >= 400) return response;
            string location = response.Headers["Location"];
            Uri next;
            bool valid = Uri.TryCreate(request.RequestUri, location, out next);
            response.Dispose();
            if (!valid || next.Scheme != "https" || next.UserInfo.Length != 0) throw new WebException("Unsafe update redirect");
            request = GetWebRequest(next);
        }
        throw new WebException("Too many update redirects");
    }
}
'@
                $client = New-Object SimulatorUpdateClient
                $client.Headers.Add('User-Agent', 'SelfDrivingSimulator/1')
                try {
                    # 원문 서명 확인 전에는 JSON의 경로·세대·파일 목록을 신뢰하지 않는다.
                    $bytes = $client.DownloadData([uri]::new($base, 'manifest.json'))
                    if ($bytes.Length -gt 16777216) { throw 'Manifest exceeds 16 MiB' }
                    Assert-UpdateSignature $bytes ($client.DownloadString([uri]::new($base, 'manifest.sig'))) $settings.public_key
                    $manifest = [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json
                    if ([long]$manifest.version -ne $current.version) {
                        Assert-UpdateManifest $manifest $current.version
                        $current = Install-UpdateGeneration $Root $current $manifest {
                            param($path, $destination, $hash)
                            $asset = 'files/' + $path
                            if ($settings.layout -eq 'github-assets') { $asset = $hash + '.blob' }
                            $client.DownloadFile([uri]::new($base, $asset), $destination)
                        }
                    }
                } finally { $client.Dispose() }
            }
        }
    } catch {
        # 네트워크·서명·파일·새 실행 검사 실패는 이전 실행 파일을 변경하지 않는다.
        Add-Content -LiteralPath (Join-Path $Root 'update.log') -Value ((Get-Date -Format o) + ' ' + $_.Exception.Message)
    }
    $engine = Join-Path $current.path 'godot.exe'
    if (-not (Test-Path -LiteralPath $engine)) { throw 'Simulator runtime missing' }
    $process = Start-Process -FilePath $engine -ArgumentList @('--path', ('"' + (Join-Path $current.path 'godot') + '"')) -WorkingDirectory $current.path -PassThru
    $process.WaitForExit()
    exit $process.ExitCode
} catch {
    Add-Type -AssemblyName System.Windows.Forms
    [Windows.Forms.MessageBox]::Show($_.Exception.Message, 'Simulator launch failed') | Out-Null
    exit 1
} finally { if ($null -ne $lock) { $lock.Dispose() } }
