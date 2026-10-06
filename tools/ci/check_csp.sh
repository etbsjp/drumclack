#!/usr/bin/env bash
# tauri.conf.json の CSP（画面から外へ通信できないようにする設定）が、決めた値から
# 変わっていないことを確かめる。変える必要があるときは、下の EXPECTED も同じ PR で
# 更新し、なぜ変えるのかをレビューで確認する。
#
# 比べるのは `app.security` 全体。CSP 以外の項目（dangerousRemoteDomainIpcAccess 等）が
# 足されたときも落ちる。

set -euo pipefail

CONFIG="${1:-src-tauri/tauri.conf.json}"

EXPECTED='{"csp":"default-src '"'"'self'"'"'; connect-src ipc: http://ipc.localhost"}'

actual="$(jq -S -c '.app.security' "$CONFIG")"
expected="$(jq -S -c . <<<"$EXPECTED")"

if [ "$actual" != "$expected" ]; then
  echo "NG: tauri.conf.json の app.security が決めた値と違います" >&2
  echo "  期待: $expected" >&2
  echo "  実際: $actual" >&2
  exit 1
fi
echo "OK: tauri.conf.json の CSP は決めた値のままです"
