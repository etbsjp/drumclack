//! 演奏モード。演奏用の割り当てを使う条件と、「鳴った音の名前」を画面へ送る経路。
//!
//! 演奏用の割り当てが使われるのは、「演奏の画面が開いていて、かつ Drumclack の窓が最前面」の間だけ。
//! 画面が開いているか（`set_play_view_open`）と窓が最前面か（窓のフォーカスの変化）を、ここで1つの印にまとめる。
//!
//! 画面へ送るのは「鳴った音の名前」だけで、送るのは演奏用の割り当てで鳴らしたときだけ（Rust 側で遮断）。
//! 既定の割り当てでは、音の名前の列が打鍵の種別の列そのものになるため、タイピング用で鳴らした音は送らない。
//! キーの位置・打鍵の時刻は、ここには届かない。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// 画面へ送る2つの知らせ。音の名前と、演奏用の割り当てを使っているかの真偽だけを運ぶ。
pub trait PlayEvents: Send + Sync {
    /// 演奏用の割り当てで音が鳴ったとき（音の名前だけ）。
    fn sound_played(&self, sound_name: &str);
    /// 演奏用の割り当てを使う／使わないが切り替わったとき。
    fn mode_changed(&self, playing: bool);
}

/// 演奏用の割り当てを使うかの印と、画面への知らせの出口。
pub struct PlayMode {
    /// 演奏の画面が開いているか（画面が知らせる）。
    view_open: AtomicBool,
    /// Drumclack の窓が最前面か（窓のフォーカスの変化）。
    window_focused: AtomicBool,
    events: RwLock<Option<Arc<dyn PlayEvents>>>,
}

impl PlayMode {
    /// 画面は開いておらず、窓も最前面でない状態で始める（タイピング用）。
    pub fn new() -> Self {
        Self {
            view_open: AtomicBool::new(false),
            window_focused: AtomicBool::new(false),
            events: RwLock::new(None),
        }
    }

    /// 画面への知らせの出口を付ける（付けるまでは何も送らない）。
    pub fn set_events(&self, events: Arc<dyn PlayEvents>) {
        *self.events.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(events);
    }

    /// 今、演奏用の割り当てを使うか。
    pub fn is_active(&self) -> bool {
        self.view_open.load(Ordering::Relaxed) && self.window_focused.load(Ordering::Relaxed)
    }

    /// 演奏の画面が開いた／閉じたと記録する。切り替え後に演奏用を使うかを返す。
    pub fn set_view_open(&self, open: bool) -> bool {
        self.change(|| self.view_open.store(open, Ordering::Relaxed))
    }

    /// 窓が最前面になった／外れたと記録する。
    pub fn set_window_focused(&self, focused: bool) {
        self.change(|| self.window_focused.store(focused, Ordering::Relaxed));
    }

    /// 印を書き換え、演奏用を使うかが変わったときだけ画面へ知らせる。変更後の値を返す。
    fn change(&self, write: impl FnOnce()) -> bool {
        let before = self.is_active();
        write();
        let after = self.is_active();
        if before != after {
            if let Some(events) = self.events() {
                events.mode_changed(after);
            }
        }
        after
    }

    /// 鳴らした音の名前を画面へ送る。呼ぶのは、演奏用の割り当てで鳴らしたときだけ（遮断は呼び出し側の条件）。
    pub fn notify_played(&self, sound_name: &str) {
        if let Some(events) = self.events() {
            events.sound_played(sound_name);
        }
    }

    fn events(&self) -> Option<Arc<dyn PlayEvents>> {
        self.events.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }
}

/// Tauri のイベントで、メインの窓にだけ送る。
pub struct TauriPlayEvents {
    app: tauri::AppHandle,
}

impl TauriPlayEvents {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

/// 画面が受け取るイベント名。`web/play.js` と同じ名前にそろえる。
pub const EVENT_SOUND_PLAYED: &str = "drum-played";
pub const EVENT_MODE_CHANGED: &str = "play-mode-changed";
const MAIN_WINDOW: &str = "main";

impl PlayEvents for TauriPlayEvents {
    fn sound_played(&self, sound_name: &str) {
        use tauri::Emitter;
        // 画面に届かなくても、鳴らす処理は止めない。
        let _ = self.app.emit_to(MAIN_WINDOW, EVENT_SOUND_PLAYED, sound_name);
    }

    fn mode_changed(&self, playing: bool) {
        use tauri::Emitter;
        let _ = self.app.emit_to(MAIN_WINDOW, EVENT_MODE_CHANGED, playing);
    }
}

#[cfg(test)]
pub mod test_support {
    use std::sync::Mutex;

    use super::PlayEvents;

    /// 送られた知らせを溜めるだけの出口。
    #[derive(Default)]
    pub struct Recorder {
        pub sounds: Mutex<Vec<String>>,
        pub modes: Mutex<Vec<bool>>,
    }

    impl PlayEvents for Recorder {
        fn sound_played(&self, sound_name: &str) {
            self.sounds.lock().unwrap().push(sound_name.to_string());
        }
        fn mode_changed(&self, playing: bool) {
            self.modes.lock().unwrap().push(playing);
        }
    }

    impl Recorder {
        pub fn sounds(&self) -> Vec<String> {
            self.sounds.lock().unwrap().clone()
        }
        pub fn modes(&self) -> Vec<bool> {
            self.modes.lock().unwrap().clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::test_support::Recorder;
    use super::PlayMode;

    fn rig() -> (PlayMode, Arc<Recorder>) {
        let mode = PlayMode::new();
        let recorder = Arc::new(Recorder::default());
        mode.set_events(recorder.clone());
        (mode, recorder)
    }

    #[test]
    fn plays_only_while_the_view_is_open_and_the_window_is_frontmost() {
        let (mode, _) = rig();
        assert!(!mode.is_active(), "起動直後はタイピング用");
        mode.set_window_focused(true);
        assert!(!mode.is_active(), "窓が最前面でも、演奏の画面が閉じていればタイピング用");
        mode.set_view_open(true);
        assert!(mode.is_active());
        mode.set_window_focused(false);
        assert!(!mode.is_active(), "別のアプリに切り替えたらタイピング用に戻る");
        mode.set_window_focused(true);
        assert!(mode.is_active());
        mode.set_view_open(false);
        assert!(!mode.is_active(), "区画を移ったらタイピング用に戻る");
    }

    #[test]
    fn mode_changes_are_announced_once_per_change_and_the_return_value_matches() {
        let (mode, recorder) = rig();
        assert!(!mode.set_view_open(true), "窓が最前面でないので、まだ演奏用ではない");
        mode.set_window_focused(true);
        mode.set_window_focused(true);
        assert!(mode.set_view_open(true), "同じ値の再通知");
        mode.set_window_focused(false);
        assert_eq!(recorder.modes(), [true, false], "切り替わったときだけ知らせる");
    }
}
