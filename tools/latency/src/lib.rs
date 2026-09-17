//! 打鍵音（キークリック等の物理的な操作音）と、それに続く発音（キック音等のアプリが鳴らす音）
//! の立ち上がり時刻の差（レイテンシ）を WAV の波形から検出するためのライブラリ。
//!
//! 検出アルゴリズムの考え方:
//!
//! 1. 振幅（絶対値）の低いしきい値（`click_threshold`）を最初に超えたところを「打鍵音（クリック）」
//!    の立ち上がりとみなす。
//! 2. その直後から一定時間内（`max_window_ms`）に、より高いしきい値（`kick_threshold`）を
//!    最初に超えたところを「発音（キック等）」の立ち上がりとみなす。
//!    - 打鍵音自体の音量は `kick_threshold` を超えない前提（超えないようにしきい値を調整する）。
//!    - もし遅延が 0ms に近く、打鍵音と発音がほぼ同時に鳴っている場合は、合成波形の振幅が
//!      クリックのしきい値を超えた瞬間に、すでに発音側のしきい値も超えているため、
//!      同じサンプル位置がクリックの立ち上がりにも発音の立ち上がりにもなり、結果として
//!      正しく「差 0ms」と判定できる。
//! 3. 発音（または未ペアの打鍵音）を検出したら、そこから振幅が `click_threshold` を
//!    `quiet_ms` の間ずっと連続して下回るまでは次の打鍵音の検出を休止する。
//!    発音の減衰は正弦波的に0点を何度も横切るため、瞬時振幅が一瞬しきい値を下回った
//!    だけでは「鳴り止んだ」と判定せず、一定時間連続して静かな状態が続いたことを
//!    もって初めて次の打鍵音の検出を再開する（そうしないと、まだ鳴っている発音の
//!    減衰の途中を新しい打鍵音として誤検出してしまう）。
//! 4. `max_window_ms` 以内に発音が見つからなかった場合は「打鍵音は検出したが発音が見つからな
//!    かった（未ペア）」として記録し、次の打鍵音の検出へ進む。

use std::path::Path;

/// 検出パラメータ。
#[derive(Debug, Clone)]
pub struct DetectConfig {
    /// 打鍵音（クリック）の立ち上がりとみなす振幅のしきい値（0.0〜1.0、フルスケール比）。
    pub click_threshold: f32,
    /// 発音（キック等）の立ち上がりとみなす振幅のしきい値（0.0〜1.0、フルスケール比）。
    /// `click_threshold` より大きい値にすること。
    pub kick_threshold: f32,
    /// 打鍵音の後、発音を探す最大時間（ミリ秒）。この時間内に発音が見つからなければ未ペア扱い。
    pub max_window_ms: f64,
    /// 発音（または未ペアの打鍵音）を検出した後、次の打鍵音の検出を再開するために必要な、
    /// 「振幅が `click_threshold` を連続して下回っている」静けさの継続時間（ミリ秒）。
    /// 発音の減衰・余韻を次の打鍵音と誤検出しないための設定。
    /// 想定する発音の最低周波数の1周期より十分長い値にする（短すぎると、まだ減衰しきっていない
    /// 音の0点交差を「静か」と誤判定し、その振動の山を新しい打鍵音として誤検出する）。
    pub quiet_ms: f64,
    /// エンベロープフォロワーの release 時間（ミリ秒）。
    /// 減衰していく音（特に低い周波数の音）は波形が0点を何度も横切るため、瞬時振幅を
    /// そのまま使うと「まだ鳴っているのに一瞬しきい値を下回った」と誤判定してしまう。
    /// この時間だけピーク値を保持してから減衰させることで、その誤判定を防ぐ
    /// （立ち上がり検出の精度には影響しない。減衰側のみ滑らかにする）。
    pub release_ms: f64,
}

impl Default for DetectConfig {
    fn default() -> Self {
        Self {
            click_threshold: 0.02,
            kick_threshold: 0.2,
            max_window_ms: 300.0,
            quiet_ms: 30.0,
            release_ms: 8.0,
        }
    }
}

/// 検出できた「打鍵音→発音」の1組。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pair {
    /// 打鍵音の立ち上がり時刻（先頭からの経過ミリ秒）。
    pub click_ms: f64,
    /// 発音の立ち上がり時刻（先頭からの経過ミリ秒）。
    pub kick_ms: f64,
    /// 遅延（ミリ秒） = kick_ms - click_ms。
    pub delay_ms: f64,
}

/// 解析結果。
#[derive(Debug, Clone, Default)]
pub struct AnalysisResult {
    /// 検出できた（打鍵音, 発音）の組。
    pub pairs: Vec<Pair>,
    /// 打鍵音は検出できたが、`max_window_ms` 以内に発音が見つからなかった回数。
    pub unmatched_clicks: usize,
}

impl AnalysisResult {
    /// 検出組数が0件かどうか。
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// 遅延（ミリ秒）の中央値。組が無ければ `None`。
    pub fn median_delay_ms(&self) -> Option<f64> {
        if self.pairs.is_empty() {
            return None;
        }
        let mut delays: Vec<f64> = self.pairs.iter().map(|p| p.delay_ms).collect();
        delays.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = delays.len();
        if n % 2 == 1 {
            Some(delays[n / 2])
        } else {
            Some((delays[n / 2 - 1] + delays[n / 2]) / 2.0)
        }
    }

    /// 遅延（ミリ秒）の最大値。組が無ければ `None`。
    pub fn max_delay_ms(&self) -> Option<f64> {
        self.pairs
            .iter()
            .map(|p| p.delay_ms)
            .fold(None, |acc, v| match acc {
                None => Some(v),
                Some(m) if v > m => Some(v),
                Some(m) => Some(m),
            })
    }
}

/// 各サンプル位置の「エンベロープ」（振幅の絶対値、複数チャンネルがある場合はチャンネル間の最大値）
/// を計算する。
fn envelope(samples_per_channel: &[Vec<f32>]) -> Vec<f32> {
    let len = samples_per_channel.first().map(|c| c.len()).unwrap_or(0);
    let mut env = vec![0.0f32; len];
    for ch in samples_per_channel {
        for (i, &s) in ch.iter().enumerate() {
            let a = s.abs();
            if a > env[i] {
                env[i] = a;
            }
        }
    }
    env
}

/// モノラル化前の、チャンネルごとのサンプル列とサンプルレートから解析を行う。
pub fn analyze_channels(
    samples_per_channel: &[Vec<f32>],
    sample_rate: u32,
    cfg: &DetectConfig,
) -> AnalysisResult {
    let env = envelope(samples_per_channel);
    analyze_envelope(&env, sample_rate, cfg)
}

/// 瞬時振幅列にピークホールド式のエンベロープフォロワー（release）をかける。
///
/// 減衰していく音（特に低い周波数の音）は波形が0点を何度も横切るため、瞬時振幅を
/// そのまま閾値判定に使うと「まだ鳴っているのに一瞬しきい値を下回った」と誤判定してしまう。
/// ここでピーク値を `release_ms` の時定数で保持してから減衰させることで、その誤判定を防ぐ。
/// 立ち上がり（アタック）はそのまま反映されるため、オンセット検出の精度には影響しない。
fn apply_release(raw: &[f32], sample_rate: u32, release_ms: f64) -> Vec<f32> {
    if raw.is_empty() {
        return Vec::new();
    }
    let release_samples = (release_ms / 1000.0 * sample_rate as f64).max(1.0);
    // 1サンプルあたりの減衰係数（時定数 release_samples の指数減衰）。
    let decay_per_sample = (-1.0 / release_samples).exp() as f32;

    let mut out = vec![0.0f32; raw.len()];
    out[0] = raw[0];
    for i in 1..raw.len() {
        let held = out[i - 1] * decay_per_sample;
        out[i] = if raw[i] > held { raw[i] } else { held };
    }
    out
}

/// `start` 以降で、振幅が `threshold` を `quiet_samples` 個連続して下回る区間を探し、
/// その区間の直後のインデックスを返す。見つからなければ配列の末尾（`env.len()`）を返す。
fn find_quiet_point(env: &[f32], start: usize, threshold: f32, quiet_samples: usize) -> usize {
    let n = env.len();
    let mut run = 0usize;
    let mut idx = start;
    while idx < n {
        if env[idx] < threshold {
            run += 1;
            if run >= quiet_samples {
                return idx + 1;
            }
        } else {
            run = 0;
        }
        idx += 1;
    }
    n
}

/// 既にモノラル化された振幅の絶対値列（エンベロープ）から解析を行う。
/// テストなどで直接波形を組み立てて検証する場合はこちらを使う。
pub fn analyze_envelope(env: &[f32], sample_rate: u32, cfg: &DetectConfig) -> AnalysisResult {
    let mut result = AnalysisResult::default();
    if env.is_empty() || sample_rate == 0 {
        return result;
    }

    let env = apply_release(env, sample_rate, cfg.release_ms);
    let env = env.as_slice();

    let ms_per_sample = 1000.0 / sample_rate as f64;
    let window_samples = ((cfg.max_window_ms / ms_per_sample).round() as usize).max(1);
    let quiet_samples = ((cfg.quiet_ms / ms_per_sample).round() as usize).max(1);

    let mut i = 0usize;
    let n = env.len();

    while i < n {
        // 1. 打鍵音（クリック）の立ち上がりを探す。
        let click_idx = match (i..n).find(|&idx| env[idx] >= cfg.click_threshold) {
            Some(idx) => idx,
            None => break,
        };

        // 2. click_idx から max_window_ms 以内で発音（キック）の立ち上がりを探す。
        let search_end = (click_idx + window_samples).min(n);
        let kick_idx = (click_idx..search_end).find(|&idx| env[idx] >= cfg.kick_threshold);

        let scan_from = match kick_idx {
            Some(k_idx) => {
                let click_ms = click_idx as f64 * ms_per_sample;
                let kick_ms = k_idx as f64 * ms_per_sample;
                result.pairs.push(Pair {
                    click_ms,
                    kick_ms,
                    delay_ms: kick_ms - click_ms,
                });
                k_idx
            }
            None => {
                // 発音が見つからなかった: 未ペアの打鍵音として記録する。
                result.unmatched_clicks += 1;
                click_idx
            }
        };

        // 3. 振幅が click_threshold を quiet_ms の間ずっと連続して下回るまで、次の打鍵音の
        // 検出を休止する。単発の0点交差では「静か」と判定しない（減衰中の音の誤検出防止）。
        i = find_quiet_point(env, scan_from, cfg.click_threshold, quiet_samples);
    }

    result
}

/// WAV ファイルを読み込み、チャンネルごとのサンプル列（-1.0〜1.0 に正規化した f32）と
/// サンプルレートを返す。
///
/// 対応フォーマット: 8/16/24/32bit 整数 PCM、および 32bit 浮動小数点 PCM。
pub fn read_wav_channels(path: &Path) -> Result<(Vec<Vec<f32>>, u32), String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("WAV ファイルを開けませんでした: {e}"))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let sample_rate = spec.sample_rate;

    let mut per_channel: Vec<Vec<f32>> = vec![Vec::new(); channels.max(1)];

    match spec.sample_format {
        hound::SampleFormat::Float => {
            let samples = reader
                .samples::<f32>()
                .collect::<Result<Vec<f32>, _>>()
                .map_err(|e| format!("WAV サンプルの読み取りに失敗しました: {e}"))?;
            for (i, s) in samples.into_iter().enumerate() {
                per_channel[i % channels].push(s);
            }
        }
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max_amplitude = (1i64 << (bits - 1)) as f32;
            let samples = reader
                .samples::<i32>()
                .collect::<Result<Vec<i32>, _>>()
                .map_err(|e| format!("WAV サンプルの読み取りに失敗しました: {e}"))?;
            for (i, s) in samples.into_iter().enumerate() {
                per_channel[i % channels].push(s as f32 / max_amplitude);
            }
        }
    }

    Ok((per_channel, sample_rate))
}

/// WAV ファイルを読み込んで解析するショートカット関数。
pub fn analyze_wav_file(path: &Path, cfg: &DetectConfig) -> Result<AnalysisResult, String> {
    let (channels, sample_rate) = read_wav_channels(path)?;
    Ok(analyze_channels(&channels, sample_rate, cfg))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 指数減衰する波形を `dst` に加算する（クリック音・キック音の合成に使う）。
    fn add_decaying_tone(
        dst: &mut [f32],
        start_sample: usize,
        sample_rate: u32,
        freq_hz: f32,
        peak_amplitude: f32,
        decay_ms: f32,
    ) {
        let sr = sample_rate as f32;
        let tau_samples = (decay_ms / 1000.0) * sr / 3.0; // 3τで概ね減衰しきる目安
        for (offset, sample) in dst.iter_mut().skip(start_sample).enumerate() {
            let t = offset as f32 / sr;
            let envelope = (-(offset as f32) / tau_samples.max(1.0)).exp();
            let value =
                peak_amplitude * envelope * (2.0 * std::f32::consts::PI * freq_hz * t).sin();
            *sample += value;
            if envelope < 1e-4 {
                break;
            }
        }
    }

    /// 「クリック音の delay_ms 後にキック音」を pair_count 組、interval_ms 間隔で並べた
    /// モノラル波形（振幅の絶対値＝エンベロープではなく、実波形そのもの）を生成する。
    fn build_wave(
        sample_rate: u32,
        pair_count: usize,
        delay_ms: f64,
        interval_ms: f64,
    ) -> Vec<f32> {
        let total_ms = interval_ms * pair_count as f64 + 500.0;
        let total_samples = (total_ms / 1000.0 * sample_rate as f64) as usize;
        let mut wave = vec![0.0f32; total_samples];

        for i in 0..pair_count {
            let click_start_ms = 200.0 + interval_ms * i as f64;
            let click_start = (click_start_ms / 1000.0 * sample_rate as f64).round() as usize;
            // 打鍵音: 短く減衰する高めの音（クリック的なノイズの代わりに高周波トーンで模擬）。
            add_decaying_tone(&mut wave, click_start, sample_rate, 3000.0, 0.15, 5.0);

            let kick_start_ms = click_start_ms + delay_ms;
            let kick_start = (kick_start_ms / 1000.0 * sample_rate as f64).round() as usize;
            // 発音: 低めの周波数で、クリックより大きく、長めに減衰する音（キック的）。
            add_decaying_tone(&mut wave, kick_start, sample_rate, 80.0, 0.8, 150.0);
        }

        wave
    }

    fn to_envelope(wave: &[f32]) -> Vec<f32> {
        wave.iter().map(|s| s.abs()).collect()
    }

    fn assert_delays_close(result: &AnalysisResult, expected_delay_ms: f64, tolerance_ms: f64) {
        assert!(
            !result.pairs.is_empty(),
            "ペアが1つも検出されなかった（期待値: {expected_delay_ms}ms 付近）"
        );
        for (idx, pair) in result.pairs.iter().enumerate() {
            let diff = (pair.delay_ms - expected_delay_ms).abs();
            assert!(
                diff <= tolerance_ms,
                "組{}: 遅延 {:.3}ms が期待値 {}±{}ms の範囲外",
                idx + 1,
                pair.delay_ms,
                expected_delay_ms,
                tolerance_ms
            );
        }
        let median = result.median_delay_ms().unwrap();
        assert!(
            (median - expected_delay_ms).abs() <= tolerance_ms,
            "中央値 {:.3}ms が期待値 {}±{}ms の範囲外",
            median,
            expected_delay_ms,
            tolerance_ms
        );
    }

    #[test]
    fn detects_100ms_delay_for_ten_pairs() {
        let sample_rate = 44_100;
        let wave = build_wave(sample_rate, 10, 100.0, 500.0);
        let env = to_envelope(&wave);
        let cfg = DetectConfig::default();
        let result = analyze_envelope(&env, sample_rate, &cfg);

        assert_eq!(result.pairs.len(), 10, "10組すべてを検出できていない");
        assert_eq!(result.unmatched_clicks, 0);
        assert_delays_close(&result, 100.0, 2.0);
    }

    #[test]
    fn detects_30ms_delay_for_ten_pairs() {
        let sample_rate = 44_100;
        let wave = build_wave(sample_rate, 10, 30.0, 500.0);
        let env = to_envelope(&wave);
        let cfg = DetectConfig::default();
        let result = analyze_envelope(&env, sample_rate, &cfg);

        assert_eq!(result.pairs.len(), 10, "10組すべてを検出できていない");
        assert_eq!(result.unmatched_clicks, 0);
        assert_delays_close(&result, 30.0, 2.0);
    }

    #[test]
    fn detects_0ms_delay_for_ten_pairs() {
        let sample_rate = 44_100;
        let wave = build_wave(sample_rate, 10, 0.0, 500.0);
        let env = to_envelope(&wave);
        let cfg = DetectConfig::default();
        let result = analyze_envelope(&env, sample_rate, &cfg);

        assert_eq!(result.pairs.len(), 10, "10組すべてを検出できていない");
        assert_eq!(result.unmatched_clicks, 0);
        assert_delays_close(&result, 0.0, 2.0);
    }

    #[test]
    fn silence_reports_zero_pairs_not_zero_ms() {
        let sample_rate = 44_100;
        // 2秒分の完全な無音。
        let env = vec![0.0f32; sample_rate as usize * 2];
        let cfg = DetectConfig::default();
        let result = analyze_envelope(&env, sample_rate, &cfg);

        assert!(result.is_empty(), "無音なのにペアを検出してしまっている");
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.unmatched_clicks, 0);
        // 「0組」であることは median が None であることでも確認できる（0ms と誤解されない）。
        assert_eq!(result.median_delay_ms(), None);
        assert_eq!(result.max_delay_ms(), None);
    }

    #[test]
    fn click_without_kick_is_reported_as_unmatched_not_as_zero_delay() {
        let sample_rate = 44_100;
        let mut wave = vec![0.0f32; sample_rate as usize];
        // クリック音だけを鳴らし、キック音は鳴らさない（アプリが無反応なケースを模擬）。
        add_decaying_tone(&mut wave, 4410, sample_rate, 3000.0, 0.15, 5.0);
        let env = to_envelope(&wave);
        let cfg = DetectConfig::default();
        let result = analyze_envelope(&env, sample_rate, &cfg);

        assert!(result.pairs.is_empty());
        assert_eq!(result.unmatched_clicks, 1);
    }
}
