//! 無料キット（808風・合成）の8音と、1音4変種の合成。
//!
//! キック以外の7音は、この1つのファイルの「音ごとの数値の表」[`DRUM_SPECS`] から
//! 合成する。キックの波形だけは `kick.rs` が持つ（既存の合成をそのまま使う）が、
//! 音量の釣り合い・止め合い・変種の振れ幅はここの定数にまとめている。
//!
//! 合成は起動時に1回だけ。乱数の種は音の番号と変種の番号から決まるので、
//! 何度合成しても同じ波形になる（テストと WAV 書き出しが再現できる）。
//!
//! ## 調整のしかた（何を変えると何が変わるか）
//!
//! | 数値 | 変えると |
//! |---|---|
//! | `peak` | その音の大きさ（他の音との釣り合い）。波形の最大値をこの値に揃える |
//! | `duration_sec` / `fade_out_sec` | 鳴る長さ / 末尾のフェード。フェード開始時点で十分小さく、かつ最低音の1.5周期以上の長さが要る（テストが見張る） |
//! | `tone.start_hz` `tone.end_hz` `tone.pitch_decay_sec` | 太鼓の音程と、叩いた瞬間の音程の落ち方 |
//! | `tone.decay_sec` / `tone.level` | 音程成分の伸び / 他の成分との混ざり具合 |
//! | `metal.*` | ハイハットの金属感（6つの矩形波）。`hp_hz` を上げるほど細く高く聞こえる |
//! | `noise.level` / `noise.decay_sec` | ザーッという成分の量と伸び |
//! | `noise.hp_hz` / `noise.lp_hz` | ノイズの低域・高域を削る位置（音の明るさ・太さ） |
//! | `noise.bursts` `burst_gap_sec` `burst_decay_sec` | クラップの「パパパッ」と重なる回数・間隔・各回の短さ |
//! | `PITCH_SPREAD` / `DECAY_SPREAD` | 変種の高さ（±1.5%）と減衰（±5%）の振れ幅 |
//! | `VARIANT_PITCH_STEPS` / `VARIANT_DECAY_STEPS` | 4つの変種それぞれが振れ幅のどこに当たるか（-1〜+1） |

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use crate::kick;
use crate::voices::Kit;

/// 1音あたりの変種の数。
pub const VARIANT_COUNT: usize = 4;

/// ハイハット（閉じ・開き）が共有する止め合いの組の番号。
pub const HAT_CHOKE_GROUP: u8 = 1;

/// 変種の高さの振れ幅（比率）。±1.5%。
const PITCH_SPREAD: f32 = 0.015;
/// 変種の減衰時間の振れ幅（比率）。±5%。
const DECAY_SPREAD: f32 = 0.05;
/// 変種ごとの高さの位置（-1〜+1）。変種0は基準の音（振れなし）。
const VARIANT_PITCH_STEPS: [f32; VARIANT_COUNT] = [0.0, -1.0, 1.0, 0.5];
/// 変種ごとの減衰の位置（-1〜+1）。高さと同じ向きに揃えないよう、組み合わせを散らしてある。
const VARIANT_DECAY_STEPS: [f32; VARIANT_COUNT] = [0.0, 1.0, -1.0, -0.5];

/// 変種 `variant` の（高さの倍率, 減衰時間の倍率）。
pub fn variant_scales(variant: usize) -> (f32, f32) {
    let v = variant % VARIANT_COUNT;
    (1.0 + PITCH_SPREAD * VARIANT_PITCH_STEPS[v], 1.0 + DECAY_SPREAD * VARIANT_DECAY_STEPS[v])
}

/// 音程のある成分（サイン波）。`level` が 0 なら使わない。
pub struct Tone {
    pub start_hz: f32,
    pub end_hz: f32,
    /// 音程が `start_hz` から `end_hz` へ落ちる時定数（秒）。
    pub pitch_decay_sec: f32,
    pub decay_sec: f32,
    pub level: f32,
    /// 同じ減衰で重ねる高い倍音（リム用）。`partial_level` が 0 なら使わない。
    pub partial_hz: f32,
    pub partial_level: f32,
}

/// 金属的な成分（808のハイハットと同じ6つの矩形波の和）。`level` が 0 なら使わない。
pub struct Metal {
    pub level: f32,
    pub decay_sec: f32,
    /// ハイパスの位置（Hz）。2段かける。
    pub hp_hz: f32,
}

/// ノイズ成分。`level` が 0 なら使わない。
pub struct Noise {
    pub level: f32,
    pub decay_sec: f32,
    /// 0 なら無効。
    pub hp_hz: f32,
    /// 0 なら無効。
    pub lp_hz: f32,
    /// ノイズを重ねて鳴らす回数（クラップ用。1なら普通の1回）。最後の1回だけ `decay_sec` で伸びる。
    pub bursts: u32,
    pub burst_gap_sec: f32,
    pub burst_decay_sec: f32,
}

/// 1音ぶんの合成の数値。
pub struct DrumSpec {
    /// 設定ファイルに保存される名前（設計の正本の表のとおり）。
    pub name: &'static str,
    pub choke_group: Option<u8>,
    /// 波形の最大値。音ごとの音量の釣り合いはここで決める。
    pub peak: f32,
    pub duration_sec: f32,
    /// 末尾のフェードアウト（余弦）。フェード開始時点でほぼ無音で、かつ最低音の1周期より十分長いこと。
    pub fade_out_sec: f32,
    pub tone: Tone,
    pub metal: Metal,
    pub noise: Noise,
}

const NO_TONE: Tone =
    Tone { start_hz: 0.0, end_hz: 0.0, pitch_decay_sec: 1.0, decay_sec: 1.0, level: 0.0, partial_hz: 0.0, partial_level: 0.0 };
const NO_METAL: Metal = Metal { level: 0.0, decay_sec: 1.0, hp_hz: 0.0 };
const NO_NOISE: Noise =
    Noise { level: 0.0, decay_sec: 1.0, hp_hz: 0.0, lp_hz: 0.0, bursts: 1, burst_gap_sec: 0.0, burst_decay_sec: 1.0 };

/// キック（波形は `kick.rs`）の最大値。音量の釣り合いの基準。
#[cfg(test)]
pub const KICK_PEAK: f32 = kick::PEAK_AMPLITUDE;

/// 808の6つの矩形波の周波数（Hz）。ハイハットの金属感の元。
const METAL_FREQS_HZ: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

/// キック以外の7音の数値表。音の釣り合い（`peak`）は仮値。
pub const DRUM_SPECS: [DrumSpec; 7] = [
    DrumSpec {
        name: "snare",
        choke_group: None,
        peak: 0.8,
        duration_sec: 0.4,
        fade_out_sec: 0.04,
        tone: Tone { start_hz: 220.0, end_hz: 180.0, pitch_decay_sec: 0.02, decay_sec: 0.07, level: 0.6, ..NO_TONE },
        metal: NO_METAL,
        noise: Noise { level: 1.0, decay_sec: 0.11, hp_hz: 1200.0, lp_hz: 9000.0, ..NO_NOISE },
    },
    DrumSpec {
        name: "hat_closed",
        choke_group: Some(HAT_CHOKE_GROUP),
        peak: 0.45,
        duration_sec: 0.16,
        fade_out_sec: 0.025,
        tone: NO_TONE,
        metal: Metal { level: 1.0, decay_sec: 0.03, hp_hz: 6000.0 },
        noise: Noise { level: 0.5, decay_sec: 0.03, hp_hz: 7000.0, ..NO_NOISE },
    },
    DrumSpec {
        name: "hat_open",
        choke_group: Some(HAT_CHOKE_GROUP),
        peak: 0.5,
        duration_sec: 0.8,
        fade_out_sec: 0.06,
        tone: NO_TONE,
        metal: Metal { level: 1.0, decay_sec: 0.16, hp_hz: 6000.0 },
        noise: Noise { level: 0.5, decay_sec: 0.16, hp_hz: 7000.0, ..NO_NOISE },
    },
    DrumSpec {
        name: "clap",
        choke_group: None,
        peak: 0.7,
        duration_sec: 0.5,
        fade_out_sec: 0.05,
        tone: NO_TONE,
        metal: NO_METAL,
        noise: Noise {
            level: 1.0,
            decay_sec: 0.11,
            hp_hz: 900.0,
            lp_hz: 3200.0,
            bursts: 4,
            burst_gap_sec: 0.011,
            burst_decay_sec: 0.004,
        },
    },
    DrumSpec {
        name: "rim",
        choke_group: None,
        peak: 0.6,
        duration_sec: 0.15,
        fade_out_sec: 0.02,
        tone: Tone {
            start_hz: 480.0,
            end_hz: 480.0,
            pitch_decay_sec: 1.0,
            decay_sec: 0.012,
            level: 1.0,
            partial_hz: 1750.0,
            partial_level: 0.7,
        },
        metal: NO_METAL,
        noise: Noise { level: 0.15, decay_sec: 0.006, hp_hz: 2000.0, ..NO_NOISE },
    },
    DrumSpec {
        name: "tom_low",
        choke_group: None,
        peak: 0.8,
        duration_sec: 0.7,
        fade_out_sec: 0.06,
        tone: Tone { start_hz: 150.0, end_hz: 100.0, pitch_decay_sec: 0.04, decay_sec: 0.2, level: 1.0, ..NO_TONE },
        metal: NO_METAL,
        noise: Noise { level: 0.04, decay_sec: 0.01, hp_hz: 800.0, ..NO_NOISE },
    },
    DrumSpec {
        name: "tom_high",
        choke_group: None,
        peak: 0.75,
        duration_sec: 0.55,
        fade_out_sec: 0.05,
        tone: Tone { start_hz: 260.0, end_hz: 170.0, pitch_decay_sec: 0.035, decay_sec: 0.15, level: 1.0, ..NO_TONE },
        metal: NO_METAL,
        noise: Noise { level: 0.04, decay_sec: 0.01, hp_hz: 800.0, ..NO_NOISE },
    },
];

impl DrumSpec {
    /// 使っている成分のうち最も低い周波数（Hz）。フェード長がこの1周期より十分長いかの確認に使う。
    #[cfg(test)]
    pub fn lowest_hz(&self) -> f32 {
        let mut lowest = f32::MAX;
        if self.tone.level > 0.0 {
            lowest = lowest.min(self.tone.start_hz.min(self.tone.end_hz));
        }
        if self.metal.level > 0.0 {
            lowest = lowest.min(METAL_FREQS_HZ[0]);
        }
        if self.noise.level > 0.0 {
            lowest = lowest.min(if self.noise.hp_hz > 0.0 { self.noise.hp_hz } else { 100.0 });
        }
        lowest
    }
}

/// 乱数（xorshift32）。種が同じなら同じ列になる。
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        // 種から1段混ぜて、隣り合う種でも列が似ないようにする。0 は使えない。
        let mut z = seed.wrapping_add(0x9E37_79B9);
        z = (z ^ (z >> 16)).wrapping_mul(0x85EB_CA6B);
        z = (z ^ (z >> 13)).wrapping_mul(0xC2B2_AE35);
        z ^= z >> 16;
        Self(if z == 0 { 0x1234_5678 } else { z })
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// -1.0 以上 1.0 未満。
    pub fn next_signed(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

/// 一次のローパスフィルタの係数。ハイパスにも使う（ローパスの出力を入力から引いた残りがハイパス）。
fn lp_coef(cutoff_hz: f32, sample_rate: u32) -> f32 {
    1.0 - (-std::f32::consts::TAU * cutoff_hz / sample_rate as f32).exp()
}

/// 変種ごとの乱数の種。音の番号と変種の番号だけから決まる固定値。
fn seed_for(sound_index: u32, variant: usize) -> u32 {
    sound_index.wrapping_mul(0x0001_0001).wrapping_add(variant as u32 + 1)
}

/// 末尾のフェード。最後のサンプルでちょうど 0 になる余弦カーブ（両端で傾きが 0）。
fn tail_fade(i: usize, len: usize, fade_len: usize) -> f32 {
    let remaining = (len - 1 - i) as f32;
    let ratio = (remaining / fade_len.max(1) as f32).clamp(0.0, 1.0);
    0.5 - 0.5 * (std::f32::consts::PI * ratio).cos()
}

/// 1音・1変種の波形を合成する。波形の最大値は `spec.peak` に揃える。
pub fn synthesize(spec: &DrumSpec, sample_rate: u32, sound_index: u32, variant: usize) -> Vec<f32> {
    let (pitch, decay) = variant_scales(variant);
    synthesize_with(spec, sample_rate, seed_for(sound_index, variant), pitch, decay)
}

/// 乱数の種・高さの倍率・減衰の倍率を直接指定して合成する。
/// 種と高さ・減衰を切り離して検証できるよう、[`synthesize`] から分けてある。
fn synthesize_with(spec: &DrumSpec, sample_rate: u32, seed: u32, pitch: f32, decay: f32) -> Vec<f32> {
    let len = ((spec.duration_sec * sample_rate as f32).ceil() as usize).max(2);
    let fade_len = (spec.fade_out_sec * sample_rate as f32).round() as usize;
    let dt = 1.0 / sample_rate as f32;
    let mut rng = Rng::new(seed);

    let (tone, metal, noise) = (&spec.tone, &spec.metal, &spec.noise);
    let (mut tone_phase, mut partial_phase) = (0.0_f32, 0.0_f32);
    let mut metal_phase = [0.0_f32; 6];
    let metal_a = lp_coef(metal.hp_hz.max(1.0), sample_rate);
    let (mut metal_lp1, mut metal_lp2) = (0.0_f32, 0.0_f32);
    let noise_hp_a = lp_coef(noise.hp_hz.max(1.0), sample_rate);
    let noise_lp_a = lp_coef(noise.lp_hz.max(1.0), sample_rate);
    let (mut noise_hp_state, mut noise_lp_state) = (0.0_f32, 0.0_f32);

    let mut samples = Vec::with_capacity(len);
    for i in 0..len {
        let t = i as f32 * dt;
        let mut x = 0.0_f32;

        if tone.level > 0.0 {
            let freq = (tone.end_hz + (tone.start_hz - tone.end_hz) * (-t / tone.pitch_decay_sec).exp()) * pitch;
            tone_phase += freq * dt;
            let env = (-t / (tone.decay_sec * decay)).exp();
            x += tone.level * env * (std::f32::consts::TAU * tone_phase).sin();
            if tone.partial_level > 0.0 {
                partial_phase += tone.partial_hz * pitch * dt;
                x += tone.partial_level * env * (std::f32::consts::TAU * partial_phase).sin();
            }
        }

        if metal.level > 0.0 {
            let mut sum = 0.0_f32;
            for (phase, hz) in metal_phase.iter_mut().zip(METAL_FREQS_HZ) {
                *phase = (*phase + hz * pitch * dt).fract();
                sum += if *phase < 0.5 { 1.0 } else { -1.0 };
            }
            let mut m = sum / METAL_FREQS_HZ.len() as f32;
            metal_lp1 += metal_a * (m - metal_lp1);
            m -= metal_lp1;
            metal_lp2 += metal_a * (m - metal_lp2);
            m -= metal_lp2;
            x += metal.level * (-t / (metal.decay_sec * decay)).exp() * m;
        }

        if noise.level > 0.0 {
            // 乱数は毎サンプル必ず1つ進める（フィルタの有無で列がずれないように）。
            let mut n = rng.next_signed();
            if noise.hp_hz > 0.0 {
                noise_hp_state += noise_hp_a * (n - noise_hp_state);
                n -= noise_hp_state;
            }
            if noise.lp_hz > 0.0 {
                noise_lp_state += noise_lp_a * (n - noise_lp_state);
                n = noise_lp_state;
            }
            // 重ねる回数ぶんの減衰の和。最後の1回だけ長く伸びる。
            let mut env = 0.0_f32;
            for k in 0..noise.bursts {
                let start = k as f32 * noise.burst_gap_sec;
                if t >= start {
                    let d = if k + 1 == noise.bursts { noise.decay_sec } else { noise.burst_decay_sec };
                    env += (-(t - start) / (d * decay)).exp();
                }
            }
            x += noise.level * env * n;
        }

        samples.push(x * tail_fade(i, len, fade_len));
    }

    let max = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    if max > 0.0 {
        let scale = spec.peak / max;
        for s in &mut samples {
            *s *= scale;
        }
    }
    samples
}

/// 無料キット（808風）。8音×4変種を合成し、止め合いの組を付けて返す。
pub fn build_free_kit(sample_rate: u32) -> Kit {
    let mut kit = Kit::new();
    let kick_variants = (0..VARIANT_COUNT)
        .map(|v| {
            let (pitch, decay) = variant_scales(v);
            kick::synthesize_kick_variant(sample_rate, pitch, decay)
        })
        .collect();
    kit.add("kick", kick_variants, None).expect("合成したキックは空ではない");
    for (i, spec) in DRUM_SPECS.iter().enumerate() {
        let variants = (0..VARIANT_COUNT).map(|v| synthesize(spec, sample_rate, i as u32 + 1, v)).collect();
        kit.add(spec.name, variants, spec.choke_group).expect("合成した音は空ではない");
    }
    kit
}

/// 変種の選び方。直前に同じ音で鳴らした変種とは別の変種を選ぶ。
///
/// キー監視のスレッドから `&self` で呼ぶ（ロックなし）。音ごとに直前の変種を覚え、
/// 変種が2つ以上ある音では必ず別のものを返す。乱数の種は固定値なので、
/// 打鍵の列が同じなら選ばれる列も同じ。
pub struct VariantPicker {
    last: Vec<AtomicU8>,
    rng: AtomicU32,
}

/// 「まだ鳴らしていない」を表す値。
const NO_LAST_VARIANT: u8 = u8::MAX;

impl VariantPicker {
    pub fn new(sound_count: usize) -> Self {
        Self {
            last: (0..sound_count).map(|_| AtomicU8::new(NO_LAST_VARIANT)).collect(),
            rng: AtomicU32::new(Rng::new(0xD2_C1_AC).0),
        }
    }

    fn next_random(&self) -> u32 {
        let mut rng = Rng(self.rng.load(Ordering::Relaxed));
        let value = rng.next_u32();
        self.rng.store(rng.0, Ordering::Relaxed);
        value
    }

    /// 音 `sound`（変種が `count` 個）で次に鳴らす変種の番号。
    ///
    /// 単一のスレッド（キー監視のスレッド）から呼ぶ前提。原子的な値で持っているのは
    /// `&self` で呼べるようにするためで、複数スレッドから同時に呼ぶと同じ変種が続くことがある。
    pub fn pick(&self, sound: usize, count: usize) -> usize {
        debug_assert!(count <= u8::MAX as usize, "変種の数は u8 に収まる範囲");
        if count <= 1 {
            return 0;
        }
        let Some(last_cell) = self.last.get(sound) else {
            return 0;
        };
        let last = last_cell.load(Ordering::Relaxed);
        let picked = if usize::from(last) < count {
            // 直前の変種を除いた count-1 個から選ぶ。
            let r = self.next_random() as usize % (count - 1);
            if r >= usize::from(last) {
                r + 1
            } else {
                r
            }
        } else {
            self.next_random() as usize % count
        };
        last_cell.store(picked as u8, Ordering::Relaxed);
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voices::{PlayRequest, VoicePool};

    const SR: u32 = 48_000;

    /// テスト対象の全音。（名前, 全変種の波形, フェード秒, 最低周波数Hz, 止め合いの組）
    struct Rendered {
        name: &'static str,
        variants: Vec<Vec<f32>>,
        fade_out_sec: f32,
        lowest_hz: f32,
    }

    /// 製品と同じ `build_free_kit` の出力から、全音・全変種の波形を取り出す。
    /// テスト側で引くのは名前・フェード長・最低周波数だけ。
    fn all_rendered(sample_rate: u32) -> Vec<Rendered> {
        let kit = build_free_kit(sample_rate);
        let info: Vec<(&'static str, f32, f32)> = std::iter::once(("kick", kick::FADE_OUT_SEC, kick::LOWEST_FREQ_HZ))
            .chain(DRUM_SPECS.iter().map(|spec| (spec.name, spec.fade_out_sec, spec.lowest_hz())))
            .collect();
        info.into_iter()
            .map(|(name, fade_out_sec, lowest_hz)| {
                let index = kit.index_of(name).unwrap_or_else(|| panic!("{name} がキットに無い"));
                Rendered {
                    name,
                    variants: (0..kit.variant_count(index))
                        .map(|v| kit.variant_samples(index, v).expect("変種がある").to_vec())
                        .collect(),
                    fade_out_sec,
                    lowest_hz,
                }
            })
            .collect()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
    }

    /// 同じ値が連続する最長の長さ（振幅が `floor` を超える区間だけ数える）。頭打ちの検出。
    ///
    /// これは生成した波形側の確認。頭打ちの本丸は、複数の音が重なる出力段の混合テスト
    /// （`realistic_mix_does_not_stick_to_full_scale`）で見る。
    fn longest_flat_run(samples: &[f32], floor: f32) -> usize {
        let (mut run, mut longest) = (1usize, 1usize);
        for i in 1..samples.len() {
            if samples[i] == samples[i - 1] && samples[i].abs() > floor {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 1;
            }
        }
        longest
    }

    fn rms_diff(a: &[f32], b: &[f32]) -> f32 {
        let n = a.len().min(b.len());
        (a[..n].iter().zip(&b[..n]).map(|(x, y)| (x - y) * (x - y)).sum::<f32>() / n as f32).sqrt()
    }

    fn rms(a: &[f32]) -> f32 {
        (a.iter().map(|x| x * x).sum::<f32>() / a.len() as f32).sqrt()
    }

    #[test]
    fn kit_has_the_eight_named_sounds_with_four_variants_each() {
        let kit = build_free_kit(SR);
        for name in ["kick", "snare", "hat_closed", "hat_open", "clap", "rim", "tom_low", "tom_high"] {
            let index = kit.index_of(name).unwrap_or_else(|| panic!("{name} がキットに無い"));
            assert_eq!(kit.variant_count(index), VARIANT_COUNT, "{name} の変種の数");
        }
    }

    #[test]
    fn synthesis_is_deterministic() {
        let a = all_rendered(SR);
        let b = all_rendered(SR);
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.variants, y.variants, "{} が合成のたびに変わる", x.name);
        }
    }

    // 末尾が0に収束していて、打ち切られていないこと。
    // 1) 最後のサンプルが0。2) 最後の2msはほぼ無音。
    // 3) フェード開始の直前（10ms）の振幅がすでにピークの10%以下（鳴っている最中に断ち切っていない）。
    // 4) フェードの長さが最低音の1.5周期以上（波の途中で切れない）。
    #[test]
    fn every_sound_ends_at_zero_without_being_cut_off() {
        for sample_rate in [44_100, 48_000] {
            for r in all_rendered(sample_rate) {
                for (v, s) in r.variants.iter().enumerate() {
                    let tag = format!("{} 変種{v} @{sample_rate}Hz", r.name);
                    let pk = peak(s);
                    assert!(s.last().unwrap().abs() <= 1e-4, "{tag}: 最後のサンプルが0でない: {}", s.last().unwrap());
                    let last2ms = (sample_rate as usize * 2 / 1000).max(1);
                    let tail_peak = peak(&s[s.len() - last2ms..]);
                    assert!(tail_peak <= 0.01 * pk, "{tag}: 末尾2msがまだ鳴っている: {tail_peak} (peak {pk})");

                    let fade_len = (r.fade_out_sec * sample_rate as f32).round() as usize;
                    let before = s.len() - fade_len;
                    let ten_ms = sample_rate as usize / 100;
                    let level_before_fade = peak(&s[before.saturating_sub(ten_ms)..before]);
                    assert!(
                        level_before_fade <= 0.1 * pk,
                        "{tag}: フェード開始時点でまだ大きい（打ち切りに近い）: {level_before_fade} (peak {pk})"
                    );
                    assert!(
                        r.fade_out_sec * r.lowest_hz >= 1.5,
                        "{tag}: フェードが最低音{}Hzの1.5周期に足りない（{:.2}周期）",
                        r.lowest_hz,
                        r.fade_out_sec * r.lowest_hz
                    );
                }
            }
        }
    }

    // 頭打ちで平らになった区間が無く、ピークが狙いの大きさまで出ていること。
    // 「超えていない」だけでは、頭を切った波形も通ってしまう。
    // 実際に頭が潰れていれば、潰れた区間は同じ値が連続する。
    #[test]
    fn no_sound_is_flattened_and_peaks_reach_their_target() {
        let targets: Vec<(&str, f32)> = std::iter::once(("kick", KICK_PEAK))
            .chain(DRUM_SPECS.iter().map(|s| (s.name, s.peak)))
            .collect();
        for r in all_rendered(SR) {
            let target = targets.iter().find(|(n, _)| *n == r.name).unwrap().1;
            for (v, s) in r.variants.iter().enumerate() {
                let pk = peak(s);
                assert!(s.iter().all(|x| x.is_finite()), "{} 変種{v}: 非数がある", r.name);
                assert!((pk - target).abs() <= 0.01 * target, "{} 変種{v}: ピーク{pk}が狙い{target}と違う", r.name);
                let run = longest_flat_run(s, 0.01 * pk);
                assert!(run <= 2, "{} 変種{v}: 頭打ちと思われる連続同値が{run}個", r.name);
                // 出だし（最初の50ms）にもピークの半分以上の音が鳴っている＝出だしが欠けていない。
                let head = &s[..SR as usize / 20];
                assert!(peak(head) >= 0.5 * pk, "{} 変種{v}: 出だしが小さい（{} / peak {pk}）", r.name, peak(head));
            }
        }
    }

    // 同じ音の4変種が互いに一定以上違うこと（差の実効値が、音の実効値の一定割合以上）。
    // 変種を同一にすると差が0になって落ちる。
    #[test]
    fn variants_of_a_sound_differ_from_each_other() {
        // 実測の最小値（rms差/rms）は 0.39（rim）。余裕を見て 0.2。
        const MIN_RELATIVE_DIFF: f32 = 0.2;
        for r in all_rendered(SR) {
            for a in 0..VARIANT_COUNT {
                for b in (a + 1)..VARIANT_COUNT {
                    let diff = rms_diff(&r.variants[a], &r.variants[b]);
                    let level = rms(&r.variants[a]).max(rms(&r.variants[b]));
                    assert!(
                        diff >= MIN_RELATIVE_DIFF * level,
                        "{} の変種{a}と{b}が似すぎ: 差{diff:.4} / 実効値{level:.4}",
                        r.name
                    );
                }
            }
        }
    }

    // 乱数の種を固定して、高さ・減衰の振れだけで4変種が違うこと。
    // ノイズ系（snare・hat・clap）は、種が違うだけで上のテストを通ってしまうため、
    // 種を切り離した状態でも「高さ・減衰の振れ」が波形に効いていることをここで確かめる。
    // 振れ幅（`VARIANT_*_STEPS` / `*_SPREAD`）を 0 にすると差が0になって落ちる。
    #[test]
    fn pitch_and_decay_alone_make_the_variants_differ() {
        // 実測の最小は clap の 0.016（種のない音で減衰だけが効く）。下限はその約 6 割。
        const MIN_RELATIVE_DIFF: f32 = 0.01;
        type Render = Box<dyn Fn(usize) -> Vec<f32>>;
        let mut sounds: Vec<(&str, Render)> = vec![(
            "kick",
            Box::new(|v| {
                let (p, d) = variant_scales(v);
                kick::synthesize_kick_variant(SR, p, d)
            }),
        )];
        for spec in &DRUM_SPECS {
            sounds.push((
                spec.name,
                Box::new(move |v| {
                    let (p, d) = variant_scales(v);
                    synthesize_with(spec, SR, 12345, p, d)
                }),
            ));
        }
        for (name, render) in &sounds {
            let waves: Vec<Vec<f32>> = (0..VARIANT_COUNT).map(render).collect();
            for a in 0..VARIANT_COUNT {
                for b in (a + 1)..VARIANT_COUNT {
                    let diff = rms_diff(&waves[a], &waves[b]);
                    let level = rms(&waves[a]).max(rms(&waves[b]));
                    println!("[計測] {name} 変種{a}/{b} 種固定の相対差 {:.3}", diff / level);
                    assert!(
                        diff >= MIN_RELATIVE_DIFF * level,
                        "{name} の変種{a}と{b}が、種を固定すると似すぎ: 差{diff:.4} / 実効値{level:.4} = {:.3}",
                        diff / level
                    );
                }
            }
        }
    }

    // 変種の振れ幅が決めた範囲（高さ±1.5%・減衰±5%）に収まっていること。
    #[test]
    fn variant_spread_stays_within_the_designed_range() {
        for v in 0..VARIANT_COUNT {
            let (pitch, decay) = variant_scales(v);
            assert!((pitch - 1.0).abs() <= PITCH_SPREAD + 1e-6, "変種{v}の高さ倍率{pitch}");
            assert!((decay - 1.0).abs() <= DECAY_SPREAD + 1e-6, "変種{v}の減衰倍率{decay}");
        }
        // 4つの変種の高さ・減衰の組が互いに違う。
        for a in 0..VARIANT_COUNT {
            for b in (a + 1)..VARIANT_COUNT {
                assert_ne!(variant_scales(a), variant_scales(b), "変種{a}と{b}の振れが同じ");
            }
        }
    }

    // 直前と同じ変種が続かない（1000回）。全変種が使われること、音ごとに別々に覚えること。
    #[test]
    fn picker_never_repeats_the_previous_variant_in_a_thousand_picks() {
        let picker = VariantPicker::new(8);
        let mut last = [usize::MAX; 8];
        let mut seen = [[false; VARIANT_COUNT]; 8];
        let mut state = 7u32;
        for _ in 0..1000 {
            // 音はばらばらの順で叩く（ある音が連続する場合も、間に他の音が挟まる場合もある）。
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let sound = (state >> 24) as usize % 8;
            let v = picker.pick(sound, VARIANT_COUNT);
            assert!(v < VARIANT_COUNT);
            assert_ne!(v, last[sound], "音{sound}で同じ変種{v}が続いた");
            last[sound] = v;
            seen[sound][v] = true;
        }
        assert!(seen.iter().all(|s| s.iter().all(|&x| x)), "選ばれない変種がある: {seen:?}");
    }

    #[test]
    fn picker_with_a_single_variant_always_returns_zero() {
        let picker = VariantPicker::new(1);
        for _ in 0..10 {
            assert_eq!(picker.pick(0, 1), 0);
        }
    }

    // 止め合い: 閉じが開きを止める／開きどうしは新しい音が前を止める。
    // 閉じ・開きの名前は発音処理に出てこず、キットに付けた組だけで決まる。
    #[test]
    fn hats_share_a_choke_group_so_closed_stops_open_and_open_stops_open() {
        let kit = build_free_kit(SR);
        let (closed, open, snare) =
            (kit.index_of("hat_closed").unwrap(), kit.index_of("hat_open").unwrap(), kit.index_of("snare").unwrap());
        let fade = 240;
        let req = |s: usize| PlayRequest { sound: s as u16, variant: 0, volume: 1.0 };

        // 閉じが開きを止める: 5ms（フェード）後、開きの寄与は0。閉じだけが残る。
        let mut pool = VoicePool::new();
        pool.start(&kit, req(open), fade);
        for _ in 0..2000 {
            pool.mix_next_sample(&kit);
        }
        pool.start(&kit, req(closed), fade);
        for _ in 0..fade + 1 {
            pool.mix_next_sample(&kit);
        }
        assert_eq!(pool.active_count(), 1, "閉じが開きを止めていない");

        // 開きどうし: 新しい音が前の音を止める。
        let mut pool = VoicePool::new();
        pool.start(&kit, req(open), fade);
        for _ in 0..2000 {
            pool.mix_next_sample(&kit);
        }
        pool.start(&kit, req(open), fade);
        for _ in 0..fade + 1 {
            pool.mix_next_sample(&kit);
        }
        assert_eq!(pool.active_count(), 1, "開きどうしで前の音が止まっていない");

        // ハイハット以外（スネア）は止めない。
        let mut pool = VoicePool::new();
        pool.start(&kit, req(open), fade);
        pool.start(&kit, req(snare), fade);
        for _ in 0..fade + 1 {
            pool.mix_next_sample(&kit);
        }
        assert_eq!(pool.active_count(), 2, "スネアが開きハイハットを止めてしまった");
    }

    // 現実的な混合条件での頭打ちの張り付き割合。
    // 閉じハイハットを30ms間隔で連打しつつ、キックとスネアも鳴らす（この3音の場面。
    // 8音すべてが同時に鳴る場面は実際の打鍵では起こらないので扱わない）。
    // 出力は audio.rs と同じ式（合算×VOICE_GAIN→tanh）。張り付き＝出力の絶対値が0.999以上。
    #[test]
    fn realistic_mix_does_not_stick_to_full_scale() {
        let ratio = mixed_scene_stuck_ratio(SR);
        assert!(ratio.stuck < 0.001, "頭打ちに張り付くサンプルの割合が高い: {:.3}%", ratio.stuck * 100.0);
        // 張り付き0%でも波形が潰れていないこと（出力の連続同値が無い）。
        assert!(ratio.flat_run <= 2, "出力が頭打ちで平らになっている: {}個連続", ratio.flat_run);
    }

    struct SceneResult {
        stuck: f32,
        pre_tanh_over_2: f32,
        out_peak: f32,
        flat_run: usize,
    }

    fn mixed_scene_stuck_ratio(sample_rate: u32) -> SceneResult {
        let kit = build_free_kit(sample_rate);
        let picker = VariantPicker::new(8);
        let mut pool = VoicePool::new();
        let (closed, kick_i, snare) =
            (kit.index_of("hat_closed").unwrap(), kit.index_of("kick").unwrap(), kit.index_of("snare").unwrap());
        let fade = (sample_rate as f32 * 0.005).round() as u32;
        let interval = sample_rate as usize * 30 / 1000;
        let total = sample_rate as usize * 2;
        let gain = crate::audio::VOICE_GAIN;
        let (mut stuck, mut over2) = (0usize, 0usize);
        let mut out = Vec::with_capacity(total);
        for i in 0..total {
            // 閉じハイハットは1秒間30ms間隔、キックとスネアは0.2秒に同時、0.6秒にもう一度。
            let hit = |s: usize, pool: &mut VoicePool| {
                let v = picker.pick(s, VARIANT_COUNT);
                pool.start(&kit, PlayRequest { sound: s as u16, variant: v as u8, volume: 1.0 }, fade);
            };
            if i % interval == 0 && i < sample_rate as usize {
                hit(closed, &mut pool);
            }
            if i == sample_rate as usize / 5 || i == sample_rate as usize * 3 / 5 {
                hit(kick_i, &mut pool);
                hit(snare, &mut pool);
            }
            let sum = pool.mix_next_sample(&kit) * gain;
            if sum.abs() > 2.0 {
                over2 += 1;
            }
            let y = sum.tanh();
            if y.abs() >= 0.999 {
                stuck += 1;
            }
            out.push(y);
        }
        SceneResult {
            stuck: stuck as f32 / total as f32,
            pre_tanh_over_2: over2 as f32 / total as f32,
            out_peak: peak(&out),
            flat_run: longest_flat_run(&out, 0.01),
        }
    }

    // ---- 手動で実行する計測・書き出し（`cargo test -- --ignored --nocapture`） ----

    /// 起動時の合成にかかる時間と、増えたメモリ（波形バッファの合計サイズ）を表示する。
    #[test]
    #[ignore = "計測用。cargo test --release -- --ignored --nocapture measure_startup"]
    fn measure_startup_synthesis() {
        // プロセスの常駐メモリ（KB）。`ps` が無い環境では None。
        fn rss_kb() -> Option<u64> {
            let out = std::process::Command::new("ps")
                .args(["-o", "rss=", "-p", &std::process::id().to_string()])
                .output()
                .ok()?;
            String::from_utf8(out.stdout).ok()?.trim().parse().ok()
        }
        for sample_rate in [44_100, 48_000, 96_000] {
            let rss_before = rss_kb();
            let start = std::time::Instant::now();
            let kit = build_free_kit(sample_rate);
            let elapsed = start.elapsed();
            let rss_after = rss_kb();
            let bytes: usize = (0..8).map(|i| kit.buffer_bytes(i)).sum();
            println!(
                "[計測] {sample_rate}Hz: 合成 {:.1} ms / 波形バッファ {:.2} MB / 常駐メモリ増 {:?} KB",
                elapsed.as_secs_f64() * 1000.0,
                bytes as f64 / 1_048_576.0,
                rss_before.zip(rss_after).map(|(b, a)| a as i64 - b as i64)
            );
            drop(kit);
        }
    }

    #[test]
    #[ignore = "計測用。cargo test --release -- --ignored --nocapture measure_mixed_scene"]
    fn measure_mixed_scene() {
        for sample_rate in [44_100, 48_000] {
            let r = mixed_scene_stuck_ratio(sample_rate);
            println!(
                "[計測] {sample_rate}Hz 混合: 張り付き(|出力|>=0.999) {:.4}% / 合算x0.7が2超 {:.4}% / 出力ピーク {:.3} / 連続同値 {}",
                r.stuck * 100.0,
                r.pre_tanh_over_2 * 100.0,
                r.out_peak,
                r.flat_run
            );
        }
    }

    /// 16bit PCM・モノラルの WAV にする。
    fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut b = Vec::with_capacity(44 + data_len as usize);
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // モノラル
        b.extend_from_slice(&sample_rate.to_le_bytes());
        b.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
        }
        b
    }

    #[test]
    fn wav_writer_produces_a_valid_header_and_quantized_samples() {
        let bytes = wav_bytes(&[0.0, 0.5, -0.5, 1.0], 48_000);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len() - 8);
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 48_000);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8);
        let pcm: Vec<i16> = bytes[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(pcm, vec![0, 16384, -16384, 32767]);
    }

    /// 8音×4変種=32個の WAV を書き出す（音声デバイス不要）。
    /// 出力先は環境変数 `DRUMCLACK_WAV_DIR`（無ければ `target/wav`）。
    /// 例: `DRUMCLACK_WAV_DIR=wav-out cargo test --release -- --ignored export_wavs`
    #[test]
    #[ignore = "WAV 書き出し用。CI の成果物で人が聞く"]
    fn export_wavs() {
        let dir = std::env::var("DRUMCLACK_WAV_DIR").unwrap_or_else(|_| "target/wav".to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let mut count = 0;
        for r in all_rendered(SR) {
            for (v, s) in r.variants.iter().enumerate() {
                let path = std::path::Path::new(&dir).join(format!("{}_v{}.wav", r.name, v + 1));
                std::fs::write(&path, wav_bytes(s, SR)).unwrap();
                count += 1;
            }
        }
        assert_eq!(count, 32);
        println!("[書き出し] {count} 個の WAV を {dir} に書きました");
    }
}
