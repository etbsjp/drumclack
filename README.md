# drumclack

常駐型タイピングドラムアプリの成立性を確かめるための最小の試作（プロトタイプ）です。
他のアプリで文字を入力しているあいだも、キーを押すたびに合成したバスドラム（キック）音が鳴ります。
画面には状態表示だけがあり、音の割り当て変更などの設定機能はありません。

- Tauri 2 ＋ Rust
- バージョン: 0.1.0（当面上げません）

## できること／できないこと

**できること**

- OS全体のキー入力を検知して、キーを押すたびに合成済みのキック音を鳴らす
- 起動中の状態（稼働状況・権限・音声デバイス・直近の発音・同時発音数）を画面に表示し続ける

**やらないこと（意図的にスコープ外）**

- 音の割り当て変更画面
- 複数の音色
- 署名・インストーラーの作成
- 通信（外部サーバーとの通信は一切行いません）
- 打った文字の保存・表示・ログ出力（扱うのは「押された」という時刻のみです）

## ビルド方法

前提: Rust（stable）と Tauri 2 のビルドに必要なプラットフォームツールチェーンが入っていること。
Node.js や npm は不要です（フロントエンドはビルド不要の静的ファイル `web/` をそのまま使います）。

```sh
# 開発ビルド + 起動
cargo run --manifest-path src-tauri/Cargo.toml

# リリースビルド（未署名バイナリ）
cargo build --release --manifest-path src-tauri/Cargo.toml
```

生成物:

- macOS: `src-tauri/target/release/drumclack`
- Windows: `src-tauri/target/release/drumclack.exe`

インストーラー（.app バンドルや .msi 等）は作成しません。単体の実行バイナリのみです。

## macOS で入力監視の許可を与える手順

macOS では、他アプリ操作中のキー入力を検知するために **入力監視（Input Monitoring）** の許可が必要です。

1. drumclack を一度起動する（この時点では音は鳴りません。OS が候補として認識するだけです）
2. `システム設定` を開く
3. `プライバシーとセキュリティ` → `入力監視` を開く
4. 一覧から `drumclack` を探し、トグルを ON にする
5. drumclack を再起動する

許可されていない間は、アプリの画面上に「入力監視の権限」の項目が表示され、
上と同じ手順が案内文として表示されます。**このアプリ自身がシステム設定を書き換えることはありません**
（許可状態を読み取って表示するだけです）。

## Windows 版の入手方法

Windows 版は GitHub Actions のビルド成果物（アーティファクト）から入手します。ローカルでの署名は行っていない、
未署名の実行ファイルです。

1. このリポジトリの `Actions` タブを開く
2. `Build and test` ワークフローの実行結果一覧から、対象のコミット・タグの実行を開く
3. `Artifacts` から `drumclack-windows` をダウンロードして展開する
4. 展開した `drumclack.exe` を実行する（未署名のため、初回起動時に Windows Defender SmartScreen の警告が出る場合があります）

CI は Pull Request・`main` ブランチへの push・`v` から始まるタグの push のたびに実行されます。
**GitHub Actions の Windows 環境では、実際にキー入力を検知できるかの動作確認はできません**（対話的なログイン
セッションや権限の都合）。CI で確認しているのは「ビルドとユニットテストが通ること」のみです。

## テスト

```sh
cargo test --manifest-path src-tauri/Cargo.toml
```

以下を確認するユニットテストがあります（`src-tauri/src/kick.rs`, `src-tauri/src/voices.rs`, `src-tauri/src/audio.rs`）。

- 合成したキック音が無音でないこと
- 合成したキック音、および複数ボイスを重ねた最終出力が音割れ（振幅が [-1.0, 1.0] を超える）しないこと
- 同時発音数が上限（16）を超えないこと、かつ上限到達時に**既存の発音を打ち切らない**こと

## 測定用の環境変数

`DRUMCLACK_TEST_DELAY_MS` を設定すると、キー押下から発音までの間、指定ミリ秒だけ意図的に遅延させます
（レイテンシ測定の際の陽性対照用）。画面にも「テストモード: 遅延 ◯ms」と表示され、通常状態と区別できます。

```sh
DRUMCLACK_TEST_DELAY_MS=200 cargo run --manifest-path src-tauri/Cargo.toml
```

## 実装メモ

### キー入力の取得方法（`src-tauri/src/keyboard.rs`）

macOSとWindowsで異なる方式を採用しています。

- **macOS**: `CGEventTap` を **listen-only モード**（`kCGEventTapOptionListenOnly`）で、
  外部クレートを介さず直接 FFI で張っています（`src-tauri/src/keyboard.rs` の `macos_tap`
  モジュール）。これは他アプリへのイベント伝播を止めない“傍受のみ”のタップで、必要な権限も
  「アクセシビリティ」ではなく **「入力監視（Input Monitoring）」のみ** で済みます。
  「受信のみ・他アプリの入力を妨げない」という要件にそのまま合致します。

  **`rdev` クレートを使わない理由**: 当初は [`rdev`](https://crates.io/crates/rdev) の `listen()`
  （内部実装は同じく listen-only の `CGEventTap`）を使っていましたが、`rdev` はキーイベントごとに
  `TSMGetInputSourceProperty` など HIToolbox の入力ソース関連 API を内部で呼び出しており、
  macOS 15 以降ではメインスレッド以外からこれらを呼ぶと `dispatch_assert_queue_fail` で
  アプリごと停止する事例が報告されています（[tauri-apps/tauri#7839（discussion）
  “rdev breaks the app on key press”](https://github.com/tauri-apps/tauri/discussions/7839)、
  [dsh-tauri-desk/deepseek-harness-desktop#398](https://github.com/dsh-tauri-desk/deepseek-harness-desktop/pull/398)
  では `rdev` 0.5.3 のこの問題を `CGEventTap` 直接監視への切り替えで回避したと報告されています）。
  `keyboard::spawn_listener` は `thread::spawn` した別スレッドから監視を開始する実装のため、
  このリポジトリでも該当しうる問題でした。押されたキーを文字列化する必要はそもそも無い
  （時刻のみ扱う）ため、キーコード→文字列変換の一切無い、HIToolboxに触れない素の
  `CGEventTap` 直叩きへ切り替えています。
- **Windows**: 引き続き [`rdev`](https://crates.io/crates/rdev) の `listen()` を使っています
  （`src-tauri/src/keyboard.rs` の `rdev_listener` モジュール、`Cargo.toml` では
  macOS以外向けの依存として限定）。内部で `SetWindowsHookEx(WH_KEYBOARD_LL, ...)` による
  **低レベルキーボードフック**を使い、これも受信専用でフック内でイベントを握りつぶす
  （消費してブロックする）ことはありません。上記のmacOS固有の問題はWindowsには当てはまらないため、
  こちらは変更していません。

権限確認（macOS のみ、`src-tauri/src/permission.rs`）には `IOHIDCheckAccess`（IOKit / `IOHIDLib.h`、
macOS 10.15+ で公開されている API）を使っています。これはプロンプトを出さず、現在の許可状態
（許可済み／未許可／不明）だけを読み取る API です。**このアプリは許可状態の確認のみを行い、
システム設定の変更や許可の強制取得は一切行いません。**

押されたキーの種類（文字・キーコード）は一切保持しません。`EventType::KeyPress(_)` のイベント種別
だけを見て「押されたタイミング」として扱い、鳴らす・時刻を記録する、以外のことはしていません。

### 音の合成・再生（`src-tauri/src/kick.rs`, `src-tauri/src/audio.rs`, `src-tauri/src/voices.rs`）

- 起動時（音声デバイス初期化時）に一度だけ、サイン波をベースに **ピッチと音量の両方を指数的に減衰させる**
  ことで808風のキック音を合成し、メモリ上の `Vec<f32>` に保持します。以降は毎回合成し直さず、
  このバッファを使い回します。
- 再生には [`cpal`](https://crates.io/crates/cpal) を使い、デフォルトの出力デバイスへ直接書き込みます。
  バッファサイズ（フレーム数）・サンプルレート・チャンネル数は起動時に標準出力へログ出力します。
- 同時発音（ポリフォニー）は最大 **16 音**。上限に達した状態で新しいキー入力があっても、
  鳴っている途中の音を打ち切らず、単に新しい発音を追加しないだけの挙動にしています
  （`voices::VoicePool::trigger()` が上限到達時に `false` を返す）。

### 画面の状態表示（`web/`）

画面には次の項目のみを表示します（設定操作は一切ありません）。

- 常駐稼働中の表示
- 入力監視の権限状態（macOS のみ。未許可時は「システム設定 > プライバシーとセキュリティ > 入力監視 で
  許可してください」と次の行動を明示。Windows ではこの項目自体を表示しません）
- 音声デバイス初期化の成否（失敗時は原因の要約を表示）
- 直近の発音状況（最終発音時刻、または「待機中」）
- 現在の同時発音数 / 上限16
- `DRUMCLACK_TEST_DELAY_MS` 設定時のみ、テストモード表示

状態の正常・異常は色だけでなく必ずテキストでも表現し、コントラスト比は WCAG AA
（通常テキストで 4.5:1 以上）を満たすよう配色しています（`web/style.css`）。

### ウィンドウを閉じたとき

ウィンドウの「閉じる」操作は、システムトレイへの格納ではなく **プロセスごと終了** します
（`src-tauri/src/main.rs` の `on_window_event` で `CloseRequested` を捕まえて `exit(0)` しています）。
