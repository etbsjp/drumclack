//! キーの割り当て。キーの位置から鳴らす音を決める。
//!
//! 位置から音への変換は [`AssignmentTable::sound_for`] の1か所だけで行い、キーの位置はここで使い切る
//! （音の名前より先へ位置を渡さない）。割り当ての検索はキー監視のスレッドで済ませ、
//! 音声コールバックには「音・変種・音量」の要求だけが届く。
//!
//! 優先順位は「キー個別の上書き ＞ グループの指定 ＞ 既定」。既定は組（タイピング用・演奏用）ごとに
//! 持ち、演奏用だけ「キーごとの既定」がある（`default_key_sound`）。

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use crate::audio::AudioEngine;
use crate::dynamics::Dynamics;
use crate::key_position::KeyPosition;
use crate::settings::{Assignments, KeyGroup, Settings, Sound};

/// 割り当ての組。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AssignmentSet {
    Typing,
    // 演奏用への切り替え条件（演奏の画面が最前面）は後の issue。それまでは表を作るだけで使われない。
    #[allow(dead_code)]
    Play,
}

/// キーの位置が属するグループ。`_` を使わず全種類を並べて、キーの種類を足したときに
/// どのグループかを必ず決めさせる。
fn group_of(position: KeyPosition) -> KeyGroup {
    use KeyPosition::*;
    match position {
        KeyA | KeyB | KeyC | KeyD | KeyE | KeyF | KeyG | KeyH | KeyI | KeyJ | KeyK | KeyL | KeyM
        | KeyN | KeyO | KeyP | KeyQ | KeyR | KeyS | KeyT | KeyU | KeyV | KeyW | KeyX | KeyY
        | KeyZ => KeyGroup::Letters,

        Space => KeyGroup::Space,
        Enter | NumpadEnter => KeyGroup::Enter,
        Backspace | Delete => KeyGroup::Delete,

        // 句読点・括弧などの記号と、テンキーの記号（+ - * / . と、機種により現れる = ,）。
        Minus | Equal | BracketLeft | BracketRight | Backslash | Semicolon | Quote | Backquote
        | Comma | Period | Slash | IntlBackslash | IntlRo | IntlYen | NumpadAdd
        | NumpadSubtract | NumpadMultiply | NumpadDivide | NumpadDecimal | NumpadEqual
        | NumpadComma => KeyGroup::Symbols,

        // 上段とテンキーの数字。0 は偶数。
        Digit0 | Digit2 | Digit4 | Digit6 | Digit8 | Numpad0 | Numpad2 | Numpad4 | Numpad6
        | Numpad8 => KeyGroup::DigitsEven,
        Digit1 | Digit3 | Digit5 | Digit7 | Digit9 | Numpad1 | Numpad3 | Numpad5 | Numpad7
        | Numpad9 => KeyGroup::DigitsOdd,

        // Tab・Esc・ファンクションキーと、押しても文字が出ない特殊キー。
        Tab | Escape | F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12 | F13
        | F14 | F15 | F16 | F17 | F18 | F19 | F20 | F21 | F22 | F23 | F24 | PrintScreen | Pause
        | ContextMenu => KeyGroup::Control,

        // 矢印・Home/End・PageUp/PageDown。Insert は編集位置の操作として同じ仲間にする。
        ArrowUp | ArrowDown | ArrowLeft | ArrowRight | Home | End | PageUp | PageDown | Insert => {
            KeyGroup::Navigation
        }

        // 修飾キーと、押しても文字が出ないロック系・日本語の切り替えキー。
        ShiftLeft | ShiftRight | ControlLeft | ControlRight | AltLeft | AltRight | MetaLeft
        | MetaRight | CapsLock | NumLock | ScrollLock | Fn | Convert | NonConvert | KanaMode
        | Lang1 | Lang2 => KeyGroup::Modifiers,
    }
}

/// グループの既定の音。
fn default_group_sound(set: AssignmentSet, group: KeyGroup) -> Sound {
    match set {
        AssignmentSet::Typing => match group {
            KeyGroup::Letters => Sound::HatClosed,
            KeyGroup::Space => Sound::Kick,
            KeyGroup::Enter => Sound::Snare,
            KeyGroup::Delete => Sound::Rim,
            KeyGroup::Symbols => Sound::HatOpen,
            KeyGroup::DigitsEven => Sound::TomHigh,
            KeyGroup::DigitsOdd => Sound::TomLow,
            KeyGroup::Control => Sound::Clap,
            KeyGroup::Navigation | KeyGroup::Modifiers => Sound::None,
        },
        // 演奏用はスペース以外のグループを無音にし、ホームポジション周辺だけ個別に鳴らす。
        AssignmentSet::Play => match group {
            KeyGroup::Space => Sound::Kick,
            _ => Sound::None,
        },
    }
}

/// キーごとの既定の音（演奏用のホームポジション周辺。配置は実機で叩いて決める仮の値）。
fn default_key_sound(set: AssignmentSet, position: KeyPosition) -> Option<Sound> {
    use KeyPosition::*;
    match set {
        AssignmentSet::Typing => None,
        AssignmentSet::Play => match position {
            KeyA => Some(Sound::Kick),
            KeyS => Some(Sound::Snare),
            KeyD => Some(Sound::Rim),
            KeyF => Some(Sound::Clap),
            KeyJ => Some(Sound::HatClosed),
            KeyK => Some(Sound::HatOpen),
            KeyL => Some(Sound::TomHigh),
            Semicolon => Some(Sound::TomLow),
            _ => None,
        },
    }
}

/// 全キーぶんの「鳴らす音」を引き当て済みの表。設定が変わったときだけ作り直す。
pub struct AssignmentTable {
    /// `KeyPosition as usize` で引く（`KeyPosition::ALL` と同じ並び）。
    sounds: Box<[Sound]>,
}

impl AssignmentTable {
    pub fn build(set: AssignmentSet, assignments: &Assignments) -> Self {
        // 設定の文字列のキー名を位置にそろえる（ここまで来る名前は読み取り時に検査済み）。
        let key_overrides: HashMap<KeyPosition, Sound> = assignments
            .keys
            .iter()
            .filter_map(|(name, sound)| KeyPosition::from_code_name(name).map(|p| (p, *sound)))
            .collect();

        let sounds = KeyPosition::ALL
            .iter()
            .map(|&position| resolve(set, position, &key_overrides, &assignments.groups))
            .collect();
        Self { sounds }
    }

    /// キーの位置を、鳴らす音に変える唯一の場所。
    pub fn sound_for(&self, position: KeyPosition) -> Sound {
        self.sounds[position as usize]
    }
}

/// キー個別の上書き ＞ グループの指定 ＞ 既定（キーごとの既定 ＞ グループの既定）。
fn resolve(
    set: AssignmentSet,
    position: KeyPosition,
    key_overrides: &HashMap<KeyPosition, Sound>,
    group_overrides: &BTreeMap<KeyGroup, Sound>,
) -> Sound {
    let group = group_of(position);
    key_overrides
        .get(&position)
        .or_else(|| group_overrides.get(&group))
        .copied()
        .or_else(|| default_key_sound(set, position))
        .unwrap_or_else(|| default_group_sound(set, group))
}

struct Tables {
    typing: AssignmentTable,
    play: AssignmentTable,
}

impl Tables {
    fn build(settings: &Settings) -> Self {
        Self {
            typing: AssignmentTable::build(AssignmentSet::Typing, &settings.typing),
            play: AssignmentTable::build(AssignmentSet::Play, &settings.play),
        }
    }

    fn get(&self, set: AssignmentSet) -> &AssignmentTable {
        match set {
            AssignmentSet::Typing => &self.typing,
            AssignmentSet::Play => &self.play,
        }
    }
}

/// キー監視が読む、今の割り当てとオン／オフ。設定が変わったら [`LiveAssignments::apply`] で差し替える。
pub struct LiveAssignments {
    enabled: AtomicBool,
    tables: RwLock<Arc<Tables>>,
    /// 強弱（打鍵の間隔で音量を決める）。幅は設定の `dynamics` で、`apply` が差し替える。
    pub dynamics: Dynamics,
}

impl LiveAssignments {
    /// 既定の割り当て・オンで始める（設定を読み込んだら `apply` で上書きされる）。
    pub fn new() -> Self {
        let settings = Settings::default();
        Self {
            enabled: AtomicBool::new(settings.enabled),
            tables: RwLock::new(Arc::new(Tables::build(&settings))),
            dynamics: Dynamics::new(settings.dynamics),
        }
    }

    /// 設定の内容を、次の打鍵から効くよう反映する。表は先に作ってから、ポインタだけ差し替える。
    pub fn apply(&self, settings: &Settings) {
        let tables = Arc::new(Tables::build(settings));
        *self.tables.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = tables;
        self.enabled.store(settings.enabled, Ordering::Relaxed);
        self.dynamics.set_width(settings.dynamics);
    }

    /// 今使う割り当ての組。演奏用への切り替え条件（演奏の画面が最前面）は後の issue。
    fn active_set(&self) -> AssignmentSet {
        AssignmentSet::Typing
    }

    /// 新しく押されたキーで鳴らす音の名前。オフのとき・無音のキーのときは `None`（発音を要求しない）。
    /// 監視は続けたまま、鳴らす直前で捨てる。
    pub fn sound_name_for_key(&self, position: KeyPosition) -> Option<&'static str> {
        if !self.enabled.load(Ordering::Relaxed) {
            return None;
        }
        let tables = self.tables.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        match tables.get(self.active_set()).sound_for(position) {
            Sound::None => None,
            sound => Some(sound.name()),
        }
    }
}

/// 音の名前を指定して1回鳴らす（画面の試聴用）。オフ中でも鳴る。`none` は何もしない。
/// 知らない名前はエラー。
pub fn preview(engine: &AudioEngine, sound_name: &str) -> Result<(), String> {
    match Sound::from_name(sound_name) {
        None => Err("知らない音の名前です".to_string()),
        Some(Sound::None) => Ok(()),
        Some(sound) => {
            engine.play_varied(sound.name(), 1.0);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use KeyGroup::*;

    /// 全キーの所属グループ。実装とは別に、設計の表（#9「3. タイピング用の既定」）から書き起こしたもの。
    fn expected_groups() -> Vec<(KeyGroup, &'static [&'static str])> {
        vec![
            (
                Letters,
                &[
                    "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ",
                    "KeyK", "KeyL", "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR", "KeyS", "KeyT",
                    "KeyU", "KeyV", "KeyW", "KeyX", "KeyY", "KeyZ",
                ][..],
            ),
            (Space, &["Space"][..]),
            (Enter, &["Enter", "NumpadEnter"][..]),
            (Delete, &["Backspace", "Delete"][..]),
            (
                Symbols,
                &[
                    "Minus", "Equal", "BracketLeft", "BracketRight", "Backslash", "Semicolon",
                    "Quote", "Backquote", "Comma", "Period", "Slash", "IntlBackslash", "IntlRo",
                    "IntlYen", "NumpadAdd", "NumpadSubtract", "NumpadMultiply", "NumpadDivide",
                    "NumpadDecimal", "NumpadEqual", "NumpadComma",
                ][..],
            ),
            (
                DigitsEven,
                &[
                    "Digit0", "Digit2", "Digit4", "Digit6", "Digit8", "Numpad0", "Numpad2",
                    "Numpad4", "Numpad6", "Numpad8",
                ][..],
            ),
            (
                DigitsOdd,
                &[
                    "Digit1", "Digit3", "Digit5", "Digit7", "Digit9", "Numpad1", "Numpad3",
                    "Numpad5", "Numpad7", "Numpad9",
                ][..],
            ),
            (
                Control,
                &[
                    "Tab", "Escape", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10",
                    "F11", "F12", "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21",
                    "F22", "F23", "F24", "PrintScreen", "Pause", "ContextMenu",
                ][..],
            ),
            (
                Navigation,
                &[
                    "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp",
                    "PageDown", "Insert",
                ][..],
            ),
            (
                Modifiers,
                &[
                    "ShiftLeft", "ShiftRight", "ControlLeft", "ControlRight", "AltLeft", "AltRight",
                    "MetaLeft", "MetaRight", "CapsLock", "NumLock", "ScrollLock", "Fn", "Convert",
                    "NonConvert", "KanaMode", "Lang1", "Lang2",
                ][..],
            ),
        ]
    }

    fn pos(name: &str) -> KeyPosition {
        KeyPosition::from_code_name(name).unwrap_or_else(|| panic!("知らないキー名 {name}"))
    }

    fn typing(assignments: &Assignments) -> AssignmentTable {
        AssignmentTable::build(AssignmentSet::Typing, assignments)
    }

    fn play(assignments: &Assignments) -> AssignmentTable {
        AssignmentTable::build(AssignmentSet::Play, assignments)
    }

    #[test]
    fn every_key_belongs_to_the_group_in_the_design_table() {
        let mut expected: HashMap<&str, KeyGroup> = HashMap::new();
        for (group, names) in expected_groups() {
            for name in names {
                assert!(expected.insert(name, group).is_none(), "{name} が2つのグループに書かれている");
            }
        }
        // 見落とし（表に無いキー）も、表だけにあるキーも無い。
        assert_eq!(expected.len(), KeyPosition::ALL.len());
        for &position in KeyPosition::ALL {
            let name = position.code_name();
            assert_eq!(group_of(position), expected[name], "{name}");
        }
        // 10グループすべてにキーがある。
        for group in KeyGroup::ALL {
            assert!(expected.values().any(|g| g == group), "{}", group.name());
        }
    }

    #[test]
    fn digits_split_by_even_and_odd_on_both_the_top_row_and_the_keypad() {
        for digit in 0..=9 {
            let want = if digit % 2 == 0 { DigitsEven } else { DigitsOdd };
            assert_eq!(group_of(pos(&format!("Digit{digit}"))), want, "Digit{digit}");
            assert_eq!(group_of(pos(&format!("Numpad{digit}"))), want, "Numpad{digit}");
        }
    }

    #[test]
    fn keypad_symbols_and_enter_and_jis_only_keys_land_in_the_agreed_groups() {
        for name in ["NumpadAdd", "NumpadSubtract", "NumpadMultiply", "NumpadDivide", "NumpadDecimal"] {
            assert_eq!(group_of(pos(name)), Symbols, "{name}");
        }
        assert_eq!(group_of(pos("NumpadEnter")), Enter);
        assert_eq!(group_of(pos("Enter")), Enter);
        // JIS 固有: ¥ と ろ は記号、英数・かな・変換・無変換・カタカナひらがなは修飾キー側（無音）。
        for name in ["IntlYen", "IntlRo"] {
            assert_eq!(group_of(pos(name)), Symbols, "{name}");
        }
        for name in ["Lang1", "Lang2", "KanaMode", "Convert", "NonConvert", "CapsLock"] {
            assert_eq!(group_of(pos(name)), Modifiers, "{name}");
        }
    }

    #[test]
    fn typing_defaults_follow_the_design_table() {
        let table = typing(&Assignments::default());
        let cases = [
            ("KeyA", Sound::HatClosed),
            ("Space", Sound::Kick),
            ("Enter", Sound::Snare),
            ("NumpadEnter", Sound::Snare),
            ("Backspace", Sound::Rim),
            ("Delete", Sound::Rim),
            ("Comma", Sound::HatOpen),
            ("NumpadAdd", Sound::HatOpen),
            ("Digit2", Sound::TomHigh),
            ("Numpad8", Sound::TomHigh),
            ("Digit1", Sound::TomLow),
            ("Numpad9", Sound::TomLow),
            ("Tab", Sound::Clap),
            ("Escape", Sound::Clap),
            ("F5", Sound::Clap),
            ("ArrowLeft", Sound::None),
            ("PageDown", Sound::None),
            ("ShiftLeft", Sound::None),
            ("MetaRight", Sound::None),
            ("CapsLock", Sound::None),
            ("Lang1", Sound::None),
        ];
        for (name, sound) in cases {
            assert_eq!(table.sound_for(pos(name)), sound, "{name}");
        }
        // 8音と「無音」がすべて使われている（既定で8音が鳴る）。
        let used: std::collections::HashSet<Sound> =
            KeyPosition::ALL.iter().map(|&p| table.sound_for(p)).collect();
        for sound in Sound::ALL {
            assert!(used.contains(sound), "{}", sound.name());
        }
    }

    #[test]
    fn play_defaults_put_eight_sounds_around_the_home_row_and_the_rest_silent() {
        let table = play(&Assignments::default());
        let cases = [
            ("KeyA", Sound::Kick),
            ("KeyS", Sound::Snare),
            ("KeyD", Sound::Rim),
            ("KeyF", Sound::Clap),
            ("KeyJ", Sound::HatClosed),
            ("KeyK", Sound::HatOpen),
            ("KeyL", Sound::TomHigh),
            ("Semicolon", Sound::TomLow),
            ("Space", Sound::Kick),
        ];
        for (name, sound) in cases {
            assert_eq!(table.sound_for(pos(name)), sound, "{name}");
        }
        let audible = KeyPosition::ALL.iter().filter(|&&p| table.sound_for(p) != Sound::None).count();
        assert_eq!(audible, cases.len(), "ほかのキーは無音");
    }

    #[test]
    fn key_override_beats_group_which_beats_default() {
        let mut assignments = Assignments::default();
        assignments.groups.insert(Letters, Sound::Snare);
        assignments.keys.insert("KeyJ".to_string(), Sound::Kick);
        let table = typing(&assignments);

        assert_eq!(table.sound_for(pos("KeyJ")), Sound::Kick, "キー個別 > グループ");
        assert_eq!(table.sound_for(pos("KeyK")), Sound::Snare, "グループ > 既定");
        assert_eq!(table.sound_for(pos("Space")), Sound::Kick, "ほかのグループは既定のまま");
        assert_eq!(table.sound_for(pos("Enter")), Sound::Snare);
        assert_eq!(table.sound_for(pos("Backspace")), Sound::Rim);

        // キー個別の指定を外せば、グループに従う。グループの指定も外せば、既定に戻る。
        assignments.keys.remove("KeyJ");
        assert_eq!(typing(&assignments).sound_for(pos("KeyJ")), Sound::Snare);
        assignments.groups.remove(&Letters);
        assert_eq!(typing(&assignments).sound_for(pos("KeyJ")), Sound::HatClosed);
    }

    #[test]
    fn group_setting_beats_the_per_key_defaults_of_the_play_set() {
        let mut assignments = Assignments::default();
        assignments.groups.insert(Letters, Sound::Clap);
        let table = play(&assignments);
        assert_eq!(table.sound_for(pos("KeyA")), Sound::Clap, "グループの指定 > キーごとの既定");
        assert_eq!(table.sound_for(pos("Semicolon")), Sound::TomLow, "記号グループは指定が無いので既定");

        assignments.keys.insert("KeyA".to_string(), Sound::None);
        assert_eq!(play(&assignments).sound_for(pos("KeyA")), Sound::None);
    }

    #[test]
    fn silent_override_wins_over_group_and_default() {
        let mut assignments = Assignments::default();
        assignments.keys.insert("KeyA".to_string(), Sound::None);
        let table = typing(&assignments);
        assert_eq!(table.sound_for(pos("KeyA")), Sound::None);
        assert_eq!(table.sound_for(pos("KeyB")), Sound::HatClosed);
    }

    #[test]
    fn live_assignments_report_no_sound_when_disabled_or_silent() {
        let live = LiveAssignments::new();
        assert_eq!(live.sound_name_for_key(pos("KeyA")), Some("hat_closed"));
        assert_eq!(live.sound_name_for_key(pos("ArrowUp")), None, "無音のキー");

        let off = Settings { enabled: false, ..Settings::default() };
        live.apply(&off);
        assert_eq!(live.sound_name_for_key(pos("KeyA")), None, "オフ");
        live.apply(&Settings::default());
        assert_eq!(live.sound_name_for_key(pos("KeyA")), Some("hat_closed"));
    }

    #[test]
    fn preview_plays_once_by_name_and_rejects_unknown_names() {
        let engine = AudioEngine::new(crate::drums::build_free_kit(48_000), 48_000);
        assert!(preview(&engine, "snare").is_ok());
        assert_eq!(engine.drain_requests_for_test().len(), 1);

        assert!(preview(&engine, "none").is_ok());
        assert!(preview(&engine, "cowbell").is_err());
        assert!(preview(&engine, "").is_err());
        assert_eq!(engine.drain_requests_for_test().len(), 0);
    }
}
