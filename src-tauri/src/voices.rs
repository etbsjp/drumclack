//! 音の器（キット）・発音の要求・同時発音（ポリフォニー）の管理。
//!
//! 音声コールバックの中でロック・メモリの確保・解放をしないため、ここの型は
//! 次の分担で作っている。
//!
//! - `Kit`: 起動時に作り、以後は読むだけ（コールバックは参照するだけ）
//! - `RequestQueue`: キー監視側（書く）→音声コールバック（読む）の固定長待ち行列。原子的な値だけで動く
//! - `VoicePool`: 音声コールバックだけが持つ。固定長配列で、確保も解放もしない

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// 同時発音数の上限。
pub const MAX_VOICES: usize = 16;

/// 発音の要求を溜める待ち行列の長さ（2 のべき乗）。
/// 音声コールバック1回（128フレーム≒2.7ms）の間に届く打鍵は数個なので、
/// 十分に余裕を持たせた仮値。あふれた要求は鳴らさず捨てる。
pub const REQUEST_QUEUE_CAPACITY: usize = 64;

/// 音量の指定の上限。強弱の揺らぎで 1.0 をわずかに超える指定を許すための余裕。
pub const MAX_REQUEST_VOLUME: f32 = 2.0;

/// 音1つ分のデータ。波形バッファ（変種）を1個以上と、止め合いの組を持つ。
#[derive(Debug)]
pub struct Sound {
    variants: Vec<Vec<f32>>,
    choke_group: Option<u8>,
}

/// [`Kit::add`] が音を受け付けなかった理由。
#[derive(Debug, PartialEq, Eq)]
pub enum KitError {
    /// 波形バッファが1個もない、または空のバッファがある。
    NoWaveform,
    /// 同じ名前の音がすでにある。
    DuplicateName,
    /// 音の数が上限（`u16` の範囲）を超える。
    TooManySounds,
}

/// 「音の名前 → 音」の対応。止め合いの組はここ（音ごとの属性）に持ち、
/// 発音処理は音の名前を知らない。
#[derive(Debug, Default)]
pub struct Kit {
    sounds: Vec<(String, Sound)>,
}

impl Kit {
    pub fn new() -> Self {
        Self::default()
    }

    /// 音を追加する。`variants` は1個以上の空でない波形バッファ。
    /// `choke_group` が同じ音どうしは、新しい音が鳴ると鳴っている音がフェードで止まる。
    pub fn add(
        &mut self,
        name: &str,
        variants: Vec<Vec<f32>>,
        choke_group: Option<u8>,
    ) -> Result<(), KitError> {
        if variants.is_empty() || variants.iter().any(|v| v.is_empty()) {
            return Err(KitError::NoWaveform);
        }
        if self.sounds.iter().any(|(n, _)| n == name) {
            return Err(KitError::DuplicateName);
        }
        if self.sounds.len() > usize::from(u16::MAX) {
            return Err(KitError::TooManySounds);
        }
        self.sounds.push((name.to_string(), Sound { variants, choke_group }));
        Ok(())
    }

    /// 名前から音の番号を引く。キットに無い名前は `None`（＝無音扱い）。
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.sounds.iter().position(|(n, _)| n == name)
    }

    /// キットの音の数。
    pub fn sound_count(&self) -> usize {
        self.sounds.len()
    }

    /// 音の変種の数。キットに無い番号は 0。
    pub fn variant_count(&self, index: usize) -> usize {
        self.sound(index).map_or(0, |s| s.variants.len())
    }

    /// 音の全変種の波形バッファの合計サイズ（バイト）。メモリ増加の計測用。
    #[cfg(test)]
    pub fn buffer_bytes(&self, index: usize) -> usize {
        self.sound(index).map_or(0, |s| s.variants.iter().map(|v| v.len() * std::mem::size_of::<f32>()).sum())
    }

    fn sound(&self, index: usize) -> Option<&Sound> {
        self.sounds.get(index).map(|(_, s)| s)
    }

    /// 変種番号は変種の数で折り返す（範囲外の指定でも必ず1つ選ばれる）。
    fn buffer(&self, index: usize, variant: usize) -> Option<&[f32]> {
        let sound = self.sound(index)?;
        Some(&sound.variants[variant % sound.variants.len()])
    }
}

/// 発音の要求 `鳴らす(音, 変種, 音量)`。音は名前ではなく番号で持つ
/// （名前の解決はキー監視側で済ませ、音声コールバックは文字列を扱わない）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayRequest {
    pub sound: u16,
    pub variant: u8,
    pub volume: f32,
}

impl PlayRequest {
    fn pack(self) -> u64 {
        u64::from(self.sound) | (u64::from(self.variant) << 16) | (u64::from(self.volume.to_bits()) << 24)
    }

    fn unpack(bits: u64) -> Self {
        Self {
            sound: (bits & 0xFFFF) as u16,
            variant: ((bits >> 16) & 0xFF) as u8,
            volume: f32::from_bits((bits >> 24) as u32),
        }
    }
}

struct Slot {
    sequence: AtomicUsize,
    data: AtomicU64,
}

/// 固定長・ロックなしの待ち行列（複数の書き手・1人の読み手）。
///
/// 各スロットの通し番号で空き・埋まりを判定する方式。書き手は
/// `push` でスロットを予約して値を置き、読み手は `pop` で取り出す。
/// どちらも確保・解放・ロックをしない。`pop` は音声コールバック1本からだけ呼ぶこと。
pub struct RequestQueue {
    slots: Vec<Slot>,
    mask: usize,
    enqueue_pos: AtomicUsize,
    dequeue_pos: AtomicUsize,
}

impl RequestQueue {
    /// `capacity` は 2 のべき乗にすること（そうでなければ切り上げる）。
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(2).next_power_of_two();
        let slots = (0..capacity)
            .map(|i| Slot { sequence: AtomicUsize::new(i), data: AtomicU64::new(0) })
            .collect();
        Self { slots, mask: capacity - 1, enqueue_pos: AtomicUsize::new(0), dequeue_pos: AtomicUsize::new(0) }
    }

    /// 要求を積む。いっぱいなら `false`（その要求は捨てる）。
    pub fn push(&self, request: PlayRequest) -> bool {
        let mut pos = self.enqueue_pos.load(Ordering::Relaxed);
        loop {
            let slot = &self.slots[pos & self.mask];
            let sequence = slot.sequence.load(Ordering::Acquire);
            let diff = sequence as isize - pos as isize;
            if diff == 0 {
                match self.enqueue_pos.compare_exchange_weak(
                    pos,
                    pos.wrapping_add(1),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        slot.data.store(request.pack(), Ordering::Relaxed);
                        slot.sequence.store(pos.wrapping_add(1), Ordering::Release);
                        return true;
                    }
                    Err(current) => pos = current,
                }
            } else if diff < 0 {
                return false;
            } else {
                pos = self.enqueue_pos.load(Ordering::Relaxed);
            }
        }
    }

    /// 最も古い要求を取り出す。空なら `None`。読み手は1人だけ。
    pub fn pop(&self) -> Option<PlayRequest> {
        let pos = self.dequeue_pos.load(Ordering::Relaxed);
        let slot = &self.slots[pos & self.mask];
        let sequence = slot.sequence.load(Ordering::Acquire);
        if sequence != pos.wrapping_add(1) {
            return None;
        }
        let request = PlayRequest::unpack(slot.data.load(Ordering::Relaxed));
        slot.sequence.store(pos.wrapping_add(self.mask).wrapping_add(1), Ordering::Release);
        self.dequeue_pos.store(pos.wrapping_add(1), Ordering::Relaxed);
        Some(request)
    }
}

/// 再生中の1音。`position` は変種の波形バッファ内の再生位置。
#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    sound: usize,
    variant: usize,
    position: usize,
    gain: f32,
    fading: bool,
    fade_total: u32,
    fade_left: u32,
}

/// 現在再生中のボイスを管理する固定長のプール。音声コールバックだけが持つ。
#[derive(Debug)]
pub struct VoicePool {
    voices: [Voice; MAX_VOICES],
    len: usize,
}

impl Default for VoicePool {
    fn default() -> Self {
        Self::new()
    }
}

impl VoicePool {
    pub fn new() -> Self {
        Self { voices: [Voice::default(); MAX_VOICES], len: 0 }
    }

    /// 現在再生中の発音数（フェードで止めている途中の音を含む）。
    pub fn active_count(&self) -> usize {
        self.len
    }

    /// 発音を開始しようとする。開始できたら `true`。
    ///
    /// - キットに無い音番号は何もしない（`false`）
    /// - 上限 [`MAX_VOICES`] に達していたら何もしない（`false`）。鳴っている音は止めない
    /// - 開始できる場合だけ、同じ止め合いの組で鳴っている音に `fade_samples` サンプルの
    ///   フェードアウトを掛ける。0 なら即座に切る（段差が出るので本番では使わない）
    pub fn start(&mut self, kit: &Kit, request: PlayRequest, fade_samples: u32) -> bool {
        let index = usize::from(request.sound);
        let Some(sound) = kit.sound(index) else {
            return false;
        };
        if self.len >= MAX_VOICES {
            return false;
        }

        if let Some(group) = sound.choke_group {
            for voice in &mut self.voices[..self.len] {
                let same_group = kit.sound(voice.sound).and_then(|s| s.choke_group) == Some(group);
                if same_group && !voice.fading {
                    voice.fading = true;
                    voice.fade_total = fade_samples;
                    voice.fade_left = fade_samples;
                }
            }
        }

        self.voices[self.len] = Voice {
            sound: index,
            variant: usize::from(request.variant),
            position: 0,
            gain: request.volume.clamp(0.0, MAX_REQUEST_VOLUME),
            fading: false,
            fade_total: 0,
            fade_left: 0,
        };
        self.len += 1;
        true
    }

    /// 全ボイスを1サンプル分進めて合算値を返す。鳴り終わった／止め終わった音はここで取り除く。
    pub fn mix_next_sample(&mut self, kit: &Kit) -> f32 {
        let mut sum = 0.0_f32;
        let mut kept = 0;
        for read in 0..self.len {
            let mut voice = self.voices[read];
            let Some(buffer) = kit.buffer(voice.sound, voice.variant) else {
                continue;
            };
            // 即切り（フェード長0）の音は、フェード開始の時点で鳴らさずに取り除く。
            if voice.position >= buffer.len() || (voice.fading && voice.fade_left == 0) {
                continue;
            }

            let mut sample = buffer[voice.position] * voice.gain;
            if voice.fading {
                sample *= voice.fade_left as f32 / voice.fade_total as f32;
                voice.fade_left -= 1;
            }
            sum += sample;
            voice.position += 1;

            if voice.position < buffer.len() && !(voice.fading && voice.fade_left == 0) {
                self.voices[kept] = voice;
                kept += 1;
            }
        }
        self.len = kept;
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant_kit(level: f32, len: usize, group: Option<u8>) -> Kit {
        let mut kit = Kit::new();
        kit.add("a", vec![vec![level; len]], group).unwrap();
        kit
    }

    fn req(sound: u16) -> PlayRequest {
        PlayRequest { sound, variant: 0, volume: 1.0 }
    }

    #[test]
    fn kit_rejects_empty_waveforms_and_duplicate_names() {
        let mut kit = Kit::new();
        assert_eq!(kit.add("x", vec![], None), Err(KitError::NoWaveform));
        assert_eq!(kit.add("x", vec![vec![]], None), Err(KitError::NoWaveform));
        assert_eq!(kit.add("x", vec![vec![0.1], vec![]], None), Err(KitError::NoWaveform));
        kit.add("x", vec![vec![0.1]], None).unwrap();
        assert_eq!(kit.add("x", vec![vec![0.2]], None), Err(KitError::DuplicateName));
        assert_eq!(kit.index_of("x"), Some(0));
        assert_eq!(kit.index_of("y"), None);
    }

    #[test]
    fn variant_index_wraps_around_the_number_of_variants() {
        let mut kit = Kit::new();
        kit.add("a", vec![vec![0.1], vec![0.2], vec![0.3]], None).unwrap();
        assert_eq!(kit.buffer(0, 1), Some(&[0.2][..]));
        assert_eq!(kit.buffer(0, 4), Some(&[0.2][..]));
    }

    #[test]
    fn request_survives_packing() {
        let r = PlayRequest { sound: 513, variant: 3, volume: 0.37 };
        assert_eq!(PlayRequest::unpack(r.pack()), r);
    }

    #[test]
    fn queue_is_fifo_and_reports_full_and_empty() {
        let q = RequestQueue::new(4);
        assert_eq!(q.pop(), None);
        for i in 0..4 {
            assert!(q.push(req(i)), "{i}個目は積めるはず");
        }
        assert!(!q.push(req(99)), "いっぱいなら積めない");
        for i in 0..4 {
            assert_eq!(q.pop().map(|r| r.sound), Some(i));
        }
        assert_eq!(q.pop(), None);
        // 一周して再利用できる。
        for round in 0..10u16 {
            assert!(q.push(req(round)));
            assert_eq!(q.pop().map(|r| r.sound), Some(round));
        }
    }

    #[test]
    fn queue_delivers_every_request_from_concurrent_writers() {
        use std::sync::Arc;
        let q = Arc::new(RequestQueue::new(REQUEST_QUEUE_CAPACITY));
        let writers: Vec<_> = (0..4u16)
            .map(|w| {
                let q = q.clone();
                std::thread::spawn(move || {
                    for i in 0..500u16 {
                        let r = PlayRequest { sound: w * 1000 + i, variant: 0, volume: 1.0 };
                        while !q.push(r) {
                            std::thread::yield_now();
                        }
                    }
                })
            })
            .collect();

        let mut seen = std::collections::HashSet::new();
        while seen.len() < 2000 {
            if let Some(r) = q.pop() {
                assert!(seen.insert(r.sound), "同じ要求が2回出てきた: {}", r.sound);
            } else {
                std::thread::yield_now();
            }
        }
        for w in writers {
            w.join().unwrap();
        }
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn start_up_to_max_then_reject_without_cutting_existing_voices() {
        let kit = constant_kit(1.0, 1000, None);
        let mut pool = VoicePool::new();
        for i in 0..MAX_VOICES {
            assert!(pool.start(&kit, req(0), 240), "{i}音目は発音できるはず");
        }
        assert!(!pool.start(&kit, req(0), 240), "17音目は鳴らさない");
        assert_eq!(pool.active_count(), MAX_VOICES);
        // 鳴っている16音は1サンプル目で全部寄与している（止められていない）。
        assert_eq!(pool.mix_next_sample(&kit), MAX_VOICES as f32);
    }

    #[test]
    fn unknown_sound_index_is_ignored() {
        let kit = constant_kit(1.0, 10, None);
        let mut pool = VoicePool::new();
        assert!(!pool.start(&kit, req(7), 240));
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn finished_voices_free_up_slots() {
        let kit = constant_kit(1.0, 4, None);
        let mut pool = VoicePool::new();
        for _ in 0..MAX_VOICES {
            assert!(pool.start(&kit, req(0), 240));
        }
        for _ in 0..4 {
            pool.mix_next_sample(&kit);
        }
        assert_eq!(pool.active_count(), 0);
        assert!(pool.start(&kit, req(0), 240));
    }

    #[test]
    fn mixing_applies_per_voice_volume_and_sums() {
        let kit = constant_kit(0.5, 2, None);
        let mut pool = VoicePool::new();
        pool.start(&kit, PlayRequest { sound: 0, variant: 0, volume: 1.0 }, 240);
        pool.start(&kit, PlayRequest { sound: 0, variant: 0, volume: 0.5 }, 240);
        assert!((pool.mix_next_sample(&kit) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn rejected_new_voice_does_not_choke_existing_voices() {
        // 15音（組なし）＋同じ組の1音で満杯にし、同じ組の新しい音を要求する。
        let mut kit = Kit::new();
        kit.add("plain", vec![vec![1.0; 2000]], None).unwrap();
        kit.add("hat_a", vec![vec![1.0; 2000]], Some(1)).unwrap();
        kit.add("hat_b", vec![vec![1.0; 2000]], Some(1)).unwrap();
        let mut pool = VoicePool::new();
        for _ in 0..MAX_VOICES - 1 {
            assert!(pool.start(&kit, req(0), 240));
        }
        assert!(pool.start(&kit, req(1), 240));
        assert!(!pool.start(&kit, req(2), 240), "満杯なので鳴らさない");

        // 鳴らさなかった音は誰も止めない: フェード長(240)を十分に過ぎても全音が鳴っている。
        for _ in 0..400 {
            assert_eq!(pool.mix_next_sample(&kit), MAX_VOICES as f32);
        }
    }
}
