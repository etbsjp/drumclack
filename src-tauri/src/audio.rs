//! 音声出力エンジン。
//!
//! 起動時にキック波形をメモリに合成し、cpal で直接デバイスへ再生する。
//! 発音のトリガーはキー入力スレッドから、ミキシングは cpal のオーディオ
//! コールバック（別スレッド）から行われるため、[`voices::VoicePool`] は
//! `Mutex` で共有する。

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::kick;
use crate::voices::VoicePool;

/// 合成・再生を担うエンジン本体。
/// cpal の `Stream` はここでは保持せず [`OutputStream`] 側が握る
/// （`Stream` は `Send` だが UI スレッドで drop されないよう別途管理する）。
pub struct AudioEngine {
    kick: Vec<f32>,
    pool: Mutex<VoicePool>,
    sample_rate: u32,
}

impl AudioEngine {
    fn new(sample_rate: u32) -> Self {
        Self { kick: kick::synthesize_kick(sample_rate), pool: Mutex::new(VoicePool::new()), sample_rate }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// 新しい発音を試みる。上限に達していれば何もしない（既存の発音は継続する）。
    /// 戻り値は実際に発音を開始できたかどうか。
    pub fn trigger(&self) -> bool {
        match self.pool.lock() {
            Ok(mut pool) => pool.trigger(),
            Err(_) => false,
        }
    }

    pub fn active_voice_count(&self) -> usize {
        self.pool.lock().map(|p| p.active_count()).unwrap_or(0)
    }

    /// オーディオコールバックから呼ばれる。1サンプル分の合算値を返す。
    /// 複数ボイスが重なった場合の出力レベル超過（音割れ）を防ぐため、
    /// 最終段で [-1.0, 1.0] にクランプするシンプルなリミッターを掛ける。
    fn next_sample(&self) -> f32 {
        let mixed = match self.pool.lock() {
            Ok(mut pool) => pool.mix_next_sample(&self.kick),
            Err(_) => 0.0,
        };
        mixed.clamp(-1.0, 1.0)
    }
}

/// 起動時に一度だけ音声出力を初期化し、専用スレッドでストリームを再生し続ける。
///
/// `cpal::Stream` はプラットフォームによって `Send`/`Sync` を実装しない
/// （例: macOS の CoreAudio バックエンド）ため、Tauri の管理状態
/// （`Manager::manage`、複数スレッドから触られる前提）には乗せられない。
/// そのため、ストリームの生成・保持をこの専用スレッド1本に閉じ込め、
/// 他スレッドとやり取りする値は `Arc<AudioEngine>`（プレーンなデータのみで
/// 構成され `Send + Sync`）に限定する。
///
/// 戻り値は初期化結果。失敗した場合は理由を日本語の短い文言で返す
/// （画面表示にそのまま使う）。スレッドはアプリのプロセスが終了するまで
/// 生き続け、ストリームを保持し続ける（明示的な停止APIは持たない。
/// プロセス終了時にOSがまとめて破棄する）。
pub fn spawn_output_stream() -> Result<Arc<AudioEngine>, String> {
    let (tx, rx) = std::sync::mpsc::channel::<Result<Arc<AudioEngine>, String>>();

    std::thread::spawn(move || {
        let result = build_stream();
        let engine_for_reply = match &result {
            Ok((engine, _stream)) => Ok(engine.clone()),
            Err(e) => Err(e.clone()),
        };

        if tx.send(engine_for_reply).is_err() {
            // 受信側（setup）が既に諦めている場合は何もできることがない。
            return;
        }

        match result {
            // ストリームをこのスレッドのスタックに保持したまま眠り続けることで
            // 再生を維持する（drop されると再生が止まるため）。
            Ok((_engine, _stream)) => loop {
                std::thread::park();
            },
            Err(_) => {
                // 初期化失敗時はこのスレッドの役目は終わり。
            }
        }
    });

    rx.recv().map_err(|_| "音声出力スレッドの初期化応答を受信できませんでした。".to_string())?
}

/// デフォルトの出力デバイスを開き、[`AudioEngine`] と再生ストリームを構築する。
/// 呼び出し元スレッドの外へ `cpal::Stream` を持ち出さない前提の内部関数。
fn build_stream() -> Result<(Arc<AudioEngine>, cpal::Stream), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "出力デバイスが見つかりませんでした。音声出力機器の接続を確認してください。".to_string())?;

    let supported_config = device
        .default_output_config()
        .map_err(|e| format!("出力デバイスの設定取得に失敗しました: {e}"))?;

    let sample_format = supported_config.sample_format();
    let channels = supported_config.channels() as usize;
    let sample_rate = supported_config.sample_rate().0;

    // バッファサイズは低遅延寄りの値を希望するが、デバイスが対応する範囲に
    // 収まらない場合は要求せず（デバイス既定値に委ねる）、初期化失敗を避ける。
    const PREFERRED_BUFFER_FRAMES: u32 = 512;
    let requested_buffer_size = match supported_config.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => {
            cpal::BufferSize::Fixed(PREFERRED_BUFFER_FRAMES.clamp(*min, *max))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    };

    let mut config: cpal::StreamConfig = supported_config.into();
    config.buffer_size = requested_buffer_size;

    let engine = Arc::new(AudioEngine::new(sample_rate));
    let engine_cb = engine.clone();

    let err_fn = |e| eprintln!("[drumclack] 音声出力エラー: {e}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _| fill_buffer(data, channels, &engine_cb),
                err_fn,
                None,
            )
            .map_err(|e| format!("出力ストリームの構築に失敗しました: {e}"))?,
        other => {
            return Err(format!("未対応のサンプル形式です: {other:?}"));
        }
    };

    stream.play().map_err(|e| format!("音声出力の開始に失敗しました: {e}"))?;

    println!(
        "[drumclack] audio initialized: sample_rate={sample_rate}Hz channels={channels} requested_buffer_frames={PREFERRED_BUFFER_FRAMES} actual_config_buffer_size={:?}",
        config.buffer_size
    );

    Ok((engine, stream))
}

fn fill_buffer(data: &mut [f32], channels: usize, engine: &AudioEngine) {
    for frame in data.chunks_mut(channels.max(1)) {
        let sample = engine.next_sample();
        for out in frame.iter_mut() {
            *out = sample;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_output_never_exceeds_unity_even_with_max_voices() {
        let engine = AudioEngine::new(44_100);
        for _ in 0..crate::voices::MAX_VOICES {
            assert!(engine.trigger());
        }
        assert_eq!(engine.active_voice_count(), crate::voices::MAX_VOICES);

        // 波形の全長分サンプルを取り出し、常に [-1.0, 1.0] に収まることを確認する。
        for _ in 0..engine.kick.len() {
            let s = engine.next_sample();
            assert!(s.is_finite());
            assert!((-1.0..=1.0).contains(&s), "出力が音割れしています: {s}");
        }
    }

    #[test]
    fn trigger_beyond_max_does_not_increase_active_count() {
        let engine = AudioEngine::new(44_100);
        for _ in 0..crate::voices::MAX_VOICES {
            assert!(engine.trigger());
        }
        assert!(!engine.trigger(), "上限を超えたら false を返す");
        assert_eq!(engine.active_voice_count(), crate::voices::MAX_VOICES);
    }
}
