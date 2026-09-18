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
    /// 末尾のフェードアウトにかける時間（秒）。
    /// `duration_sec` で打ち切った時点ではまだ振幅が残っている
    /// （`exp(-duration_sec/amp_decay_sec)` 分の音量がある）ため、
    /// フェードなしでは最終サンプルと後続の無音との間に不自然な段差
    /// （クリック音）が生じる。末尾のこの時間でなめらかに無音へ収束させる。
    fade_out_sec: f32,
}

const KICK_PARAMS: KickParams = KickParams {
    start_freq_hz: 150.0,
    end_freq_hz: 45.0,
    pitch_decay_sec: 0.035,
    amp_decay_sec: 0.28,
    duration_sec: 0.45,
    fade_out_sec: 0.008,
};

/// 波形の最大振幅。音割れ（クリッピング）防止のための上限だが、
/// clampではなく**乗算でスケールする**ことで実現する（後述）。
pub const PEAK_AMPLITUDE: f32 = 0.9;

/// 指定サンプルレートでキック1音分のモノラル波形（f32、-1.0〜1.0）を合成する。
///
/// - ピッチは `start_freq_hz` から `end_freq_hz` へ指数的に減衰する。
/// - 音量は 1.0 から指数的に減衰し、末尾でほぼ無音になる。
/// - 最終的な振幅は [`PEAK_AMPLITUDE`] を乗算してスケールする（音割れ防止）。
///   以前は `clamp(-PEAK_AMPLITUDE, PEAK_AMPLITUDE)` で頭を切っていたが、
///   サイン波の最大値は1.0のため鳴り始めの振幅が大きい区間（数msec）が
///   PEAK_AMPLITUDEで平らに潰れてしまい、それ自体が音割れの原因になっていた
///   （issue #6 原因A）。乗算スケールならピークの形を保ったまま音量だけ下がる。
/// - 末尾は `fade_out_sec` でなめらかにフェードアウトさせる。`duration_sec`
///   で単純に打ち切ると、その時点でまだ振幅が残っているため後続の無音との間に
///   不自然な段差（「プツッ」というクリック音）が生じていた（issue #6 原因B）。
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

        // 末尾のフェードアウト: 残り時間が fade_out_sec を下回ったら
        // 1.0 から 0.0 へ線形に収束させる（それより前は 1.0 で無効化）。
        let time_to_end = p.duration_sec - t;
        let fade = (time_to_end / p.fade_out_sec).clamp(0.0, 1.0);

        phase += freq * dt;
        let raw = (std::f32::consts::TAU * phase).sin();

        samples.push(raw * amp * fade * PEAK_AMPLITUDE);
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

    // issue #6 で指摘された通り、「全サンプルが±PEAK_AMPLITUDE以内」という
    // 以前のアサーションは、まさにそのclampが音割れの原因（原因A）だったため
    // 欠陥を正常と認定してしまっていた。以下の2点を確認する内容に作り直す。
    // - 頭打ち（クリップ張り付き）していないこと（原因A）
    // - 末尾に不自然な段差が無いこと（原因B）
    #[test]
    fn kick_does_not_clip() {
        let samples = synthesize_kick(48_000);

        for (i, s) in samples.iter().enumerate() {
            assert!(s.is_finite(), "サンプル{i}が非数です: {s}");
            assert!(s.abs() <= 1.0, "サンプル{i}が±1.0を超えています（音割れ）: {s}");
        }

        // 頭打ち（クリップ張り付き）していないこと。
        // 以前は sin(...) * amp を PEAK_AMPLITUDE で clamp していたため、
        // 鳴り始めの振幅が大きい区間で複数サンプルがまったく同じ値
        // （PEAK_AMPLITUDEそのもの）に張り付き、波形の頂点が平らに潰れていた。
        // なめらかなsin波なら隣接サンプルがビット同一になることは実質無いので、
        // 「振幅が大きい（0.01超）区間で連続して同じ値が続く」ことを検出する。
        let mut run = 1usize;
        let mut max_run = 1usize;
        for i in 1..samples.len() {
            if samples[i] == samples[i - 1] && samples[i].abs() > 0.01 {
                run += 1;
                max_run = max_run.max(run);
            } else {
                run = 1;
            }
        }
        assert!(
            max_run <= 2,
            "頭打ち（クリップ張り付き）と思われる連続同値サンプルがあります: {max_run}個連続"
        );

        // 末尾に不自然な段差が無いこと。
        // 以前は duration_sec でバッファを打ち切っており、その時点でまだ
        // 振幅が残っている（フェード無し）ため、最終サンプルと後続の無音（0）
        // との段差が、波形内で自然に生じる隣接サンプル差の最大値を大きく
        // 超えていた（「プツッ」というクリック音）。フェードアウト後は、
        // 末尾サンプルの絶対値が波形内の自然な段差の最大値以下になるはず。
        let mut max_adjacent_diff = 0.0_f32;
        for i in 1..samples.len() {
            max_adjacent_diff = max_adjacent_diff.max((samples[i] - samples[i - 1]).abs());
        }
        let gap_to_silence = samples.last().unwrap().abs();
        assert!(
            gap_to_silence <= max_adjacent_diff,
            "末尾サンプルの振幅（{gap_to_silence}）が波形内の自然な段差の最大値（{max_adjacent_diff}）を超えています（プツッというノイズになる）"
        );
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
