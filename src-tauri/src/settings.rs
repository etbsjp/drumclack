//! 設定の型と、寛容な読み取り・書き出し。ファイルには触らない（ファイルは `settings_store`）。
//!
//! 設定に入るのは利用者が選んだ値と、割り当ての上書きだけ。押されたキーの履歴は持たない。
//! `typing.keys` / `play.keys` のキーは、利用者が選んだ `code` 名の文字列であり、
//! キーの位置を表す型（`key_position::KeyPosition`）とは別物として扱う。

use std::collections::BTreeMap;

use serde::{Serialize, Serializer};
use serde_json::Value;

use crate::key_position;

pub const SETTINGS_VERSION: u32 = 1;
/// 知っているキット名。範囲外の名前は既定のキットに戻す。
pub const KNOWN_KITS: &[&str] = &["808"];
pub const DEFAULT_KIT: &str = "808";
pub const DEFAULT_VOLUME: f32 = 0.8;
pub const DEFAULT_DYNAMICS: f32 = 0.6;

/// 名前（設定ファイル内の文字列）と列挙値の対応を1か所で持つ。
macro_rules! named_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($variant),*
        }

        #[allow(dead_code)]
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),*];

            /// 設定ファイル内の名前。
            pub fn name(self) -> &'static str {
                match self {
                    $($name::$variant => $text),*
                }
            }

            pub fn from_name(text: &str) -> Option<Self> {
                match text {
                    $($text => Some($name::$variant),)*
                    _ => None,
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.name())
            }
        }
    };
}

named_enum! {
    /// 割り当て先。キットの8音と「無音」。
    Sound {
        Kick => "kick",
        Snare => "snare",
        HatClosed => "hat_closed",
        HatOpen => "hat_open",
        Clap => "clap",
        Rim => "rim",
        TomLow => "tom_low",
        TomHigh => "tom_high",
        None => "none",
    }
}

named_enum! {
    /// 割り当ての単位となるキーのグループ。
    KeyGroup {
        Letters => "letters",
        Space => "space",
        Enter => "enter",
        Delete => "delete",
        Symbols => "symbols",
        DigitsEven => "digits_even",
        DigitsOdd => "digits_odd",
        Control => "control",
        Navigation => "navigation",
        Modifiers => "modifiers",
    }
}

named_enum! {
    Language {
        Auto => "auto",
        Ja => "ja",
        En => "en",
    }
}

named_enum! {
    /// 画面に描くキーボード配列。`Auto` は「利用者がまだ選んでいない」状態で、
    /// 実際に JIS か US のどちらで描くかは画面が OS の言語から決める（保存するのは選んだ値だけ）。
    KeyboardLayout {
        Auto => "auto",
        Jis => "jis",
        Us => "us",
    }
}

/// 割り当ての1組（タイピング用か演奏用）。入るのは既定から変えた分だけ。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Assignments {
    /// グループごとの音。既定と同じ値は入れない。
    pub groups: BTreeMap<KeyGroup, Sound>,
    /// キー個別の上書き。キーは `code` 名（`KeyJ` `Numpad5` など）の文字列。
    pub keys: BTreeMap<String, Sound>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    pub version: u32,
    pub kit: String,
    pub enabled: bool,
    /// 全体の音量。0.0〜1.0。
    pub volume: f32,
    /// 強弱の幅。0.0〜1.0。
    pub dynamics: f32,
    pub language: Language,
    pub keyboard_layout: KeyboardLayout,
    pub first_sound_done: bool,
    pub typing: Assignments,
    pub play: Assignments,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            kit: DEFAULT_KIT.to_string(),
            enabled: true,
            volume: DEFAULT_VOLUME,
            dynamics: DEFAULT_DYNAMICS,
            language: Language::Auto,
            keyboard_layout: KeyboardLayout::Auto,
            first_sound_done: false,
            typing: Assignments::default(),
            play: Assignments::default(),
        }
    }
}

impl Settings {
    /// 設定の JSON 文字列を読む。JSON として読めない・最上位がオブジェクトでないときは `None`
    /// （呼び出し側が「壊れている」として扱う）。
    #[cfg(test)]
    pub fn from_json_str(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        Self::from_value(&value)
    }

    /// 寛容に読む。知らない項目・音の名前・キーの名前は無視してその項目だけ既定にし、
    /// 範囲外の数値は丸める。最上位がオブジェクトでなければ `None`。
    /// 既定の設定に `value` を重ねたものと同じ（読み取りと更新で規則を分けない）。
    pub fn from_value(value: &Value) -> Option<Self> {
        Self::default().merged(value)
    }

    /// 今の設定に、送られた項目だけを重ねた新しい設定を返す。送られなかった項目は今の値のまま。
    /// 最上位がオブジェクトでなければ `None`。
    ///
    /// - 値の型が違う・知らない名前・範囲外の数値は、その項目だけ「今の値のまま」（数値は端へ丸める）。
    /// - 割り当て（`typing` / `play` の `groups` / `keys`）は項目単位で重ねる。値が `null` なら、
    ///   その項目の上書きを消す（グループに従う・既定に戻す）。
    /// - `version` は送られても無視する（書き出すのはこの版の形式）。
    pub fn merged(&self, patch: &Value) -> Option<Self> {
        let root = patch.as_object()?;

        let kit = root
            .get("kit")
            .and_then(Value::as_str)
            .filter(|name| KNOWN_KITS.contains(name))
            .map(str::to_string)
            .unwrap_or_else(|| self.kit.clone());

        Some(Self {
            version: SETTINGS_VERSION,
            kit,
            enabled: read_bool(root.get("enabled"), self.enabled),
            volume: read_unit_range(root.get("volume"), self.volume),
            dynamics: read_unit_range(root.get("dynamics"), self.dynamics),
            language: read_name(root.get("language"), Language::from_name, self.language),
            keyboard_layout: read_name(
                root.get("keyboard_layout"),
                KeyboardLayout::from_name,
                self.keyboard_layout,
            ),
            first_sound_done: read_bool(root.get("first_sound_done"), self.first_sound_done),
            typing: self.typing.merged(root.get("typing")),
            play: self.play.merged(root.get("play")),
        })
    }

    /// 書き出し用の JSON（人が読めるよう整形する）。
    pub fn to_json_string(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("設定は JSON に直せる");
        text.push('\n');
        text
    }
}

fn read_bool(value: Option<&Value>, default: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(default)
}

/// 0.0〜1.0 の数値。範囲外は端へ丸め、数値でなければ既定。
fn read_unit_range(value: Option<&Value>, default: f32) -> f32 {
    match value.and_then(Value::as_f64) {
        Some(number) if number.is_finite() => number.clamp(0.0, 1.0) as f32,
        _ => default,
    }
}

fn read_name<T>(value: Option<&Value>, parse: fn(&str) -> Option<T>, default: T) -> T {
    value.and_then(Value::as_str).and_then(parse).unwrap_or(default)
}

impl Assignments {
    /// 送られた割り当てを項目単位で重ねる（`Settings::merged` から呼ぶ）。
    /// 値が `null` の項目は上書きを消す。知らないグループ名・キー名・音の名前の項目は無視する。
    fn merged(&self, patch: Option<&Value>) -> Assignments {
        let mut merged = self.clone();
        let Some(section) = patch.and_then(Value::as_object) else {
            return merged;
        };

        if let Some(groups) = section.get("groups").and_then(Value::as_object) {
            for (group_name, sound_name) in groups {
                let Some(group) = KeyGroup::from_name(group_name) else {
                    continue;
                };
                if sound_name.is_null() {
                    merged.groups.remove(&group);
                } else if let Some(sound) = sound_name.as_str().and_then(Sound::from_name) {
                    merged.groups.insert(group, sound);
                }
            }
        }

        if let Some(keys) = section.get("keys").and_then(Value::as_object) {
            for (code_name, sound_name) in keys {
                if !key_position::is_known_code_name(code_name) {
                    continue;
                }
                if sound_name.is_null() {
                    merged.keys.remove(code_name);
                } else if let Some(sound) = sound_name.as_str().and_then(Sound::from_name) {
                    merged.keys.insert(code_name.clone(), sound);
                }
            }
        }

        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 設定 JSON の見本。画面側（08 以降）のテストも同じファイルを読む。
    pub const SAMPLE_JSON: &str = include_str!("../../tests/fixtures/settings.sample.json");

    fn sample_expected() -> Settings {
        let mut settings = Settings {
            kit: "808".to_string(),
            enabled: false,
            volume: 0.5,
            dynamics: 0.25,
            language: Language::En,
            keyboard_layout: KeyboardLayout::Us,
            first_sound_done: true,
            ..Settings::default()
        };
        settings.typing.groups.insert(KeyGroup::Letters, Sound::Snare);
        settings.typing.groups.insert(KeyGroup::Modifiers, Sound::Clap);
        settings.typing.keys.insert("KeyJ".to_string(), Sound::Kick);
        settings.typing.keys.insert("Numpad5".to_string(), Sound::None);
        settings.play.groups.insert(KeyGroup::Navigation, Sound::TomLow);
        settings.play.keys.insert("Space".to_string(), Sound::HatOpen);
        settings
    }

    #[test]
    fn sample_file_reads_as_expected_and_survives_a_write_and_reread() {
        let parsed = Settings::from_json_str(SAMPLE_JSON).expect("見本は読める");
        assert_eq!(parsed, sample_expected());

        let reread = Settings::from_json_str(&parsed.to_json_string()).expect("書いたものは読める");
        assert_eq!(reread, parsed);
    }

    #[test]
    fn keyboard_layout_starts_unchosen_and_keeps_reading_jis_and_us() {
        // 選ぶ前は auto。設定ファイルに項目が無い（#13 以前の形）ときも auto。
        assert_eq!(Settings::default().keyboard_layout, KeyboardLayout::Auto);
        assert_eq!(Settings::from_json_str("{}").unwrap().keyboard_layout, KeyboardLayout::Auto);
        let written: Value = serde_json::from_str(&Settings::default().to_json_string()).unwrap();
        assert_eq!(written["keyboard_layout"], json!("auto"));

        // 既に jis / us と書かれたファイルは、そのまま読める（auto に戻さない）。
        for (name, expected) in
            [("jis", KeyboardLayout::Jis), ("us", KeyboardLayout::Us), ("auto", KeyboardLayout::Auto)]
        {
            let settings = Settings::from_value(&json!({ "keyboard_layout": name })).unwrap();
            assert_eq!(settings.keyboard_layout, expected, "{name}");
        }

        // 知らない値はその項目だけ「今の値のまま」。選んだあとに auto へ戻す更新も通る。
        let chosen = Settings::from_value(&json!({"keyboard_layout": "us"})).unwrap();
        assert_eq!(chosen.merged(&json!({"keyboard_layout": "dvorak"})).unwrap().keyboard_layout, KeyboardLayout::Us);
        assert_eq!(chosen.merged(&json!({"keyboard_layout": "auto"})).unwrap().keyboard_layout, KeyboardLayout::Auto);
    }

    #[test]
    fn untouched_settings_are_written_with_empty_groups_and_keys() {
        let written: Value = serde_json::from_str(&Settings::default().to_json_string()).unwrap();
        for section in ["typing", "play"] {
            assert_eq!(written[section]["groups"], json!({}), "{section}.groups");
            assert_eq!(written[section]["keys"], json!({}), "{section}.keys");
        }
    }

    #[test]
    fn written_file_has_only_the_agreed_fields() {
        // 押されたキーの履歴にあたる項目が紛れ込んだら落とす。
        let written: Value = serde_json::from_str(&sample_expected().to_json_string()).unwrap();
        let mut top: Vec<&str> = written.as_object().unwrap().keys().map(String::as_str).collect();
        top.sort_unstable();
        assert_eq!(
            top,
            [
                "dynamics",
                "enabled",
                "first_sound_done",
                "keyboard_layout",
                "kit",
                "language",
                "play",
                "typing",
                "version",
                "volume"
            ]
        );
        for section in ["typing", "play"] {
            let mut inner: Vec<&str> =
                written[section].as_object().unwrap().keys().map(String::as_str).collect();
            inner.sort_unstable();
            assert_eq!(inner, ["groups", "keys"], "{section}");
        }
    }

    #[test]
    fn unknown_items_sounds_and_keys_are_ignored_and_the_rest_survives() {
        let text = r#"{
            "version": 7,
            "future_field": {"a": 1},
            "kit": "no_such_kit",
            "volume": 0.3,
            "language": "klingon",
            "keyboard_layout": "us",
            "typing": {
                "groups": {"letters": "snare", "no_such_group": "kick", "space": "no_such_sound", "enter": 5},
                "keys": {"KeyJ": "kick", "KeyZZ": "kick", "keyj": "kick", "Digit1": "no_such_sound"},
                "extra": true
            },
            "play": "not an object"
        }"#;
        let settings = Settings::from_json_str(text).expect("落ちない");

        assert_eq!(settings.version, SETTINGS_VERSION);
        assert_eq!(settings.kit, DEFAULT_KIT);
        assert_eq!(settings.volume, 0.3);
        assert_eq!(settings.language, Language::Auto);
        assert_eq!(settings.keyboard_layout, KeyboardLayout::Us);
        assert_eq!(settings.typing.groups, BTreeMap::from([(KeyGroup::Letters, Sound::Snare)]));
        assert_eq!(settings.typing.keys, BTreeMap::from([("KeyJ".to_string(), Sound::Kick)]));
        assert_eq!(settings.play, Assignments::default());
    }

    #[test]
    fn wrong_typed_values_fall_back_to_their_own_default_only() {
        let text = r#"{"enabled": "yes", "volume": "loud", "dynamics": 0.9, "first_sound_done": 1}"#;
        let settings = Settings::from_json_str(text).unwrap();
        assert_eq!(settings.enabled, Settings::default().enabled);
        assert_eq!(settings.volume, DEFAULT_VOLUME);
        assert_eq!(settings.dynamics, 0.9);
        assert!(!settings.first_sound_done);
    }

    #[test]
    fn out_of_range_numbers_are_rounded_into_range() {
        for (given, expected) in [(999.0, 1.0), (1.5, 1.0), (-0.1, 0.0), (-999.0, 0.0), (0.4, 0.4)] {
            let value = json!({"volume": given, "dynamics": given});
            let settings = Settings::from_value(&value).unwrap();
            assert_eq!(settings.volume, expected, "volume {given}");
            assert_eq!(settings.dynamics, expected, "dynamics {given}");
        }
        // 整数で書かれていても同じ。
        let settings = Settings::from_json_str(r#"{"volume": 999}"#).unwrap();
        assert_eq!(settings.volume, 1.0);
    }

    #[test]
    fn unreadable_text_is_not_settings() {
        for text in ["", "{", r#"{"volume": 0."#, "[1, 2]", "null", "42", "\u{0}\u{1}binary"] {
            assert!(Settings::from_json_str(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn named_values_match_the_kit_and_the_key_names() {
        // 音の名前は無料キットの名前と一致する（キットに無い名前を指すと無音になるため）。
        let kit = crate::drums::build_free_kit(48_000);
        for sound in Sound::ALL.iter().filter(|sound| **sound != Sound::None) {
            assert!(kit.index_of(sound.name()).is_some(), "{}", sound.name());
        }
        assert_eq!(Sound::ALL.len(), 9);
        assert_eq!(KeyGroup::ALL.len(), 10);

        assert!(key_position::is_known_code_name("KeyA"));
        assert!(key_position::is_known_code_name("NumpadEnter"));
        assert!(!key_position::is_known_code_name("keya"));
        assert!(!key_position::is_known_code_name(""));
    }
}
