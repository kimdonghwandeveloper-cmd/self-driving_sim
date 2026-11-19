# 로컬 게시 설정을 코드 실행·변수 확장 없이 읽는다. 오류에는 값 대신 줄 번호만 표시한다.
function Read-ReleaseEnv([string]$Path) {
    $settings = @{}
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $settings }
    $lineNumber = 0
    foreach ($line in [IO.File]::ReadAllLines($Path)) {
        $lineNumber++
        $trimmed = $line.Trim()
        if ($trimmed -eq '' -or $trimmed.StartsWith('#')) { continue }
        if ($trimmed -notmatch '^([A-Z_][A-Z0-9_]*)\s*=\s*(.*)$') { throw "Invalid release env syntax at line $lineNumber" }
        $name = $Matches[1]; $value = $Matches[2].Trim()
        if ($name -notin @('GITHUB_TOKEN','SIMULATOR_SIGNING_KEY_FILE','SIMULATOR_GITHUB_REPOSITORY')) { throw "Unsupported release env name at line $lineNumber" }
        if ($settings.ContainsKey($name)) { throw "Duplicate release env name at line $lineNumber" }
        if ($value.StartsWith('"') -or $value.StartsWith("'")) {
            if ($value.Length -lt 2 -or $value[$value.Length-1] -ne $value[0]) { throw "Unclosed release env quote at line $lineNumber" }
            $value = $value.Substring(1,$value.Length-2)
        }
        $settings[$name] = $value
    }
    return $settings
}
