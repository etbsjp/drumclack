//! 同時発音（ポリフォニー）の管理。
//!
//! キーを連打しても、鳴っている途中の音を打ち切らずに重ねて再生するため、
//! 現在再生中の「発音（ボイス）」をリストで持ち、上限 [`MAX_VOICES`] を
//! 超えたら新しい発音は追加しない（＝既存の発音を止めない）方針にする。

/// 同時発音数の上限。
pub const MAX_VOICES: usize = 16;

/// 再生中の1音を表す。`position` は合成済みキック波形バッファ内の再生位置。
#[derive(Debug, Clone, Copy)]
struct Voice {
    position: usize,
}

/// 現在再生中のボイス一覧を管理するプール。
///
/// 実際の音声コールバック（cpal）とキー入力スレッドの両方から触られるため、
/// 呼び出し側（[`crate::audio`]）で `Mutex` 等に包んで共有する想定。
/// このモジュール自体はロックを持たない純粋なロジックとし、単体テストしやすくしている。
#[derive(Debug, Default)]
pub struct VoicePool {
    voices: Vec<Voice>,
}

impl VoicePool {
    pub fn new() -> Self {
        Self { voices: Vec::with_capacity(MAX_VOICES) }
    }

    /// 現在再生中の発音数。
    pub fn active_count(&self) -> usize {
        self.voices.len()
    }

    /// 新しい発音を開始しようとする。
    ///
    /// 上限 [`MAX_VOICES`] に達している場合は何もせず `false` を返す
    /// （＝既存の発音を止めてまで新しい発音を割り込ませない）。
    /// 空きがあれば新しいボイスを先頭位置から追加し `true` を返す。
    pub fn trigger(&mut self) -> bool {
        if self.voices.len() >= MAX_VOICES {
            return false;
        }
        self.voices.push(Voice { position: 0 });
        true
    }

    /// キック波形バッファ `kick` を参照し、全ボイスを1サンプル分進めて合算値を返す。
    /// 再生し終えたボイス（`position` がバッファ長を超えたもの）はここで取り除く。
    pub fn mix_next_sample(&mut self, kick: &[f32]) -> f32 {
        if kick.is_empty() {
            return 0.0;
        }

        let mut sum = 0.0_f32;
        self.voices.retain_mut(|voice| {
            if voice.position >= kick.len() {
                return false;
            }
            sum += kick[voice.position];
            voice.position += 1;
            voice.position < kick.len()
        });

        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_increments_active_count_up_to_max() {
        let mut pool = VoicePool::new();
        for i in 0..MAX_VOICES {
            assert!(pool.trigger(), "{i}音目は発音できるはず");
            assert_eq!(pool.active_count(), i + 1);
        }
    }

    #[test]
    fn trigger_beyond_max_is_rejected_and_does_not_cut_existing_voices() {
        let mut pool = VoicePool::new();
        for _ in 0..MAX_VOICES {
            assert!(pool.trigger());
        }
        assert_eq!(pool.active_count(), MAX_VOICES);

        // 上限を超える17音目は追加されない。
        assert!(!pool.trigger());
        assert_eq!(
            pool.active_count(),
            MAX_VOICES,
            "上限超過時に既存の発音数が変化してはいけない（＝前の音を切らない）"
        );
    }

    #[test]
    fn finished_voices_free_up_slots_for_new_triggers() {
        let mut pool = VoicePool::new();
        let kick = vec![1.0_f32; 4]; // 4サンプルで鳴り終わる短い波形。
        for _ in 0..MAX_VOICES {
            assert!(pool.trigger());
        }
        assert!(!pool.trigger());

        // 波形の長さ分だけ進めると全ボイスが鳴り終わる。
        for _ in 0..kick.len() {
            pool.mix_next_sample(&kick);
        }
        assert_eq!(pool.active_count(), 0, "鳴り終わった発音は自動的に片付く");

        // 空きができたので再度トリガできる。
        assert!(pool.trigger());
        assert_eq!(pool.active_count(), 1);
    }

    #[test]
    fn mixing_sums_all_active_voices_without_clipping_the_logic_layer() {
        let mut pool = VoicePool::new();
        let kick = vec![0.5_f32, 0.25];
        pool.trigger();
        pool.trigger();
        // 2音同時: 0.5 + 0.5 = 1.0
        assert!((pool.mix_next_sample(&kick) - 1.0).abs() < 1e-6);
        // 2音目: 0.25 + 0.25 = 0.5
        assert!((pool.mix_next_sample(&kick) - 0.5).abs() < 1e-6);
        // 波形終端に達したので両ボイスとも消える。
        assert_eq!(pool.active_count(), 0);
    }
}
