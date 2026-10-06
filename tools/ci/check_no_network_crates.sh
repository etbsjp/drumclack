#!/usr/bin/env bash
# 通信系・自動更新系のクレートが依存に入っていないことを確かめる。
#
# 1. drumclack が直接宣言している依存（Cargo.toml）に、通信用クレートが無いこと。
# 2. 依存の全体（Cargo.lock）に、自動更新・通信用のクレート／プラグインが無いこと。
#
# 注意: reqwest / hyper / tokio は Tauri 本体が内部で引き込む（cargo tree で tauri 経由）ため、
# 全体の検査（2）の対象には入れず、直接依存の検査（1）だけで見張る。
# 検査の対象を足すときは、下の2つの一覧に名前を足す。

set -euo pipefail

MANIFEST="${1:-src-tauri/Cargo.toml}"
LOCKFILE="${2:-src-tauri/Cargo.lock}"

# 直接依存に入れてはいけないもの（通信を自前で行うクレート）
DENY_DIRECT=(
  reqwest hyper hyper-util ureq isahc surf attohttpc curl h2
  tungstenite tokio-tungstenite websocket native-tls rustls openssl
  self_update
)

# 依存の全体のどこにも入れてはいけないもの（自動更新・通信のプラグインと専用クレート）
DENY_ANYWHERE=(
  tauri-plugin-updater tauri-plugin-http tauri-plugin-websocket tauri-plugin-upload
  tauri-plugin-sentry tauri-plugin-aptabase
  ureq isahc surf attohttpc self_update minisign-verify
)

failed=0

# 直接依存の名前（build / dev も含む）
direct_names="$(cargo metadata --manifest-path "$MANIFEST" --format-version 1 --no-deps \
  | jq -r '.packages[0].dependencies[].name')"

for name in "${DENY_DIRECT[@]}"; do
  if grep -qx -- "$name" <<<"$direct_names"; then
    echo "NG: 直接依存に通信系のクレート '$name' が入っています（${MANIFEST}）" >&2
    failed=1
  fi
done

for name in "${DENY_ANYWHERE[@]}"; do
  if grep -qx -- "name = \"$name\"" "$LOCKFILE"; then
    echo "NG: 依存に通信系・自動更新系のクレート '$name' が入っています（${LOCKFILE}）" >&2
    failed=1
  fi
done

if [ "$failed" -ne 0 ]; then
  exit 1
fi
echo "OK: 通信系・自動更新系のクレートは依存に入っていません"
