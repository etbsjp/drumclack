//! 打鍵音（キークリック等の物理的な操作音）と、それに続く発音（キック音等のアプリが鳴らす音）
//! の立ち上がり時刻の差（レイテンシ）を WAV の波形から検出するためのライブラリ。
//!
//! 検出アルゴリズムの考え方（周波数帯分離方式）:
//!
//! 1. 波形を周波数帯で2つに分離する。打鍵音（クリック等の物理的な操作音）は高域
//!    （`click_highpass_hz` 以上のハイパスフィルタ）、発音（キック等）は低域
//!    （`kick_lowpass_hz` 以下のローパスフィルタ）に主なエネルギーがある、という前提。
//!    それぞれのフィルタは単純な一次IIR（RCフィルタ相当）で実装している。
//! 2. 高域側（打鍵音帯域）・低域側（発音帯域）それぞれで独立にエンベロープ（振幅の絶対値、
//!    ピークホールド式のリリースをかけたもの）を作り、独立にしきい値（`click_threshold` /
//!    `kick_threshold`）を超えた立ち上がり時刻の列を検出する。
//!    - 周波数帯で分離しているため、打鍵音の振幅そのものが発音より大きくても、発音側の
//!      帯域（低域）には打鍵音の高域成分がほとんど残らないため誤検出しない
//!      （逆に発音側が打鍵音側の帯域を汚すのを防ぐのも同様）。振幅の大小関係に依存しない。
//! 3. 打鍵音の立ち上がり時刻を古い順に1つずつ見て、その時刻以降 `max_window_ms` 以内にある
//!    最も早い発音の立ち上がり時刻とペアにする（一度使った発音は次の打鍵音には使い回さない）。
//!    `max_window_ms` 以内に発音が見つからなければ「未ペア（発音なし）」として記録する。
//! 4. ペアの遅延（発音時刻－打鍵音時刻）が `min_delay_ms` 未満（既定3ms）の場合は、
//!    物理的にありえない値として「判定不能」に分類し、中央値・最大値の集計から除外する。
//!    実際の機器・OS・アプリの処理を経て3ms未満で発音することは考えにくいため、そのような
//!    極端に短い値が出た場合は、打鍵音自身の帯域漏れなど検出側の誤りを疑うべき、という考え方。
//! 5. 検出した打鍵音の数より、ペア（判定不能を含む）の数の方が多くなっている場合は、
//!    検出ロジックに矛盾がある可能性があるとして警告を出せるようにしている
//!    （`AnalysisResult::count_mismatch_warning`）。

use std::path::Path;

/// 検出パラメータ。
#[derive(Debug, Clone)]
pub struct DetectConfig {
    /// 打鍵音（クリック）帯域（ハイパス後）で、立ち上がりとみなす振幅のしきい値
    /// （0.0〜1.0、フルスケール比）。
    pub click_threshold: f32,
    /// 発音（キック等）帯域（ローパス後）で、立ち上がりとみなす振幅のしきい値
    /// （0.0〜1.0、フルスケール比）。
    /// 周波数帯で分離しているため、`click_threshold` との大小関係に制約はない。
    pub kick_threshold: f32,
    /// 打鍵音の後、発音を探す最大時間（ミリ秒）。この時間内に発音が見つからなければ未ペア扱い。
    pub max_window_ms: f64,
    /// 発音（または未ペアの打鍵音）を検出した後、次の立ち上がりの検出を再開するために必要な、
    /// 「振幅がしきい値を連続して下回っている」静けさの継続時間（ミリ秒）。
    /// 発音の減衰・余韻を次の立ち上がりと誤検出しないための設定。
    /// 想定する音の最低周波数の1周期より十分長い値にする（短すぎると、まだ減衰しきっていない
    /// 音の0点交差を「静か」と誤判定し、その振動の山を新しい立ち上がりとして誤検出する）。
    pub quiet_ms: f64,
    /// エンベロープフォロワーの release 時間（ミリ秒）。
    /// 減衰していく音（特に低い周波数の音）は波形が0点を何度も横切るため、瞬時振幅を
    /// そのまま使うと「まだ鳴っているのに一瞬しきい値を下回った」と誤判定してしまう。
    /// この時間だけピーク値を保持してから減衰させることで、その誤判定を防ぐ
    /// （立ち上がり検出の精度には影響しない。減衰側のみ滑らかにする）。
    pub release_ms: f64,
    /// 打鍵音帯域を取り出すハイパスフィルタのカットオフ周波数（Hz）。
    /// この周波数以上に主なエネルギーがある音を打鍵音として扱う。
    pub click_highpass_hz: f64,
    /// 発音帯域を取り出すローパスフィルタのカットオフ周波数（Hz）。
    /// この周波数以下に主なエネルギーがある音を発音として扱う。
    pub kick_lowpass_hz: f64,
    /// これ未満の遅延（ミリ秒）は物理的にありえない値とみなし、「判定不能」に分類する。
    /// 中央値・最大値の集計からは除外される。
    pub min_delay_ms: f64,
}

impl Default for DetectConfig {
    fn default() -> Self {
        Self {
            click_threshold: 0.02,
            kick_threshold: 0.1,
            max_window_ms: 300.0,
            quiet_ms: 30.0,
            release_ms: 8.0,
            click_highpass_hz: 1000.0,
            kick_lowpass_hz: 200.0,
            min_delay_ms: 3.0,
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
    /// 検出できた（打鍵音, 発音）の組のうち、遅延が `min_delay_ms` 以上で有効と判断できたもの。
    pub pairs: Vec<Pair>,
    /// 検出できた（打鍵音, 発音）の組のうち、遅延が `min_delay_ms` 未満で
    /// 「判定不能」に分類されたもの（中央値・最大値の集計からは除外する）。
    pub indeterminate_pairs: Vec<Pair>,
    /// 打鍵音は検出できたが、`max_window_ms` 以内に発音が見つからなかった回数。
    pub unmatched_clicks: usize,
    /// 検出した打鍵音の立ち上がりの総数（`pairs` + `indeterminate_pairs` + `unmatched_clicks` の元数）。
    pub detected_clicks: usize,
    /// 検出した発音の立ち上がりの総数。
    pub detected_kicks: usize,
}

impl AnalysisResult {
    /// 有効なペア・判定不能なペアのいずれも1件も無いかどうか。
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty() && self.indeterminate_pairs.is_empty()
    }

    /// 遅延（ミリ秒）の中央値。有効なペア（`pairs`）が無ければ `None`。
    /// 「判定不能」に分類されたペアは集計に含めない。
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

    /// 遅延（ミリ秒）の最大値。有効なペア（`pairs`）が無ければ `None`。
    /// 「判定不能」に分類されたペアは集計に含めない。
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

    /// 有効なペアと判定不能なペースの合計数（打鍵音とペアになった組の総数）。
    pub fn total_paired(&self) -> usize {
        self.pairs.len() + self.indeterminate_pairs.len()
    }

    /// 検出した打鍵数とペアの数が矛盾していないかを確認し、矛盾していれば警告メッセージを返す。
    /// 通常、1つの打鍵音は高々1つのペア（有効または判定不能）にしかならないため、
    /// ペアの総数が検出した打鍵数を超えることは無いはず。超えている場合は、検出ロジックに
    /// 何らかの矛盾がある可能性が高い（結果の信頼性を疑うべき）ことを示す。
    pub fn count_mismatch_warning(&self) -> Option<String> {
        let total = self.total_paired();
        if total > self.detected_clicks {
            Some(format!(
                "検出した打鍵数({})より、ペアとして数えた組数({})の方が多くなっています。検出ロジックに矛盾がある可能性があるため、結果は参考値としてください。",
                self.detected_clicks, total
            ))
        } else {
            None
        }
    }
}

/// 複数チャンネルのサンプル列を平均してモノラル化する（周波数帯分離フィルタは
/// 符号付きの波形が必要なため、振幅の絶対値ではなく単純平均でモノラル化する）。
fn to_mono(samples_per_channel: &[Vec<f32>]) -> Vec<f32> {
    let len = samples_per_channel.first().map(|c| c.len()).unwrap_or(0);
    let channel_count = samples_per_channel.len().max(1) as f32;
    let mut mono = vec![0.0f32; len];
    for ch in samples_per_channel {
        for (i, &s) in ch.iter().enumerate() {
            mono[i] += s;
        }
    }
    for m in mono.iter_mut() {
        *m /= channel_count;
    }
    mono
}

/// 一次ローパスIIRフィルタ（RC回路相当）。`cutoff_hz` 以下の周波数を主に通す。
/// 発音（キック等の低域成分）を打鍵音（高域成分）から分離するために使う。
fn lowpass(raw: &[f32], sample_rate: u32, cutoff_hz: f64) -> Vec<f32> {
    if raw.is_empty() {
        return Vec::new();
    }
    let dt = 1.0 / sample_rate as f64;
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff_hz.max(1.0));
    let alpha = (dt / (rc + dt)) as f32;
    let mut out = vec![0.0f32; raw.len()];
    out[0] = raw[0] * alpha;
    for i in 1..raw.len() {
        out[i] = out[i - 1] + alpha * (raw[i] - out[i - 1]);
    }
    out
}

/// 一次ハイパスIIRフィルタ。`cutoff_hz` 以上の周波数を主に通す。
/// 打鍵音（クリック等の高域成分）を発音（低域成分）から分離するために使う。
fn highpass(raw: &[f32], sample_rate: u32, cutoff_hz: f64) -> Vec<f32> {
    if raw.is_empty() {
        return Vec::new();
    }
    let dt = 1.0 / sample_rate as f64;
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff_hz.max(1.0));
    let alpha = (rc / (rc + dt)) as f32;
    let mut out = vec![0.0f32; raw.len()];
    out[0] = raw[0];
    for i in 1..raw.len() {
        out[i] = alpha * (out[i - 1] + raw[i] - raw[i - 1]);
    }
    out
}

/// 振幅の絶対値列に変換する。
fn abs_series(raw: &[f32]) -> Vec<f32> {
    raw.iter().map(|s| s.abs()).collect()
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

/// エンベロープ（振幅の絶対値、release適用済み）から、しきい値の立ち上がり時刻（ms）の列を求める。
/// 一度立ち上がりを検出したら、`quiet_ms` の間ずっと `threshold` を下回るまで次の立ち上がりの
/// 検出を休止する（減衰中の0点交差の連続検出を防ぐ）。
fn find_onsets(env: &[f32], sample_rate: u32, threshold: f32, quiet_ms: f64) -> Vec<f64> {
    let ms_per_sample = 1000.0 / sample_rate as f64;
    let quiet_samples = ((quiet_ms / ms_per_sample).round() as usize).max(1);
    let mut onsets = Vec::new();
    let mut i = 0usize;
    let n = env.len();
    while i < n {
        let idx = match (i..n).find(|&idx| env[idx] >= threshold) {
            Some(idx) => idx,
            None => break,
        };
        onsets.push(idx as f64 * ms_per_sample);
        i = find_quiet_point(env, idx, threshold, quiet_samples);
    }
    onsets
}

/// 打鍵音の立ち上がり時刻より明らかに前のキック検出は無関係のノイズ・既に消費済みとみなして
/// スキップするための許容幅（ミリ秒）。フィルタの群遅延特性により、ほぼ同時の立ち上がりが
/// ごくわずかに前後することがあるための余裕（この幅自体は `min_delay_ms` による
/// 判定不能の判定より十分小さい値にしている）。
const KICK_LOOKBACK_TOLERANCE_MS: f64 = 5.0;

/// モノラル化済みの波形（符号付き、-1.0〜1.0）から解析を行う。
/// テストなどで直接波形を組み立てて検証する場合はこちらを使う。
pub fn analyze_waveform(wave: &[f32], sample_rate: u32, cfg: &DetectConfig) -> AnalysisResult {
    let mut result = AnalysisResult::default();
    if wave.is_empty() || sample_rate == 0 {
        return result;
    }

    // 打鍵音（クリック、高域）と発音（キック、低域）を周波数帯で分離してから、
    // それぞれ独立に立ち上がりを検出する。振幅の大小関係に依存しないため、
    // 打鍵音が発音より大きく録れていても、打鍵音自身を発音と誤検出しない。
    let click_band = highpass(wave, sample_rate, cfg.click_highpass_hz);
    let kick_band = lowpass(wave, sample_rate, cfg.kick_lowpass_hz);

    let click_env = apply_release(&abs_series(&click_band), sample_rate, cfg.release_ms);
    let kick_env = apply_release(&abs_series(&kick_band), sample_rate, cfg.release_ms);

    let click_onsets = find_onsets(&click_env, sample_rate, cfg.click_threshold, cfg.quiet_ms);
    let kick_onsets = find_onsets(&kick_env, sample_rate, cfg.kick_threshold, cfg.quiet_ms);

    result.detected_clicks = click_onsets.len();
    result.detected_kicks = kick_onsets.len();

    // 打鍵音の立ち上がりを古い順に見て、それぞれに対して max_window_ms 以内で
    // 一番早い（まだ使っていない）発音の立ち上がりをペアにする（二分探索的な二重ポインタ）。
    let mut kick_ptr = 0usize;
    for &click_ms in &click_onsets {
        while kick_ptr < kick_onsets.len()
            && kick_onsets[kick_ptr] < click_ms - KICK_LOOKBACK_TOLERANCE_MS
        {
            // この打鍵音より明らかに前にある発音は、無関係のノイズか既に消費済みのため無視する。
            kick_ptr += 1;
        }

        if kick_ptr < kick_onsets.len() && kick_onsets[kick_ptr] <= click_ms + cfg.max_window_ms {
            let kick_ms = kick_onsets[kick_ptr];
            kick_ptr += 1;
            let delay_ms = kick_ms - click_ms;
            let pair = Pair {
                click_ms,
                kick_ms,
                delay_ms,
            };
            if delay_ms < cfg.min_delay_ms {
                // 物理的にありえない短さの遅延: 検出側の誤り（帯域漏れ等）を疑い、判定不能として扱う。
                result.indeterminate_pairs.push(pair);
            } else {
                result.pairs.push(pair);
            }
        } else {
            // 発音が見つからなかった: 未ペアの打鍵音として記録する。
            result.unmatched_clicks += 1;
        }
    }

    result
}

/// モノラル化前の、チャンネルごとのサンプル列とサンプルレートから解析を行う。
pub fn analyze_channels(
    samples_per_channel: &[Vec<f32>],
    sample_rate: u32,
    cfg: &DetectConfig,
) -> AnalysisResult {
    let mono = to_mono(samples_per_channel);
    analyze_waveform(&mono, sample_rate, cfg)
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
    /// モノラル波形（符号付きの実波形そのもの）を生成する。
    /// 周波数関係（打鍵音3000Hz・発音80Hz）は既存テストのまま固定し、振幅のみ引数で変えられる。
    fn build_wave(
        sample_rate: u32,
        pair_count: usize,
        delay_ms: f64,
        interval_ms: f64,
        click_amp: f32,
        kick_amp: f32,
    ) -> Vec<f32> {
        let total_ms = interval_ms * pair_count as f64 + 500.0;
        let total_samples = (total_ms / 1000.0 * sample_rate as f64) as usize;
        let mut wave = vec![0.0f32; total_samples];

        for i in 0..pair_count {
            let click_start_ms = 200.0 + interval_ms * i as f64;
            let click_start = (click_start_ms / 1000.0 * sample_rate as f64).round() as usize;
            // 打鍵音: 短く減衰する高めの音（クリック的なノイズの代わりに高周波トーンで模擬）。
            add_decaying_tone(&mut wave, click_start, sample_rate, 3000.0, click_amp, 5.0);

            let kick_start_ms = click_start_ms + delay_ms;
            let kick_start = (kick_start_ms / 1000.0 * sample_rate as f64).round() as usize;
            // 発音: 低めの周波数で、長めに減衰する音（キック的）。
            add_decaying_tone(&mut wave, kick_start, sample_rate, 80.0, kick_amp, 150.0);
        }

        wave
    }

    fn assert_delays_close(result: &AnalysisResult, expected_delay_ms: f64, tolerance_ms: f64) {
        assert!(
            !result.pairs.is_empty(),
            "有効なペアが1つも検出されなかった（期待値: {expected_delay_ms}ms 付近）"
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
        let wave = build_wave(sample_rate, 10, 100.0, 500.0, 0.15, 0.8);
        let cfg = DetectConfig::default();
        let result = analyze_waveform(&wave, sample_rate, &cfg);

        assert_eq!(result.pairs.len(), 10, "10組すべてを有効なペアとして検出できていない");
        assert!(
            result.indeterminate_pairs.is_empty(),
            "有効な遅延なのに判定不能に分類されたペアがある"
        );
        assert!(result.count_mismatch_warning().is_none());
        assert_delays_close(&result, 100.0, 2.0);
    }

    #[test]
    fn detects_30ms_delay_for_ten_pairs() {
        let sample_rate = 44_100;
        let wave = build_wave(sample_rate, 10, 30.0, 500.0, 0.15, 0.8);
        let cfg = DetectConfig::default();
        let result = analyze_waveform(&wave, sample_rate, &cfg);

        assert_eq!(result.pairs.len(), 10, "10組すべてを有効なペアとして検出できていない");
        assert!(result.indeterminate_pairs.is_empty());
        assert!(result.count_mismatch_warning().is_none());
        assert_delays_close(&result, 30.0, 2.0);
    }

    #[test]
    fn zero_delay_is_reported_as_indeterminate_not_as_valid_zero_ms() {
        let sample_rate = 44_100;
        // 遅延0msは物理的にはあり得ない値（min_delay_ms未満）として「判定不能」に分類されるべきで、
        // 「0ms」を正常な計測結果として中央値に出してはいけない。
        let wave = build_wave(sample_rate, 10, 0.0, 500.0, 0.15, 0.8);
        let cfg = DetectConfig::default();
        let result = analyze_waveform(&wave, sample_rate, &cfg);

        assert!(
            result.pairs.is_empty(),
            "遅延0ms付近が判定不能ではなく正規のペアとして扱われている"
        );
        assert_eq!(
            result.median_delay_ms(),
            None,
            "0msを正常な計測結果として中央値に出してはいけない"
        );
        assert_eq!(
            result.max_delay_ms(),
            None,
            "0msを正常な計測結果として最大値に出してはいけない"
        );
        assert!(
            !result.indeterminate_pairs.is_empty(),
            "判定不能として記録されるべき組が1つも無い"
        );
    }

    #[test]
    fn silence_reports_zero_pairs_not_zero_ms() {
        let sample_rate = 44_100;
        // 2秒分の完全な無音。
        let wave = vec![0.0f32; sample_rate as usize * 2];
        let cfg = DetectConfig::default();
        let result = analyze_waveform(&wave, sample_rate, &cfg);

        assert!(result.is_empty(), "無音なのにペアを検出してしまっている");
        assert_eq!(result.pairs.len(), 0);
        assert_eq!(result.indeterminate_pairs.len(), 0);
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
        let cfg = DetectConfig::default();
        let result = analyze_waveform(&wave, sample_rate, &cfg);

        assert!(result.pairs.is_empty());
        assert!(result.indeterminate_pairs.is_empty());
        assert_eq!(result.unmatched_clicks, 1);
    }

    /// バグ報告: 打鍵音(0.5)が発音(0.3)より大きい録音で、振幅だけで判定するアルゴリズムは
    /// 打鍵音自身を発音と誤検出し、約0msの誤った遅延を報告していた
    /// （本当の遅延は100msなのに「30ms以下で合格」と誤判定されてしまう不具合）。
    /// 周波数帯分離方式では、遅延を正しく100ms付近と読むか、判定不能と明示するかのいずれかに
    /// なるべきで、「約0ms」を合格値として出してはいけない。
    #[test]
    fn loud_click_quiet_kick_100ms_delay_is_not_misread_as_zero() {
        let sample_rate = 44_100;
        let wave = build_wave(sample_rate, 10, 100.0, 500.0, 0.5, 0.3);
        let cfg = DetectConfig::default();
        let result = analyze_waveform(&wave, sample_rate, &cfg);

        match result.median_delay_ms() {
            Some(median) => {
                assert!(
                    (median - 100.0).abs() <= 2.0,
                    "中央値 {median:.3}ms が期待値 100±2ms の範囲外（0ms付近への誤読の疑い）"
                );
            }
            None => {
                // 有効なペアが1つも無いなら、その理由（判定不能／未ペア）が明示されているべきで、
                // 単に「0ms」と黙って報告されてはいけない。
                assert!(
                    !result.indeterminate_pairs.is_empty() || result.unmatched_clicks > 0,
                    "遅延を読み取れなかった理由（判定不能／未ペア）が記録されていない"
                );
            }
        }
        // 有効なペア（pairs）に、判定不能になるべき短い遅延が紛れ込んでいないことも直接確認する。
        for pair in &result.pairs {
            assert!(
                pair.delay_ms >= cfg.min_delay_ms,
                "判定不能になるべき短い遅延({:.3}ms)が正規のペアに混入している",
                pair.delay_ms
            );
        }
    }

    /// バグ報告のもう1パターン: 打鍵音(0.5)・発音(0.8)とも十分大きい場合でも、
    /// 振幅だけで判定すると誤って約0msと読んでしまっていたケース。
    #[test]
    fn loud_click_loud_kick_100ms_delay_is_not_misread_as_zero() {
        let sample_rate = 44_100;
        let wave = build_wave(sample_rate, 10, 100.0, 500.0, 0.5, 0.8);
        let cfg = DetectConfig::default();
        let result = analyze_waveform(&wave, sample_rate, &cfg);

        match result.median_delay_ms() {
            Some(median) => {
                assert!(
                    (median - 100.0).abs() <= 2.0,
                    "中央値 {median:.3}ms が期待値 100±2ms の範囲外（0ms付近への誤読の疑い）"
                );
            }
            None => {
                assert!(
                    !result.indeterminate_pairs.is_empty() || result.unmatched_clicks > 0,
                    "遅延を読み取れなかった理由（判定不能／未ペア）が記録されていない"
                );
            }
        }
        for pair in &result.pairs {
            assert!(pair.delay_ms >= cfg.min_delay_ms);
        }
    }

    #[test]
    fn count_mismatch_warning_fires_when_pairs_exceed_detected_clicks() {
        // 検出した打鍵数(1)より、ペア（有効+判定不能）の総数(2)の方が多い矛盾した状態を
        // 意図的に作り、警告が出ることを確認する（通常の検出処理ではこの状態にはならないが、
        // 万一の検出ロジックの矛盾を利用者に伝えるための安全網）。
        let result = AnalysisResult {
            pairs: vec![Pair {
                click_ms: 0.0,
                kick_ms: 10.0,
                delay_ms: 10.0,
            }],
            indeterminate_pairs: vec![Pair {
                click_ms: 0.0,
                kick_ms: 1.0,
                delay_ms: 1.0,
            }],
            unmatched_clicks: 0,
            detected_clicks: 1,
            detected_kicks: 2,
        };
        let warning = result.count_mismatch_warning();
        assert!(
            warning.is_some(),
            "組数が打鍵数を超えているのに警告が出ていない"
        );
    }

    #[test]
    fn count_mismatch_warning_is_none_when_counts_are_consistent() {
        let result = AnalysisResult {
            pairs: vec![Pair {
                click_ms: 0.0,
                kick_ms: 10.0,
                delay_ms: 10.0,
            }],
            indeterminate_pairs: vec![],
            unmatched_clicks: 1,
            detected_clicks: 2,
            detected_kicks: 1,
        };
        assert!(result.count_mismatch_warning().is_none());
    }
}
