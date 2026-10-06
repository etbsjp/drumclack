#!/usr/bin/env bash
# 検査スクリプト（check_csp.sh / check_no_network_crates.sh）の自己テスト。
# 「正しいものは通り、悪いものは exit 1 で落ちる」ことを確かめる。
# 検査が緩んで何でも通すようになっても、CI が気づけるようにするためのもの。

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failed=0

# 引数のコマンドが失敗（exit 1）することを期待する。
expect_fail() {
  local label="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    echo "NG: 落ちるはずの入力が通りました: $label" >&2
    failed=1
  else
    echo "OK: 期待どおり落ちました: $label"
  fi
}

# 引数のコマンドが成功することを期待する。
expect_pass() {
  local label="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    echo "OK: 期待どおり通りました: $label"
  else
    echo "NG: 通るはずの入力が落ちました: $label" >&2
    failed=1
  fi
}

# --- CSP ---
CONF="$ROOT/src-tauri/tauri.conf.json"
jq '.app.security.csp = "default-src *; connect-src *"' "$CONF" >"$WORK/csp_wide.json"
jq 'del(.app.security.csp)' "$CONF" >"$WORK/csp_missing.json"
jq '.app.security.dangerousDisableAssetCspModification = true' "$CONF" >"$WORK/csp_extra_key.json"

expect_pass "CSP: 現在の設定" bash "$HERE/check_csp.sh" "$CONF"
expect_fail "CSP: 緩めた値" bash "$HERE/check_csp.sh" "$WORK/csp_wide.json"
expect_fail "CSP: csp が無い" bash "$HERE/check_csp.sh" "$WORK/csp_missing.json"
expect_fail "CSP: 項目が足された" bash "$HERE/check_csp.sh" "$WORK/csp_extra_key.json"

# --- 通信系クレート ---
mkdir -p "$WORK/dummy/src"
touch "$WORK/dummy/src/lib.rs"
cat >"$WORK/dummy/Cargo.toml" <<'EOF'
[package]
name = "dummy"
version = "0.0.0"
edition = "2021"

[dependencies]
reqwest = "0.12"
EOF
mkdir -p "$WORK/clean/src"
touch "$WORK/clean/src/lib.rs"
cat >"$WORK/clean/Cargo.toml" <<'EOF'
[package]
name = "clean"
version = "0.0.0"
edition = "2021"
EOF
printf '[[package]]\nname = "clean"\nversion = "0.0.0"\n' >"$WORK/clean.lock"
printf '[[package]]\nname = "tauri-plugin-updater"\nversion = "2.0.0"\n' >"$WORK/updater.lock"

expect_pass "依存: 通信系なし" bash "$HERE/check_no_network_crates.sh" "$WORK/clean/Cargo.toml" "$WORK/clean.lock"
expect_fail "依存: reqwest を直接依存に持つ" bash "$HERE/check_no_network_crates.sh" "$WORK/dummy/Cargo.toml" "$WORK/clean.lock"
expect_fail "依存: lock に自動更新プラグイン" bash "$HERE/check_no_network_crates.sh" "$WORK/clean/Cargo.toml" "$WORK/updater.lock"

exit "$failed"
