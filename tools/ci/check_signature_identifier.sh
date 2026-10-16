#!/usr/bin/env bash
# `codesign -dv` の出力から署名の識別子（Identifier=）を読み、期待する値と一致することを確かめる。
#
# 署名の識別子がバンドルの識別子（jp.etbs.drumclack）と違うと、macOS の許可の台帳
# （入力監視）で食い違って見えるため、配布物では一致させる。
#
# 使い方: check_signature_identifier.sh <codesign -dv の出力を保存したファイル> <期待する識別子>

set -euo pipefail

OUTPUT_FILE="${1:?codesign -dv の出力ファイルを渡してください}"
EXPECTED="${2:?期待する識別子を渡してください}"

# codesign は診断を標準エラーへ出す。呼び出し側が 2>file で保存した前提で読む。
actual="$(grep -m1 '^Identifier=' "$OUTPUT_FILE" | cut -d= -f2- || true)"

if [ -z "$actual" ]; then
  echo "NG: 署名の識別子（Identifier=）が見つかりません（${OUTPUT_FILE}）" >&2
  exit 1
fi

if [ "$actual" != "$EXPECTED" ]; then
  echo "NG: 署名の識別子が期待と違います" >&2
  echo "  期待: $EXPECTED" >&2
  echo "  実際: $actual" >&2
  exit 1
fi
echo "OK: 署名の識別子は ${EXPECTED} です"
