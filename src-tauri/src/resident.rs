//! 常駐（メニューバー／トレイ）の判断と部品のうち、Tauri に依らない部分。
//!
//! - アイコンの3状態（オン／オフ／問題あり）と、その絵（RGBA）
//! - 起動時に窓を出すかどうかの判断
//! - メニューの文言（日英の小さな対応表）
//! - 「初めて音が鳴った」を1回だけ設定へ書く仕組み
//!
//! Tauri の窓・メニュー・トレイへのつなぎ込みは `tray.rs`。ここは打鍵の内容を一切扱わない
//! （入るのは「オンか」「許可があるか」「音声デバイスが使えるか」「音が鳴ったか」の真偽だけ）。

use std::sync::atomic::{AtomicBool, Ordering};

use crate::settings::Language;

// ============================================================================
// アイコンの状態
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    On,
    Off,
    /// 入力監視が未許可、または音声デバイスの初期化に失敗している。オン／オフより優先して見せる。
    Problem,
}

/// アイコンの状態を決める。問題があれば、オンでもオフでも「問題あり」。
pub fn icon_state(enabled: bool, input_permission_ok: bool, audio_ok: bool) -> IconState {
    if !input_permission_ok || !audio_ok {
        IconState::Problem
    } else if enabled {
        IconState::On
    } else {
        IconState::Off
    }
}

/// 起動時に窓を出すか。出すのは、初めて音が鳴るまで／入力監視が未許可／音声デバイスの初期化に失敗、のどれか。
/// どれにも当たらなければ、窓は出さずに常駐だけする。
pub fn show_window_at_launch(first_sound_done: bool, input_permission_ok: bool, audio_ok: bool) -> bool {
    !first_sound_done || !input_permission_ok || !audio_ok
}

// ---- アイコンの絵 ------------------------------------------------------------------------------
//
// 調整するときは下の定数を変える（絵そのものは `icon_rgba`）。
//  - Mac はテンプレート画像として使うため、色は無視され、形（透明度）だけが使われる。
//  - Windows は色がそのまま出る。

pub const ICON_SIZE: u32 = 32;
/// 円の外側の半径（ピクセル）。
const ICON_OUTER_RADIUS: f32 = 11.0;
/// 「オフ」の輪の内側の半径。
const ICON_RING_INNER_RADIUS: f32 = 8.0;
/// Windows のトレイで使う色（RGB）。明るい背景でも暗い背景でも見える中間の色にしてある。
const ICON_COLOR_ON: [u8; 3] = [46, 158, 91];
const ICON_COLOR_OFF: [u8; 3] = [128, 128, 128];
const ICON_COLOR_PROBLEM: [u8; 3] = [230, 126, 34];

/// 「問題あり」の円から抜く「！」の位置（ピクセル。x の範囲, y の範囲）。
const BANG_BAR: ((f32, f32), (f32, f32)) = ((14.0, 18.0), (8.0, 18.0));
const BANG_DOT: ((f32, f32), (f32, f32)) = ((14.0, 18.0), (21.0, 25.0));

fn inside(rect: ((f32, f32), (f32, f32)), x: f32, y: f32) -> bool {
    let ((x0, x1), (y0, y1)) = rect;
    x >= x0 && x < x1 && y >= y0 && y < y1
}

/// 状態ごとのアイコン（`ICON_SIZE` 四方の RGBA）。
///  - オン: 塗りつぶした円
///  - オフ: 輪（中が抜けている）
///  - 問題あり: 塗りつぶした円に「！」が抜けている
pub fn icon_rgba(state: IconState) -> Vec<u8> {
    let color = match state {
        IconState::On => ICON_COLOR_ON,
        IconState::Off => ICON_COLOR_OFF,
        IconState::Problem => ICON_COLOR_PROBLEM,
    };
    let center = ICON_SIZE as f32 / 2.0;
    let mut pixels = Vec::with_capacity((ICON_SIZE * ICON_SIZE * 4) as usize);
    for row in 0..ICON_SIZE {
        for col in 0..ICON_SIZE {
            let (x, y) = (col as f32 + 0.5, row as f32 + 0.5);
            let distance = ((x - center).powi(2) + (y - center).powi(2)).sqrt();
            // 縁を1ピクセルほどなだらかにする。
            let mut coverage = (ICON_OUTER_RADIUS - distance + 0.5).clamp(0.0, 1.0);
            match state {
                IconState::Off => {
                    coverage = coverage.min((distance - ICON_RING_INNER_RADIUS + 0.5).clamp(0.0, 1.0));
                }
                IconState::Problem => {
                    if inside(BANG_BAR, x, y) || inside(BANG_DOT, x, y) {
                        coverage = 0.0;
                    }
                }
                IconState::On => {}
            }
            pixels.extend_from_slice(&[color[0], color[1], color[2], (coverage * 255.0).round() as u8]);
        }
    }
    pixels
}

// ============================================================================
// メニューの文言（日英）
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuLang {
    Ja,
    En,
}

/// 設定の言語（auto / ja / en）から、メニューの言語を決める。`auto` は OS の言語で、
/// 日本語なら日本語、それ以外は英語（画面側の `resolveAuto` と同じ規則）。
pub fn resolve_menu_lang(language: Language, os_locale: Option<&str>) -> MenuLang {
    match language {
        Language::Ja => MenuLang::Ja,
        Language::En => MenuLang::En,
        Language::Auto => {
            let is_japanese = os_locale.map(|l| l.to_ascii_lowercase().starts_with("ja")).unwrap_or(false);
            if is_japanese {
                MenuLang::Ja
            } else {
                MenuLang::En
            }
        }
    }
}

/// メニュー5項目の文言。
pub struct MenuText {
    pub enabled: &'static str,
    pub open_settings: &'static str,
    pub open_play: &'static str,
    pub launch_at_login: &'static str,
    pub quit: &'static str,
}

pub fn menu_text(lang: MenuLang) -> &'static MenuText {
    static JA: MenuText = MenuText {
        enabled: "オン",
        open_settings: "設定を開く",
        open_play: "演奏モードを開く",
        launch_at_login: "ログイン時に起動",
        quit: "終了",
    };
    static EN: MenuText = MenuText {
        enabled: "On",
        open_settings: "Open Settings",
        open_play: "Open Play Mode",
        launch_at_login: "Launch at Login",
        quit: "Quit",
    };
    match lang {
        MenuLang::Ja => &JA,
        MenuLang::En => &EN,
    }
}

// ============================================================================
// 「初めて音が鳴った」を1回だけ書く
// ============================================================================

/// 「音が鳴った」を見張り、最初の1回だけ `on_first` を呼ぶ。2回目以降は何もしない。
/// すでに `first_sound_done` が真で起動したときは、最初から何もしない（ファイルを書き直さない）。
pub struct FirstSoundGate {
    done: AtomicBool,
    on_first: Box<dyn Fn() + Send + Sync>,
}

impl FirstSoundGate {
    pub fn new(already_done: bool, on_first: impl Fn() + Send + Sync + 'static) -> Self {
        Self { done: AtomicBool::new(already_done), on_first: Box::new(on_first) }
    }

    /// 今までに音が鳴ったか（`played`）を渡す。鳴っていて、まだ書いていなければ1回だけ書く。
    pub fn observe(&self, played: bool) {
        if played && !self.done.swap(true, Ordering::SeqCst) {
            (self.on_first)();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioEngine;
    use crate::drums::build_free_kit;
    use crate::settings_store::{load, SettingsStore};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "drumclack-resident-test-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn engine() -> Arc<AudioEngine> {
        Arc::new(AudioEngine::new(build_free_kit(48_000), 48_000))
    }

    fn settings_file(dir: &TempDir) -> PathBuf {
        dir.0.join("settings.json")
    }

    // ---- オン／オフの保存と読み直し ----

    #[test]
    fn on_off_survives_saving_and_reopening() {
        let dir = TempDir::new();
        let store = SettingsStore::open(Some(dir.0.clone()), Some(engine()));
        assert!(store.get().settings.enabled, "既定はオン");

        store.set_enabled(false);
        drop(store);
        let reopened = SettingsStore::open(Some(dir.0.clone()), Some(engine()));
        assert!(!reopened.get().settings.enabled, "オフにして保存→読み直しでもオフのまま");
        assert!(!load(&dir.0).settings.enabled, "ファイルにもオフが書かれている");

        reopened.set_enabled(true);
        drop(reopened);
        let again = SettingsStore::open(Some(dir.0.clone()), Some(engine()));
        assert!(again.get().settings.enabled, "オンに戻して保存→読み直しでオン");
    }

    #[test]
    fn menu_toggle_changes_what_the_key_listener_reads() {
        // メニューのオン／オフは、キー監視が読む値（LiveAssignments）に届く。監視そのものは止めない。
        use crate::assignment::LiveAssignments;
        use crate::key_position::KeyPosition;
        let live = Arc::new(LiveAssignments::new());
        let store = SettingsStore::open_with(None, Some(engine()), live.clone());
        let key = KeyPosition::from_code_name("KeyA").expect("KeyA は知っているキー");
        assert!(live.sound_name_for_key(key).is_some());
        store.set_enabled(false);
        assert!(live.sound_name_for_key(key).is_none(), "オフの間は音の名前が返らない（発音の直前で捨てる）");
        store.set_enabled(true);
        assert!(live.sound_name_for_key(key).is_some());
    }

    // ---- first_sound_done を1回だけ書く ----

    /// 本番と同じ配線（ゲート → ストアの mark_first_sound_done）で、書き込みの回数を数える。
    fn gate_writing_to(store: Arc<SettingsStore>, already_done: bool) -> FirstSoundGate {
        FirstSoundGate::new(already_done, move || {
            store.mark_first_sound_done();
        })
    }

    #[test]
    fn first_sound_done_is_written_once_when_the_first_sound_plays() {
        let dir = TempDir::new();
        let store = Arc::new(SettingsStore::open(Some(dir.0.clone()), Some(engine())));
        let gate = gate_writing_to(store.clone(), false);

        gate.observe(false);
        assert!(!settings_file(&dir).exists(), "鳴る前は何も書かない");
        assert!(!store.get().settings.first_sound_done);

        gate.observe(true);
        assert!(load(&dir.0).settings.first_sound_done, "最初に鳴ったとき、真で保存される");

        // 2回目以降は書かない: ファイルを消しておき、何度鳴っても作り直されないことで確かめる。
        fs::remove_file(settings_file(&dir)).unwrap();
        gate.observe(true);
        gate.observe(true);
        assert!(!settings_file(&dir).exists(), "2回目以降の発音ではファイルを書かない");
    }

    #[test]
    fn nothing_is_written_when_first_sound_was_already_done() {
        let dir = TempDir::new();
        let store = Arc::new(SettingsStore::open(Some(dir.0.clone()), Some(engine())));
        store.mark_first_sound_done();
        fs::remove_file(settings_file(&dir)).unwrap();

        // 保存済みの値を読んで起動し直したとき（already_done = true）は、鳴っても書かない。
        let gate = gate_writing_to(store, true);
        gate.observe(true);
        assert!(!settings_file(&dir).exists());
    }

    #[test]
    fn gate_calls_back_exactly_once_however_often_it_is_told() {
        use std::sync::atomic::AtomicUsize;
        let count = Arc::new(AtomicUsize::new(0));
        let counter = count.clone();
        let gate = FirstSoundGate::new(false, move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        for _ in 0..5 {
            gate.observe(false);
        }
        assert_eq!(count.load(Ordering::SeqCst), 0);
        for _ in 0..5 {
            gate.observe(true);
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    // ---- アイコンの状態・窓を出す条件 ----

    #[test]
    fn icon_state_follows_enabled_and_problems_with_problem_first() {
        assert_eq!(icon_state(true, true, true), IconState::On);
        assert_eq!(icon_state(false, true, true), IconState::Off);
        assert_eq!(icon_state(true, false, true), IconState::Problem, "未許可");
        assert_eq!(icon_state(true, true, false), IconState::Problem, "音声デバイス失敗");
        assert_eq!(icon_state(false, false, false), IconState::Problem, "オフでも問題は見せる");
    }

    #[test]
    fn window_is_shown_at_launch_only_in_the_three_agreed_cases() {
        assert!(!show_window_at_launch(true, true, true), "全部そろっていれば窓は出さない");
        assert!(show_window_at_launch(false, true, true), "初めて音が鳴るまで");
        assert!(show_window_at_launch(true, false, true), "入力監視が未許可");
        assert!(show_window_at_launch(true, true, false), "音声デバイスの初期化に失敗");
    }

    #[test]
    fn the_three_icons_are_really_different_pictures() {
        let on = icon_rgba(IconState::On);
        let off = icon_rgba(IconState::Off);
        let problem = icon_rgba(IconState::Problem);
        let expected_len = (ICON_SIZE * ICON_SIZE * 4) as usize;
        assert_eq!(on.len(), expected_len);
        assert_ne!(on, off);
        assert_ne!(on, problem);
        assert_ne!(off, problem);

        let alpha = |pixels: &[u8], x: u32, y: u32| pixels[((y * ICON_SIZE + x) * 4 + 3) as usize];
        let c = ICON_SIZE / 2;
        assert_eq!(alpha(&on, c, c), 255, "オンは中心まで塗られている");
        assert_eq!(alpha(&off, c, c), 0, "オフは輪で、中心が抜けている");
        assert_eq!(alpha(&problem, c, c - 4), 0, "問題ありは中心の上に「！」が抜けている");
        assert_eq!(alpha(&on, 0, 0), 0, "角は透明");
    }

    // ---- メニューの文言 ----

    #[test]
    fn menu_language_follows_the_setting_and_the_os_locale_for_auto() {
        assert_eq!(resolve_menu_lang(Language::Ja, Some("en-US")), MenuLang::Ja);
        assert_eq!(resolve_menu_lang(Language::En, Some("ja-JP")), MenuLang::En);
        assert_eq!(resolve_menu_lang(Language::Auto, Some("ja-JP")), MenuLang::Ja);
        assert_eq!(resolve_menu_lang(Language::Auto, Some("JA")), MenuLang::Ja);
        assert_eq!(resolve_menu_lang(Language::Auto, Some("fr-FR")), MenuLang::En);
        assert_eq!(resolve_menu_lang(Language::Auto, None), MenuLang::En);
    }

    #[test]
    fn menu_texts_exist_in_both_languages_and_differ() {
        let ja = menu_text(MenuLang::Ja);
        let en = menu_text(MenuLang::En);
        let pairs = [
            (ja.enabled, en.enabled),
            (ja.open_settings, en.open_settings),
            (ja.open_play, en.open_play),
            (ja.launch_at_login, en.launch_at_login),
            (ja.quit, en.quit),
        ];
        for (j, e) in pairs {
            assert!(!j.is_empty() && !e.is_empty());
            assert_ne!(j, e, "日英で同じ文言になっている: {j}");
        }
    }
}
