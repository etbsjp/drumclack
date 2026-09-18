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

/// 1音あたりの基本ゲイン。フルスケール(1.0)のまま合算すると、複数ボイスが
/// 同じ位相で重なった場合に大きく超えてしまう（16音なら理論上14.4）ため、
/// あらかじめ抑えておく。値は「16音重なってもソフトクリップの飽和域に
/// 極端に長く留まらない」程度を目安に決め打ちしている（試作段階）。
const VOICE_GAIN: f32 = 0.3;

/// ソフトクリップ。`tanh` でなめらかに ±1.0 へ飽和させる。
/// `clamp` のような急激な折れ線と違い、入力が大きくなるほど徐々に
/// 傾きが緩んでいくため、複数ボイスが重なって一時的に大きな値になっても
/// 矩形波的な硬い歪みにならない。
fn soft_clip(x: f32) -> f32 {
    x.tanh()
}

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

    /// ロック済みの `VoicePool` から1サンプル分の出力を作る（合算＋レベル補正）。
    ///
    /// 複数ボイスが重なった場合、単純な合算だと最大 [`crate::voices::MAX_VOICES`]
    /// 音が同じ位相で重なる最悪ケースで理論上 `MAX_VOICES * PEAK_AMPLITUDE`
    /// （16音なら14.4）まで達する。以前はここを `clamp(-1.0, 1.0)` だけで
    /// 抑えていたため、鳴っている間ずっと出力が ±1.0 に張り付く矩形波になり
    /// 激しく音割れしていた（issue #6 原因C）。
    ///
    /// その次に「同時発音数 `N` で割る」方式を試したが、新しい音が重なった
    /// 瞬間（＝`N` が変化する瞬間）に既存の音の音量が不連続にジャンプして
    /// しまい、末尾の段差（issue #6 原因B）とほぼ同じ大きさの「プツッ」を
    /// 重なりの瞬間へ移しただけになっていた（PR #7 のレビューで実測確認）。
    /// ゲインが同時発音数に依存する限り、発音数が変わる瞬間に必ず不連続点が
    /// できてしまうため、この方式は採用しない。
    ///
    /// 代わりに、**同時発音数に依存しない固定ゲイン（[`VOICE_GAIN`]）を掛けた
    /// うえで [`soft_clip`] をかける**方式にする。ゲインが一定なので、発音数の
    /// 増減で既存の音の音量がジャンプすることはない。合算値が大きくなり得る分
    /// （複数ボイスが重なった場合）は `tanh` でなめらかに飽和させ、`clamp` の
    /// ような急激な折れ線（矩形波化）を避ける。
    fn mix_locked(pool: &mut VoicePool, kick: &[f32]) -> f32 {
        let mixed = pool.mix_next_sample(kick);
        soft_clip(mixed * VOICE_GAIN)
    }

    /// オーディオコールバックから呼ばれる。1サンプル分の出力を返す。
    /// テスト等でサンプル単位に呼ぶための入り口で、都度ロックを取る。
    /// 実際の再生経路（[`fill_buffer`]）はバッファ単位でロックするため、
    /// 本番の毎サンプルロックは発生しない（下記 [`AudioEngine::fill_output`] 参照）。
    fn next_sample(&self) -> f32 {
        match self.pool.lock() {
            Ok(mut pool) => Self::mix_locked(&mut pool, &self.kick),
            Err(_) => 0.0,
        }
    }

    /// オーディオコールバックから呼ばれる本番の再生経路。
    /// `data`（インターリーブ済みの出力バッファ）をチャンネル数ごとに分割し、
    /// フレーム単位でモノラルのキック音を書き込む。
    ///
    /// 以前は1サンプルごとに `pool.lock()` していたため、48kHzなら
    /// 毎秒48,000回ロックが発生し、キー入力側の `trigger()` とロック競合して
    /// 待機時CPUが上昇する原因になっていた（issue #6 原因D）。
    /// ロックをバッファ単位（cpalのコールバック1回につき1回）に減らすことで、
    /// ロック回数をバッファサイズ分の1（既定バッファ512フレームなら約1/512）
    /// に減らし、待機時CPUをほぼ0%に抑える。
    fn fill_output(&self, data: &mut [f32], channels: usize) {
        let mut pool = match self.pool.lock() {
            Ok(pool) => pool,
            Err(_) => {
                data.fill(0.0);
                return;
            }
        };

        for frame in data.chunks_mut(channels.max(1)) {
            let sample = Self::mix_locked(&mut pool, &self.kick);
            for out in frame.iter_mut() {
                *out = sample;
            }
        }
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
    engine.fill_output(data, channels.max(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    // issue #6 で指摘された通り、「合算後が±1.0を超えない」ことだけを見る
    // 以前のアサーションは、まさにその clamp が音割れの原因（原因C）だったため
    // 欠陥を正常と認定してしまっていた。「クリップに張り付いたサンプルが
    // 一定割合を超えない（歪んでいない）」ことを確認する内容に作り直す。
    #[test]
    fn engine_output_never_exceeds_unity_even_with_max_voices() {
        let engine = AudioEngine::new(44_100);
        // 全ボイスを同じタイミングでトリガーする＝全ボイスが同じ位相で重なる
        // 最悪ケース（16音同時に同じ波形位置を再生）を作る。
        for _ in 0..crate::voices::MAX_VOICES {
            assert!(engine.trigger());
        }
        assert_eq!(engine.active_voice_count(), crate::voices::MAX_VOICES);

        let total = engine.kick.len();
        let mut clipped = 0usize;
        for _ in 0..total {
            let s = engine.next_sample();
            assert!(s.is_finite());
            assert!((-1.0..=1.0).contains(&s), "出力が±1.0を超えています: {s}");
            if s.abs() >= 0.999 {
                clipped += 1;
            }
        }

        // 以前は合算後を clamp するだけだったため、16音同時では合計が最大14.4
        // まで達し、鳴っている間ほぼ全区間で ±1.0 に張り付く矩形波になっていた
        // （クリップ率は非常に高い）。固定ゲイン（VOICE_GAIN）＋ソフトクリップ
        // （tanh）後は、16音同時でも tanh が飽和域に留まる区間はごく短い
        // （鳴り始めの数十ms程度）ため、張り付いたサンプルの割合は小さいはず。
        let clipped_ratio = clipped as f32 / total as f32;
        assert!(
            clipped_ratio < 0.05,
            "クリップに張り付いたサンプルの割合が高すぎます（歪んでいます）: {:.1}% ({clipped}/{total})",
            clipped_ratio * 100.0
        );
    }

    // fill_output（バッファ単位で1回だけロックする本番経路）が、
    // next_sample（1サンプルずつロックする経路）と同じ結果になることを
    // 確認する。issue #6 原因D対応でロック粒度を変えただけで、
    // 合成・ミキシングのロジック自体は変えていないことの回帰確認。
    #[test]
    fn fill_output_matches_repeated_next_sample_for_mono() {
        let engine_buffered = AudioEngine::new(44_100);
        assert!(engine_buffered.trigger());
        assert!(engine_buffered.trigger());

        let n = 64usize;
        let mut buffered = vec![0.0_f32; n];
        engine_buffered.fill_output(&mut buffered, 1);

        let engine_per_sample = AudioEngine::new(44_100);
        assert!(engine_per_sample.trigger());
        assert!(engine_per_sample.trigger());
        let per_sample: Vec<f32> = (0..n).map(|_| engine_per_sample.next_sample()).collect();

        assert_eq!(
            buffered, per_sample,
            "バッファ単位でロックしても1サンプルずつロックした場合と同じ出力になるはず"
        );
    }

    // PR #7 で指摘された回帰: 「同時発音数で割る」方式（原因Cの旧対応）は、
    // 新しい音が重なった瞬間に既存の音の音量を不連続に変化させてしまい、
    // 末尾の段差（issue #6 原因B）とほぼ同じ大きさの「プツッ」を、
    // 重なりの瞬間へ移しただけになっていた（実測: 2音目で段差0.0796、
    // 3音目で0.0927）。基準値には、キック波形そのものが元々持っている
    // 自然な隣接差の最大値（生の `synthesize_kick` から計測。実測で約0.0177）
    // を使う。エンジン出力（ゲイン・ソフトクリップ適用後）はこれよりさらに
    // 小さいスケールになるはずなので、この基準を安全に使える。
    // 100ms間隔で2音・3音を重ねても、エンジン出力の隣接サンプル差の最大値が
    // この基準を超えないことを確認する。
    #[test]
    fn overlapping_voices_do_not_create_larger_step_than_single_voice() {
        let sample_rate = 44_100;

        // 基準値: キック波形（生の合成波形、ゲイン・クリップ適用前）が
        // 単発再生時に自然に持つ隣接サンプル差の最大値。
        let raw_kick = kick::synthesize_kick(sample_rate);
        let single_max_diff = raw_kick
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);

        // 100ms間隔で2音目・3音目をトリガーし、重なりの瞬間を跨いで
        // 波形の全長分＋余裕を観測する。
        let overlap = AudioEngine::new(sample_rate);
        let interval = (0.1 * sample_rate as f32) as usize; // 100ms
        let total = raw_kick.len() + interval * 2;

        assert!(overlap.trigger());
        let mut overlap_samples = Vec::with_capacity(total);
        for i in 0..total {
            if i == interval {
                assert!(overlap.trigger(), "2音目");
            }
            if i == interval * 2 {
                assert!(overlap.trigger(), "3音目");
            }
            overlap_samples.push(overlap.next_sample());
        }
        let overlap_max_diff = overlap_samples
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);

        assert!(
            overlap_max_diff <= single_max_diff,
            "重なりの瞬間に単発時より大きな段差が生じています（プツッというノイズになる）: \
             overlap={overlap_max_diff} single={single_max_diff}"
        );
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
