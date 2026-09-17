#!/usr/bin/env bash
#
# record_mac.sh - 指定したプロセスの CPU 使用率・メモリ使用量を1秒ごとに CSV へ記録する（macOS 用）。
#
# 使い方:
#   ./record_mac.sh <PID または プロセス名> <出力CSVパス> [記録間隔(秒), デフォルト1]
#
# 例:
#   ./record_mac.sh 12345 cpu_mem.csv
#   ./record_mac.sh "drumclack" cpu_mem.csv 1
#
# 動作:
#   - 第1引数が数値ならその PID を直接使う。数値でなければプロセス名として
#     `pgrep -x` で検索し、最初に見つかった PID を使う。
#   - 対象プロセスが終了する（`kill -0` が失敗する）まで、指定間隔でループして
#     `ps` から %CPU・メモリ(RSS)・%MEM を取得し、CSV に1行ずつ追記する。
#   - Ctrl+C で途中終了した場合も、それまでの記録は CSV に残る。
#
# 注意:
#   - macOS の `ps -o %cpu` は Activity Monitor の「%CPU」と同様、1コア=100% を基準にした値。
#     マルチスレッドのプロセスでは 100% を超えることがある。
#   - 常駐メモリ(RSS)を計測対象としている。仮想メモリ(VSZ)ではない。

set -u

usage() {
    echo "使い方: $0 <PID または プロセス名> <出力CSVパス> [記録間隔(秒), デフォルト1]" >&2
    exit 1
}

if [ "$#" -lt 2 ]; then
    usage
fi

TARGET="$1"
OUTPUT_CSV="$2"
INTERVAL="${3:-1}"

# 第1引数が数値かどうかで PID 指定かプロセス名指定かを判定する。
if [[ "$TARGET" =~ ^[0-9]+$ ]]; then
    PID="$TARGET"
else
    # プロセス名（完全一致）で検索し、最初に見つかった PID を使う。
    PID="$(pgrep -x "$TARGET" | head -n 1)"
    if [ -z "$PID" ]; then
        echo "エラー: プロセス名 '$TARGET' に一致するプロセスが見つかりませんでした。" >&2
        echo "  ヒント: 完全一致で検索しています。部分一致で探す場合は 'pgrep -f $TARGET' で PID を調べて、" >&2
        echo "  そのPIDを直接この引数に指定してください。" >&2
        exit 1
    fi
    echo "プロセス名 '$TARGET' -> PID $PID を記録対象にします。" >&2
fi

# 対象プロセスが実際に存在するか確認する。
if ! kill -0 "$PID" 2>/dev/null; then
    echo "エラー: PID $PID のプロセスが見つかりません。" >&2
    exit 1
fi

# CSV ヘッダーを書き込む（新規ファイルの場合のみ）。
if [ ! -f "$OUTPUT_CSV" ]; then
    echo "timestamp,pid,cpu_percent,mem_rss_kb,mem_rss_mb,mem_percent" > "$OUTPUT_CSV"
fi

echo "PID $PID の CPU・メモリ使用量を ${INTERVAL} 秒間隔で '$OUTPUT_CSV' に記録します。" >&2
echo "停止するには Ctrl+C を押してください。" >&2

while kill -0 "$PID" 2>/dev/null; do
    TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    # %cpu, rss(KB), %mem を1回の ps 呼び出しでまとめて取得する。
    READING="$(ps -o %cpu=,rss=,%mem= -p "$PID" 2>/dev/null)"
    if [ -z "$READING" ]; then
        # ps 実行の間にプロセスが終了した場合はループを抜ける。
        break
    fi
    CPU_PERCENT="$(echo "$READING" | awk '{print $1}')"
    RSS_KB="$(echo "$READING" | awk '{print $2}')"
    MEM_PERCENT="$(echo "$READING" | awk '{print $3}')"
    RSS_MB="$(awk -v kb="$RSS_KB" 'BEGIN { printf "%.2f", kb / 1024 }')"

    echo "${TS},${PID},${CPU_PERCENT},${RSS_KB},${RSS_MB},${MEM_PERCENT}" >> "$OUTPUT_CSV"

    sleep "$INTERVAL"
done

echo "PID $PID のプロセスが終了したため記録を終了しました。出力: $OUTPUT_CSV" >&2
