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
use crate::state::AppState;

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
const ICON_COLOR_PROBLEM: [u8; 3] = [200, 95, 10];

/// 「問題あり」の円から抜く「！」の位置（ピクセル。x の範囲, y の範囲）。
/// 幅6pxの太さにして、小さい表示でもオンの円と取り違えにくくしてある。
const BANG_BAR: ((f32, f32), (f32, f32)) = ((13.0, 19.0), (7.0, 18.0));
const BANG_DOT: ((f32, f32), (f32, f32)) = ((13.0, 19.0), (21.0, 26.0));

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
    /// トレイのツールチップ（状態つき）。
    pub tooltip_on: &'static str,
    pub tooltip_off: &'static str,
    pub tooltip_problem: &'static str,
}

impl MenuText {
    pub fn tooltip(&self, state: IconState) -> &'static str {
        match state {
            IconState::On => self.tooltip_on,
            IconState::Off => self.tooltip_off,
            IconState::Problem => self.tooltip_problem,
        }
    }
}

pub fn menu_text(lang: MenuLang) -> &'static MenuText {
    static JA: MenuText = MenuText {
        enabled: "オン",
        open_settings: "設定を開く",
        open_play: "演奏モードを開く",
        launch_at_login: "ログイン時に起動",
        quit: "終了",
        tooltip_on: "drumclack: オン",
        tooltip_off: "drumclack: オフ",
        tooltip_problem: "drumclack: 問題あり（設定を開いて確認）",
    };
    static EN: MenuText = MenuText {
        enabled: "On",
        open_settings: "Open Settings",
        open_play: "Open Play Mode",
        launch_at_login: "Launch at Login",
        quit: "Quit",
        tooltip_on: "drumclack: On",
        tooltip_off: "drumclack: Off",
        tooltip_problem: "drumclack: Problem (open Settings to check)",
    };
    match lang {
        MenuLang::Ja => &JA,
        MenuLang::En => &EN,
    }
}

// ============================================================================
// 「初めて音が鳴った」を1回だけ書く
// ============================================================================

/// 「音が鳴った」を見張り、最初の1回だけ `on_first`（保存）を呼ぶ。保存に成功したら、2回目以降は何もしない。
/// 保存に失敗した（`on_first` が `false` を返した）ときは、次に見たときにもう一度試す。
/// すでに `first_sound_done` が真で起動したときは、最初から何もしない（ファイルを書き直さない）。
pub struct FirstSoundGate {
    done: AtomicBool,
    on_first: Box<dyn Fn() -> bool + Send + Sync>,
}

impl FirstSoundGate {
    /// `on_first` は保存して、成功したかを返す。
    pub fn new(already_done: bool, on_first: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self { done: AtomicBool::new(already_done), on_first: Box::new(on_first) }
    }

    /// 今までに音が鳴ったか（`played`）を渡す。鳴っていて、まだ保存できていなければ保存を試みる。
    pub fn observe(&self, played: bool) {
        if played && !self.done.swap(true, Ordering::SeqCst) && !(self.on_first)() {
            self.done.store(false, Ordering::SeqCst);
        }
    }
}

/// 見張り1回分。エンジンが実際に音を鳴らした記録があるかを見て、ゲートへ渡す。
/// 見るのは「鳴ったか」の真偽だけ（打鍵の内容・時刻は扱わない）。
pub fn watch_first_sound(state: &AppState, gate: &FirstSoundGate) {
    gate.observe(state.last_play_ms().is_some());
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
        FirstSoundGate::new(already_done, move || !store.mark_first_sound_done().save_failed)
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
            true
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

    #[test]
    fn a_failed_save_is_retried_on_the_next_look_until_it_succeeds_then_never_again() {
        use std::sync::atomic::AtomicUsize;
        let attempts = Arc::new(AtomicUsize::new(0));
        let counter = attempts.clone();
        // 1回目・2回目は保存に失敗し、3回目で成功する。
        let gate = FirstSoundGate::new(false, move || counter.fetch_add(1, Ordering::SeqCst) + 1 >= 3);
        gate.observe(true);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        gate.observe(true);
        assert_eq!(attempts.load(Ordering::SeqCst), 2, "失敗したので次の周期で再試行する");
        gate.observe(true);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        for _ in 0..3 {
            gate.observe(true);
        }
        assert_eq!(attempts.load(Ordering::SeqCst), 3, "成功したあとは書かない");
    }

    #[test]
    fn with_the_real_store_a_save_that_cannot_be_written_is_reported_as_failure() {
        // 保存先が無い設定（書けない）では mark_first_sound_done の結果が失敗になり、ゲートは再試行を続ける。
        use std::sync::atomic::AtomicUsize;
        let store = Arc::new(SettingsStore::open(None, Some(engine())));
        let attempts = Arc::new(AtomicUsize::new(0));
        let counter = attempts.clone();
        let gate = FirstSoundGate::new(false, move || {
            counter.fetch_add(1, Ordering::SeqCst);
            !store.mark_first_sound_done().save_failed
        });
        gate.observe(true);
        gate.observe(true);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    // ---- 見張り1回分（AppState を受ける） ----

    #[test]
    fn one_watch_pass_writes_first_sound_done_only_after_a_sound_really_played() {
        use crate::audio::Mixer;
        use crate::state::AudioInitStatus;
        let dir = TempDir::new();
        let engine = engine();
        let state = AppState::new(AudioInitStatus::Ok { sample_rate: 48_000 }, Some(engine.clone()));
        let store = Arc::new(SettingsStore::open(Some(dir.0.clone()), Some(engine.clone())));
        let gate = gate_writing_to(store, false);

        // 要求を積んだだけ（まだ鳴っていない）では書かない。
        watch_first_sound(&state, &gate);
        engine.play("kick", 0, 1.0);
        watch_first_sound(&state, &gate);
        assert!(!settings_file(&dir).exists(), "音声コールバックが受理する前は書かない");

        // 実際に鳴ったら、次の見張りで書く。
        let mut mixer = Mixer::new(engine.clone());
        let mut buf = vec![0.0_f32; 128];
        mixer.fill_output(&mut buf, 1);
        watch_first_sound(&state, &gate);
        assert!(load(&dir.0).settings.first_sound_done);

        // その後は書かない。
        fs::remove_file(settings_file(&dir)).unwrap();
        watch_first_sound(&state, &gate);
        assert!(!settings_file(&dir).exists());
    }

    // ---- 画面に権限を出さない・自動で登録しない ----

    #[test]
    fn the_screen_capability_lists_only_the_six_commands_and_no_plugin_permission() {
        const CAPABILITY: &str = include_str!("../capabilities/default.json");
        let value: serde_json::Value = serde_json::from_str(CAPABILITY).unwrap();
        let mut permissions: Vec<&str> =
            value["permissions"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
        permissions.sort();
        assert_eq!(
            permissions,
            [
                "allow-get-settings",
                "allow-get-status",
                "allow-open-input-monitoring-settings",
                "allow-preview-sound",
                "allow-restart-app",
                "allow-update-settings"
            ]
        );
        assert!(!CAPABILITY.contains("autostart"), "ログイン時の起動の権限を画面に出していない");
        assert!(!CAPABILITY.contains("single-instance"));
    }

    #[test]
    fn autostart_is_never_registered_at_startup_only_from_the_menu_handler() {
        const MAIN_RS: &str = include_str!("main.rs");
        const TRAY_RS: &str = include_str!("tray.rs");
        // main.rs（起動の配線）は、登録も解除も呼ばない。
        for call in [".enable()", ".disable()", "autolaunch()"] {
            assert!(!MAIN_RS.contains(call), "main.rs が {call} を呼んでいる");
        }
        // tray.rs でも、登録・解除はメニューの処理（handle_menu_event）より後ろにしか無い。
        let handler = TRAY_RS.find("fn handle_menu_event").expect("メニューの処理がある");
        for call in [".enable()", ".disable()"] {
            let first = TRAY_RS.find(call).expect("メニューから切り替える処理がある");
            assert!(first > handler, "メニュー以外（起動時など）で {call} を呼んでいる");
        }
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

    #[test]
    fn tooltips_name_the_state_in_both_languages() {
        for lang in [MenuLang::Ja, MenuLang::En] {
            let text = menu_text(lang);
            let all = [
                text.tooltip(IconState::On),
                text.tooltip(IconState::Off),
                text.tooltip(IconState::Problem),
            ];
            assert_ne!(all[0], all[1]);
            assert_ne!(all[0], all[2]);
            assert_ne!(all[1], all[2]);
        }
        assert_ne!(menu_text(MenuLang::Ja).tooltip(IconState::On), menu_text(MenuLang::En).tooltip(IconState::On));
    }

    #[test]
    fn the_problem_mark_is_wide_enough_to_tell_from_the_on_circle() {
        // 中心の行で、円の中の透明な画素（「！」の棒）が6px以上あること。
        let pixels = icon_rgba(IconState::Problem);
        let row = ICON_SIZE / 2 - 4;
        let transparent = (0..ICON_SIZE).filter(|&x| pixels[((row * ICON_SIZE + x) * 4 + 3) as usize] == 0).count();
        let outside = (0..ICON_SIZE).filter(|&x| (x as f32 + 0.5 - 16.0).abs() > ICON_OUTER_RADIUS).count();
        assert!(transparent - outside >= 6, "「！」の太さが足りない: {}", transparent - outside);
    }
}
