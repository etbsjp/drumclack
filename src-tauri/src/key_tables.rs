//! OS が返すキー番号を `KeyPosition`（Web 標準の `code` 名）へ直す対応表。
//!
//! 表は純粋関数で、OS の API を呼ばない。そのため macOS・Windows の両方の CI で
//! 両 OS 分のテストが走る。使う側は OS ごとに片方だけなので、未使用警告を許す。
//!
//! - Mac: 仮想キーコード（`kVK_*`）。物理位置を表し、入力ソース（配列）に依存しない。
//! - Windows: 走査コード（セット1）と拡張キーフラグ。NumLock の状態で変わらない物理位置。
//!   仮想キーコード（`vkCode`）は配列・NumLock に依存するため読まない。
#![allow(dead_code)]

use crate::key_position::KeyPosition;
use crate::key_position::KeyPosition::*;

/// Mac の仮想キーコード→キーの位置。対応表にないキー（メディアキー等）は `None`。
pub fn mac_keycode_to_position(keycode: u16) -> Option<KeyPosition> {
    Some(match keycode {
        0 => KeyA,
        1 => KeyS,
        2 => KeyD,
        3 => KeyF,
        4 => KeyH,
        5 => KeyG,
        6 => KeyZ,
        7 => KeyX,
        8 => KeyC,
        9 => KeyV,
        10 => IntlBackslash,
        11 => KeyB,
        12 => KeyQ,
        13 => KeyW,
        14 => KeyE,
        15 => KeyR,
        16 => KeyY,
        17 => KeyT,
        18 => Digit1,
        19 => Digit2,
        20 => Digit3,
        21 => Digit4,
        22 => Digit6,
        23 => Digit5,
        24 => Equal,
        25 => Digit9,
        26 => Digit7,
        27 => Minus,
        28 => Digit8,
        29 => Digit0,
        30 => BracketRight,
        31 => KeyO,
        32 => KeyU,
        33 => BracketLeft,
        34 => KeyI,
        35 => KeyP,
        36 => Enter,
        37 => KeyL,
        38 => KeyJ,
        39 => Quote,
        40 => KeyK,
        41 => Semicolon,
        42 => Backslash,
        43 => Comma,
        44 => Slash,
        45 => KeyN,
        46 => KeyM,
        47 => Period,
        48 => Tab,
        49 => Space,
        50 => Backquote,
        51 => Backspace,
        53 => Escape,
        54 => MetaRight,
        55 => MetaLeft,
        56 => ShiftLeft,
        57 => CapsLock,
        58 => AltLeft,
        59 => ControlLeft,
        60 => ShiftRight,
        61 => AltRight,
        62 => ControlRight,
        63 => Fn,
        64 => F17,
        65 => NumpadDecimal,
        67 => NumpadMultiply,
        69 => NumpadAdd,
        71 => NumLock, // テンキーの Clear。Web 標準では NumLock の位置にあたる。
        75 => NumpadDivide,
        76 => NumpadEnter,
        78 => NumpadSubtract,
        79 => F18,
        80 => F19,
        81 => NumpadEqual,
        82 => Numpad0,
        83 => Numpad1,
        84 => Numpad2,
        85 => Numpad3,
        86 => Numpad4,
        87 => Numpad5,
        88 => Numpad6,
        89 => Numpad7,
        90 => F20,
        91 => Numpad8,
        92 => Numpad9,
        93 => IntlYen,
        94 => IntlRo,
        95 => NumpadComma,
        96 => F5,
        97 => F6,
        98 => F7,
        99 => F3,
        100 => F8,
        101 => F9,
        102 => Lang2, // 英数
        103 => F11,
        104 => Lang1, // かな
        105 => F13,
        106 => F16,
        107 => F14,
        109 => F10,
        110 => ContextMenu,
        111 => F12,
        113 => F15,
        114 => Insert, // Help
        115 => Home,
        116 => PageUp,
        117 => Delete,
        118 => F4,
        119 => End,
        120 => F2,
        121 => PageDown,
        122 => F1,
        123 => ArrowLeft,
        124 => ArrowRight,
        125 => ArrowDown,
        126 => ArrowUp,
        _ => return None,
    })
}

/// 修飾キー変化（flagsChanged）で起きたことの種類。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModifierAction {
    Down,
    Up,
    /// CapsLock のように、押すたびに状態が切り替わり、離したことが届かないキー。
    /// 押された1回として扱う。
    Pressed,
}

// 左右を区別するデバイス依存のフラグ（IOKit `NX_DEVICE*KEYMASK`）。
const DEVICE_LEFT_CONTROL: u64 = 0x0000_0001;
const DEVICE_LEFT_SHIFT: u64 = 0x0000_0002;
const DEVICE_RIGHT_SHIFT: u64 = 0x0000_0004;
const DEVICE_LEFT_COMMAND: u64 = 0x0000_0008;
const DEVICE_RIGHT_COMMAND: u64 = 0x0000_0010;
const DEVICE_LEFT_ALT: u64 = 0x0000_0020;
const DEVICE_RIGHT_ALT: u64 = 0x0000_0040;
const DEVICE_RIGHT_CONTROL: u64 = 0x0000_2000;
// 左右を区別しないフラグ。Fn は左右が無い。
const FLAG_SECONDARY_FN: u64 = 0x0080_0000;

/// Mac の修飾キー変化（flagsChanged）を、どのキーが押された／離されたかへ直す。
/// 修飾キー以外のキーコードは `None`。
///
/// Mac は修飾キーを通常の押下・離しとは別の flagsChanged で届け、押したか離したかは
/// フラグの該当ビットで分かる。左右が同時に押されたままでも判定できるよう、
/// 左右を区別するデバイス依存ビットを使う。
pub fn mac_modifier_change(keycode: u16, flags: u64) -> Option<(KeyPosition, ModifierAction)> {
    let (position, mask) = match keycode {
        54 => (MetaRight, DEVICE_RIGHT_COMMAND),
        55 => (MetaLeft, DEVICE_LEFT_COMMAND),
        56 => (ShiftLeft, DEVICE_LEFT_SHIFT),
        58 => (AltLeft, DEVICE_LEFT_ALT),
        59 => (ControlLeft, DEVICE_LEFT_CONTROL),
        60 => (ShiftRight, DEVICE_RIGHT_SHIFT),
        61 => (AltRight, DEVICE_RIGHT_ALT),
        62 => (ControlRight, DEVICE_RIGHT_CONTROL),
        63 => (Fn, FLAG_SECONDARY_FN),
        57 => return Some((CapsLock, ModifierAction::Pressed)),
        _ => return None,
    };
    let action = if flags & mask != 0 { ModifierAction::Down } else { ModifierAction::Up };
    Some((position, action))
}

/// Windows の走査コード（セット1）と拡張キーフラグ→キーの位置。
/// 対応表にないキー、および Windows が差し込む偽の Shift（拡張付き 0x2A）は `None`。
pub fn windows_scancode_to_position(scancode: u32, extended: bool) -> Option<KeyPosition> {
    Some(match (scancode, extended) {
        (0x01, false) => Escape,
        (0x02, false) => Digit1,
        (0x03, false) => Digit2,
        (0x04, false) => Digit3,
        (0x05, false) => Digit4,
        (0x06, false) => Digit5,
        (0x07, false) => Digit6,
        (0x08, false) => Digit7,
        (0x09, false) => Digit8,
        (0x0A, false) => Digit9,
        (0x0B, false) => Digit0,
        (0x0C, false) => Minus,
        (0x0D, false) => Equal,
        (0x0E, false) => Backspace,
        (0x0F, false) => Tab,
        (0x10, false) => KeyQ,
        (0x11, false) => KeyW,
        (0x12, false) => KeyE,
        (0x13, false) => KeyR,
        (0x14, false) => KeyT,
        (0x15, false) => KeyY,
        (0x16, false) => KeyU,
        (0x17, false) => KeyI,
        (0x18, false) => KeyO,
        (0x19, false) => KeyP,
        (0x1A, false) => BracketLeft,
        (0x1B, false) => BracketRight,
        (0x1C, false) => Enter,
        (0x1C, true) => NumpadEnter,
        (0x1D, false) => ControlLeft,
        (0x1D, true) => ControlRight,
        (0x1E, false) => KeyA,
        (0x1F, false) => KeyS,
        (0x20, false) => KeyD,
        (0x21, false) => KeyF,
        (0x22, false) => KeyG,
        (0x23, false) => KeyH,
        (0x24, false) => KeyJ,
        (0x25, false) => KeyK,
        (0x26, false) => KeyL,
        (0x27, false) => Semicolon,
        (0x28, false) => Quote,
        (0x29, false) => Backquote,
        (0x2A, false) => ShiftLeft,
        (0x2B, false) => Backslash,
        (0x2C, false) => KeyZ,
        (0x2D, false) => KeyX,
        (0x2E, false) => KeyC,
        (0x2F, false) => KeyV,
        (0x30, false) => KeyB,
        (0x31, false) => KeyN,
        (0x32, false) => KeyM,
        (0x33, false) => Comma,
        (0x34, false) => Period,
        (0x35, false) => Slash,
        (0x35, true) => NumpadDivide,
        (0x36, false) => ShiftRight,
        (0x37, false) => NumpadMultiply,
        (0x37, true) => PrintScreen,
        (0x38, false) => AltLeft,
        (0x38, true) => AltRight,
        (0x39, false) => Space,
        (0x3A, false) => CapsLock,
        (0x3B, false) => F1,
        (0x3C, false) => F2,
        (0x3D, false) => F3,
        (0x3E, false) => F4,
        (0x3F, false) => F5,
        (0x40, false) => F6,
        (0x41, false) => F7,
        (0x42, false) => F8,
        (0x43, false) => F9,
        (0x44, false) => F10,
        (0x45, true) => NumLock,
        (0x45, false) => Pause,
        (0x46, false) => ScrollLock,
        (0x47, false) => Numpad7,
        (0x47, true) => Home,
        (0x48, false) => Numpad8,
        (0x48, true) => ArrowUp,
        (0x49, false) => Numpad9,
        (0x49, true) => PageUp,
        (0x4A, false) => NumpadSubtract,
        (0x4B, false) => Numpad4,
        (0x4B, true) => ArrowLeft,
        (0x4C, false) => Numpad5,
        (0x4D, false) => Numpad6,
        (0x4D, true) => ArrowRight,
        (0x4E, false) => NumpadAdd,
        (0x4F, false) => Numpad1,
        (0x4F, true) => End,
        (0x50, false) => Numpad2,
        (0x50, true) => ArrowDown,
        (0x51, false) => Numpad3,
        (0x51, true) => PageDown,
        (0x52, false) => Numpad0,
        (0x52, true) => Insert,
        (0x53, false) => NumpadDecimal,
        (0x53, true) => Delete,
        (0x56, false) => IntlBackslash,
        (0x57, false) => F11,
        (0x58, false) => F12,
        (0x59, false) => NumpadEqual,
        (0x5B, true) => MetaLeft,
        (0x5C, true) => MetaRight,
        (0x5D, true) => ContextMenu,
        (0x64, false) => F13,
        (0x65, false) => F14,
        (0x66, false) => F15,
        (0x67, false) => F16,
        (0x68, false) => F17,
        (0x69, false) => F18,
        (0x6A, false) => F19,
        (0x6B, false) => F20,
        (0x6C, false) => F21,
        (0x6D, false) => F22,
        (0x6E, false) => F23,
        (0x70, false) => KanaMode,
        (0x71, false) => Lang2,
        (0x72, false) => Lang1,
        (0x73, false) => IntlRo,
        (0x76, false) => F24,
        (0x79, false) => Convert,
        (0x7B, false) => NonConvert,
        (0x7D, false) => IntlYen,
        (0x7E, false) => NumpadComma,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac(keycode: u16) -> &'static str {
        mac_keycode_to_position(keycode).map_or("(なし)", |p| p.code_name())
    }

    fn win(scancode: u32, extended: bool) -> &'static str {
        windows_scancode_to_position(scancode, extended).map_or("(なし)", |p| p.code_name())
    }

    fn mac_mod(keycode: u16, flags: u64) -> (&'static str, &'static str) {
        match mac_modifier_change(keycode, flags) {
            Some((p, ModifierAction::Down)) => (p.code_name(), "down"),
            Some((p, ModifierAction::Up)) => (p.code_name(), "up"),
            Some((p, ModifierAction::Pressed)) => (p.code_name(), "pressed"),
            None => ("(なし)", "-"),
        }
    }

    #[test]
    fn mac_keycodes_map_to_web_code_names() {
        // 文字・数字・記号（配列に依存せず位置で決まる）
        let cases: &[(u16, &str)] = &[
            (0, "KeyA"), (6, "KeyZ"), (16, "KeyY"), (18, "Digit1"), (29, "Digit0"),
            (49, "Space"), (36, "Enter"), (51, "Backspace"), (117, "Delete"), (53, "Escape"),
            (50, "Backquote"), (41, "Semicolon"), (10, "IntlBackslash"),
            // テンキー
            (82, "Numpad0"), (87, "Numpad5"), (92, "Numpad9"), (76, "NumpadEnter"),
            (69, "NumpadAdd"), (78, "NumpadSubtract"), (67, "NumpadMultiply"),
            (75, "NumpadDivide"), (65, "NumpadDecimal"), (81, "NumpadEqual"), (71, "NumLock"),
            // JIS 固有
            (93, "IntlYen"), (94, "IntlRo"), (95, "NumpadComma"), (102, "Lang2"), (104, "Lang1"),
            // 移動・ファンクション
            (126, "ArrowUp"), (123, "ArrowLeft"), (115, "Home"), (121, "PageDown"),
            (122, "F1"), (111, "F12"), (105, "F13"), (90, "F20"),
            // 修飾キー（左右）
            (55, "MetaLeft"), (54, "MetaRight"), (56, "ShiftLeft"), (60, "ShiftRight"),
            (59, "ControlLeft"), (62, "ControlRight"), (58, "AltLeft"), (61, "AltRight"),
            (57, "CapsLock"), (63, "Fn"),
        ];
        for (keycode, expected) in cases {
            assert_eq!(mac(*keycode), *expected, "Mac keycode {keycode}");
        }
        // 表にないコードは音を選ばない
        assert_eq!(mac(52), "(なし)");
        assert_eq!(mac(200), "(なし)");
    }

    #[test]
    fn mac_modifier_flags_tell_press_from_release_per_side() {
        // 左 Shift だけ押された。generic の Shift ビット(0x20000)も立つが判定には使わない。
        assert_eq!(mac_mod(56, 0x0002_0000 | 0x2), ("ShiftLeft", "down"));
        // 左右とも押した状態から左だけ離した。generic は立ったままでも右が残っていれば左は離し扱い。
        assert_eq!(mac_mod(56, 0x0002_0000 | 0x4), ("ShiftLeft", "up"));
        assert_eq!(mac_mod(60, 0x0002_0000 | 0x4), ("ShiftRight", "down"));
        assert_eq!(mac_mod(55, 0x0010_0000 | 0x8), ("MetaLeft", "down"));
        assert_eq!(mac_mod(54, 0x0010_0000 | 0x10), ("MetaRight", "down"));
        assert_eq!(mac_mod(54, 0), ("MetaRight", "up"));
        assert_eq!(mac_mod(59, 0x0004_0000 | 0x1), ("ControlLeft", "down"));
        assert_eq!(mac_mod(62, 0x0004_0000 | 0x2000), ("ControlRight", "down"));
        assert_eq!(mac_mod(58, 0x0008_0000 | 0x20), ("AltLeft", "down"));
        assert_eq!(mac_mod(61, 0x0008_0000 | 0x40), ("AltRight", "down"));
        assert_eq!(mac_mod(63, 0x0080_0000), ("Fn", "down"));
        assert_eq!(mac_mod(63, 0), ("Fn", "up"));
        // CapsLock は離したことが届かないので、押された1回として扱う
        assert_eq!(mac_mod(57, 0x0001_0000), ("CapsLock", "pressed"));
        assert_eq!(mac_mod(57, 0), ("CapsLock", "pressed"));
        // 修飾キーではないコードは対象外
        assert_eq!(mac_mod(0, 0x2), ("(なし)", "-"));
    }

    #[test]
    fn windows_scancodes_map_to_web_code_names() {
        let cases: &[(u32, bool, &str)] = &[
            (0x1E, false, "KeyA"), (0x2C, false, "KeyZ"), (0x15, false, "KeyY"),
            (0x02, false, "Digit1"), (0x0B, false, "Digit0"), (0x39, false, "Space"),
            (0x1C, false, "Enter"), (0x0E, false, "Backspace"), (0x01, false, "Escape"),
            (0x29, false, "Backquote"), (0x56, false, "IntlBackslash"),
            // テンキー: 拡張なし。NumLock の状態に関係なくテンキーの位置の名前になる
            (0x52, false, "Numpad0"), (0x4C, false, "Numpad5"), (0x49, false, "Numpad9"),
            (0x4E, false, "NumpadAdd"), (0x4A, false, "NumpadSubtract"),
            (0x37, false, "NumpadMultiply"), (0x53, false, "NumpadDecimal"),
            // テンキー Enter / 割り算は拡張付きで本体側と区別される
            (0x1C, true, "NumpadEnter"), (0x35, true, "NumpadDivide"), (0x35, false, "Slash"),
            // 本体の移動キーは拡張付き。同じ走査コードのテンキーとは別の名前
            (0x47, true, "Home"), (0x47, false, "Numpad7"), (0x4F, true, "End"),
            (0x52, true, "Insert"), (0x53, true, "Delete"),
            (0x48, true, "ArrowUp"), (0x50, true, "ArrowDown"),
            (0x4B, true, "ArrowLeft"), (0x4D, true, "ArrowRight"),
            (0x49, true, "PageUp"), (0x51, true, "PageDown"),
            // NumLock と Pause は同じ走査コード 0x45 を拡張フラグで区別する
            (0x45, true, "NumLock"), (0x45, false, "Pause"),
            (0x37, true, "PrintScreen"), (0x46, false, "ScrollLock"), (0x5D, true, "ContextMenu"),
            // JIS 固有
            (0x7D, false, "IntlYen"), (0x73, false, "IntlRo"), (0x79, false, "Convert"),
            (0x7B, false, "NonConvert"), (0x70, false, "KanaMode"),
            (0x72, false, "Lang1"), (0x71, false, "Lang2"), (0x7E, false, "NumpadComma"),
            // 修飾キー（左右）
            (0x2A, false, "ShiftLeft"), (0x36, false, "ShiftRight"),
            (0x1D, false, "ControlLeft"), (0x1D, true, "ControlRight"),
            (0x38, false, "AltLeft"), (0x38, true, "AltRight"),
            (0x5B, true, "MetaLeft"), (0x5C, true, "MetaRight"), (0x3A, false, "CapsLock"),
            // ファンクション
            (0x3B, false, "F1"), (0x58, false, "F12"), (0x64, false, "F13"), (0x76, false, "F24"),
        ];
        for (scancode, extended, expected) in cases {
            assert_eq!(win(*scancode, *extended), *expected, "走査コード {scancode:#x} 拡張={extended}");
        }
        // NumLock オフ時に Windows が差し込む偽の Shift（拡張付き 0x2A / 0x36）は音を選ばない
        assert_eq!(win(0x2A, true), "(なし)");
        assert_eq!(win(0x36, true), "(なし)");
        assert_eq!(win(0xFF, false), "(なし)");
    }

    #[test]
    fn mac_and_windows_use_the_same_names_for_the_same_physical_key() {
        // 両 OS で同じキーに同じ名前が付く（設定ファイルの `code` 名は OS 共通）
        let pairs: &[(u16, u32, bool)] = &[
            (0, 0x1E, false),    // A
            (49, 0x39, false),   // Space
            (36, 0x1C, false),   // Enter
            (76, 0x1C, true),    // NumpadEnter
            (87, 0x4C, false),   // Numpad5
            (93, 0x7D, false),   // IntlYen
            (123, 0x4B, true),   // ArrowLeft
            (56, 0x2A, false),   // ShiftLeft
            (62, 0x1D, true),    // ControlRight
            (55, 0x5B, true),    // MetaLeft
        ];
        for (keycode, scancode, extended) in pairs {
            assert_eq!(mac(*keycode), win(*scancode, *extended), "mac {keycode} / win {scancode:#x}");
        }
    }
}
