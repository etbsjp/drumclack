// 常駐型タイピングドラムの成立性検証プロトタイプ。
//
// 画面は状態表示のみ。他アプリ操作中でもキー入力を受信専用フックで検知し、
// 起動時に合成しておいたキック音を cpal で鳴らす。通信処理は一切行わない。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod keyboard;
mod kick;
mod permission;
mod state;
mod voices;

use std::sync::Arc;

use tauri::Manager;

use state::{AppState, AudioInitStatus, StatusSnapshot};

/// 画面が定期ポーリングする、唯一の Tauri コマンド。
/// 副作用は持たず、現在の状態を読み取って返すだけ。
#[tauri::command]
fn get_status(app_state: tauri::State<'_, Arc<AppState>>) -> StatusSnapshot {
    state::build_snapshot(&app_state)
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // 音声デバイスの初期化は起動時に一度だけ行う。失敗しても
            // アプリは起動を続け、画面に失敗理由を表示する（キー監視は続行する
            // ＝ 権限や音声デバイスの状態はそれぞれ独立して表示するため）。
            // cpal::Stream は Send/Sync ではないため、専用スレッドに閉じ込めて
            // 再生を維持する（詳細は audio::spawn_output_stream のコメント参照）。
            let (audio_init, audio_engine) = match audio::spawn_output_stream() {
                Ok(engine) => {
                    (AudioInitStatus::Ok { sample_rate: engine.sample_rate() }, Some(engine))
                }
                Err(reason) => {
                    eprintln!("[drumclack] 音声デバイス初期化に失敗しました: {reason}");
                    (AudioInitStatus::Err(reason), None)
                }
            };

            let app_state = Arc::new(AppState::new(audio_init, audio_engine.clone()));

            // キー入力の監視は、鳴らす対象（音声エンジン）が用意できた場合のみ開始する。
            if let Some(engine) = audio_engine {
                keyboard::spawn_listener(engine, app_state.clone());
            }

            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_status])
        .on_window_event(|window, event| {
            // 「閉じる」操作はトレイ格納ではなく、プロセスごと終了する方針。
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                window.app_handle().exit(0);
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running drumclack");
}
