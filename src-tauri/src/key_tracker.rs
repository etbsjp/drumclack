//! 押しっぱなし（キーリピート）を無視し、「最初の1回だけ押された」を取り出す状態機械。
//!
//! OS には依存しない純粋なロジックで、macOS・Windows の両方でテストされる。
//! 押下中のキーの集合を持つが、保存・出力はしない（この型は `Debug` も持たない）。

use std::collections::HashSet;

use crate::key_position::KeyPosition;

/// 監視から届くイベント。
pub enum KeyEvent {
    /// キーが押された。`autorepeat` は OS が付けるリピートの印（Windows のフックには無いので偽）。
    Down { position: KeyPosition, autorepeat: bool },
    Up { position: KeyPosition },
    /// 監視が OS に一時的に止められた。止まっている間の「離した」を取りこぼしうる。
    MonitoringInterrupted,
}

pub struct KeyTracker {
    pressed: HashSet<KeyPosition>,
}

impl KeyTracker {
    pub fn new() -> Self {
        // 同時に押されるキーは多くても数個。コールバック内で確保が起きないよう余裕を持たせる。
        Self { pressed: HashSet::with_capacity(32) }
    }

    /// イベントを処理し、「新しく押された」ときだけそのキーの位置を返す。
    pub fn handle(&mut self, event: KeyEvent) -> Option<KeyPosition> {
        match event {
            KeyEvent::Down { position, autorepeat } => {
                // OS のリピートの印が付いていれば、集合の中身に関わらず無視する。
                // （監視の一時停止で集合を空にした後も、押しっぱなしのキーのリピートを鳴らさないため）
                if autorepeat {
                    return None;
                }
                // 印の無い Windows では、押下中の集合に既にあるキーの再押下がリピートにあたる。
                if self.pressed.insert(position) {
                    Some(position)
                } else {
                    None
                }
            }
            KeyEvent::Up { position } => {
                self.pressed.remove(&position);
                None
            }
            // 離したイベントを取りこぼすと、そのキーが集合に残り続けて二度と鳴らなくなる。
            KeyEvent::MonitoringInterrupted => {
                self.pressed.clear();
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_position::KeyPosition::*;

    fn down(t: &mut KeyTracker, position: KeyPosition, autorepeat: bool) -> bool {
        t.handle(KeyEvent::Down { position, autorepeat }).is_some()
    }

    fn up(t: &mut KeyTracker, position: KeyPosition) {
        t.handle(KeyEvent::Up { position });
    }

    #[test]
    fn press_ten_repeats_release_press_counts_exactly_two() {
        // Mac: リピートには OS の印が付く
        let mut t = KeyTracker::new();
        let mut pressed = 0;
        pressed += down(&mut t, KeyA, false) as u32;
        for _ in 0..10 {
            pressed += down(&mut t, KeyA, true) as u32;
        }
        up(&mut t, KeyA);
        pressed += down(&mut t, KeyA, false) as u32;
        assert_eq!(pressed, 2);
    }

    #[test]
    fn repeats_without_os_flag_are_ignored_too() {
        // Windows: リピートも印の無い普通の押下として届く
        let mut t = KeyTracker::new();
        let mut pressed = 0;
        pressed += down(&mut t, KeyA, false) as u32;
        for _ in 0..10 {
            pressed += down(&mut t, KeyA, false) as u32;
        }
        up(&mut t, KeyA);
        pressed += down(&mut t, KeyA, false) as u32;
        assert_eq!(pressed, 2);
    }

    #[test]
    fn repeat_flag_is_honored_even_after_the_held_set_was_cleared() {
        // 監視の一時停止で集合が空になった後、押しっぱなしのキーのリピートが鳴ってはいけない
        let mut t = KeyTracker::new();
        assert!(down(&mut t, KeyA, false));
        t.handle(KeyEvent::MonitoringInterrupted);
        let mut pressed = 0;
        for _ in 0..10 {
            pressed += down(&mut t, KeyA, true) as u32;
        }
        assert_eq!(pressed, 0);
    }

    #[test]
    fn different_keys_held_together_each_sound_once() {
        let mut t = KeyTracker::new();
        assert!(down(&mut t, KeyA, false));
        assert!(down(&mut t, KeyS, false));
        assert!(!down(&mut t, KeyA, true));
        assert!(!down(&mut t, KeyS, true));
    }

    #[test]
    fn key_held_during_monitoring_interruption_can_sound_again() {
        // 一時停止の間にキーを離した（離したイベントは届かない）→ 次の押下を鳴らせる
        let mut t = KeyTracker::new();
        assert!(down(&mut t, KeyA, false));
        t.handle(KeyEvent::MonitoringInterrupted);
        assert!(down(&mut t, KeyA, false));
    }

    #[test]
    fn without_interruption_a_missed_release_blocks_the_key() {
        // 上のテストの対照: 一時停止の通知が無ければ、取りこぼした離しのせいで鳴らない
        let mut t = KeyTracker::new();
        assert!(down(&mut t, KeyA, false));
        assert!(!down(&mut t, KeyA, false));
    }
}
