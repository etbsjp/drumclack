//! キーの物理的な位置を表す型。名前は Web 標準の `KeyboardEvent.code` に合わせる。
//!
//! この型には意図的に `Debug` / `Display` / `Serialize` を持たせない。キーの位置を
//! ログ・画面・ファイルへ出せないようにするためで、音の名前へ変換する1か所
//! （`keyboard::sound_name_for`）の外へ出してはならない。
//! 下の静的検査（`assert_not_implemented!`）が、これらを実装した時点でコンパイルを落とす。

macro_rules! key_positions {
    ($($name:ident),* $(,)?) => {
        // OS ごとに作られない種類があるため、未使用の種類を許す。
        #[allow(dead_code)]
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        pub enum KeyPosition {
            $($name),*
        }

        /// 設定ファイルに書かれた名前が、知っているキーの名前か。文字列から真偽を返すだけで、
        /// 位置から文字列へ直す経路ではない。
        pub fn is_known_code_name(name: &str) -> bool {
            const NAMES: &[&str] = &[$(stringify!($name)),*];
            NAMES.contains(&name)
        }

        // テスト専用。製品ビルドには「位置→文字列」の経路を作らない。
        #[cfg(test)]
        impl KeyPosition {
            pub fn code_name(self) -> &'static str {
                match self {
                    $(KeyPosition::$name => stringify!($name)),*
                }
            }
        }
    };
}

key_positions! {
    // 文字キー
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
    KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    // 上段の数字
    Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    // 記号（JIS 固有の IntlRo / IntlYen を含む）
    Minus, Equal, BracketLeft, BracketRight, Backslash, Semicolon, Quote, Backquote,
    Comma, Period, Slash, IntlBackslash, IntlRo, IntlYen,
    // 編集・移動
    Escape, Tab, Space, Enter, Backspace, Delete, Insert,
    Home, End, PageUp, PageDown, ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    // ファンクション
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
    // その他
    PrintScreen, ScrollLock, Pause, NumLock, ContextMenu, CapsLock, Fn,
    // 修飾キー（左右を区別する）
    ShiftLeft, ShiftRight, ControlLeft, ControlRight, AltLeft, AltRight, MetaLeft, MetaRight,
    // テンキー
    Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
    NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide, NumpadDecimal,
    NumpadEnter, NumpadEqual, NumpadComma,
    // 日本語キーボード
    Convert, NonConvert, KanaMode, Lang1, Lang2,
}

/// 指定したトレイトを型が実装していたらコンパイルを落とす（`static_assertions` 相当を依存追加なしで行う）。
/// 実装があると `some_item` の指す先が曖昧になり、型推論が失敗する。
macro_rules! assert_not_implemented {
    ($ty:ty: $tr:path) => {
        const _: () = {
            trait AmbiguousIfImplemented<A> {
                fn some_item() {}
            }
            impl<T: ?Sized> AmbiguousIfImplemented<()> for T {}
            #[allow(dead_code)]
            struct Invalid;
            impl<T: ?Sized + $tr> AmbiguousIfImplemented<Invalid> for T {}
            let _ = <$ty as AmbiguousIfImplemented<_>>::some_item;
        };
    };
}

assert_not_implemented!(KeyPosition: std::fmt::Debug);
assert_not_implemented!(KeyPosition: std::fmt::Display);
assert_not_implemented!(KeyPosition: serde::Serialize);
