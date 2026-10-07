// 常駐型タイピングドラムの成立性検証プロトタイプ。
//
// 画面は状態表示のみ。他アプリ操作中でもキー入力を受信専用フックで検知し、
// 起動時に合成しておいたキック音を cpal で鳴らす。通信処理は一切行わない。
//
// メニューバー／トレイに常駐する。窓を閉じても終了せず（終了はトレイのメニューの「終了」）、
// 起動時は窓を出さない（初めて音が鳴るまで／入力監視が未許可／音声デバイス失敗のときは出す）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assignment;
mod audio;
mod drums;
mod dynamics;
mod keyboard;
mod key_position;
mod key_tables;
mod key_tracker;
mod kick;
mod permission;
mod resident;
mod settings;
mod settings_store;
mod state;
mod tray;
mod voices;

use std::sync::Arc;

use tauri::Manager;

use resident::FirstSoundGate;
use settings_store::{SettingsSnapshot, SettingsStore};
use state::{AppState, AudioInitStatus, StatusSnapshot};

/// 画面が定期ポーリングする、状態表示用の Tauri コマンド。
/// 副作用は持たず、現在の状態を読み取って返すだけ。
#[tauri::command]
fn get_status(app_state: tauri::State<'_, Arc<AppState>>) -> StatusSnapshot {
    state::build_snapshot(&app_state)
}

/// 現在の設定を返す（画面用の命令その1。権限は `capabilities/default.json`）。
#[tauri::command]
fn get_settings(store: tauri::State<'_, Arc<SettingsStore>>) -> SettingsSnapshot {
    store.get()
}

/// 設定を更新する（画面用の命令その2）。送られた項目だけを今の設定に重ね、送られなかった項目は
/// 今の値のまま残す。割り当ては項目単位で重ね、値が `null` の項目は上書きを消す。
/// 範囲外の値は丸め、即時に保存し、音量・割り当て・オン／オフを鳴らす側へ反映する。
/// 整えたあとの設定全体を返す。
#[tauri::command]
fn update_settings(
    store: tauri::State<'_, Arc<SettingsStore>>,
    settings: serde_json::Value,
) -> Result<SettingsSnapshot, String> {
    store.update(&settings)
}

/// 音の名前（`kick` など）を指定して1回鳴らす（画面用の命令その3。オフ中でも鳴る）。
#[tauri::command]
fn preview_sound(app_state: tauri::State<'_, Arc<AppState>>, sound: String) -> Result<(), String> {
    let engine = app_state.audio_engine.as_ref().ok_or("音声デバイスが使えません")?;
    assignment::preview(engine, &sound)
}

/// 入力監視の設定画面を開く（画面用の命令その4）。macOS 以外ではエラーを返す。
#[tauri::command]
fn open_input_monitoring_settings() -> Result<(), String> {
    permission::open_input_monitoring_settings()
}

/// アプリ自身を起動し直す（画面用の命令その5）。入力監視の許可は、起動し直すまで反映されない。
/// `async` にして、メインスレッド以外から呼ぶ。そうすると終了の処理（二重起動の見張りの後片付け）を
/// 通ってから起動し直すので、新しく起動した方が「すでに動いている」と判断して消えることがない。
#[tauri::command]
async fn restart_app(app: tauri::AppHandle) {
    app.restart()
}

fn main() {
    tauri::Builder::default()
        // 二重に起動したら、動いている方の設定の窓を開く（渡された引数・作業フォルダは見ない）。
        // 他のプラグインより先に登録する。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main_window(app, Some("settings"));
        }))
        // ログイン時に起動。切り替えはトレイのメニュー（Rust 側）だけが行い、画面には権限を出さない。
        // ここで登録はしない（利用者がメニューを押したときだけ登録する）。
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
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

            // 設定は OS 標準の設定フォルダ（jp.etbs.drumclack）から読む。フォルダが分からないときは
            // 既定で動き、保存だけ行わない。音量・割り当て・オン／オフは読み込み時に鳴らす側へ反映される。
            // 割り当てを反映してからキー監視を始める（起動直後の打鍵が既定で鳴らないように）。
            let config_dir = app.path().app_config_dir().ok();
            let store = Arc::new(SettingsStore::open_with(
                config_dir,
                app_state.audio_engine.clone(),
                app_state.assignments.clone(),
            ));
            app.manage(store.clone());

            // キー入力の監視は、鳴らす対象（音声エンジン）が用意できた場合のみ開始する。
            if let Some(engine) = audio_engine {
                keyboard::spawn_listener(engine, app_state.clone());
            }

            app.manage(app_state.clone());

            // 常駐: トレイとメニューを作り、起動時に窓を出すかを決め、状態の見張りを始める。
            // 窓を出すのは、初めて音が鳴るまで／入力監視が未許可／音声デバイスの初期化失敗のとき。
            tray::setup(app.handle())?;
            let first_sound_done = store.get().settings.first_sound_done;
            tray::apply_launch_visibility(
                app.handle(),
                resident::show_window_at_launch(
                    first_sound_done,
                    app_state.input_permission_ok(),
                    app_state.audio_ok(),
                ),
            );
            // 初めて音が鳴ったら、first_sound_done を真にして1回だけ保存する。
            // 保存に失敗したら、次の周期でもう一度試す（成否は設定のスナップショットの save_failed）。
            let first_sound = FirstSoundGate::new(first_sound_done, move || {
                !store.mark_first_sound_done().save_failed
            });
            tray::spawn_watcher(app.handle().clone(), first_sound);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_status,
            get_settings,
            update_settings,
            preview_sound,
            open_input_monitoring_settings,
            restart_app
        ])
        .on_window_event(|window, event| {
            // 窓を閉じても終了せず、隠して常駐を続ける（Mac は Dock からも外す）。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                tray::hide_main_window(window.app_handle());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running drumclack");
}
