# resources

指定したプロセスの CPU 使用率・メモリ使用量を1秒ごとに CSV へ記録するスクリプトです。
Mac 用（`record_mac.sh`）と Windows 用（`record_windows.ps1`）を用意しています。

対象プロセスは PID またはプロセス名で指定できます。プロセスが終了すると自動的に記録を終了します
（Ctrl+C で途中終了した場合も、それまでの記録は CSV に残ります）。

## Mac: `record_mac.sh`

```sh
chmod +x record_mac.sh
./record_mac.sh <PID または プロセス名> <出力CSVパス> [記録間隔(秒), デフォルト1]
```

例:

```sh
# PID を指定する場合
./record_mac.sh 12345 cpu_mem.csv

# プロセス名（完全一致）を指定する場合
./record_mac.sh "drumclack" cpu_mem.csv

# 記録間隔を変える場合（例: 0.5秒間隔）
./record_mac.sh drumclack cpu_mem.csv 0.5
```

出力される CSV の列:

| 列名 | 内容 |
|---|---|
| `timestamp` | 記録時刻（UTC, ISO8601） |
| `pid` | 対象プロセスの PID |
| `cpu_percent` | CPU 使用率（`ps -o %cpu` の値。1コア=100%基準。Activity Monitor の「% CPU」列と同様） |
| `mem_rss_kb` | 常駐メモリ（RSS）使用量（KB） |
| `mem_rss_mb` | 常駐メモリ（RSS）使用量（MB） |
| `mem_percent` | システム全体に対するメモリ使用率（`ps -o %mem` の値） |

プロセス名で複数のプロセスが一致する場合は、最初に見つかったものだけを対象にします。
特定のプロセスを狙い撃ちしたい場合は、`ps aux | grep <名前>` 等で PID を確認し、PID を直接指定してください。

## Windows: `record_windows.ps1`

PowerShell（Windows PowerShell 5.1 以降、または PowerShell 7 系）で実行してください。

```powershell
# 実行ポリシーの都合で直接実行できない場合は、次のように呼び出してください。
powershell -ExecutionPolicy Bypass -File .\record_windows.ps1 -Target 12345 -OutputCsv cpu_mem.csv
```

```powershell
# PID を指定する場合
.\record_windows.ps1 -Target 12345 -OutputCsv cpu_mem.csv

# プロセス名を指定する場合（Get-Process -Name と同じ形式。.exe 拡張子は付けない）
.\record_windows.ps1 -Target "drumclack" -OutputCsv cpu_mem.csv

# 記録間隔を変える場合（例: 0.5秒間隔）
.\record_windows.ps1 -Target drumclack -OutputCsv cpu_mem.csv -IntervalSeconds 0.5
```

出力される CSV の列:

| 列名 | 内容 |
|---|---|
| `timestamp` | 記録時刻（UTC, ISO8601） |
| `pid` | 対象プロセスの PID |
| `cpu_percent` | CPU 使用率（%）。`Get-Process` の累積 CPU 時間の増分を区間の経過時間・論理コア数で正規化した値。Windows タスクマネージャーの「CPU」列と同様の基準。 |
| `mem_working_set_mb` | ワーキングセット（実メモリ使用量）（MB） |
| `mem_working_set_bytes` | ワーキングセット（実メモリ使用量）（バイト） |

## 記録した CSV の使い方

「待機時（アイドル時）の CPU・メモリ」を測る場合は、アプリを起動して何も操作しない状態で
一定時間（例: 30秒〜1分程度）記録し、CSV の `cpu_percent` がほぼ 0 に近い値で推移しているか、
`mem_rss_mb` / `mem_working_set_mb` が 50MB 以下に収まっているかを確認してください。

## 注意

- 記録用の CSV や、計測に使った録音ファイルはリポジトリにコミットしないでください。
