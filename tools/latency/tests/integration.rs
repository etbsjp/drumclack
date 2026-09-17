//! 実際の WAV ファイル（16bit PCM）の書き出し・読み込みを通した end-to-end テスト。
//!
//! テスト用の WAV は毎回 OS の一時ディレクトリに生成し、テスト終了後に削除する
//! （リポジトリには一切コミットしない）。

use latency::{analyze_wav_file, DetectConfig};
use std::path::PathBuf;

/// テスト終了時（成功・失敗いずれでも）に一時ファイルを削除するためのガード。
struct TempWavFile(PathBuf);

impl Drop for TempWavFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 一意な一時 WAV ファイルパスを発行する。
fn temp_wav_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let unique = format!(
        "drumclack-latency-test-{}-{}-{}.wav",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    path.push(unique);
    path
}

/// 指数減衰する正弦波を `dst` に加算する。
fn add_decaying_tone(
    dst: &mut [f32],
    start_sample: usize,
    sample_rate: u32,
    freq_hz: f32,
    peak_amplitude: f32,
    decay_ms: f32,
) {
    let sr = sample_rate as f32;
    let tau_samples = (decay_ms / 1000.0) * sr / 3.0;
    for (offset, sample) in dst.iter_mut().skip(start_sample).enumerate() {
        let t = offset as f32 / sr;
        let env = (-(offset as f32) / tau_samples.max(1.0)).exp();
        let value = peak_amplitude * env * (2.0 * std::f32::consts::PI * freq_hz * t).sin();
        *sample += value;
        if env < 1e-4 {
            break;
        }
    }
}

/// 「クリック音の delay_ms 後にキック音」を pair_count 組並べたモノラル波形を作る。
fn build_wave(sample_rate: u32, pair_count: usize, delay_ms: f64, interval_ms: f64) -> Vec<f32> {
    let total_ms = interval_ms * pair_count as f64 + 500.0;
    let total_samples = (total_ms / 1000.0 * sample_rate as f64) as usize;
    let mut wave = vec![0.0f32; total_samples];

    for i in 0..pair_count {
        let click_start_ms = 200.0 + interval_ms * i as f64;
        let click_start = (click_start_ms / 1000.0 * sample_rate as f64).round() as usize;
        add_decaying_tone(&mut wave, click_start, sample_rate, 3000.0, 0.15, 5.0);

        let kick_start_ms = click_start_ms + delay_ms;
        let kick_start = (kick_start_ms / 1000.0 * sample_rate as f64).round() as usize;
        add_decaying_tone(&mut wave, kick_start, sample_rate, 80.0, 0.8, 150.0);
    }

    wave
}

/// f32 波形を 16bit PCM WAV として書き出す。
fn write_wav_16bit(path: &PathBuf, wave: &[f32], sample_rate: u32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("WAV の作成に失敗");
    for &s in wave {
        let clamped = s.clamp(-1.0, 1.0);
        let sample_i16 = (clamped * i16::MAX as f32) as i16;
        writer
            .write_sample(sample_i16)
            .expect("サンプル書き込みに失敗");
    }
    writer.finalize().expect("WAV のファイナライズに失敗");
}

#[test]
fn end_to_end_100ms_delay_wav_is_read_correctly() {
    let sample_rate = 44_100;
    let wave = build_wave(sample_rate, 10, 100.0, 500.0);
    let path = temp_wav_path("100ms");
    let _guard = TempWavFile(path.clone());
    write_wav_16bit(&path, &wave, sample_rate);

    let cfg = DetectConfig::default();
    let result = analyze_wav_file(&path, &cfg).expect("解析に失敗");

    assert_eq!(result.pairs.len(), 10, "10組すべてを検出できていない");
    let median = result.median_delay_ms().expect("中央値が取得できない");
    assert!(
        (median - 100.0).abs() <= 2.0,
        "中央値 {median:.3}ms が 100±2ms の範囲外"
    );
    let max = result.max_delay_ms().expect("最大値が取得できない");
    assert!(
        (max - 100.0).abs() <= 2.0,
        "最大値 {max:.3}ms が 100±2ms の範囲外"
    );
}

#[test]
fn end_to_end_silence_wav_reports_zero_pairs() {
    let sample_rate = 44_100;
    let wave = vec![0.0f32; sample_rate as usize * 2]; // 2秒の無音
    let path = temp_wav_path("silence");
    let _guard = TempWavFile(path.clone());
    write_wav_16bit(&path, &wave, sample_rate);

    let cfg = DetectConfig::default();
    let result = analyze_wav_file(&path, &cfg).expect("解析に失敗");

    assert!(result.is_empty(), "無音なのにペアを検出してしまっている");
    assert_eq!(
        result.median_delay_ms(),
        None,
        "0msと誤って出してはいけない"
    );
}
