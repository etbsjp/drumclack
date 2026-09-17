<#
.SYNOPSIS
    指定したプロセスの CPU 使用率・メモリ使用量を1秒ごとに CSV へ記録する（Windows 用）。

.DESCRIPTION
    -Target に PID（数値）またはプロセス名（拡張子 .exe を除いた名前。Get-Process -Name と同じ形式）
    を指定すると、そのプロセスが終了するまで -IntervalSeconds 間隔で CPU 使用率(%)とメモリ使用量(MB)
    を CSV に1行ずつ追記する。

    CPU 使用率は Get-Process の CPU プロパティ（プロセス開始からの累積使用時間・秒）の差分から、
    区間ごとの使用率を計算する（(区間内のCPU時間の増分) / (区間の経過時間) / 論理コア数 * 100）。
    これにより、Windows のタスクマネージャーの「CPU」列と同様の、コア数で正規化された値になる。

.PARAMETER Target
    PID（数値）またはプロセス名。

.PARAMETER OutputCsv
    出力する CSV ファイルのパス。

.PARAMETER IntervalSeconds
    記録間隔（秒）。デフォルトは 1。

.EXAMPLE
    .\record_windows.ps1 -Target 12345 -OutputCsv cpu_mem.csv

.EXAMPLE
    .\record_windows.ps1 -Target "drumclack" -OutputCsv cpu_mem.csv -IntervalSeconds 1
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Target,

    [Parameter(Mandatory = $true)]
    [string]$OutputCsv,

    [Parameter(Mandatory = $false)]
    [double]$IntervalSeconds = 1
)

$ErrorActionPreference = "Stop"

# Target が数値なら PID として、そうでなければプロセス名として解決する。
function Resolve-TargetProcess {
    param([string]$Target)

    $pidValue = 0
    if ([int]::TryParse($Target, [ref]$pidValue)) {
        try {
            return Get-Process -Id $pidValue -ErrorAction Stop
        } catch {
            throw "PID $pidValue のプロセスが見つかりませんでした。"
        }
    } else {
        $procs = Get-Process -Name $Target -ErrorAction SilentlyContinue
        if (-not $procs) {
            throw "プロセス名 '$Target' に一致するプロセスが見つかりませんでした。"
        }
        # 同名プロセスが複数ある場合は最初の1つを対象にする。
        return $procs | Select-Object -First 1
    }
}

$proc = Resolve-TargetProcess -Target $Target
$targetPid = $proc.Id
$processorCount = [Environment]::ProcessorCount

Write-Host "PID $targetPid ($($proc.ProcessName)) の CPU・メモリ使用量を $IntervalSeconds 秒間隔で '$OutputCsv' に記録します。"
Write-Host "停止するには Ctrl+C を押してください。"

# CSV ヘッダーを書き込む（新規ファイルの場合のみ）。
if (-not (Test-Path -Path $OutputCsv)) {
    "timestamp,pid,cpu_percent,mem_working_set_mb,mem_working_set_bytes" | Out-File -FilePath $OutputCsv -Encoding utf8
}

$previousCpuSeconds = $proc.CPU
$previousTime = Get-Date

while ($true) {
    Start-Sleep -Seconds $IntervalSeconds

    try {
        $current = Get-Process -Id $targetPid -ErrorAction Stop
    } catch {
        Write-Host "PID $targetPid のプロセスが終了したため記録を終了しました。出力: $OutputCsv"
        break
    }

    $now = Get-Date
    $elapsedSeconds = ($now - $previousTime).TotalSeconds
    $currentCpuSeconds = $current.CPU
    $cpuDeltaSeconds = $currentCpuSeconds - $previousCpuSeconds

    $cpuPercent = 0
    if ($elapsedSeconds -gt 0) {
        $cpuPercent = [Math]::Round((($cpuDeltaSeconds / $elapsedSeconds) / $processorCount) * 100, 2)
    }

    $memBytes = $current.WorkingSet64
    $memMb = [Math]::Round($memBytes / 1MB, 2)
    $timestamp = $now.ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")

    "$timestamp,$targetPid,$cpuPercent,$memMb,$memBytes" | Out-File -FilePath $OutputCsv -Encoding utf8 -Append

    $previousCpuSeconds = $currentCpuSeconds
    $previousTime = $now
}
