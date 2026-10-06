//! 強弱。直前の打鍵からの間隔で、1打ごとの音量を決める。
//!
//! 間が空いた打鍵ほど強く、連打は軽くする。基準は「直前に鳴らした打鍵からの間隔」だけで、
//! どのキーかは問わない（キーの位置はここへ渡さない）。無音のキーは呼び出し側が数えない。
//! 時刻は単調時計（`Instant`）で、打鍵の時点に呼び出し側が取って渡す。画面表示用の
//! 壁時計（`AudioEngine::last_accepted_play_ms`）とは別物で、混ぜない。
//!
//! 音量の数値はすべて仮の値。実機で聞いて決める（調整は下の定数だけで済む）。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::drums::Rng;

/// この間隔（ミリ秒）以下は最小の音量。仮値。
pub const SHORT_INTERVAL_MS: f32 = 80.0;
/// この間隔（ミリ秒）以上は最大の音量。仮値。
pub const LONG_INTERVAL_MS: f32 = 400.0;
/// 幅 1.0 のとき、最小の音量が最大の音量より下がる量（dB）。仮値。
pub const MIN_ATTENUATION_DB: f32 = 10.0;
/// 毎回の揺らぎの大きさ（±dB）。幅に比例し、幅 0 では 0。仮値。
pub const JITTER_DB: f32 = 1.0;
/// 揺らぎの乱数の既定の種。テストでは別の値で作れる。
const DEFAULT_JITTER_SEED: u32 = 0xD1_CE_5E;

/// 間隔の位置（0.0 = 最小側 〜 1.0 = 最大側）を、音量の割合（0.0〜1.0）へ直す曲線。仮値。
/// 滑らかにつなぐため、両端の傾きが 0 の smoothstep にしている。
/// 単調増加であること（間隔が長いほど強い）を崩さないこと。
fn curve(position: f32) -> f32 {
    position * position * (3.0 - 2.0 * position)
}

/// 間隔から、80ms 以下を 0.0、400ms 以上を 1.0 とした位置を作る。
fn interval_position(interval: Duration) -> f32 {
    let ms = interval.as_secs_f32() * 1000.0;
    ((ms - SHORT_INTERVAL_MS) / (LONG_INTERVAL_MS - SHORT_INTERVAL_MS)).clamp(0.0, 1.0)
}

fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// 1打の音量（1.0 が上限）。
///
/// - `interval`: 直前の打鍵からの間隔。直前が無い（最初の打鍵）ときは `None` で、最大側として扱う。
/// - `width`: 強弱の幅（0.0〜1.0。範囲外・数値でない値は丸める）。0 で常に 1.0。
/// - `jitter_unit`: 揺らぎの乱数（-1.0〜1.0）。±[`JITTER_DB`]×幅 dB の音量の揺れになる。
///
/// 揺らぎで上限の 1.0 を超えないよう、最大の音量はあらかじめ揺らぎの分だけ下げてある
/// （幅 0 では下げない）。
pub fn volume_for(interval: Option<Duration>, width: f32, jitter_unit: f32) -> f32 {
    let width = if width.is_finite() { width.clamp(0.0, 1.0) } else { 0.0 };
    let jitter_db = JITTER_DB * width;
    let loudest = db_to_gain(-jitter_db);
    let quietest = loudest * db_to_gain(-MIN_ATTENUATION_DB * width);
    let position = interval.map_or(1.0, interval_position);
    let level = quietest + (loudest - quietest) * curve(position);
    let jitter = if jitter_unit.is_finite() { jitter_unit.clamp(-1.0, 1.0) } else { 0.0 };
    level * db_to_gain(jitter_db * jitter)
}

/// 強弱の状態。キー監視のスレッドが、打鍵ごとに [`Dynamics::next_volume`] を呼ぶ。
/// 音声コールバックからは触らない（音量は発音の要求に載せて渡す）。
pub struct Dynamics {
    /// 強弱の幅（`f32` のビット列）。設定の変更で差し替わる。
    width_bits: AtomicU32,
    /// 直前に鳴らした打鍵の時刻（単調時計）と、揺らぎの乱数。
    inner: Mutex<Inner>,
}

struct Inner {
    last_hit: Option<Instant>,
    rng: Rng,
}

impl Dynamics {
    pub fn new(width: f32) -> Self {
        Self::with_seed(width, DEFAULT_JITTER_SEED)
    }

    /// 揺らぎの乱数の種を指定して作る（テスト用に、列を固定できる）。
    pub fn with_seed(width: f32, seed: u32) -> Self {
        Self {
            width_bits: AtomicU32::new(width.to_bits()),
            inner: Mutex::new(Inner { last_hit: None, rng: Rng::new(seed) }),
        }
    }

    /// 強弱の幅を差し替える。次の打鍵から効く。
    pub fn set_width(&self, width: f32) {
        self.width_bits.store(width.to_bits(), Ordering::Relaxed);
    }

    pub fn width(&self) -> f32 {
        f32::from_bits(self.width_bits.load(Ordering::Relaxed))
    }

    /// 打鍵の時刻 `now`（打鍵の時点で取った単調時計）を受け取り、その打鍵の音量を返して、
    /// `now` を「直前の打鍵」として覚える。音を鳴らす打鍵でだけ呼ぶこと（無音のキーは呼ばない）。
    pub fn next_volume(&self, now: Instant) -> f32 {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // 時計は戻らない想定だが、念のため負の間隔は 0 に丸める。
        let interval = inner.last_hit.map(|last| now.saturating_duration_since(last));
        inner.last_hit = Some(now);
        let jitter_unit = inner.rng.next_signed();
        volume_for(interval, self.width(), jitter_unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Option<Duration> {
        Some(Duration::from_millis(n))
    }

    fn db(gain: f32) -> f32 {
        20.0 * gain.log10()
    }

    #[test]
    fn volume_rises_monotonically_with_interval_and_hits_both_ends() {
        // 揺らぎ 0 で、幅 1.0。80ms 以下は最小、400ms 以上は最大。
        let at = |interval: Option<Duration>| volume_for(interval, 1.0, 0.0);
        let (short, mid, long) = (at(ms(80)), at(ms(240)), at(ms(400)));
        assert!(short < mid && mid < long, "単調に増えていない: {short} {mid} {long}");

        // 両端は決めた値: 最大は揺らぎ分の余白を引いた値、最小はそこから -10dB。
        let loudest = db_to_gain(-JITTER_DB);
        assert!((long - loudest).abs() < 1e-6, "最大が決めた値でない: {long}");
        assert!((db(long) - db(short) - MIN_ATTENUATION_DB).abs() < 1e-3, "最小が最大の-10dBでない");

        // 範囲の外は端と同じ。
        assert_eq!(at(ms(0)), short);
        assert_eq!(at(ms(30)), short);
        assert_eq!(at(ms(5000)), long);
        assert_eq!(at(None), long, "最初の打鍵は最大側");

        // 細かく刻んでも、一度も下がらない。
        let mut previous = 0.0;
        for t in (0..=450).step_by(5) {
            let v = at(ms(t));
            assert!(v >= previous, "{t}ms で下がった: {v} < {previous}");
            previous = v;
        }
    }

    #[test]
    fn middle_interval_is_smoothly_between_the_ends_not_a_step() {
        // 「滑らかにつなぐ」: 端の近くでも途中でも、段差でなく少しずつ変わる。
        let at = |n: u64| volume_for(ms(n), 1.0, 0.0);
        let total = at(400) - at(80);
        let step = (at(81) - at(80)).abs().max((at(400) - at(399)).abs());
        assert!(step < total * 0.02, "端で急に変わっている: {step} / {total}");
        assert!(at(240) > at(80) + total * 0.3 && at(240) < at(400) - total * 0.3, "中間が端に近すぎる");
    }

    #[test]
    fn width_zero_is_constant_even_with_jitter() {
        for interval in [None, ms(0), ms(80), ms(240), ms(400), ms(9999)] {
            for jitter in [-1.0, -0.3, 0.0, 0.7, 1.0] {
                assert_eq!(volume_for(interval, 0.0, jitter), 1.0, "{interval:?} {jitter}");
            }
        }

        // 状態を持つ経路でも同じ。間隔も揺らぎもばらばらな打鍵で、音量が1通りだけ。
        let dynamics = Dynamics::with_seed(0.0, 7);
        let start = Instant::now();
        let mut at_ms = 0;
        let mut seen = Vec::new();
        for gap in [10u64, 90, 500, 33, 250, 400, 80] {
            at_ms += gap;
            seen.push(dynamics.next_volume(start + Duration::from_millis(at_ms)));
        }
        assert!(seen.iter().all(|v| *v == 1.0), "幅0なのに音量が変わった: {seen:?}");
    }

    #[test]
    fn width_scales_the_gap_between_short_and_long_intervals() {
        // 揺らぎを除いた中心の値で、最小が最大の -10dB。幅が半分なら -5dB。
        let ratio_db = |width: f32| db(volume_for(ms(80), width, 0.0)) - db(volume_for(ms(400), width, 0.0));
        assert!((ratio_db(1.0) + 10.0).abs() < 1e-3);
        assert!((ratio_db(0.5) + 5.0).abs() < 1e-3);
        assert!(ratio_db(0.0).abs() < 1e-3);
    }

    #[test]
    fn jitter_is_within_one_db_of_the_centre_and_never_exceeds_full_scale() {
        for interval in [ms(80), ms(240), ms(400)] {
            let centre = volume_for(interval, 1.0, 0.0);
            for jitter in [-1.0, 1.0] {
                let v = volume_for(interval, 1.0, jitter);
                assert!((db(v) - db(centre) - jitter * JITTER_DB).abs() < 1e-3, "揺らぎが±{JITTER_DB}dBでない");
            }
        }
        assert!(volume_for(ms(400), 1.0, 1.0) <= 1.0 + 1e-6, "揺らぎで上限を超えた");

        // 範囲外の値・数値でない値は丸める。
        assert_eq!(volume_for(ms(240), 1.0, 5.0), volume_for(ms(240), 1.0, 1.0));
        assert_eq!(volume_for(ms(240), 1.0, f32::NAN), volume_for(ms(240), 1.0, 0.0));
        assert_eq!(volume_for(ms(240), f32::NAN, 1.0), 1.0);
        assert_eq!(volume_for(ms(240), 9.0, 0.0), volume_for(ms(240), 1.0, 0.0));
    }

    #[test]
    fn seeded_jitter_is_reproducible_in_range_and_actually_varies() {
        let run = |seed: u32| {
            let dynamics = Dynamics::with_seed(1.0, seed);
            let start = Instant::now();
            // 間隔を毎回 240ms に固定し、変わるのは揺らぎだけにする（最初の1打は間隔なしなので除く）。
            (0..200u64)
                .map(|i| dynamics.next_volume(start + Duration::from_millis(240 * i)))
                .skip(1)
                .collect::<Vec<_>>()
        };
        let a = run(42);
        assert_eq!(a, run(42), "同じ種なのに列が違う");
        assert_ne!(a, run(43), "別の種なのに同じ列");

        let centre = volume_for(ms(240), 1.0, 0.0);
        let lowest = centre * db_to_gain(-JITTER_DB);
        let highest = centre * db_to_gain(JITTER_DB);
        assert!(a.iter().all(|v| *v >= lowest - 1e-6 && *v <= highest + 1e-6), "範囲(±1dB)を出た");
        // 実際に揺れていて、両側へ振れている（常に同じ値・片側だけ、ではない）。
        let (min, max) = a.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(max - min > (highest - lowest) * 0.8, "揺らぎが小さすぎる: {min}..{max}");
        assert!(min < centre && max > centre);
    }

    #[test]
    fn interval_is_measured_from_the_previous_hit_only() {
        // 間隔は直前の1打から測る（最初の1打からではない）。
        let dynamics = Dynamics::with_seed(1.0, 9);
        let start = Instant::now();
        let hits = [0u64, 300, 350, 700];
        let v: Vec<f32> = hits.iter().map(|t| dynamics.next_volume(start + Duration::from_millis(*t))).collect();
        // 350ms の打鍵は直前から 50ms（最小側）、700ms の打鍵は直前から 350ms（最大に近い）。
        assert!(v[2] <= volume_for(ms(80), 1.0, 1.0) + 1e-6, "50ms の打鍵が最小側でない: {v:?}");
        assert!(v[3] > volume_for(ms(300), 1.0, -1.0), "350ms の打鍵が大きくない: {v:?}");
    }

    #[test]
    fn width_can_be_changed_while_running() {
        let dynamics = Dynamics::with_seed(0.0, 1);
        let start = Instant::now();
        assert_eq!(dynamics.next_volume(start), 1.0);
        dynamics.set_width(1.0);
        let quiet = dynamics.next_volume(start + Duration::from_millis(30));
        assert!(quiet < 0.4, "幅を上げたのに連打が小さくならない: {quiet}");
    }
}
