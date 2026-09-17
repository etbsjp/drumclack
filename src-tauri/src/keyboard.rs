//! OS全体からのキー入力を「受信専用」で監視する。
//!
//! 採用ライブラリと理由は README の「実装メモ」節を参照。要点だけ書くと:
//!
//! - macOS: `rdev::listen` は内部で `CGEventTap` を **listen-only** モードで
//!   張る。これは他アプリへのイベント伝播を止めない“傍受のみ”のタップで、
//!   必要な権限も「アクセシビリティ」ではなく「入力監視（Input Monitoring）」
//!   のみで済む。今回の要件（受信専用・他アプリの入力を妨げない）に合致する。
//! - Windows: `rdev::listen` は `SetWindowsHookEx(WH_KEYBOARD_LL, ...)` による
//!   低レベルキーボードフックを使う。これも受信専用で、フック内でイベントを
//!   握りつぶす（消費する）ことはしない。
//!
//! ここで扱うのは「キーが押された」というタイミングのみ。どのキーが押されたか
//! （文字・キーコード）は一切保存・表示・ログ出力しない。

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rdev::EventType;

use crate::audio::AudioEngine;
use crate::state::AppState;

/// キー入力監視を別スレッドで開始する。
///
/// `rdev::listen` はブロッキング呼び出し（内部でOSのイベントループを回す）
/// のため、専用スレッドを立てて実行する。権限が無い環境では `listen` 自体が
/// エラーを返すか、あるいはイベントが一切届かない状態になるが、
/// いずれの場合もアプリ本体はクラッシュしない。
pub fn spawn_listener(engine: Arc<AudioEngine>, state: Arc<AppState>) {
    thread::spawn(move || {
        let test_delay_ms = state.test_delay_ms;

        let callback = move |event: rdev::Event| {
            // 押されたキーの種類（Key の中身）には一切触れない。
            // イベントの種別が KeyPress かどうかだけを見て、押下タイミングとして扱う。
            if !matches!(event.event_type, EventType::KeyPress(_)) {
                return;
            }

            let engine = engine.clone();
            let state = state.clone();

            match test_delay_ms {
                // `DRUMCLACK_TEST_DELAY_MS` が設定されている場合のみ、
                // 測定用の陽性対照として意図的に遅延させてから発音する。
                // listen() 自体のスレッドを止めないよう、別スレッドで待つ。
                Some(ms) => {
                    thread::spawn(move || {
                        thread::sleep(Duration::from_millis(ms));
                        trigger(&engine, &state);
                    });
                }
                None => trigger(&engine, &state),
            }
        };

        if let Err(err) = rdev::listen(callback) {
            eprintln!(
                "[drumclack] キー入力の監視を開始できませんでした（入力監視の権限が未許可の可能性があります）: {err:?}"
            );
        }
    });
}

fn trigger(engine: &AudioEngine, state: &AppState) {
    // 上限（16音）に達している場合、trigger() は何もせず false を返す。
    // その場合も直近発音時刻は更新しない（実際には鳴っていないため）。
    if engine.trigger() {
        state.record_play_now();
    }
}
