param([Parameter(Mandatory=$true)][string]$OutputPath)
$ErrorActionPreference = 'Stop'
# 콘솔 창 대신 오류 대화상자를 제공하는 Windows EXE를 만든다.
$source = @'
using System;
using System.Diagnostics;
using System.IO;
using System.Windows.Forms;
public static class SimulatorLauncher {
    // 실행 파일 위치를 기준으로 업데이트 런처를 호출하고 종료까지 기다린다.
    [STAThread] public static int Main() {
        try {
            string root = AppDomain.CurrentDomain.BaseDirectory.TrimEnd(Path.DirectorySeparatorChar);
            string script = Path.Combine(root, "updater", "run-simulator.ps1");
            if (!File.Exists(script)) throw new FileNotFoundException("Updater script missing", script);
            var info = new ProcessStartInfo(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), @"WindowsPowerShell\v1.0\powershell.exe"));
            info.Arguments = "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"" + script + "\" -Root \"" + root + "\"";
            info.UseShellExecute = false;
            info.CreateNoWindow = true;
            info.WorkingDirectory = root;
            using (var process = Process.Start(info)) { process.WaitForExit(); return process.ExitCode; }
        } catch (Exception error) { MessageBox.Show(error.Message, "Simulator launch failed"); return 1; }
    }
}
'@
Add-Type -TypeDefinition $source -ReferencedAssemblies System.Windows.Forms -OutputAssembly ([IO.Path]::GetFullPath($OutputPath)) -OutputType WindowsApplication
