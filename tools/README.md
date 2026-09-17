# 計測ツール

drumclack の合否判定（遅延30ms以下・待機時CPUほぼ0%・メモリ50MB以下）に使う計測道具集です。
アプリ本体（`src-tauri/`）とは独立したツールで、それぞれ単独で利用できます。

- [`latency/`](./latency/README.md) — 打鍵音（クリック音）から発音（キック音等）までの遅延を、録音した WAV ファイルから計測する Rust 製 CLI。
- [`resources/`](./resources/README.md) — 指定したプロセスの CPU 使用率・メモリ使用量を1秒ごとに CSV へ記録するスクリプト（Mac / Windows 両対応）。

## スマホの録音（m4a）を WAV に変換する

`latency` ツールは WAV ファイルのみを読み込めます。iPhone 等で録音した `.m4a` ファイルは、
Mac 標準の `afconvert` コマンドで WAV に変換してください。

```sh
afconvert -f WAVE -d LEI16@44100 input.m4a output.wav
```

- `-f WAVE`: 出力ファイル形式を WAV にする
- `-d LEI16@44100`: 出力のサンプル形式をリトルエンディアン16bit整数PCM、サンプルレート44100Hzにする

動作確認済み（このリポジトリの CI 環境ではなく、ローカルの Mac 上で実際に m4a → wav 変換を行い、
`latency` ツールで読み込めることを確認しています）。

## 測定用の録音ファイルについて

計測に使った実際の録音ファイル（WAV / m4a）はリポジトリにコミットしないでください。
`recordings/` は `.gitignore` 済みです。自動テストで使う WAV はすべてプログラムで生成しており、
ファイルとしてリポジトリに含めていません（`tools/latency/src/lib.rs` の単体テスト、
`tools/latency/tests/integration.rs` の結合テストを参照）。
