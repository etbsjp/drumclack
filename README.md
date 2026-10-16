# Drumclack

**English** | [日本語](#日本語)

Drumclack is a small app for macOS and Windows that plays a drum sound every time you press a key, in any app.
Type a letter and you hear a hi-hat, press Space for a kick, Enter for a snare. It sits in the menu bar (Mac) or
the notification area (Windows) and keeps running when you close its window.

- Version 1.0.0. Built with Tauri 2 and Rust.
- The sounds are synthesized 808-style drums built into the app (8 sounds). There are no extra sound packs.
- It never connects to the network, and it never stores or records what you type. See [Privacy](#privacy).

## What it does

- Plays a drum sound for each key press, chosen by the kind of key (letters, Space, Enter, Backspace, symbols, digits, ...).
  Arrow keys and modifier keys are silent by default.
- Lets you change which sound each group of keys plays, or override a single key, in the Assignments screen
  (including "Silent"). You can restore the defaults at any time.
- Has a Play screen with 8 pads and a separate set of assignments for playing the keyboard like a drum pad.
- Settings: on/off, volume, dynamics range (how much the volume follows your typing pace), language (Japanese / English),
  keyboard layout (JIS / US).
- Recovers by itself when the audio output changes (earphones plugged or unplugged, Bluetooth connecting, waking from sleep).
- Optional "Launch at Login" in the menu bar / tray menu (off by default).

It does not record audio, it has no metronome, and it has no sound packs.

## Install

Builds are made by GitHub Actions. Open the repository's **Actions** tab, open the latest successful
**Build and test** run on `main`, and download the artifact for your OS (the file is zipped by GitHub, unzip it first).

| OS | Artifact | Inside |
|---|---|---|
| macOS | `drumclack-macos` | `Drumclack_1.0.0_aarch64.dmg` (contains `Drumclack.app`) |
| Windows | `drumclack-windows` | `Drumclack_1.0.0_*.msi` (installer) |

The macOS build is for **Apple silicon** Macs (M1 or later) only. It does not run on Intel-based Macs.

These builds are **not code-signed by a paid developer account** (macOS: ad-hoc signature only, not notarized;
Windows: unsigned). That keeps Drumclack free, but your OS shows a warning the first time. This is expected.

### macOS

1. Open the `.dmg` and drag `Drumclack.app` into the `Applications` folder.
2. Open Drumclack from `Applications`. macOS says **"Drumclack" Not Opened** (or "cannot be opened because the developer
   cannot be verified"). Do not move it to the Trash. Close the message.
3. Open **System Settings > Privacy & Security**, scroll down to the Security section, and press **Open Anyway**
   next to the message about Drumclack. Enter your password, then press **Open**.
   - On macOS 14 and earlier you can instead right-click (Control-click) `Drumclack.app`, choose **Open**, then **Open** again.
   - You only need to do this once per downloaded copy.
4. The Drumclack window opens. Continue with [Input Monitoring](#macos-allow-input-monitoring).

### Windows

1. Double-click the `.msi`. If Windows shows **"Windows protected your PC"** (Microsoft Defender SmartScreen), press
   **More info**, then **Run anyway**. The "Unknown publisher" prompt that follows is for the same reason; accept it to install.
2. Start Drumclack from the Start menu. The window opens. No permission setup is needed on Windows.

### The first launch and the next launches

- **First launch:** the Drumclack window opens. On a Mac it guides you through the permission (below).
  Press a key; if you hear a drum, you are ready.
- **From the second launch on, no window opens.** After Drumclack has played a sound once, it starts quietly and
  stays in the menu bar (Mac) or notification area (Windows), and keys play drums right away. If you start it and
  "nothing appears", that is normal: just type. (On Windows the icon may sit under the `^` arrow of the notification area.)
- To open the window again, click the icon and choose **Open Settings**, or simply open Drumclack once more
  (from Applications, Spotlight, or the Start menu).
- Closing the window does not quit Drumclack. To quit, use **Quit** in the icon's menu, or on a Mac in the
  Drumclack menu at the top left of the screen.

## macOS: allow Input Monitoring

To hear drums while you type in other apps, macOS must allow Drumclack to receive key presses. This is the
**Input Monitoring** permission (not "Accessibility"). Drumclack only listens; it never blocks or changes your keys, and it
never changes System Settings by itself.

While the permission is missing, the top of the window shows 3 steps:

1. Press **Open Input Monitoring settings**. In System Settings > Input Monitoring, turn on Drumclack.
2. After allowing it, press **Restart**. The change does not apply until you restart.
3. After restarting, press any key. If you hear a drum, you are done.

### After every update, allow it again

The builds are not signed with a paid certificate, so macOS treats **each new version as a different app** and drops the
permission. After you replace Drumclack with a new version:

1. Open System Settings > Privacy & Security > Input Monitoring.
2. If Drumclack is in the list but is still not allowed, switching it off and on does not fix it. Select Drumclack in the list and remove it with the **−** button.
3. Add it again with the **+** button (choose `Drumclack.app` in `Applications`).
4. Go back to the Drumclack window and press **Restart**.

### If the menu bar icon is not visible

On a Mac with a notch, when the menu bar has many icons, the Drumclack icon can end up hidden behind the notch.
macOS does not tell you when this happens. You can still reach everything:

- While the window is closed, open Drumclack once more; the settings window appears.
- While the window is open, use the **Drumclack** menu at the top left of the screen and choose
  **Open Settings** or **Open Play Mode**. **Quit** is in the same menu.

## Troubleshooting

- **No sound on a Mac, and the window says Input Monitoring is not allowed.** Follow the steps in
  [After every update, allow it again](#after-every-update-allow-it-again).
- **Drumclack is in the Input Monitoring list but is still not allowed.** The list can keep an old entry that no longer
  matches. In Terminal, run the command below, then start Drumclack again and redo the permission steps. It only removes
  Drumclack's own entry.

  ```sh
  tccutil reset ListenEvent jp.etbs.drumclack
  ```

- **Nothing happens when I start it.** From the second launch on the window stays closed on purpose; look for the icon
  or just type. Open the app once more to bring up the window.
- **It started and quit again right away on a Mac.** If you quit and start it within a second or two, macOS can fail the
  launch (error -600). Wait 2 to 3 seconds and start it again.
- **The window shows a problem with the audio output.** Check that your speakers or headphones are connected and not used
  exclusively by another app. It recovers by itself when the output is available.

## Privacy

- **Key positions are used only to choose a sound.** Drumclack reads the physical position of the key (such as "the A key"
  or "Enter"), turns it into a sound name in a single place, and plays that sound. It does **not read characters** (no
  text, no input-source or IME information), and it does **not save, log, or send** key positions or the times of
  your key presses.
- A short-lived note in memory is kept only to work properly: which keys are held down right now (so that holding a key
  plays one sound, not many) and when the previous key was pressed (for the dynamics). They are never written anywhere and
  disappear when the app quits.
- **No network.** Drumclack has no code that connects to a server, no analytics, and no automatic updates. The window
  is a local page whose content security policy blocks connections to the outside. Continuous integration checks that no
  networking or auto-update library is added.
- The window's "Details" shows only the time of the last sound and the number of sounds playing at once. The Play screen
  lights a pad with the name of the sound that played. The screen is never given key positions.
- The only file Drumclack writes is its settings file (below). It holds your settings and assignments, not a typing history.
- The console log (visible only if you start the app from a terminal) contains no key positions, times, or typed
  characters. When the audio output is rebuilt it prints only the sample rate and the number of channels, not the name of
  your device (a Bluetooth device's name can contain your own name).
- Input Monitoring on macOS lets the system deliver key events to Drumclack. You can turn it off at any time in
  System Settings.

## Settings file

Drumclack keeps its settings in one JSON file. It is created the first time a setting is saved (the first drum sound you
trigger counts, so a normal first run creates it).

| OS | Location |
|---|---|
| macOS | `~/Library/Application Support/jp.etbs.drumclack/settings.json` |
| Windows | `%APPDATA%\jp.etbs.drumclack\settings.json` |

To reset everything, quit Drumclack and delete that file (or press **Restore defaults** in the Assignments screen for
assignments only). A broken file is set aside as `settings.broken.json` and the app starts with the defaults.

## Default assignments

Typing assignments (used whenever you type in any app):

| Group | Keys | Default sound |
|---|---|---|
| Letters | A to Z | Closed hi-hat |
| Space | Space | Kick |
| Enter | Enter, numpad Enter | Snare |
| Delete | Backspace, Delete | Rimshot |
| Symbols | Punctuation, brackets, numpad `+ - * / .` (and `=` `,`), JIS `¥` `ろ` | Open hi-hat |
| Even digits | 0 2 4 6 8 (top row and numpad) | High tom |
| Odd digits | 1 3 5 7 9 (top row and numpad) | Low tom |
| Control | Tab, Esc, F1 to F24 (and PrintScreen, Pause, Menu key) | Clap |
| Navigation | Arrows, Home / End, PageUp / PageDown (and Insert) | Silent |
| Modifiers | Shift, Ctrl, Alt, Cmd / Win, CapsLock (and NumLock, ScrollLock, Fn), Eisu / Kana, Henkan / Muhenkan | Silent |

Play assignments (used only while the Play screen is open and the Drumclack window is in front; every other key is silent):

| Key | Sound |
|---|---|
| A | Kick |
| S | Snare |
| D | Rimshot |
| F | Clap |
| J | Closed hi-hat |
| K | Open hi-hat |
| L | High tom |
| ; | Low tom |
| Space | Kick |

## Build from source

You need Rust (stable) and the platform toolchain for Tauri 2. Node.js is not needed to build the app
(the screen is plain static files in `web/`).

```sh
# Run a development build
cargo run --manifest-path src-tauri/Cargo.toml

# Make the installable bundles (Tauri CLI 2.x). macOS: .app and .dmg, Windows: .msi
cd src-tauri
npx --yes @tauri-apps/cli@2.11.5 build --bundles app,dmg   # macOS
npx --yes @tauri-apps/cli@2.11.5 build --bundles msi       # Windows
```

The bundles are written under `src-tauri/target/release/bundle/`. The continuous integration workflow
(`.github/workflows/build.yml`) does exactly this and uploads the result.

## Measuring latency

`DRUMCLACK_TEST_DELAY_MS` adds a deliberate delay (in milliseconds) between a key press and the sound. It is a positive
control for latency measurements, and the window shows a "Test mode" line while it is set.

```sh
DRUMCLACK_TEST_DELAY_MS=200 cargo run --manifest-path src-tauri/Cargo.toml
```

To measure the real delay from a recording, see [`tools/latency`](tools/latency/README.md) and [`tools/`](tools/README.md).

---

# 日本語

[English](#drumclack) | **日本語**

Drumclack は、キーを押すたびにドラムの音が鳴る、Mac と Windows 用の小さなアプリです。どのアプリで文字を打っていても、
文字キーはハイハット、スペースはキック、Enter はスネア、というように鳴ります。メニューバー（Mac）／通知領域（Windows）に
常駐し、窓を閉じても動き続けます。

- バージョン 1.0.0。Tauri 2 ＋ Rust で作っています。
- 音は、アプリに内蔵した合成の808風ドラム（8音）です。音源パックはありません。
- 通信はしません。打った内容を保存も記録もしません。詳しくは「プライバシー」を見てください。

## できること

- キーを押すたびに、キーの種類（文字・スペース・Enter・削除・記号・数字など）に応じたドラムの音を鳴らす。
  矢印キー・修飾キーは、既定では鳴りません。
- 割り当ての画面で、グループごと・キーごとに音を変える（「無音」も選べる）。いつでも既定に戻せる。
- 演奏の画面（パッド8個）と、キーボードをパッドのように叩くための演奏用の割り当て。
- 設定: オン／オフ、音量、強弱の幅（打鍵の速さで音量がどれだけ変わるか）、言語（日本語／英語）、キーボードの配列（JIS／US）。
- イヤホンの抜き差し・Bluetooth の接続と切断・スリープ復帰など、音の出口が変わっても自動で鳴り続ける。
- メニューバー／トレイのメニューから「ログイン時に起動」を選べる（既定はオフ）。

録音の機能・メトロノーム・音源パックはありません。

## 入れ方

配布物は GitHub Actions が作ります。このリポジトリの **Actions** タブから、`main` の最新の成功した **Build and test** の
実行を開き、お使いの OS の成果物（アーティファクト）をダウンロードします（GitHub が zip にしているので、展開してください）。

| OS | 成果物 | 中身 |
|---|---|---|
| macOS | `drumclack-macos` | `Drumclack_1.0.0_aarch64.dmg`（`Drumclack.app` が入っています） |
| Windows | `drumclack-windows` | `Drumclack_1.0.0_*.msi`（インストーラ） |

Mac 版は **Apple シリコン**（M1 以降）の Mac 用です。Intel プロセッサの Mac では動きません。

この配布物は、有料の開発者アカウントでの署名をしていません（Mac は署名の識別子だけの署名で、公証もしていません。
Windows は署名なしです）。無料で配るための割り切りで、初回にだけ OS の警告が出ます。これは想定どおりの動きです。

### Mac

1. `.dmg` を開き、`Drumclack.app` を `アプリケーション` フォルダへドラッグします。
2. `アプリケーション` から Drumclack を開きます。**「"Drumclack"は開けません」**（または「開発元を検証できないため開けません」）と出ます。
   「ゴミ箱に入れる」は押さず、メッセージを閉じます。
3. **システム設定 > プライバシーとセキュリティ** を開き、下へスクロールしてセキュリティの欄にある、Drumclack についての
   メッセージの横の **「このまま開く」** を押します。パスワードを入力し、**「開く」** を押します。
   - macOS 14 以前では、`Drumclack.app` を右クリック（control を押しながらクリック）→ **「開く」** → もう一度 **「開く」** でも開けます。
   - ダウンロードしたものごとに、これは1回だけです。
4. Drumclack の窓が開きます。次の「入力監視の許可」へ進みます。

### Windows

1. `.msi` をダブルクリックします。**「Windows によって PC が保護されました」**（Microsoft Defender SmartScreen）と出たら、
   **「詳細情報」** → **「実行」** を押します。続けて出る「発行元不明」の確認も同じ理由なので、許可してインストールします。
2. スタートメニューから Drumclack を起動します。窓が開きます。Windows には許可の手順はありません。

### 初回の起動と、2回目以降の起動

- **初回の起動:** Drumclack の窓が開きます。Mac では、許可の手順が窓に出ます（下の節）。何かキーを押して、ドラムが鳴れば準備完了です。
- **2回目以降の起動では、窓は出ません。** 一度鳴らしたあとは、起動しても窓を出さずに、メニューバー（Mac）／通知領域（Windows）に
  常駐し、そのままキーで鳴ります。「起動したのに何も出ない」と感じても、それが正常です。そのまま打ってみてください
  （Windows では、アイコンが通知領域の `^` の中に入っていることがあります）。
- 窓をもう一度出すには、アイコンをクリックして **「設定を開く」** を選ぶか、Drumclack をもう一度開きます
  （アプリケーション・Spotlight・スタートメニューから）。
- 窓を閉じても終了しません。終了は、アイコンのメニューの **「終了」** から。Mac では、画面左上の Drumclack のメニューの「終了」からも終了できます。

## Mac: 入力監視の許可

他のアプリで文字を打っているあいだもドラムを鳴らすため、macOS に、Drumclack がキーの押下を受け取ることを許可する必要があります。
これは **入力監視（Input Monitoring）** の許可です（「アクセシビリティ」ではありません）。Drumclack は聞くだけで、キーを止めたり
書き換えたりしません。システム設定を自分で書き換えることもありません。

許可されていない間は、窓の上部に3段の手順が出ます。

1. 「入力監視の設定を開く」を押し、システム設定の入力監視で、Drumclack をオンにする。
2. 許可したら、「起動し直す」を押す（押すまで反映されません）。
3. 起動し直したら、何かキーを押す。ドラムが鳴れば完了です。

### 更新したら、そのたびに許可し直す

この配布物は有料の証明書で署名していないため、macOS は **新しい版を別のアプリとして扱い**、許可を外します。
新しい版に入れ替えたあとは、次の手順をやり直してください。

1. システム設定 > プライバシーとセキュリティ > 入力監視 を開く。
2. 更新したあとなど、Drumclack が一覧にあるのに許可されないときは、スイッチのオン・オフでは直りません。一覧から Drumclack を選んで「−」ボタンで消す。
3. 「+」ボタンで追加し直す（`アプリケーション` の `Drumclack.app` を選びます）。
4. Drumclack の窓に戻り、「起動し直す」を押す。

### メニューバーにアイコンが見えないとき

画面上部に切り欠き（ノッチ）のある Mac では、メニューバーのアイコンが多いと、Drumclack のアイコンが切り欠きの裏に
隠れることがあります。macOS は隠れたことを知らせません。そのときも、次の方法で操作できます。

- 窓を閉じているときは、Drumclack をもう一度開くと、設定の窓が出ます。
- 窓が出ているときは、画面左上の **Drumclack** のメニューから「設定を開く」「演奏モードを開く」を選べます。
  「終了」も同じメニューにあります。

## 困ったとき

- **Mac で音が鳴らず、窓に入力監視が許可されていないと出ている。** 上の「更新したら、そのたびに許可し直す」の手順に従ってください。
- **入力監視の一覧に Drumclack があるのに、許可されない。** 一覧に古い登録が残って食い違うことがあります。ターミナルで
  次のコマンドを実行すると、Drumclack の登録だけが消えます。そのあと Drumclack を起動し直して、許可の手順をやり直してください。

  ```sh
  tccutil reset ListenEvent jp.etbs.drumclack
  ```

- **起動したのに何も出ない。** 2回目以降は窓を出さないのが仕様です。アイコンを探すか、そのまま打ってみてください。
  窓を出したいときは、アプリをもう一度開きます。
- **Mac で、終了してすぐ起動したら起動に失敗した。** 終了してすぐ起動すると、macOS が起動に失敗する（エラー -600）ことがあります。
  2〜3秒待ってから、もう一度起動してください。
- **窓に音声出力の問題が出ている。** スピーカーやイヤホンがつながっているか、他のアプリが専有していないかを確認してください。
  使える状態に戻れば自動で直ります。

## プライバシー

- **キーの位置は、音を選ぶことにだけ使います。** Drumclack は、キーの物理的な位置（「A のキー」「Enter」など）を読み、
  音の名前に変える1か所でだけ使って、その音を鳴らします。**文字は読みません**（入力された文字・入力ソース・IME の情報は取りません）。
  キーの位置も打鍵の時刻も、**保存も記録も送信もしません**。
- 動作のために、メモリの上だけに短い記憶を持ちます。いま押されているキー（押しっぱなしで何度も鳴らないようにするため）と、
  直前の打鍵の時刻（強弱のため）です。どこにも書き出さず、アプリを終了すると消えます。
- **通信はしません。** サーバーへ接続するコード・利用状況の送信・自動更新はありません。窓はアプリ内のページで、外への接続を
  CSP（コンテンツセキュリティポリシー）で止めています。通信系・自動更新系のライブラリが入ると CI が落ちるようにしてあります。
- 窓の「詳細」に出るのは、直近の発音の時刻と同時発音数だけです。演奏の画面は、鳴った音の名前でパッドを光らせます。
  画面には、キーの位置を渡しません。
- Drumclack が書くファイルは、設定ファイル（下）だけです。入っているのは設定と割り当てで、打鍵の履歴ではありません。
- 端末のログ（アプリを端末から起動したときだけ見えます）には、キーの位置・時刻・打った文字を出しません。音の出口を作り直したときの
  ログに出すのは、サンプルレートとチャンネル数だけで、機器の名前は出しません（Bluetooth 機器の名前には持ち主の名前が入ることがあるため）。
- Mac の入力監視は、OS がキーの押下を Drumclack へ渡す許可です。システム設定でいつでもオフにできます。

## 設定ファイル

設定は JSON 1ファイルです。設定が最初に保存されたときに作られます（最初にドラムが鳴ったときも保存されるので、
ふつうは初回の利用で作られます）。

| OS | 場所 |
|---|---|
| macOS | `~/Library/Application Support/jp.etbs.drumclack/settings.json` |
| Windows | `%APPDATA%\jp.etbs.drumclack\settings.json` |

すべてを最初に戻したいときは、Drumclack を終了してこのファイルを削除してください（割り当てだけなら、割り当ての画面の
「既定に戻す」）。壊れたファイルは `settings.broken.json` へ退避して、既定で起動します。

## 既定の割り当て

タイピング用（どのアプリで打っていても使う割り当て）:

| グループ | 含むキー | 既定の音 |
|---|---|---|
| 文字（`letters`） | A〜Z | 閉じハイハット（`hat_closed`） |
| スペース（`space`） | スペース | キック（`kick`） |
| Enter（`enter`） | Enter、テンキーの Enter | スネア（`snare`） |
| 削除（`delete`） | Backspace、Delete | リム（`rim`） |
| 記号（`symbols`） | 句読点・括弧などの記号、テンキーの `+ - * / .`（と `=` `,`）、JIS の `¥` `ろ` | 開きハイハット（`hat_open`） |
| 偶数の数字（`digits_even`） | 0 2 4 6 8（上段とテンキー） | 高タム（`tom_high`） |
| 奇数の数字（`digits_odd`） | 1 3 5 7 9（上段とテンキー） | 低タム（`tom_low`） |
| 制御（`control`） | Tab・Esc・F1〜F24（と PrintScreen・Pause・メニューキー） | クラップ（`clap`） |
| 移動（`navigation`） | 矢印・Home／End・PageUp／PageDown（と Insert） | 無音（`none`） |
| 修飾（`modifiers`） | Shift・Ctrl・Alt・Cmd／Win・CapsLock（と NumLock・ScrollLock・Fn）・英数／かな・変換／無変換 | 無音（`none`） |

演奏用（演奏の画面が開いていて、Drumclack の窓が最前面のときだけ使います。ほかのキーは無音）:

| キー | 音 |
|---|---|
| A | キック |
| S | スネア |
| D | リム |
| F | クラップ |
| J | 閉じハイハット |
| K | 開きハイハット |
| L | 高タム |
| ; | 低タム |
| スペース | キック |

## ビルド方法（開発者向け）

Rust（stable）と、Tauri 2 のビルドに必要なプラットフォームのツールチェーンが必要です。アプリのビルドに Node.js は不要です
（フロントエンドはビルド不要の静的ファイル `web/` をそのまま使います）。画面のテスト（後述）を動かすときだけ Node.js 22 以降を使います。

```sh
# 開発ビルド + 起動
cargo run --manifest-path src-tauri/Cargo.toml

# 配布物（Tauri CLI 2.x）。macOS は .app と .dmg、Windows は .msi
cd src-tauri
npx --yes @tauri-apps/cli@2.11.5 build --bundles app,dmg   # macOS
npx --yes @tauri-apps/cli@2.11.5 build --bundles msi       # Windows
```

配布物は `src-tauri/target/release/bundle/` に出ます。CI（`.github/workflows/build.yml`）が同じ手順で作り、成果物に残します。
署名は、Mac は ad-hoc 署名（署名の識別子はバンドルの識別子 `jp.etbs.drumclack` に揃えます）、Windows は署名なしです。
CI は、Mac の `.app` の署名の識別子がバンドルの識別子と一致することを検査します。

CI は Pull Request・`main` ブランチへの push・`v` から始まるタグの push のたびに実行されます。
**GitHub Actions の Windows 環境では、実際にキー入力を検知できるかの動作確認はできません**（対話的なログインセッションや権限の都合）。
CI で確認しているのは、ビルドとテスト、配布物の中身（識別子・版数）までです。

## テスト

```sh
cargo test --manifest-path src-tauri/Cargo.toml
```

以下を確認するユニットテストがあります（`src-tauri/src/kick.rs`, `src-tauri/src/voices.rs`, `src-tauri/src/audio.rs`）。

- 合成したキック音が無音でないこと
- 合成したキック音、および複数ボイスを重ねた最終出力が音割れ（振幅が [-1.0, 1.0] を超える）しないこと
- 同時発音数が上限（16）を超えないこと、かつ上限到達時に**既存の発音を打ち切らない**こと
- 両 OS のキーの値が Web 標準の `code` 名へ直ること（テンキー・JIS 固有キー・左右の修飾キーを含む。`key_tables.rs`）
- 押しっぱなしを無視すること（押下→リピート10回→離す→押下で、押されたと扱うのがちょうど2回）、
  監視が一時停止された後に押下中だったキーがもう一度鳴らせること（`key_tracker.rs`）
- 設定ファイルの読み書き（書いて読み直すと同じ・壊れたら退避・書き込み途中の失敗で元が無傷・知らない項目の無視・範囲外の丸め。
  `settings.rs`, `settings_store.rs`。設定 JSON の見本は `tests/fixtures/settings.sample.json`）
- 設定の更新が差分であること（`volume` だけの更新で他の項目が変わらない・`null` でその上書きだけが消える）
- 全キーのグループ分け（数字の偶奇が上段とテンキーの両方で分かれる・テンキーの記号と Enter・JIS 固有キーを含む）、
  優先順位（キー個別 ＞ グループ ＞ 既定）、無音のキーとオフ中は発音の要求が0件、試聴（`assignment.rs`, `keyboard.rs`）
- 音の出口を開けたときのログに、機器名が入らないこと（`output_recovery.rs`）

## 測定用の環境変数

`DRUMCLACK_TEST_DELAY_MS` を設定すると、キー押下から発音までの間、指定ミリ秒だけ意図的に遅延させます
（レイテンシ測定の際の陽性対照用）。画面にも「テストモード: 遅延 ◯ms」と表示され、通常状態と区別できます。

```sh
DRUMCLACK_TEST_DELAY_MS=200 cargo run --manifest-path src-tauri/Cargo.toml
```

録音から実際の遅延を測る道具は [`tools/latency`](tools/latency/README.md)（と [`tools/`](tools/README.md)）にあります。

## 実装メモ

### キー入力の取得方法（`src-tauri/src/keyboard.rs`）

macOSとWindowsで異なる方式を採用しています（どちらも OS の API を直接呼び、外部のキー入力クレートは使いません）。

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
  このリポジトリでも該当しうる問題でした。キーを文字に直す必要はそもそも無いため、
  文字・入力ソースを引く API を一切呼ばず、HIToolboxに触れない素の `CGEventTap` 直叩きへ
  切り替えています。読むのは物理キーのコード（配列に依存しない位置）、OS が付けるリピートの印、
  修飾キーのフラグだけです。キーを離したイベントと修飾キーの変化（Mac では通常の押下とは別の
  イベントで届く）も受け取ります。
- **Windows**: `rdev` をやめ、`SetWindowsHookExW(WH_KEYBOARD_LL, ...)` による
  **低レベルキーボードフック**を直接呼んでいます（`src-tauri/src/keyboard.rs` の `windows_hook`
  モジュール）。これも受信専用で、フック内でイベントを握りつぶす（消費してブロックする）ことは
  ありません。`rdev` をやめた理由は、(1) こちらが読まなくても打鍵のたびに内部で文字を作っていた、
  (2) キーボードのフックに加えてマウスのフックも同時に張っていた、(3) 取れるキーが配列依存の
  名前で、テンキーの Enter を区別できなかった、の3点です。直接呼ぶ版が読むのは**走査コードと
  拡張キーの印・離しの印だけ**で、仮想キーコード（配列と NumLock の状態で変わる）は読みません。
  マウスのフックは張りません。NumLock の状態でキーの名前は変わらず、テンキーの Enter も区別できます。

キーの位置は音を選ぶことにだけ使い、保存も記録もしません。両 OS の値は、Web 標準の
`KeyboardEvent.code` 名（`KeyA` `Digit1` `NumpadEnter` `IntlYen` など）をもとにした共通の型へ
直します（対応表は `src-tauri/src/key_tables.rs`）。この型には表示・ログ出力・保存のための機能を
持たせておらず（持たせるとコンパイルが通らない）、音の名前に変える1か所の外へ出しません。
押しっぱなしは最初の1回だけを「押された」として扱い、監視が OS に一時的に止められたときは
押下中のキーの記憶を空にします（`src-tauri/src/key_tracker.rs`）。鳴らす音は、次の「キーの割り当て」で決まります。

権限確認（macOS のみ、`src-tauri/src/permission.rs`）には `IOHIDCheckAccess`（IOKit / `IOHIDLib.h`、
macOS 10.15+ で公開されている API）を使っています。これはプロンプトを出さず、現在の許可状態
（許可済み／未許可／不明）だけを読み取る API です。**このアプリは許可状態の確認のみを行い、
システム設定の変更や許可の強制取得は一切行いません。**

押されたキーの位置は、発音の直前に音の名前へ変えたあと残しません（押下中のキーの一時的な記憶を
除く。ディスク・ログ・画面・通信には出しません）。文字は読みません。

通信が混ざらないことは CI で固定しています（`tools/ci/`）。依存に通信系・自動更新系のクレートが
入ったとき、`tauri.conf.json` の CSP が決めた値から変わったときに落ちます。

### キーの割り当て（`src-tauri/src/assignment.rs`）

キーの位置から音の名前への変換は `AssignmentTable::sound_for` の1か所だけです。割り当ての検索はキー監視側で済ませ、
音声コールバックには「音・変種・音量」の発音要求だけが届きます。優先順位は「キー個別の上書き ＞ グループの指定 ＞ 既定」です。

既定の割り当て（タイピング用の10グループと演奏用）は、上の「既定の割り当て」の表のとおりです。

演奏用（`play`）の既定は、ホームポジション周辺（`A S D F` ＝ キック／スネア／リム／クラップ、`J K L ;` ＝ 閉じハイハット／
開きハイハット／高タム／低タム、スペース ＝ キック）だけが鳴り、ほかは無音です（配置は仮の値）。
演奏用が使われるのは、「演奏の区画が開いていて、かつ Drumclack の窓が最前面」の間だけです（それ以外はタイピング用。
`src-tauri/src/play_mode.rs`）。画面が `set_play_view_open` で区画の開閉を知らせ、窓のフォーカスの変化は Rust が受けます。
演奏用で鳴らしたときだけ、Rust から画面へ「鳴った音の名前」（`drum-played`）を送り、パッドを光らせます
（タイピング用で鳴らした音は送りません。遮断は Rust 側 `keyboard.rs` の `hit.from_play_set`）。
演奏用への切り替わり（`play-mode-changed`。真偽だけ）も送ります。キーの位置・打鍵の時刻は画面に届きません。

`enabled` が偽のときは、キーの監視は続けたまま、鳴らす直前で捨てます。`preview_sound` 命令は、音の名前（`kick` など）を
指定して1回鳴らします（オフ中でも鳴ります）。

### 強弱（`src-tauri/src/dynamics.rs`）

打鍵ごとに、直前に鳴らした打鍵からの間隔で音量を決めます（間が空くほど強く、連打は軽く）。80ms 以下が最小、
400ms 以上が最大で、間はなめらかにつなぎます。毎回±1dB の揺らぎを足します。設定の `dynamics`（0〜1）が幅で、
0 は揺らぎも含めて常に同じ音量、1 は最小が最大の−10dB です（既定 0.6）。無音のキー・オフのときの打鍵は間隔に数えません。
時刻は打鍵を受け取った時点の単調時計（`Instant`）で取り、画面表示用の `last_play_ms`（壁時計）とは別に持ちます。
音量に使う数値（80／400ms・−10dB・±1dB・曲線）は仮の値で、`dynamics.rs` 先頭の定数と `curve` で調整します。

### 音の合成・再生（`src-tauri/src/kick.rs`, `src-tauri/src/audio.rs`, `src-tauri/src/voices.rs`）

- 起動時（音声デバイス初期化時。出力先のサンプルレートが変わったときだけ、下の節のとおり合成し直す）に、サイン波をベースに **ピッチと音量の両方を指数的に減衰させる**
  ことで808風のキック音を合成し、メモリ上の `Vec<f32>` に保持します。以降は毎回合成し直さず、
  このバッファを使い回します。
- 再生には [`cpal`](https://crates.io/crates/cpal) を使い、デフォルトの出力デバイスへ直接書き込みます。
  バッファサイズは 128 フレームを希望し（デバイスの対応範囲外なら範囲内へ丸める）、
  希望値・サンプルレート・チャンネル数に加え、コールバックが実際に受け取ったバッファ長（フレーム数）も
  標準出力へログ出力します（Windows の共有モードでは OS 周期に丸められ 128 が効かない場合があるため）。
- 同時発音（ポリフォニー）は最大 **16 音**。上限に達した状態で新しいキー入力があっても、
  鳴っている途中の音を打ち切らず、単に新しい発音を追加しないだけの挙動にしています
  （音声コールバックが `voices::VoicePool::start()` で上限到達時に要求を拒否する。拒否された打鍵は「直近の発音」にも反映しない）。

### 出力機器の切り替わりとスリープ復帰（`src-tauri/src/output_recovery.rs`）

イヤホンの抜き差し・Bluetooth の接続と切断・スリープ復帰・既定の出力先の変更のあとも鳴り続けるよう、
専用スレッドが音の出口（出力ストリーム）を見張り、失ったら作り直します。

- 見るもの: ①ストリームのエラー（cpal の `err_fn`）、②既定の出力先が使用中のものと違う（名前・サンプルレート・
  チャンネル数。cpal 0.15 には既定の出力先の変更通知が無いため、1秒ごとに問い合わせる）、
  ③コールバックが3回の見張り（≒3秒）つづけて進んでいない（エラーなしに止まる場合の保険）。
- 作り直しに失敗している間は、状態の帯とトレイアイコンを「問題あり」にし、間隔を空けて再試行します
  （0.5 秒から倍にして最大 5 秒）。直れば自動で「問題あり」が消えます。起動時に音声デバイスが開けなかった場合も
  同じ仕組みで復帰します（その間も打鍵の監視は動き、出口が戻れば同じエンジンで鳴ります）。
- 同じサンプルレートの出口に作り直すときは、キットの合成をやり直しません。出力先のサンプルレートが変わるとき
  （44.1kHz と 48kHz など）だけ、そのレートで合成し直します（再生中のレート変換はしないので、音の高さと長さは
  変わりません）。出口に渡すキットは直前の1レート分だけ覚えますが、発音の窓口（エンジン）が起動時のキットを持ち続けるため、最大2レート分のキットがメモリに載ります。
- 出口を失っている間に溜まった打鍵は、作り直した瞬間にまとめて鳴らさず捨てます。
- 起動時に設定の窓を出したときは、どの条件で出したかを `first_sound_not_done` / `input_permission_missing` /
  `audio_init_failed` / `restarted_from_window` の名前だけでログに1行出します（キーの位置・時刻は出しません）。
- 調整する値は、`output_recovery.rs` 先頭の `WATCH_INTERVAL`（見張りの間隔）・`RETRY_FIRST_INTERVAL` /
  `RETRY_MAX_INTERVAL`（再試行の間隔）・`HEALTHY_RESET_AFTER`・`STALL_WATCH_COUNT`（止まったと判断するまで）と、
  `audio.rs` の `SUPERVISOR_TICK_MS`（スレッドが目覚める間隔）です。すべて仮の値です。

### 画面（`web/`）

窓は1枚で、「設定／割り当て／演奏」の3区画です。最小の窓は 900×600 です。

- 上部の**状態の帯**: 正常時は1行（「正常に動いています」）。異常時は問題ごとに、何が起きたかと次の操作を出し、
  内部のエラー文字列は「詳細:」として弱く添えます。入力監視の権限（macOS のみ。Windows ではこの項目を出しません）
  と音声デバイスの失敗、まだ一度も鳴っていない（許可済みで未発音）ことが対象です。入力監視の許可は OS の判定が
  起動時のまま変わらないため、キーのイベントが実際に届いた（`get_status` の `key_events_seen`。届いた事実だけで、
  どのキーかは持ちません）ときは許可済みとして扱います。一度届いたら起動後はずっと真のままなので、許可が後から取り消されても、起動し直すまでは「許可済み」のままです（既知の制約）。直近の発音・同時発音数・テストモード（`DRUMCLACK_TEST_DELAY_MS` 設定時）は
  「詳細」に畳みます。
- 画面は設定も約1秒ごとに取り直します（メニュー／トレイで切り替えたオン／オフ、`first_sound_done` を、開いている窓へ反映するため）。
  送っている最中の更新や、動かしている最中のスライダーは、取り直した値で上書きしません。
- **設定の区画**: オン／オフ、音量、強弱の幅、言語、キーボードの配列。変更は即時に保存し（スライダーは手を離したとき）、
  `update_settings` へは変えた項目だけを送ります。保存に失敗したとき、設定ファイルを読めず既定で起動したときは、その旨を1行出します。
- **演奏の区画**（`web/play.js`）: 8個のパッド（クリック＝`pointerdown` で試聴、演奏用の割り当てキーを表示、鳴った音の名前が届くと光る）、
  「いま演奏用／タイピング用」の表示、窓が背面のときは薄くして理由を表示、オフ中は「オンにする」。光る表現は OS の
  「動きを減らす」に従います。演奏の区画が開いている間は、画面の keydown をすべて既定動作ごと止めます
  （スペースでフォーカス中のパッドが押されて二重に鳴るのを防ぐため。打鍵そのものは Rust の監視が受けます）。
  画面が受け取るイベントは `drum-played` と `play-mode-changed` の2つだけで、画面から送る権限はありません。
- **日英の対応表**: `web/i18n/ja.json` と `en.json`（翻訳用のライブラリは使いません）。画面の文言はすべてここを通し、
  言語を切り替えると `<html lang>` も変わります。`language` と `keyboard_layout` の `auto` は、OS の言語が日本語なら日本語・JIS、
  それ以外は英語・US として扱います（選ぶまで何も保存しません）。
- **Rust との橋渡し**は `web/bridge.js` の1か所だけ。画面のほかのファイルは `window.__TAURI__` に触りません。
- 画面の起動に失敗したとき（例外・5秒たっても終わらないとき）は、`web/boot.js` が英日の直書きの文言で次の操作を出します（真っ白のままにしないため）。
- 保存失敗・設定を読めず既定で起動した旨の通知は、区画をまたいで見えるよう、帯の下・タブの上に出します。
- テスト用の属性: 要素は `data-testid`、文言の要素は `data-i18n` で選べます（文言では選びません）。

状態の正常・異常は色だけでなく必ずテキストでも表現し、コントラスト比は WCAG AA
（通常テキストで 4.5:1 以上）を満たすよう配色しています（明るい配色・暗い配色の両方。`web/style.css`）。

画面のテスト（`tests/web/`）は、素のブラウザ（Chromium）で `web/` を開き、`window.__TAURI__` を差し替えて確かめます。
対応表の読み込みは `file://` では動かないため、`tests/web/serve.js` の簡易 HTTP サーバ経由で開きます
（`tauri.conf.json` の CSP を応答ヘッダに付けるので、CSP に反する書き方も落ちます）。

```sh
cd tests/web
npm ci
npx playwright install chromium
npx playwright test
```

実物の窓（macOS は WKWebView、Windows は WebView2）での見え方は、このテストでは確かめていません。

### 設定ファイル（`src-tauri/src/settings.rs`, `src-tauri/src/settings_store.rs`）

設定は JSON 1ファイルで、読み書きは Rust 側が行います。画面から呼べる命令は `get_status` `get_settings`
`update_settings` `preview_sound` `open_input_monitoring_settings`（入力監視の設定画面を開く）
`restart_app`（アプリ自身を起動し直す）`set_play_view_open`（演奏の区画の開閉を知らせる）の7つだけです。
画面に許しているイベントの権限は、受け取り（`core:event:allow-listen`）だけです。

- macOS: `~/Library/Application Support/jp.etbs.drumclack/settings.json`
- Windows: `%APPDATA%\jp.etbs.drumclack\settings.json`

入るのは `version` `kit` `enabled` `volume`（0〜1）`dynamics`（0〜1）`language` `keyboard_layout`
`first_sound_done` と、タイピング用（`typing`）・演奏用（`play`）それぞれの割り当ての上書き（`groups` `keys`）だけです。
押されたキーの履歴は入りません。書き込みは別名のファイルへ書いてから置き換えます。
壊れたファイル（JSON として読めない等）は `settings.broken.json` へ退避して既定で起動します（前の退避があれば
`settings.broken.json.1` へ寄せます。世代は1つ）。退避できないとき、権限などで読めないだけのときは、元のファイルを
守るためその起動中は保存しません。このアプリより新しい版（`version` が 1 より大きい）のファイルは、初回の保存前に
`settings.v{n}.json` へコピーして残します（同名があれば上書きせず `settings.v{n}.1.json` 以降へ。残せなければ保存しません）。UTF-8 の BOM 付きファイルもそのまま読みます。知らない項目・音の名前・キーの名前は無視し、
範囲外の数値は 0〜1 に丸めます。

`keyboard_layout` は `auto`（まだ選んでいない。既定）`jis` `us` のいずれかで、`auto` のときは画面が OS の言語から JIS か US を決めます（日本語なら JIS）。

`update_settings` は**差分**を受け取ります（全体の置き換えではありません）。

- 送った項目だけを今の設定に重ね、送らなかった項目は今の値のまま残ります（例: `{ "volume": 0.3 }` だけを送っても、
  割り当て・言語・オン／オフは変わりません）。型の違う値・知らない名前・知らない項目は、その項目だけ今の値のままです。
- 割り当て（`typing` / `play` の `groups` / `keys`）は項目単位で重ねます。値に `null` を送ると、その項目の上書きだけを消します
  （キーなら「グループに従う」、グループなら「既定に戻す」）。ほかの項目は変わりません。
- Rust 側から設定を変える入口（オン／オフ、`first_sound_done`）も同じ差分の経路を通るので、画面の更新が Rust 側の変更を巻き戻しません。
- 設定の変更は、アプリを起動し直さずに次の打鍵から鳴り方へ反映されます。

### ウィンドウを閉じたとき（常駐）

メニューバー（Mac）／タスクトレイ（Windows）に常駐します。ウィンドウの「閉じる」操作では **終了せず、窓を隠すだけ** です
（`src-tauri/src/main.rs` の `on_window_event` で `CloseRequested` を止め、`tray.rs` の `hide_main_window` で隠します）。
終了するのは、トレイ／メニューバーのアイコンのメニューの「終了」と、Mac の左上のアプリのメニューの「終了」だけです。常駐し続けることは、OS の通知ではなく、窓の中の文で伝えます。
窓の下の注記（`resident-note`）の直下に、「メニューバーにアイコンが見えないとき」の畳んだ案内（`resident-help`）を置きます。`get_status` の `platform` が `macos` のときだけ表示し、自動では開きません。

- メニュー: オン／オフ、設定を開く、演奏モードを開く、ログイン時に起動（既定オフ・押したときだけ登録）、終了。文言は `src-tauri/src/resident.rs` の小さな日英の対応表です。
- アイコンは「オン（塗りの円）／オフ（輪）／問題あり（円に「！」）」の3状態。問題ありは、入力監視が未許可、または音声デバイスが使えないとき（起動時に開けなかった場合と、あとから出口を失って作り直せない間。直れば自動で消えます）です。
- 起動時は窓を出しません。出すのは、`first_sound_done` が偽の間／入力監視が未許可／音声デバイスの初期化に失敗したとき／
  窓の「起動し直す」ボタンで起動し直したときだけです。窓を出したときは、どの条件で出したかを名前だけでログに1行出します。
- 「起動し直す」ボタンは、起動し直す直前に設定フォルダへ印のファイル（`show_window_next_launch`）を残し、次の起動が読んで消します
  （`resident.rs` の `mark_show_window_next_launch` / `decide_window_at_launch`）。設定ファイル（`settings.json`）には入れません。
- Mac は、窓を閉じている間 Dock にアイコンを出しません。
- もう一度起動すると、動いている方の設定の窓が開きます。2通りあります。
  - 別のプロセスとして起動したとき（コマンド・`open -n`）: 新しいプロセスは終わります（二重起動の見張り）。
  - Finder・Spotlight・Dock から開き直したとき（Mac）: プロセスは増えず、アプリへの「開き直し」の合図（`RunEvent::Reopen`）が届きます。
    窓が出ていなければ設定の窓を出します（`main.rs` の `run`）。
- Mac は、左上のアプリのメニューにも「設定を開く」「演奏モードを開く」を足してあります（標準の項目は残します。`src-tauri/src/app_menu.rs`）。
  切り欠きのある Mac では、メニューバーのアイコンが多いと Drumclack のアイコンが切り欠きの裏に隠れて押せないためです。
- オフの間もキーの監視は止めません（音を出す直前で捨てます）。
