//! メニューバー（Mac）／タスクトレイ（Windows）への常駐。Tauri の窓・メニュー・トレイへのつなぎ込み。
//!
//! 判断（アイコンの状態・窓を出す条件・文言・初回の印）は `resident.rs` にあり、ここは結果を
//! OS の部品へ反映するだけ。メニューのオン／オフは画面の更新と同じ設定の経路（`SettingsStore`）を通り、
//! キー監視は止めない（監視が読むオン／オフの値が変わり、音を出す直前で捨てられる）。
//! 打鍵の内容・時刻は、ここには届かない。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::resident::{
    icon_rgba, icon_state, menu_text, resolve_menu_lang, watch_first_sound, FirstSoundGate, IconState, MenuLang, ICON_SIZE,
};
use crate::settings_store::SettingsStore;
use crate::state::AppState;

const MAIN_WINDOW: &str = "main";
/// 状態（許可・音声デバイス・オン／オフ・言語）を見に行く間隔。調整するときはここを変える。
const POLL_INTERVAL: Duration = Duration::from_millis(1000);

const ID_ENABLED: &str = "enabled";
const ID_OPEN_SETTINGS: &str = "open_settings";
const ID_OPEN_PLAY: &str = "open_play";
const ID_AUTOSTART: &str = "autostart";
const ID_QUIT: &str = "quit";

/// 作ったトレイとメニュー項目。見た目を最新に保つために持つ。
struct Tray {
    icon: TrayIcon<Wry>,
    enabled: CheckMenuItem<Wry>,
    open_settings: MenuItem<Wry>,
    open_play: MenuItem<Wry>,
    autostart: CheckMenuItem<Wry>,
    quit: MenuItem<Wry>,
    /// 左上のアプリのメニューに足した項目（Mac のみ）。組み立てに失敗したときは `None`（トレイは動かす）。
    #[cfg(target_os = "macos")]
    app_menu: Option<crate::app_menu::AppMenu>,
    /// 最後に反映したアイコンの状態と言語。変わったときだけ OS の部品を更新する。
    rendered: Mutex<Option<(IconState, MenuLang)>>,
}

fn load_icon(state: IconState) -> Image<'static> {
    Image::new_owned(icon_rgba(state), ICON_SIZE, ICON_SIZE)
}

/// 窓を出す。Mac は Dock にアイコンを戻す。`section` があれば、その区画（settings / assign / play）を開く。
pub fn show_main_window(app: &AppHandle, section: Option<&str>) {
    #[cfg(target_os = "macos")]
    log_if_failed("activation policy", app.set_activation_policy(tauri::ActivationPolicy::Regular));

    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        eprintln!("[drumclack] 窓を出せませんでした: メインの窓が見つかりません");
        return;
    };
    // 失敗しても止めずに続ける。失敗は戻り値で捨てず、原因が追えるようにログへ出す（打鍵の内容は含まない）。
    log_if_failed("show", window.show());
    log_if_failed("unminimize", window.unminimize());
    log_if_failed("set_focus", window.set_focus());
    if let Some(section) = section {
        // 画面側の入口（web/main.js の drumclackShowSection）を呼ぶだけ。画面に権限は要らない。
        log_if_failed(
            "eval",
            window.eval(format!("window.drumclackShowSection && window.drumclackShowSection({section:?})")),
        );
    }
}

pub fn log_if_failed<E: std::fmt::Display>(step: &str, result: Result<(), E>) {
    if let Err(error) = result {
        eprintln!("[drumclack] 窓の操作に失敗しました（{step}）: {error}");
    }
}

/// 窓を閉じる（隠す）。プロセスは終わらない。Mac は Dock からアイコンを外す。
pub fn hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
    // 隠した窓は最前面ではない（フォーカスの知らせを待たずに、演奏用の割り当てを止める）。
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        state.assignments.play_mode.set_window_focused(false);
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

/// 起動時の窓の扱いを決める。窓は最初は隠してあり（tauri.conf.json の `visible: false`）、
/// 出す条件に当たるときだけ出す。出さないときは、Mac は Dock にも出さない。
pub fn apply_launch_visibility(app: &AppHandle, show: bool) {
    if show {
        show_main_window(app, None);
    } else {
        #[cfg(target_os = "macos")]
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
}

fn os_locale() -> Option<String> {
    sys_locale::get_locale()
}

fn current_lang(store: &SettingsStore) -> MenuLang {
    resolve_menu_lang(store.get().settings.language, os_locale().as_deref())
}

fn autostart_enabled(app: &AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// トレイとメニューを作って、アプリに持たせる。
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let store = app.state::<Arc<SettingsStore>>().inner().clone();
    let state = app.state::<Arc<AppState>>().inner().clone();
    let settings = store.get().settings;
    let lang = current_lang(&store);
    let text = menu_text(lang);
    let icon_state = icon_state(settings.enabled, state.input_permission_ok(), state.audio_ok());

    // 「ログイン時に起動」は、利用者がこの項目を押したときだけ切り替える（起動時に勝手に登録しない）。
    // ここでは今の登録状態を読んで、チェックに映すだけ。
    let enabled = CheckMenuItem::with_id(app, ID_ENABLED, text.enabled, true, settings.enabled, None::<&str>)?;
    let open_settings = MenuItem::with_id(app, ID_OPEN_SETTINGS, text.open_settings, true, None::<&str>)?;
    let open_play = MenuItem::with_id(app, ID_OPEN_PLAY, text.open_play, true, None::<&str>)?;
    let autostart =
        CheckMenuItem::with_id(app, ID_AUTOSTART, text.launch_at_login, true, autostart_enabled(app), None::<&str>)?;
    let quit = MenuItem::with_id(app, ID_QUIT, text.quit, true, None::<&str>)?;
    let separator_a = PredefinedMenuItem::separator(app)?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[&enabled, &separator_a, &open_settings, &open_play, &separator_b, &autostart, &quit],
    )?;

    let icon = TrayIconBuilder::with_id("drumclack")
        .icon(load_icon(icon_state))
        // Mac のメニューバーは、形（透明度）だけを使うテンプレート画像として出す（明暗に自動で合う）。
        .icon_as_template(true)
        .tooltip(text.tooltip(icon_state))
        .menu(&menu)
        .on_menu_event(handle_menu_event)
        .build(app)?;

    #[cfg(target_os = "macos")]
    let app_menu = match crate::app_menu::setup(app, text) {
        Ok(app_menu) => {
            app.on_menu_event(crate::app_menu::handle_menu_event);
            Some(app_menu)
        }
        Err(error) => {
            // アプリのメニューは補助の入口。作れなくても、トレイの初期化は続ける。
            eprintln!("[drumclack] アプリのメニューを作れませんでした: {error}");
            None
        }
    };

    app.manage(Arc::new(Tray {
        icon,
        enabled,
        open_settings,
        open_play,
        autostart,
        quit,
        #[cfg(target_os = "macos")]
        app_menu,
        rendered: Mutex::new(Some((icon_state, lang))),
    }));
    Ok(())
}

/// 見た目（アイコン・チェック・文言）を、今の状態に合わせる。変わっていなければ何もしない。
pub fn refresh(app: &AppHandle) {
    let (Some(tray), Some(store), Some(state)) = (
        app.try_state::<Arc<Tray>>(),
        app.try_state::<Arc<SettingsStore>>(),
        app.try_state::<Arc<AppState>>(),
    ) else {
        return;
    };

    let enabled = store.get().settings.enabled;
    let _ = tray.enabled.set_checked(enabled);

    let icon_state = icon_state(enabled, state.input_permission_ok(), state.audio_ok());
    let lang = current_lang(&store);
    let mut rendered = tray.rendered.lock().unwrap_or_else(|p| p.into_inner());
    let previous = *rendered;
    if previous.map(|(s, _)| s) != Some(icon_state) {
        let _ = tray.icon.set_icon(Some(load_icon(icon_state)));
        // set_icon のあとにテンプレート指定が外れる OS があるため、毎回付け直す。
        let _ = tray.icon.set_icon_as_template(true);
    }
    if previous != Some((icon_state, lang)) {
        // ツールチップは状態か言語が変わったときだけ更新する。
        let _ = tray.icon.set_tooltip(Some(menu_text(lang).tooltip(icon_state)));
    }
    if previous.map(|(_, l)| l) != Some(lang) {
        let text = menu_text(lang);
        let _ = tray.enabled.set_text(text.enabled);
        let _ = tray.open_settings.set_text(text.open_settings);
        let _ = tray.open_play.set_text(text.open_play);
        let _ = tray.autostart.set_text(text.launch_at_login);
        let _ = tray.quit.set_text(text.quit);
        #[cfg(target_os = "macos")]
        if let Some(app_menu) = &tray.app_menu {
            app_menu.set_text(text);
        }
    }
    *rendered = Some((icon_state, lang));
}

fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        ID_ENABLED => {
            if let Some(store) = app.try_state::<Arc<SettingsStore>>() {
                let now = store.get().settings.enabled;
                store.set_enabled(!now);
            }
            refresh(app);
        }
        ID_OPEN_SETTINGS => show_main_window(app, Some("settings")),
        ID_OPEN_PLAY => show_main_window(app, Some("play")),
        ID_AUTOSTART => {
            // 押された向きは、チェックの見た目ではなく、実際の登録状態から決める。
            let launcher = app.autolaunch();
            let result = if launcher.is_enabled().unwrap_or(false) { launcher.disable() } else { launcher.enable() };
            if let Err(error) = result {
                eprintln!("[drumclack] ログイン時の起動の切り替えに失敗しました: {error}");
            }
            if let Some(tray) = app.try_state::<Arc<Tray>>() {
                let _ = tray.autostart.set_checked(autostart_enabled(app));
            }
        }
        ID_QUIT => app.exit(0),
        _ => {}
    }
}

/// 状態を定期的に見に行く。見ているのは、音が鳴ったか（初回の印）、オン／オフ、入力監視の許可、言語だけ。
/// キー監視のスレッドには触れない。
pub fn spawn_watcher(app: AppHandle, first_sound: FirstSoundGate) {
    std::thread::spawn(move || loop {
        if let Some(state) = app.try_state::<Arc<AppState>>() {
            watch_first_sound(&state, &first_sound);
        }
        refresh(&app);
        std::thread::sleep(POLL_INTERVAL);
    });
}
