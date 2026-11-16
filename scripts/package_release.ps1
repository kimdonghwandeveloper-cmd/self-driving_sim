# Autonomous Driving Shuttle Simulation - Standalone Release Packager
param(
    [string]$OutputDir = "dist/self-driving-sim-release",
    [switch]$SkipBuild = $false,
    [switch]$Validate = $true
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path $PSScriptRoot -Parent

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  Starting Standalone Simulator Packaging Pipeline" -ForegroundColor Cyan
Write-Host "  Repository: $repoRoot" -ForegroundColor Gray
Write-Host "  Output Dir: $OutputDir" -ForegroundColor Gray
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Compile release binaries
if (-not $SkipBuild) {
    Write-Host "`n[1/4] Compiling Rust release binaries (sim-godot, sim-tools)..." -ForegroundColor Yellow
    Push-Location $repoRoot
    try {
        & cargo build --release -p sim-godot -p sim-tools
        if ($LASTEXITCODE -ne 0) { throw "Cargo release build failed with exit code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
} else {
    Write-Host "`n[1/4] Skipping build (-SkipBuild)" -ForegroundColor DarkGray
}

$dllPath = Join-Path $repoRoot "target/release/sim_godot.dll"
$toolsPath = Join-Path $repoRoot "target/release/sim-tools.exe"
if (-not (Test-Path -LiteralPath $dllPath)) { throw "sim_godot.dll not found at: $dllPath" }
if (-not (Test-Path -LiteralPath $toolsPath)) { throw "sim-tools.exe not found at: $toolsPath" }

# 2. Prepare output directory
Write-Host "`n[2/4] Creating distribution directory structure..." -ForegroundColor Yellow
$outFull = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDir))
if (Test-Path -LiteralPath $outFull) {
    Remove-Item -LiteralPath $outFull -Recurse -Force
}
New-Item -ItemType Directory -Path $outFull -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $outFull "bin") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $outFull "godot") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $outFull "target/release") -Force | Out-Null

# 3. Copy artifacts and assets
Write-Host "`n[3/4] Copying artifacts, configs, and assets..." -ForegroundColor Yellow

# Copy binaries
Copy-Item -LiteralPath $dllPath -Destination (Join-Path $outFull "bin/sim_godot.dll")
Copy-Item -LiteralPath $dllPath -Destination (Join-Path $outFull "target/release/sim_godot.dll")
Copy-Item -LiteralPath $toolsPath -Destination (Join-Path $outFull "bin/sim-tools.exe")

# Copy config, assets, testdata
foreach ($folder in @("config", "assets", "testdata")) {
    $src = Join-Path $repoRoot $folder
    if (Test-Path -LiteralPath $src) {
        Copy-Item -LiteralPath $src -Destination (Join-Path $outFull $folder) -Recurse
    }
}

# Copy godot project
$godotSrc = Join-Path $repoRoot "godot"
$godotDst = Join-Path $outFull "godot"
Get-ChildItem -LiteralPath $godotSrc | Where-Object { $_.Name -ne ".godot" -and $_.Name -ne ".import" } | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $godotDst $_.Name) -Recurse
}

# Launcher bat
$batText = "@echo off`r`nchcp 65001 > nul`r`ntitle Self-Driving Simulator`r`necho ==========================================================`r`necho   Self-Driving Shuttle Simulator (Standalone Bundle)`r`necho ==========================================================`r`necho.`r`necho [Notice] Launching with Godot 4 (godot.exe)...`r`necho   Project path: %~dp0godot\project.godot`r`necho.`r`nif exist `"%~dp0godot.exe`" (`r`n    `"%~dp0godot.exe`" --path `"%~dp0godot`"`r`n) else (`r`n    echo [Info] Place godot.exe in this root folder to run with one click.`r`n    pause`r`n)"
[System.IO.File]::WriteAllText((Join-Path $outFull "START_SIMULATOR.bat"), $batText, [System.Text.Encoding]::UTF8)

# Readme
$readmeText = "# Self-Driving Shuttle Simulator - Standalone Release Bundle`r`n`r`n### Requirements`r`n- OS: Windows 10/11 (64-bit)`r`n- GPU: Vulkan 1.2+ (Forward+) or OpenGL 3.3+ (Compatibility)`r`n- Godot 4.x runtime executable (godot.exe)`r`n`r`n### Controls`r`n- W / S (Up / Down): Throttle / Brake`r`n- A / D (Left / Right): Steering`r`n- I / K / J / L: Gear P / N / D / R`r`n- Enter: Engine ON / OFF`r`n- Space / Backspace: Emergency Stop / Release`r`n- R: Reset vehicle`r`n- C: Switch 1st-person / 3rd-person top-down view`r`n- Esc: Return to menu`r`n`r`n### Attribution`r`n- Map data: OpenStreetMap (c) OpenStreetMap contributors (ODbL)`r`n- HD Map: Lanelet2 format"
[System.IO.File]::WriteAllText((Join-Path $outFull "README.txt"), $readmeText, [System.Text.Encoding]::UTF8)

# 4. Integrity validation
if ($Validate) {
    Write-Host "`n[4/4] Validating standalone bundle integrity..." -ForegroundColor Yellow
    $hash = (Get-FileHash -LiteralPath (Join-Path $outFull "bin/sim_godot.dll") -Algorithm SHA256).Hash
    Write-Host "  - sim_godot.dll SHA-256: $hash" -ForegroundColor Green
    
    $toolsExe = Join-Path $outFull "bin/sim-tools.exe"
    $demoCourse = Join-Path $outFull "assets/maps/lanelet2-real-demo/course.toml"
    if (Test-Path -LiteralPath $demoCourse) {
        Write-Host "  - Running offline Lanelet2 validation in standalone folder:" -ForegroundColor Gray
        & $toolsExe --validate-lanelet2 $demoCourse
        if ($LASTEXITCODE -ne 0) { throw "Standalone validation failed" }
    }
}

Write-Host "`n==========================================================" -ForegroundColor Cyan
Write-Host "  Packaging complete: $outFull" -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Cyan
