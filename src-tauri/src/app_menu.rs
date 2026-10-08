//! 左上のアプリのメニュー（Mac のみ）。標準の項目（About・Hide・Quit など）は残し、
//! 「設定を開く」「演奏モードを開く」を足す。
//!
//! 切り欠きのある Mac では、メニューバーのアイコンが多いと Drumclack のアイコンが切り欠きの裏に隠れる
//! （OS は隠れたことを知らせない）。アプリが前面のときに出るこのメニューは隠れないので、入口をここにも置く。
//! 文言は、トレイのメニューと同じ日英の対応表（`resident.rs` の `menu_text`）を使う。
//! 組み立てに失敗しても、呼び出し側（トレイの初期化）は続行する。

use tauri::menu::{Menu, MenuEvent, MenuItem, MenuItemKind, PredefinedMenuItem};
use tauri::{AppHandle, Wry};

use crate::resident::MenuText;

/// トレイのメニュー項目の id と重ならない値にする（重なると、同じ押下を両方の処理が受けて二重に動く）。
const ID_OPEN_SETTINGS: &str = "app_open_settings";
const ID_OPEN_PLAY: &str = "app_open_play";
/// 「設定を開く」のショートカット（Mac の標準の設定の操作に合わせる）。
const SHORTCUT_OPEN_SETTINGS: &str = "CmdOrCtrl+,";

/// アプリのメニューに足した項目。言語が変わったときに文言を付け替えるために持つ。
pub struct AppMenu {
    open_settings: MenuItem<Wry>,
    open_play: MenuItem<Wry>,
}

impl AppMenu {
    pub fn set_text(&self, text: &MenuText) {
        crate::tray::log_if_failed("app menu settings text", self.open_settings.set_text(text.open_settings));
        crate::tray::log_if_failed("app menu play text", self.open_play.set_text(text.open_play));
    }
}

/// 足す位置を決める。`is_separator` は標準のアプリ名のメニューの各項目が区切りかどうか。
/// 標準では [About, 区切り, ...] なので、最初の区切りの次に足す。区切りが見つからない
/// （標準の並びが変わった）ときは、末尾に足す（位置決め打ちで範囲外にならないように）。
fn insertion_index(is_separator: &[bool]) -> usize {
    is_separator.iter().position(|s| *s).map(|i| i + 1).unwrap_or(is_separator.len())
}

/// 標準のメニューに項目を足して、アプリのメニューとして設定する。
pub fn setup(app: &AppHandle, text: &MenuText) -> tauri::Result<AppMenu> {
    let menu = Menu::default(app)?;
    let open_settings =
        MenuItem::with_id(app, ID_OPEN_SETTINGS, text.open_settings, true, Some(SHORTCUT_OPEN_SETTINGS))?;
    let open_play = MenuItem::with_id(app, ID_OPEN_PLAY, text.open_play, true, None::<&str>)?;

    // 先頭の項目がアプリ名のメニュー。見つからなければ、標準のままにして足さない。
    if let Some(MenuItemKind::Submenu(app_submenu)) = menu.items()?.into_iter().next() {
        let is_separator: Vec<bool> = app_submenu
            .items()?
            .iter()
            .map(|item| {
                item.as_predefined_menuitem()
                    .map(|p| p.text().map(|t| t.is_empty()).unwrap_or(false))
                    .unwrap_or(false)
            })
            .collect();
        let at = insertion_index(&is_separator);
        app_submenu.insert(&open_settings, at)?;
        app_submenu.insert(&open_play, at + 1)?;
        app_submenu.insert(&PredefinedMenuItem::separator(app)?, at + 2)?;
    } else {
        eprintln!("[drumclack] アプリのメニューの先頭がサブメニューではないため、項目を足しませんでした");
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

#[cfg(test)]
mod tests {
    use super::insertion_index;

    #[test]
    fn inserts_after_the_first_separator_of_the_standard_layout() {
        // [About, 区切り, Services, 区切り, Hide, Hide Others, 区切り, Quit]
        let layout = [false, true, false, true, false, false, true, false];
        assert_eq!(insertion_index(&layout), 2);
    }

    #[test]
    fn falls_back_to_the_end_when_there_is_no_separator() {
        assert_eq!(insertion_index(&[false, false, false]), 3);
        assert_eq!(insertion_index(&[]), 0);
    }
}
