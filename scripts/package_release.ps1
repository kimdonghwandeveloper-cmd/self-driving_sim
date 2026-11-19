# Autonomous Driving Shuttle Simulation - Standalone Release Packager
param(
    [string]$OutputDir = "dist/self-driving-sim-release",
    [string]$GodotPath = "",
    [ValidateRange(0,2147483647)][int]$ReleaseVersion = 0,
    [switch]$SkipBuild = $false,
    [switch]$Validate = $true
)

$ErrorActionPreference = "Stop"
$repoRoot = [System.IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
. (Join-Path $PSScriptRoot 'package_release_support.ps1')

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  Starting Standalone Simulator Packaging Pipeline" -ForegroundColor Cyan
Write-Host "  Repository: $repoRoot" -ForegroundColor Gray
Write-Host "  Output Dir: $OutputDir" -ForegroundColor Gray
Write-Host "==========================================================" -ForegroundColor Cyan

# 빌드·출력 교체 전에 경로와 입력을 검사한다. 검증 생략 시에도 필수 파일 누락은 허용하지 않는다.
$outFull = Get-ReleaseOutputPath $repoRoot $OutputDir
Assert-ReleaseOwnership $outFull $repoRoot
$runtime = Get-ReleaseGodot $GodotPath
Assert-ReleaseFiles $repoRoot @('scripts/build-launcher.ps1', 'scripts/run-simulator.ps1', 'scripts/updater-support.ps1')
$requiredFiles = @(
    'data/korea/manifest.toml', 'data/korea/roads.pack', 'data/korea/places.pack',
    'config/vehicle.toml', 'config/input.toml', 'config/camera.toml', 'config/road_graph.toml',
    'godot/project.godot', 'godot/sim_godot.gdextension',
    'godot/.godot/extension_list.cfg', 'godot/.godot/global_script_class_cache.cfg',
    'godot/tests/smoke_test.gd', 'godot/tests/map_course_test.gd',
    'godot/tests/yeosu_convention_test.gd', 'godot/assets/maps/yeosu-expo/convention-background.glb',
    'testdata/yeosu-expo/convention/traced-roads.json',
    'testdata/yeosu-expo/convention/north-access.toml', 'testdata/yeosu-expo/convention/central-gallery.toml',
    'testdata/yeosu-expo/convention/south-plaza.toml', 'testdata/yeosu-expo/convention/west-link.toml',
    'testdata/yeosu-expo/convention/east-plaza.toml', 'testdata/yeosu-expo/convention/west-crossing.toml',
    'testdata/yeosu-expo/route-junctions.toml', 'testdata/yeosu-expo/graph-2026-11-06.osm.json',
    'godot/tests/osm_route_drive_test.gd', 'godot/tests/scenario_record_test.gd',
    'assets/scenarios/scenario-a-basic.toml', 'assets/scenarios/scenario-b-space.toml',
    'assets/scenarios/scenario-c-hazard.toml', 'testdata/korea-yeosu/manifest.toml'
)
Assert-ReleaseFiles $repoRoot $requiredFiles
if (-not (Test-Path -LiteralPath (Join-Path $repoRoot 'godot/.godot/imported') -PathType Container)) {
    throw 'Godot import cache missing: run Godot --headless --editor --path godot --import first'
}

# 1. Compile release binaries
if (-not $SkipBuild) {
    Write-Host "`n[1/5] Compiling Rust release binaries (sim-godot, sim-tools)..." -ForegroundColor Yellow
    Push-Location $repoRoot
    try {
        & cargo.exe build --release --locked -p sim-godot -p sim-tools
        if ($LASTEXITCODE -ne 0) { throw "Cargo release build failed with exit code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
} else {
    Write-Host "`n[1/5] Skipping build (-SkipBuild)" -ForegroundColor DarkGray
}

$dllPath = Join-Path $repoRoot "target/release/sim_godot.dll"
$toolsPath = Join-Path $repoRoot "target/release/sim-tools.exe"
if (-not (Test-Path -LiteralPath $dllPath)) { throw "sim_godot.dll not found at: $dllPath" }
if (-not (Test-Path -LiteralPath $toolsPath)) { throw "sim-tools.exe not found at: $toolsPath" }
Assert-ReleaseFiles $repoRoot @('target/release/sim_godot.dll', 'target/release/sim-tools.exe')

# 2. Prepare output directory safely
Write-Host "`n[2/5] Preparing distribution directory structure safely..." -ForegroundColor Yellow
$outFull = Get-ReleaseOutputPath $repoRoot $OutputDir
Assert-ReleaseOwnership $outFull $repoRoot
if (Test-Path -LiteralPath $outFull) {
    Write-Host "  - Cleaning existing target folder: $outFull" -ForegroundColor DarkGray
    Remove-Item -LiteralPath $outFull -Recurse -Force
}
New-Item -ItemType Directory -Path $outFull -Force | Out-Null
$packageMarker = @{ format = 'simulator-release-bundle-v1'; repository = $repoRoot; validated = $false; release_version = $ReleaseVersion }
$packageMarker | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outFull '.simulator-release.json') -Encoding UTF8
New-Item -ItemType Directory -Path (Join-Path $outFull "bin") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $outFull "godot") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $outFull "target/release") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $outFull "target/debug") -Force | Out-Null

# 3. Copy artifacts, maps, configs, and assets
Write-Host "`n[3/5] Copying binaries, nationwide maps, configs, and assets..." -ForegroundColor Yellow

# Copy binaries (both debug and release paths for Godot GDExtension compatibility)
Copy-Item -LiteralPath $dllPath -Destination (Join-Path $outFull "bin/sim_godot.dll")
Copy-Item -LiteralPath $dllPath -Destination (Join-Path $outFull "target/release/sim_godot.dll")
Copy-Item -LiteralPath $dllPath -Destination (Join-Path $outFull "target/debug/sim_godot.dll")
Copy-Item -LiteralPath $toolsPath -Destination (Join-Path $outFull "bin/sim-tools.exe")

# Copy config, assets, testdata, data (including nationwide OSM data/korea)
foreach ($folder in @("config", "assets", "testdata", "data")) {
    $src = Join-Path $repoRoot $folder
    if (Test-Path -LiteralPath $src) {
        Write-Host "  - Copying $folder..." -ForegroundColor DarkGray
        Copy-Item -LiteralPath $src -Destination (Join-Path $outFull $folder) -Recurse
    }
}

# Copy godot project including import cache (.godot) so classes and assets load immediately
$godotSrc = Join-Path $repoRoot "godot"
$godotDst = Join-Path $outFull "godot"
Get-ChildItem -LiteralPath $godotSrc -Force | Where-Object { $_.Name -ne "editor" } | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $godotDst $_.Name) -Recurse
}

# 사전 검사한 런타임만 복사하고 번들 자체의 필수 파일을 다시 확인한다.
Copy-Item -LiteralPath $runtime.Gui -Destination (Join-Path $outFull "godot.exe")
Copy-Item -LiteralPath $runtime.Console -Destination (Join-Path $outFull "godot_console.exe")
Assert-ReleaseFiles $outFull ($requiredFiles + @('godot.exe', 'godot_console.exe', 'target/debug/sim_godot.dll', 'target/release/sim_godot.dll', 'bin/sim-tools.exe'))

# 4. Launcher script and documentation
Write-Host "`n[4/5] Creating launcher scripts and distribution notes..." -ForegroundColor Yellow

# 서명 키와 게시 주소는 배포 담당자가 설정한다. 초기 번들은 오프라인 EXE 실행을 지원한다.
New-Item -ItemType Directory -Path (Join-Path $outFull 'updater') | Out-Null
Copy-Item -LiteralPath (Join-Path $repoRoot 'scripts/run-simulator.ps1'), (Join-Path $repoRoot 'scripts/updater-support.ps1') -Destination (Join-Path $outFull 'updater')
& (Join-Path $repoRoot 'scripts/build-launcher.ps1') -OutputPath (Join-Path $outFull 'Simulator.exe')
@{enabled=$false; url=''; public_key=''} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outFull 'update.json') -Encoding UTF8
$batText = "@echo off`r`nchcp 65001 > nul`r`ntitle Self-Driving Simulator`r`necho ==========================================================`r`necho   Self-Driving Shuttle Simulator (Standalone Bundle)`r`necho ==========================================================`r`necho.`r`nif exist `"%~dp0godot.exe`" (`r`n    `"%~dp0godot.exe`" --path `"%~dp0godot`"`r`n) else (`r`n    echo [Info] godot.exe not found in bundle root.`r`n    echo        Place Godot 4.x executable as 'godot.exe' in this folder to launch.`r`n    pause`r`n)"
[System.IO.File]::WriteAllText((Join-Path $outFull "START_SIMULATOR.bat"), $batText, (New-Object System.Text.UTF8Encoding($false)))

$readmeText = "# Self-Driving Shuttle Simulator - Standalone Release Bundle`r`n`r`n### System Requirements`r`n- OS: Windows 10/11 (64-bit)`r`n- Graphics: Vulkan 1.2+ (Forward+) or OpenGL 3.3+ (Compatibility)`r`n- Runtime: Godot 4.7.2+ executable (godot.exe included or placed in root)`r`n`r`n### How to Run`r`n1. Double-click Simulator.exe (START_SIMULATOR.bat remains available; or run `godot.exe --path godot`).`r`n2. Select a course (e.g., Yeosu Expo OSM, Nationwide OSM, or Training) from the main menu.`r`n3. Click '▶ Start Simulation' to begin driving.`r`n`r`n### Vehicle Controls`r`n- W / S (or Up / Down): Throttle / Brake`r`n- A / D (or Left / Right): Steering`r`n- I / K / J / L: Gear P / N / D / R`r`n- Enter: Engine ON / OFF`r`n- Space / Backspace: Emergency Stop / Release`r`n- R: Reset vehicle position`r`n- C: Switch between 1st-person interior and 3rd-person top-down overhead view`r`n- Esc: Return to Main Menu`r`n`r`n### Map Data & Attribution`r`n- Nationwide Road Map: OpenStreetMap (c) OpenStreetMap contributors (ODbL 1.0)"
[System.IO.File]::WriteAllText((Join-Path $outFull "README.txt"), $readmeText, (New-Object System.Text.UTF8Encoding($false)))

# Godot 프로세스를 헤드리스로 실행하고 표준 출력과 에러를 포착하여 성공 여부를 반환하는 헬퍼 함수
function Invoke-GodotRunner {
    param(
        [string]$GodotExe,
        [string[]]$EngineArgs
    )
    $pinfo = New-Object System.Diagnostics.ProcessStartInfo
    $pinfo.FileName = $GodotExe
    $pinfo.Arguments = ($EngineArgs | ForEach-Object { '"' + $_ + '"' }) -join ' '
    $pinfo.WorkingDirectory = $outFull
    $pinfo.UseShellExecute = $false
    $pinfo.RedirectStandardOutput = $true
    $pinfo.RedirectStandardError = $true
    $pinfo.CreateNoWindow = $true
    $proc = [System.Diagnostics.Process]::Start($pinfo)
    $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
    $stderrTask = $proc.StandardError.ReadToEndAsync()
    $finished = $proc.WaitForExit(60000)
    if (-not $finished) {
        $proc.Kill()
        $proc.WaitForExit()
        throw "Godot process timed out after 60s"
    }
    $output = $stdoutTask.GetAwaiter().GetResult() + "`n" + $stderrTask.GetAwaiter().GetResult()
    return @{ ExitCode = $proc.ExitCode; Output = $output }
}

# 5. Integrity validation in isolated bundle
if ($Validate) {
    Push-Location $outFull
    try {
        & ./bin/sim-tools.exe --validate-map ./testdata/yeosu-expo/route-junctions.toml
        if ($LASTEXITCODE -ne 0) { throw "Standalone OSM CLI validation failed" }
        $checks = @(
            @{ Script = 'smoke_test.gd'; Pattern = 'SMOKE OK:' },
            @{ Script = 'map_course_test.gd'; Pattern = 'MAP COURSE OK:' },
            @{ Script = 'yeosu_convention_test.gd'; Pattern = 'CONVENTION ALL SIX OK' },
            @{ Script = 'osm_route_drive_test.gd'; Pattern = 'OSM ROUTE DRIVE OK:' },
            @{ Script = 'scenario_record_test.gd'; Pattern = 'SCENARIO RECORD OK:' }
        )
        foreach ($check in $checks) {
            $engineArgs = @('--headless', '--fixed-fps', '64', '--path', (Join-Path $outFull 'godot'), '--script', ('res://tests/' + $check.Script))
            if ($check.Script -eq 'osm_route_drive_test.gd') { $engineArgs += @('--', '--full-data') }
            $result = Invoke-GodotRunner -GodotExe (Join-Path $outFull 'godot_console.exe') -EngineArgs $engineArgs
            Write-Output $result.Output
            if ($result.ExitCode -ne 0 -or $result.Output -notmatch [regex]::Escape($check.Pattern)) {
                throw "Standalone check failed: $($check.Script)"
            }
        }
    } finally { Pop-Location }
    $packageMarker.validated = $true
    $packageMarker | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outFull '.simulator-release.json') -Encoding UTF8
    Write-Output "Packaging and all $($checks.Count + 1) standalone checks passed: $outFull"
} else {
    Write-Output "Packaging only (execution checks NOT run): $outFull"
}
