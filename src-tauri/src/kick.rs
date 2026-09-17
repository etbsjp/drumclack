//! 808風バスドラム（キック）の音声合成。
//!
//! サイン波を基本波形として、ピッチ（周波数）と音量（振幅）の両方を
//! 時間経過で指数的に減衰させることで、808系のキックドラム特有の
//! 「ボゥン」という短い低音を作る。起動時に一度だけ合成し、
//! 以降はメモリ上のバッファを使い回す（毎回合成しない）。

/// キックの合成パラメータ。
/// 値は808キックらしい聴感になるよう試作段階で決め打ちしている。
struct KickParams {
    /// 発音開始時のピッチ（Hz）。ここから `end_freq_hz` へ減衰する。
    start_freq_hz: f32,
    /// 減衰しきった後の最終ピッチ（Hz）。
    end_freq_hz: f32,
    /// ピッチ減衰の時定数（秒）。小さいほど急激に音程が下がる。
    pitch_decay_sec: f32,
    /// 音量減衰の時定数（秒）。小さいほど早く無音になる。
    amp_decay_sec: f32,
    /// 音全体の長さ（秒）。この長さでバッファを打ち切る。
    duration_sec: f32,
}

const KICK_PARAMS: KickParams = KickParams {
    start_freq_hz: 150.0,
    end_freq_hz: 45.0,
    pitch_decay_sec: 0.035,
    amp_decay_sec: 0.28,
    duration_sec: 0.45,
};

/// 波形が超えてはいけない最大振幅。音割れ（クリッピング）防止のための上限。
pub const PEAK_AMPLITUDE: f32 = 0.9;

/// 指定サンプルレートでキック1音分のモノラル波形（f32、-1.0〜1.0）を合成する。
///
/// - ピッチは `start_freq_hz` から `end_freq_hz` へ指数的に減衰する。
/// - 音量は 1.0 から指数的に減衰し、末尾でほぼ無音になる。
/// - 最終的な振幅は [`PEAK_AMPLITUDE`] でクランプし、音割れを防ぐ。
pub fn synthesize_kick(sample_rate: u32) -> Vec<f32> {
    let p = &KICK_PARAMS;
    let sample_count = ((p.duration_sec * sample_rate as f32).ceil() as usize).max(1);
    let mut samples = Vec::with_capacity(sample_count);

    // 位相を累積で持つ（瞬間周波数が変化するため、単純に t*freq では計算できない）。
    let mut phase: f32 = 0.0;
    let dt = 1.0 / sample_rate as f32;

    for i in 0..sample_count {
        let t = i as f32 * dt;

        // ピッチの指数減衰: start から end へ収束する。
        let freq =
            p.end_freq_hz + (p.start_freq_hz - p.end_freq_hz) * (-t / p.pitch_decay_sec).exp();

        // 音量の指数減衰。
        let amp = (-t / p.amp_decay_sec).exp();

        phase += freq * dt;
        let raw = (std::f32::consts::TAU * phase).sin() * amp;

        samples.push(raw.clamp(-PEAK_AMPLITUDE, PEAK_AMPLITUDE));
    }

    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kick_is_not_silent() {
        let samples = synthesize_kick(44_100);
        let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(
            peak > 0.1,
            "合成音の最大振幅が小さすぎます（無音に近い）: peak={peak}"
        );
    }

    #[test]
    fn kick_does_not_clip() {
        let samples = synthesize_kick(44_100);
        for (i, s) in samples.iter().enumerate() {
            assert!(
                s.abs() <= PEAK_AMPLITUDE + f32::EPSILON,
                "サンプル{i}が許容振幅を超えています（音割れ）: {s}"
            );
            assert!(s.is_finite(), "サンプル{i}が非数です: {s}");
        }
    }

    #[test]
    fn kick_length_matches_sample_rate() {
        let sr = 48_000;
        let samples = synthesize_kick(sr);
        // duration_sec 分のサンプル数が確保されていること。
        let expected_min = (KICK_PARAMS.duration_sec * sr as f32 * 0.9) as usize;
        assert!(samples.len() >= expected_min);
    }
}
