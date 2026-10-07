//! 左上のアプリのメニュー（Mac のみ）。標準の項目（About・Hide・Quit など）は残し、
//! 「設定を開く」「演奏モードを開く」を足す。
//!
//! 切り欠きのある Mac では、メニューバーのアイコンが多いと Drumclack のアイコンが切り欠きの裏に隠れる
//! （OS は隠れたことを知らせない）。アプリが前面のときに出るこのメニューは隠れないので、入口をここにも置く。
//! 文言は、トレイのメニューと同じ日英の対応表（`resident.rs` の `menu_text`）を使う。

use tauri::menu::{Menu, MenuEvent, MenuItem, MenuItemKind, PredefinedMenuItem};
use tauri::{AppHandle, Wry};

use crate::resident::MenuText;

/// トレイのメニュー項目の id と重ならない値にする（重なると、同じ押下を両方の処理が受けて二重に動く）。
const ID_OPEN_SETTINGS: &str = "app_open_settings";
const ID_OPEN_PLAY: &str = "app_open_play";

/// アプリのメニューに足した項目。言語が変わったときに文言を付け替えるために持つ。
pub struct AppMenu {
    open_settings: MenuItem<Wry>,
    open_play: MenuItem<Wry>,
}

impl AppMenu {
    pub fn set_text(&self, text: &MenuText) {
        let _ = self.open_settings.set_text(text.open_settings);
        let _ = self.open_play.set_text(text.open_play);
    }
}

/// 標準のメニューに項目を足して、アプリのメニューとして設定する。
pub fn setup(app: &AppHandle, text: &MenuText) -> tauri::Result<AppMenu> {
    let menu = Menu::default(app)?;
    let open_settings = MenuItem::with_id(app, ID_OPEN_SETTINGS, text.open_settings, true, None::<&str>)?;
    let open_play = MenuItem::with_id(app, ID_OPEN_PLAY, text.open_play, true, None::<&str>)?;

    // 先頭の項目がアプリ名のメニュー。標準では [About, 区切り, Services, ...] の並びなので、
    // 区切りの次（3番目）から「設定を開く」「演奏モードを開く」と区切りを差し込む。
    if let Some(MenuItemKind::Submenu(app_submenu)) = menu.items()?.into_iter().next() {
        app_submenu.insert(&open_settings, 2)?;
        app_submenu.insert(&open_play, 3)?;
        app_submenu.insert(&PredefinedMenuItem::separator(app)?, 4)?;
    }
    app.set_menu(menu)?;
    Ok(AppMenu { open_settings, open_play })
}

/// アプリのメニューの押下を受ける。ここで扱うのは足した2項目だけ（標準の項目は OS が処理する）。
pub fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        ID_OPEN_SETTINGS => crate::tray::show_main_window(app, Some("settings")),
        ID_OPEN_PLAY => crate::tray::show_main_window(app, Some("play")),
        _ => {}
    }
}
