# Autonomous Driving Shuttle Simulation - Standalone Release Packager
param(
    [string]$OutputDir = "dist/self-driving-sim-release",
    [string]$GodotPath = "",
    [switch]$SkipBuild = $false,
    [switch]$Validate = $true
)

$ErrorActionPreference = "Stop"
$repoRoot = [System.IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  Starting Standalone Simulator Packaging Pipeline" -ForegroundColor Cyan
Write-Host "  Repository: $repoRoot" -ForegroundColor Gray
Write-Host "  Output Dir: $OutputDir" -ForegroundColor Gray
Write-Host "==========================================================" -ForegroundColor Cyan

# 0. Safety validation for OutputDir
$outFull = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDir))
$driveRoot = [System.IO.Path]::GetPathRoot($outFull)

# Refuse dangerous / root / system paths
if ([string]::IsNullOrWhiteSpace($outFull) -or $outFull -eq $driveRoot -or $outFull -eq $repoRoot) {
    throw "Safety Error: OutputDir cannot be drive root, empty, or repo root ($outFull)"
}
$userProfile = $env:USERPROFILE
if ($outFull -eq $userProfile -or $outFull -eq (Join-Path $env:SystemRoot "") -or $outFull -eq $env:ProgramFiles -or $outFull -eq ${env:ProgramFiles(x86)}) {
    throw "Safety Error: OutputDir cannot be system or user profile root ($outFull)"
}
# Ensure output directory is not an ancestor of repo root
if ($repoRoot.StartsWith($outFull, [System.StringComparison]::OrdinalIgnoreCase) -and $repoRoot -ne $outFull) {
    throw "Safety Error: OutputDir cannot be an ancestor directory of repo root ($outFull)"
}

# 1. Compile release binaries
if (-not $SkipBuild) {
    Write-Host "`n[1/5] Compiling Rust release binaries (sim-godot, sim-tools)..." -ForegroundColor Yellow
    Push-Location $repoRoot
    try {
        & cargo build --release -p sim-godot -p sim-tools
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

# 2. Prepare output directory safely
Write-Host "`n[2/5] Preparing distribution directory structure safely..." -ForegroundColor Yellow
if (Test-Path -LiteralPath $outFull) {
    Write-Host "  - Cleaning existing target folder: $outFull" -ForegroundColor DarkGray
    Remove-Item -LiteralPath $outFull -Recurse -Force
}
New-Item -ItemType Directory -Path $outFull -Force | Out-Null
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

# Locate Godot executable if not provided
$godotGuiSrc = ""
$godotConsoleSrc = ""

if (-not [string]::IsNullOrWhiteSpace($GodotPath) -and (Test-Path -LiteralPath $GodotPath)) {
    if ($GodotPath.EndsWith("_console.exe", [System.StringComparison]::OrdinalIgnoreCase)) {
        $godotConsoleSrc = $GodotPath
        $potentialGui = $GodotPath.Substring(0, $GodotPath.Length - 12) + ".exe"
        if (Test-Path -LiteralPath $potentialGui) { $godotGuiSrc = $potentialGui }
    } else {
        $godotGuiSrc = $GodotPath
        $potentialConsole = $GodotPath.Substring(0, $GodotPath.Length - 4) + "_console.exe"
        if (Test-Path -LiteralPath $potentialConsole) { $godotConsoleSrc = $potentialConsole }
    }
} else {
    $searchDirs = @(
        (Join-Path $env:USERPROFILE "Downloads\Godot_v4.7.2-stable_win64.exe"),
        "C:\Program Files\Godot"
    )
    foreach ($dir in $searchDirs) {
        $gui = Join-Path $dir "Godot_v4.7.2-stable_win64.exe"
        $console = Join-Path $dir "Godot_v4.7.2-stable_win64_console.exe"
        if ((Test-Path -LiteralPath $gui) -and (Test-Path -LiteralPath $console)) {
            $godotGuiSrc = $gui
            $godotConsoleSrc = $console
            break
        }
    }
}

if (-not [string]::IsNullOrWhiteSpace($godotGuiSrc) -and (Test-Path -LiteralPath $godotGuiSrc)) {
    Write-Host "  - Packaging Godot GUI runtime: $godotGuiSrc" -ForegroundColor DarkGray
    Copy-Item -LiteralPath $godotGuiSrc -Destination (Join-Path $outFull "godot.exe")
}
if (-not [string]::IsNullOrWhiteSpace($godotConsoleSrc) -and (Test-Path -LiteralPath $godotConsoleSrc)) {
    Write-Host "  - Packaging Godot Console runtime: $godotConsoleSrc" -ForegroundColor DarkGray
    Copy-Item -LiteralPath $godotConsoleSrc -Destination (Join-Path $outFull "godot_console.exe")
}

# 4. Launcher script and documentation
Write-Host "`n[4/5] Creating launcher scripts and distribution notes..." -ForegroundColor Yellow
$batText = "@echo off`r`nchcp 65001 > nul`r`ntitle Self-Driving Simulator`r`necho ==========================================================`r`necho   Self-Driving Shuttle Simulator (Standalone Bundle)`r`necho ==========================================================`r`necho.`r`nif exist `"%~dp0godot.exe`" (`r`n    `"%~dp0godot.exe`" --path `"%~dp0godot`"`r`n) else (`r`n    echo [Info] godot.exe not found in bundle root.`r`n    echo        Place Godot 4.x executable as 'godot.exe' in this folder to launch.`r`n    pause`r`n)"
[System.IO.File]::WriteAllText((Join-Path $outFull "START_SIMULATOR.bat"), $batText, [System.Text.Encoding]::UTF8)

$readmeText = "# Self-Driving Shuttle Simulator - Standalone Release Bundle`r`n`r`n### System Requirements`r`n- OS: Windows 10/11 (64-bit)`r`n- Graphics: Vulkan 1.2+ (Forward+) or OpenGL 3.3+ (Compatibility)`r`n- Runtime: Godot 4.7.2+ executable (godot.exe included or placed in root)`r`n`r`n### How to Run`r`n1. Double-click START_SIMULATOR.bat (or run `godot.exe --path godot`).`r`n2. Select a course (e.g., Yeosu Expo Lanelet2 demo, Nationwide OSM, or Synthetic straight) from the main menu.`r`n3. Click '▶ Start Simulation' to begin driving.`r`n`r`n### Vehicle Controls`r`n- W / S (or Up / Down): Throttle / Brake`r`n- A / D (or Left / Right): Steering`r`n- I / K / J / L: Gear P / N / D / R`r`n- Enter: Engine ON / OFF`r`n- Space / Backspace: Emergency Stop / Release`r`n- R: Reset vehicle position`r`n- C: Switch between 1st-person interior and 3rd-person top-down overhead view`r`n- Esc: Return to Main Menu`r`n`r`n### Map Data & Attribution`r`n- Nationwide Road Map: OpenStreetMap (c) OpenStreetMap contributors (ODbL 1.0)`r`n- Lanelet2 HD Map Demo: Yeosu Expo area sample data"
[System.IO.File]::WriteAllText((Join-Path $outFull "README.txt"), $readmeText, [System.Text.Encoding]::UTF8)

# Godot 프로세스를 헤드리스로 실행하고 표준 출력과 에러를 포착하여 성공 여부를 반환하는 헬퍼 함수
function Invoke-GodotRunner {
    param(
        [string]$GodotExe,
        [string[]]$EngineArgs
    )
    $pinfo = New-Object System.Diagnostics.ProcessStartInfo
    $pinfo.FileName = $GodotExe
    $pinfo.Arguments = ($EngineArgs | ForEach-Object { '"' + $_ + '"' }) -join ' '
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
    Write-Host "`n[5/5] Validating standalone bundle in isolated directory..." -ForegroundColor Yellow
    
    $hash = (Get-FileHash -LiteralPath (Join-Path $outFull "bin/sim_godot.dll") -Algorithm SHA256).Hash
    Write-Host "  - sim_godot.dll SHA-256: $hash" -ForegroundColor Green
    
    # 5.1 Offline CLI validation
    $toolsExe = Join-Path $outFull "bin/sim-tools.exe"
    $demoCourse = Join-Path $outFull "assets/maps/lanelet2-real-demo/course.toml"
    if (Test-Path -LiteralPath $demoCourse) {
        Write-Host "  - [Check 1] Offline Lanelet2 course validation:" -ForegroundColor Gray
        & $toolsExe --validate-lanelet2 $demoCourse
        if ($LASTEXITCODE -ne 0) { throw "Standalone Lanelet2 validation failed" }
    }

    # 5.2 Standalone Godot execution validation
    $targetGodot = Join-Path $outFull "godot_console.exe"
    if (-not (Test-Path -LiteralPath $targetGodot)) {
        $targetGodot = Join-Path $outFull "godot.exe"
    }
    if (-not (Test-Path -LiteralPath $targetGodot) -and -not [string]::IsNullOrWhiteSpace($godotConsoleSrc)) {
        $targetGodot = $godotConsoleSrc
    }
    
    if (Test-Path -LiteralPath $targetGodot) {
        Write-Host "  - [Check 2] Standalone Godot GDExtension Class Registration (smoke_test):" -ForegroundColor Gray
        $godotPathArg = Join-Path $outFull "godot"
        $smokeRes = Invoke-GodotRunner -GodotExe $targetGodot -EngineArgs @("--headless", "--path", $godotPathArg, "--script", "res://tests/smoke_test.gd")
        if ($smokeRes.ExitCode -ne 0 -or $smokeRes.Output -notmatch "SMOKE OK:") {
            Write-Host $smokeRes.Output -ForegroundColor Red
            throw "Standalone Godot smoke test failed: SimBridge / GDExtension class not registered"
        }
        Write-Host "    -> SimBridge & GDExtension classes loaded successfully" -ForegroundColor Green

        Write-Host "  - [Check 3] Standalone Lanelet2 course test:" -ForegroundColor Gray
        $laneletRes = Invoke-GodotRunner -GodotExe $targetGodot -EngineArgs @("--headless", "--fixed-fps", "64", "--path", $godotPathArg, "--script", "res://tests/lanelet2_course_test.gd")
        if ($laneletRes.ExitCode -ne 0 -or $laneletRes.Output -notmatch "LANELET2 OK") {
            Write-Host $laneletRes.Output -ForegroundColor Red
            throw "Standalone Lanelet2 course test failed in isolated bundle"
        }
        Write-Host "    -> Lanelet2 course loaded and driven successfully in standalone bundle" -ForegroundColor Green

        # 5.3 Nationwide OSM data validation
        $koreaPack = Join-Path $outFull "data/korea/roads.pack"
        if (Test-Path -LiteralPath $koreaPack) {
            Write-Host "  - [Check 4] Standalone Nationwide OSM Route & Driving test (--full-data):" -ForegroundColor Gray
            $osmRes = Invoke-GodotRunner -GodotExe $targetGodot -EngineArgs @("--headless", "--fixed-fps", "64", "--path", $godotPathArg, "--script", "res://tests/osm_route_drive_test.gd", "--", "--full-data")
            if ($osmRes.ExitCode -ne 0 -or $osmRes.Output -notmatch "OSM ROUTE DRIVE OK") {
                Write-Host $osmRes.Output -ForegroundColor Red
                throw "Standalone Nationwide OSM route test failed in isolated bundle"
            }
            Write-Host "    -> Nationwide OSM data loaded and driven successfully in standalone bundle" -ForegroundColor Green
        }
        
        Write-Host "  - [Check 5] Standalone Scenario & JSON result test:" -ForegroundColor Gray
        $scenRes = Invoke-GodotRunner -GodotExe $targetGodot -EngineArgs @("--headless", "--fixed-fps", "64", "--path", $godotPathArg, "--script", "res://tests/scenario_record_test.gd")
        if ($scenRes.ExitCode -ne 0 -or $scenRes.Output -notmatch "SCENARIO RECORD OK") {
            Write-Host $scenRes.Output -ForegroundColor Red
            throw "Standalone scenario recording test failed in isolated bundle"
        }
        Write-Host "    -> Scenarios A/B/C and JSON result logs verified in standalone bundle" -ForegroundColor Green
    } else {
        Write-Host "  - [Notice] Godot executable not found for automated runtime validation. Skipped checks 2-5." -ForegroundColor Yellow
    }
}

Write-Host "`n==========================================================" -ForegroundColor Cyan
Write-Host "  Packaging and standalone validation complete: $outFull" -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Cyan
