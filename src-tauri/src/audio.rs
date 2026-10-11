//! 音声出力エンジン。
//!
//! 起動時に無料キットの波形をメモリに合成し、cpal で直接デバイスへ再生する。
//! 発音の要求はキー入力スレッドから固定長の待ち行列へ積み、ミキシングは cpal の
//! オーディオコールバック（別スレッド）が行う。両者の間にロックはない。
//!
//! 出力ストリームの作り直し（機器の抜き差し・スリープ復帰・既定の出力先の変更）は
//! `output_recovery.rs` が見張る。このファイルは、その見張りが使う cpal の部品を持つ。

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::drums::{build_free_kit, VariantPicker};
use crate::output_recovery::{OutputBackend, OutputInfo, OutputSupervisor, StreamHealth, FALLBACK_SAMPLE_RATE};
use crate::voices::{Kit, PlayRequest, RequestQueue, VoicePool, MAX_REQUEST_VOLUME, REQUEST_QUEUE_CAPACITY};

/// 1音あたりの基本ゲイン。フルスケール(1.0)のまま合算すると、複数ボイスが
/// 同じ位相で重なった場合に大きく超えてしまう（16音なら理論上14.4）ため、
/// あらかじめ抑えておく。
///
/// 値は実測で決めている（48kHz・キック波形をこの実装どおりに再現して計測）。
/// 常駐アプリで最も多いのは「単発の1打」なので、そこが小さくなりすぎない
/// ことを優先しつつ、高速連打で `tanh` の飽和域（|x| > 2）に入らない値を選ぶ。
///
/// | ゲイン | 単発ピーク | 5音重なり(200ms間隔) | 16音連打(30ms間隔)の飽和率 |
/// |---|---|---|---|
/// | 0.3 | 0.262 | 0.394 | 0% |
/// | 0.5 | 0.420 | 0.600 | 0% |
/// | **0.7** | **0.555** | 0.749 | 0% |
/// | 0.9 | 0.667 | 0.848 | 0.6% |
/// | 1.2 | 0.791 | 0.931 | 4.3% |
///
/// 0.3 では単発が 0.262 と小さく（clamp 方式だった頃の 0.900 比で約11dB低下）、
/// 利用者が OS 側の音量を上げて使うことになり、その状態での連打が過大になる。
pub const VOICE_GAIN: f32 = 0.7;

/// ソフトクリップ。`tanh` でなめらかに ±1.0 へ飽和させる。
/// `clamp` のような急激な折れ線と違い、入力が大きくなるほど徐々に
/// 傾きが緩んでいくため、複数ボイスが重なって一時的に大きな値になっても
/// 矩形波的な硬い歪みにならない。
fn soft_clip(x: f32) -> f32 {
    x.tanh()
}

/// 止め合いのフェードアウト長（ミリ秒）。仮値。短いと「プツッ」、長いと止め切れず重なって聞こえる。
const CHOKE_FADE_MS: f32 = 5.0;

/// フェード長（ミリ秒）をサンプル数にする。
fn fade_samples_for(sample_rate: u32, fade_ms: f32) -> u32 {
    (sample_rate as f32 * fade_ms / 1000.0).round() as u32
}

/// 現在時刻（UNIX epoch ミリ秒）。0 は「未発音」の番兵値なので 1 以上にする。
/// 音声コールバックから呼ぶため、確保もロックもしない時刻取得だけを使う。
fn now_epoch_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0).max(1)
}

/// キー監視側（書く）と音声コールバック（読む）が共有する窓口。
///
/// 中身はキット（作成後は読むだけ）と、原子的な値・ロックなしの待ち行列だけ。
/// 打鍵のスレッドと音声コールバックが触る経路に `Mutex` は無い（出力状態の `Mutex` は、
/// 見張りのスレッドが書き、状態表示が読むだけ）。ボイスの管理（[`Mixer`]）は音声コールバックだけが持つ。
/// cpal の `Stream` はここでは保持せず [`spawn_output_stream`] の専用スレッドが握る。
///
/// 出力ストリームを作り直しても、このエンジンは作り直さない（打鍵側が持つ参照をそのまま使うため）。
pub struct AudioEngine {
    /// 音の名前・変種の数を引くためのキット。波形は、出口ごとの [`Mixer`] が持つキットを使う
    /// （出力先のサンプルレートが変わると、そのレートで合成し直したキットになるため）。
    kit: Arc<Kit>,
    variant_picker: VariantPicker,
    requests: RequestQueue,
    active_voices: AtomicUsize,
    /// 音声コールバックが最後に発音を受理した時刻（UNIX epoch ミリ秒）。0 は未発音。
    last_accepted_play_ms: AtomicU64,
    /// 全体音量（`f32` のビット列）。
    master_volume_bits: AtomicU32,
    /// 音の出口の状態。見張りが書き、トレイと画面の状態表示が読む。
    output_status: Mutex<OutputStatus>,
}

/// 音の出口（出力ストリーム）の状態。「問題あり」の表示はこれを読む。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputStatus {
    /// 鳴らせる状態。いま使っている出力先のサンプルレートを持つ。
    Ok { sample_rate: u32 },
    /// 鳴らせない状態。原因の要約文言（画面にそのまま出す）。
    Err(String),
}

impl AudioEngine {
    pub fn new(kit: Kit, sample_rate: u32) -> Self {
        Self::with_shared_kit(Arc::new(kit), sample_rate)
    }

    /// 出口づくりと共有するキットでエンジンを作る。`sample_rate` はそのキットを合成したレート。
    pub fn with_shared_kit(kit: Arc<Kit>, sample_rate: u32) -> Self {
        Self {
            variant_picker: VariantPicker::new(kit.sound_count()),
            kit,
            requests: RequestQueue::new(REQUEST_QUEUE_CAPACITY),
            active_voices: AtomicUsize::new(0),
            last_accepted_play_ms: AtomicU64::new(0),
            master_volume_bits: AtomicU32::new(1.0_f32.to_bits()),
            output_status: Mutex::new(OutputStatus::Ok { sample_rate }),
        }
    }

    /// 音の出口の状態。
    pub fn output_status(&self) -> OutputStatus {
        self.output_status.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// 音の出口の状態を更新する（見張りが呼ぶ）。
    pub(crate) fn set_output_status(&self, status: OutputStatus) {
        *self.output_status.lock().unwrap_or_else(|p| p.into_inner()) = status;
    }

    /// `鳴らす(音, 変種, 音量)`。要求を待ち行列に積むだけで、ロックは取らない。
    ///
    /// キットに無い名前は何も積まず `false`（無音）。待ち行列があふれたときも `false`。
    /// 戻り値は「要求を積めたか」で、実際に鳴るか（16音の上限）は音声コールバック側で決まる。
    /// `variant` は変種の数で折り返す。`volume` は 0〜[`MAX_REQUEST_VOLUME`] に丸める
    /// （数値でない値は 0）。
    pub fn play(&self, name: &str, variant: usize, volume: f32) -> bool {
        let Some(sound) = self.kit.index_of(name) else {
            return false;
        };
        let volume = if volume.is_finite() { volume.clamp(0.0, MAX_REQUEST_VOLUME) } else { 0.0 };
        self.requests.push(PlayRequest {
            sound: sound as u16,
            variant: variant.min(usize::from(u8::MAX)) as u8,
            volume,
        })
    }

    /// 変種を自動で選んで鳴らす。直前に同じ音で鳴らした変種とは別の変種を選ぶ。
    /// それ以外は [`AudioEngine::play`] と同じ。
    pub fn play_varied(&self, name: &str, volume: f32) -> bool {
        let Some(sound) = self.kit.index_of(name) else {
            return false;
        };
        let variant = self.variant_picker.pick(sound, self.kit.variant_count(sound));
        self.play(name, variant, volume)
    }

    /// 全体音量（0.0〜1.0）を設定する。数値でない値は無視する。
    // 設定との接続は後の issue（#13 以降）。それまで本体からは呼ばれない。
    #[allow(dead_code)]
    pub fn set_master_volume(&self, volume: f32) {
        if volume.is_finite() {
            self.master_volume_bits.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }

    /// 実際に鳴り始めた最後の発音の時刻。16音の上限などで鳴らなかった要求は含まない。
    /// 時刻は音声コールバックが要求を取り込んだ時点（打鍵から最大でバッファ1回分後）。
    pub fn last_accepted_play_ms(&self) -> Option<u64> {
        match self.last_accepted_play_ms.load(Ordering::Relaxed) {
            0 => None,
            ms => Some(ms),
        }
    }

    /// テスト用: 積まれている発音の要求をすべて取り出す（本来の読み手は音声コールバックだけ）。
    #[cfg(test)]
    pub fn drain_requests_for_test(&self) -> Vec<crate::voices::PlayRequest> {
        std::iter::from_fn(|| self.requests.pop()).collect()
    }

    pub fn master_volume(&self) -> f32 {
        f32::from_bits(self.master_volume_bits.load(Ordering::Relaxed))
    }

    /// 鳴っている音の数。音声コールバックが1回ごとに更新する値を読むだけ。
    pub fn active_voice_count(&self) -> usize {
        self.active_voices.load(Ordering::Relaxed)
    }
}

/// 音声コールバックだけが持つミキサー。ボイスは固定長配列で、
/// コールバックの中でロック・確保・解放をしない。
pub struct Mixer {
    engine: Arc<AudioEngine>,
    /// 鳴らす波形。出力先のサンプルレートで合成したキット。
    kit: Arc<Kit>,
    pool: VoicePool,
    fade_samples: u32,
    /// 最初のコールバックで、溜まっていた発音の要求を鳴らさずに捨てる。
    /// 出口を失っている間に溜まった打鍵が、作り直した瞬間にまとめて鳴るのを防ぐ。
    discard_backlog: bool,
}

impl Mixer {
    /// エンジンのキットで鳴らす（エンジンを作ったときのサンプルレートの出口向け）。
    // 製品では `for_output` を使う。テストが、見張りを通さずにミキサーだけを作るのに使う。
    #[cfg(test)]
    pub fn new(engine: Arc<AudioEngine>) -> Self {
        let kit = engine.kit.clone();
        // 作ったばかりのエンジンの出力状態は、キットを合成したレートを持っている。
        let sample_rate = match engine.output_status() {
            OutputStatus::Ok { sample_rate } => sample_rate,
            OutputStatus::Err(_) => FALLBACK_SAMPLE_RATE,
        };
        Self::build(engine, kit, sample_rate, false)
    }

    /// 出口のコールバックに渡すミキサー。`kit` は `sample_rate` で合成したキットを渡す。
    pub fn for_output(engine: Arc<AudioEngine>, kit: Arc<Kit>, sample_rate: u32) -> Self {
        Self::build(engine, kit, sample_rate, true)
    }

    fn build(engine: Arc<AudioEngine>, kit: Arc<Kit>, sample_rate: u32, discard_backlog: bool) -> Self {
        let fade_samples = fade_samples_for(sample_rate, CHOKE_FADE_MS);
        Self { engine, kit, pool: VoicePool::new(), fade_samples, discard_backlog }
    }

    /// 1サンプル分の出力を作る（合算＋レベル補正）。
    ///
    /// 重なった音は、同時発音数に依存しない固定ゲイン（[`VOICE_GAIN`]）を掛けて
    /// 合算し、[`soft_clip`] で頭打ちにする。ゲインが発音数に依存すると、発音数が
    /// 変わる瞬間に既存の音の音量が跳ねて「プツッ」と鳴る（issue #6 原因B・C、
    /// PR #7 のレビューで実測）。全体音量は頭打ちの後に掛ける。
    fn next_sample(&mut self, master_volume: f32) -> f32 {
        let mixed = self.pool.mix_next_sample(&self.kit);
        soft_clip(mixed * VOICE_GAIN) * master_volume
    }

    /// オーディオコールバックから呼ばれる本番の再生経路。
    /// 溜まった発音の要求を取り込み、`data`（インターリーブ済みの出力バッファ）を
    /// チャンネル数ごとに分割して、フレーム単位でモノラルの音を書き込む。
    pub(crate) fn fill_output(&mut self, data: &mut [f32], channels: usize) {
        let discard = std::mem::replace(&mut self.discard_backlog, false);
        while let Some(request) = self.engine.requests.pop() {
            if !discard && self.pool.start(&self.kit, request, self.fade_samples) {
                self.engine.last_accepted_play_ms.store(now_epoch_ms(), Ordering::Relaxed);
            }
        }
        let master_volume = self.engine.master_volume();

        for frame in data.chunks_mut(channels.max(1)) {
            let sample = self.next_sample(master_volume);
            for out in frame.iter_mut() {
                *out = sample;
            }
        }
        self.engine.active_voices.store(self.pool.active_count(), Ordering::Relaxed);
    }
}

/// 見張りが出口の様子を見に行く間隔（スレッドが目覚める間隔）。調整するときはここを変える。
/// 切り替わりに気づくまでの最大の遅れはこの値と `output_recovery::WATCH_INTERVAL` で決まる。
const SUPERVISOR_TICK_MS: u64 = 250;

/// 起動時に音声出力を初期化し、専用スレッドでストリームの再生と見張り・作り直しを続ける。
///
/// `cpal::Stream` はプラットフォームによって `Send`/`Sync` を実装しない
/// （例: macOS の CoreAudio バックエンド）ため、Tauri の管理状態
/// （`Manager::manage`、複数スレッドから触られる前提）には乗せられない。
/// そのため、ストリームの生成・保持・作り直しをこの専用スレッド1本に閉じ込め、
/// 他スレッドとやり取りする値は `Arc<AudioEngine>`（プレーンなデータのみで
/// 構成され `Send + Sync`）に限定する。
///
/// 出力先が見つからない・開けないときも、エンジンは必ず返す（出力状態は
/// [`OutputStatus::Err`]）。出力先が使えるようになれば、見張りが自動で鳴らせる状態にする。
/// スレッドはアプリのプロセスが終了するまで生き続ける（明示的な停止APIは持たない。
/// プロセス終了時にOSがまとめて破棄する）。
pub fn spawn_output_stream() -> Arc<AudioEngine> {
    let (tx, rx) = std::sync::mpsc::channel::<Arc<AudioEngine>>();

    std::thread::spawn(move || {
        let mut supervisor =
            OutputSupervisor::start(CpalBackend::new(), Box::new(build_free_kit), Instant::now());
        if tx.send(supervisor.engine()).is_err() {
            // 受信側（setup）が既に諦めている場合は何もできることがない。
            return;
        }
        // ストリームはこのスレッドの持ち物（supervisor の中）。落とすと再生が止まるため、
        // スレッドを終わらせずに見張りを続ける。
        loop {
            std::thread::sleep(std::time::Duration::from_millis(SUPERVISOR_TICK_MS));
            // panic しても見張りを続ける（出力状態は「問題あり」になる）。
            supervisor.tick_guarded(Instant::now());
        }
    });

    rx.recv().unwrap_or_else(|_| {
        // スレッドが応答できなかった（起動の途中で異常終了）。鳴らせない状態のエンジンを返す。
        let engine = Arc::new(AudioEngine::new(build_free_kit(FALLBACK_SAMPLE_RATE), FALLBACK_SAMPLE_RATE));
        engine.set_output_status(OutputStatus::Err(
            "音声出力スレッドの初期化応答を受信できませんでした。".to_string(),
        ));
        engine
    })
}

/// 実測フレーム数の待機設定（50ms × 40回 = 最大2秒）。
const OBSERVE_POLL_INTERVAL_MS: u64 = 50;
const OBSERVE_MAX_POLLS: u32 = 40;

/// 希望バッファ長（フレーム数）。低遅延のため小さめを希望する（issue #5）。
const PREFERRED_BUFFER_FRAMES: u32 = 128;

/// 希望バッファ長をデバイスの対応範囲へ丸める。
/// 範囲があれば `[min, max]` に収め、範囲不明ならデバイス既定値に委ねる。
fn resolve_buffer_size(preferred: u32, supported: &cpal::SupportedBufferSize) -> cpal::BufferSize {
    match supported {
        cpal::SupportedBufferSize::Range { min, max } => {
            // min > max の異常値でも clamp が panic しないよう手で丸める。
            cpal::BufferSize::Fixed(preferred.max(*min).min(*max))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    }
}

/// cpal を使う出力の部品。既定の出力デバイスを調べ、ストリームを作る。
struct CpalBackend {
    host: cpal::Host,
}

impl CpalBackend {
    fn new() -> Self {
        Self { host: cpal::default_host() }
    }

    /// 既定の出力デバイスと、その既定の設定。
    fn default_output(&self) -> Result<(cpal::Device, cpal::SupportedStreamConfig), String> {
        let device = self
            .host
            .default_output_device()
            .ok_or_else(|| "出力デバイスが見つかりませんでした。音声出力機器の接続を確認してください。".to_string())?;
        let supported_config = device
            .default_output_config()
            .map_err(|e| format!("出力デバイスの設定取得に失敗しました: {e}"))?;
        Ok((device, supported_config))
    }
}

/// 出力先の情報を組み立てる。機器名が取れないときは `Err`（取れなかった名前を代わりの文字列で
/// 埋めると、問い合わせの失敗が「別の出力先に変わった」と見えて、動いている出口を誤って手放すため）。
fn output_info_from_parts(
    device_name: Result<String, impl std::fmt::Display>,
    sample_rate: u32,
    channels: usize,
) -> Result<OutputInfo, String> {
    let device_name = device_name.map_err(|e| format!("出力デバイスの名前を取得できませんでした: {e}"))?;
    Ok(OutputInfo { device_name, sample_rate, channels })
}

fn output_info_of(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
) -> Result<OutputInfo, String> {
    output_info_from_parts(device.name(), config.sample_rate().0, config.channels() as usize)
}

impl OutputBackend for CpalBackend {
    type Stream = cpal::Stream;

    fn probe(&mut self) -> Result<OutputInfo, String> {
        let (device, config) = self.default_output()?;
        output_info_of(&device, &config)
    }

    fn open(
        &mut self,
        make_mixer: &mut dyn FnMut(&OutputInfo) -> Mixer,
        health: Arc<StreamHealth>,
    ) -> Result<(OutputInfo, cpal::Stream), String> {
        let (device, supported_config) = self.default_output()?;
        let info = output_info_of(&device, &supported_config)?;
        let sample_format = supported_config.sample_format();
        let channels = info.channels;
        let sample_rate = info.sample_rate;

        // バッファサイズは低遅延寄りの値を希望するが、デバイスが対応する範囲に
        // 収まらない場合は範囲内へ丸め、範囲が不明なら要求せず（デバイス既定値に
        // 委ねる）、初期化失敗を避ける。
        let requested_buffer_size =
            resolve_buffer_size(PREFERRED_BUFFER_FRAMES, supported_config.buffer_size());

        let mut config: cpal::StreamConfig = supported_config.into();
        config.buffer_size = requested_buffer_size;

        if sample_format != cpal::SampleFormat::F32 {
            return Err(format!("未対応のサンプル形式です: {sample_format:?}"));
        }

        // コールバックが実際に受け取ったバッファ長（フレーム数）。0は未観測。
        // 要求値が効いているか（特にWindowsの共有モードではOS周期に丸められうる）を
        // 確かめるためのもの。コールバック内ではアトミックな保存だけを行い、
        // ロックやI/O（ログ出力）は一切しない。出力は別スレッドで行う。
        let observed_frames = Arc::new(AtomicUsize::new(0));

        // ストリームのエラーは、ログに出して見張りへ知らせる（見張りが出口を作り直す）。
        let err_health = health.clone();
        let err_fn = move |e: cpal::StreamError| {
            eprintln!("[drumclack] 音声出力エラー: {e}");
            err_health.mark_failed();
        };

        // コールバックを持つストリームの構築。再試行で2回呼べるよう、毎回ミキサーを作る。
        let mut build = |config: &cpal::StreamConfig| {
            let mut mixer = make_mixer(&info);
            let observed_frames_cb = observed_frames.clone();
            let health_cb = health.clone();
            device.build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    health_cb.note_callback();
                    // 初回のみ保存する（以降は読むだけで書かない）。
                    if observed_frames_cb.load(Ordering::Relaxed) == 0 {
                        observed_frames_cb.store(data.len() / channels.max(1), Ordering::Relaxed);
                    }
                    mixer.fill_output(data, channels.max(1))
                },
                err_fn.clone(),
                None,
            )
        };

        // 固定サイズでの構築に失敗したら、デバイス既定のバッファサイズで1回だけ作り直す。
        let stream = match build(&config) {
            Ok(stream) => stream,
            Err(e) if matches!(config.buffer_size, cpal::BufferSize::Fixed(_)) => {
                eprintln!(
                    "[drumclack] バッファサイズ {:?} での出力ストリーム構築に失敗したため、既定サイズで再試行します: {e}",
                    config.buffer_size
                );
                config.buffer_size = cpal::BufferSize::Default;
                build(&config).map_err(|e| format!("出力ストリームの構築に失敗しました: {e}"))?
            }
            Err(e) => return Err(format!("出力ストリームの構築に失敗しました: {e}")),
        };

        stream.play().map_err(|e| format!("音声出力の開始に失敗しました: {e}"))?;

        // requested_buffer_size は再試行後に最終的に使った値。
        println!(
            "[drumclack] audio initialized: sample_rate={sample_rate}Hz channels={channels} preferred_buffer_frames={PREFERRED_BUFFER_FRAMES} requested_buffer_size={:?}",
            config.buffer_size
        );

        // 実測フレーム数のログは、オーディオスレッドを塞がないよう別スレッドで
        // コールバックの初回実行を待って1回だけ出力する。
        std::thread::spawn(move || {
            for _ in 0..OBSERVE_MAX_POLLS {
                let frames = observed_frames.load(Ordering::Relaxed);
                if frames != 0 {
                    println!(
                        "[drumclack] audio callback observed (first callback): buffer_frames={frames} (preferred={PREFERRED_BUFFER_FRAMES})"
                    );
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(OBSERVE_POLL_INTERVAL_MS));
            }
            eprintln!("[drumclack] audio callback observed: 2秒以内にコールバックが呼ばれませんでした");
        });

        Ok((info, stream))
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::kick;

    fn range(min: u32, max: u32) -> cpal::SupportedBufferSize {
        cpal::SupportedBufferSize::Range { min, max }
    }

    #[test]
    fn output_info_requires_the_device_name() {
        let ok = output_info_from_parts(Ok::<_, String>("内蔵スピーカー".to_string()), 48_000, 2).unwrap();
        assert_eq!(ok, OutputInfo { device_name: "内蔵スピーカー".to_string(), sample_rate: 48_000, channels: 2 });
        // 名前が取れないときは Err。代わりの名前で別の出力先に見せない。
        assert!(output_info_from_parts(Err("取れない".to_string()), 48_000, 2).is_err());
    }

    #[test]
    fn buffer_size_below_min_is_raised_to_min() {
        assert!(matches!(
            resolve_buffer_size(128, &range(256, 4096)),
            cpal::BufferSize::Fixed(256)
        ));
    }

    #[test]
    fn buffer_size_above_max_is_lowered_to_max() {
        assert!(matches!(
            resolve_buffer_size(128, &range(16, 64)),
            cpal::BufferSize::Fixed(64)
        ));
    }

    #[test]
    fn buffer_size_within_range_is_kept() {
        assert!(matches!(
            resolve_buffer_size(128, &range(16, 4096)),
            cpal::BufferSize::Fixed(128)
        ));
    }

    #[test]
    fn buffer_size_unknown_falls_back_to_default() {
        assert!(matches!(
            resolve_buffer_size(128, &cpal::SupportedBufferSize::Unknown),
            cpal::BufferSize::Default
        ));
    }

    #[test]
    fn buffer_size_inverted_range_does_not_panic() {
        // min > max でも panic せず、max(min) 後に min(max) で丸まり Fixed(16) になる。
        assert!(matches!(
            resolve_buffer_size(128, &range(4096, 16)),
            cpal::BufferSize::Fixed(16)
        ));
    }

    // ---- 音の器・発音の要求・止め合い・音量 ----

    use crate::voices::MAX_VOICES;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    /// 確保・解放の回数をスレッドごとに数えるアロケータ（コールバック内で
    /// 確保も解放もしないことを確かめるため）。
    struct CountingAllocator;

    thread_local! {
        static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    fn count_allocation() {
        let _ = ALLOCATIONS.try_with(|c| c.set(c.get() + 1));
    }

    // SAFETY: 実際の確保・解放はすべて System に委ね、回数を数えるだけ。
    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count_allocation();
            System.alloc(layout)
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            count_allocation();
            System.dealloc(ptr, layout)
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            count_allocation();
            System.realloc(ptr, layout, new_size)
        }
    }

    #[global_allocator]
    static COUNTING_ALLOCATOR: CountingAllocator = CountingAllocator;

    const SR: u32 = 48_000;

    /// 書き手（エンジン）と読み手（ミキサー）を組にしたテスト用の道具。
    struct Rig {
        engine: Arc<AudioEngine>,
        mixer: Mixer,
    }

    impl Rig {
        fn new(kit: Kit) -> Self {
            let engine = Arc::new(AudioEngine::new(kit, SR));
            let mixer = Mixer::new(engine.clone());
            Self { engine, mixer }
        }

        fn free_kit() -> Self {
            Self::new(build_free_kit(SR))
        }

        /// 出力を `n` フレーム分作る（モノラル）。要求は呼び出しの先頭で取り込まれる。
        fn render(&mut self, n: usize) -> Vec<f32> {
            let mut buf = vec![0.0_f32; n];
            self.mixer.fill_output(&mut buf, 1);
            buf
        }
    }

    /// 一定値の試験波形を持つキット。`a` `b` は止め合いの組1、`other` は組2、`plain` は組なし。
    fn test_kit(a_level: f32, b_level: f32) -> Kit {
        let mut kit = Kit::new();
        kit.add("a", vec![vec![a_level; SR as usize]], Some(1)).unwrap();
        kit.add("b", vec![vec![b_level; SR as usize]], Some(1)).unwrap();
        kit.add("other", vec![vec![a_level; SR as usize]], Some(2)).unwrap();
        kit.add("plain", vec![vec![a_level; SR as usize]], None).unwrap();
        kit
    }

    fn max_adjacent_diff(samples: &[f32]) -> f32 {
        samples.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0_f32, f32::max)
    }

    #[test]
    fn kick_waveform_is_unchanged_from_before_the_kit_refactor() {
        // 変更前は「キック波形 × 0.7 → tanh」。同じ式で作った波形と1サンプルずつ一致する。
        let raw = kick::synthesize_kick(SR);
        let expected: Vec<f32> = raw.iter().map(|s| (s * 0.7).tanh()).collect();

        let mut rig = Rig::free_kit();
        assert!(rig.engine.play("kick", 0, 1.0));
        let out = rig.render(raw.len());
        assert_eq!(out, expected);
    }

    #[test]
    fn master_volume_scales_the_whole_output_exactly() {
        let raw = kick::synthesize_kick(SR);
        let mut rig = Rig::free_kit();
        rig.engine.set_master_volume(0.5);
        assert!(rig.engine.play("kick", 0, 1.0));
        let out = rig.render(raw.len());
        let expected: Vec<f32> = raw.iter().map(|s| (s * 0.7).tanh() * 0.5).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn master_volume_ignores_non_numbers_and_clamps_to_unit_range() {
        let rig = Rig::free_kit();
        assert_eq!(rig.engine.master_volume(), 1.0);
        rig.engine.set_master_volume(f32::NAN);
        assert_eq!(rig.engine.master_volume(), 1.0);
        rig.engine.set_master_volume(3.0);
        assert_eq!(rig.engine.master_volume(), 1.0);
        rig.engine.set_master_volume(-1.0);
        assert_eq!(rig.engine.master_volume(), 0.0);
    }

    #[test]
    fn per_sound_volume_ratio_matches_the_requested_ratio() {
        // 頭打ち（tanh）の逆関数で線形に戻して、音量の比を見る。
        let render_with = |volume: f32| {
            let mut rig = Rig::free_kit();
            assert!(rig.engine.play("kick", 0, volume));
            rig.render(kick::synthesize_kick(SR).len())
        };
        let full = render_with(1.0);
        let quarter = render_with(0.25);

        let mut compared = 0;
        for (f, q) in full.iter().zip(&quarter) {
            if f.abs() > 0.05 {
                let ratio = q.atanh() / f.atanh();
                assert!((ratio - 0.25).abs() < 1e-3, "音量比が指定(0.25)と違う: {ratio}");
                compared += 1;
            }
        }
        assert!(compared > 1000, "比較できたサンプルが少なすぎる: {compared}");
    }

    #[test]
    fn non_numeric_or_negative_volume_request_is_silent() {
        for volume in [f32::NAN, f32::NEG_INFINITY, -1.0] {
            let mut rig = Rig::free_kit();
            assert!(rig.engine.play("kick", 0, volume));
            assert!(rig.render(2000).iter().all(|s| *s == 0.0), "音量 {volume} は無音のはず");
        }
    }

    #[test]
    fn unknown_name_is_silent_and_does_not_add_a_voice() {
        let mut rig = Rig::free_kit();
        assert!(!rig.engine.play("cowbell", 0, 1.0));
        let out = rig.render(2000);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(rig.engine.active_voice_count(), 0);

        // 名前の大文字小文字違い・空文字も同じ。
        assert!(!rig.engine.play("Kick", 0, 1.0));
        assert!(!rig.engine.play("", 0, 1.0));
        rig.render(128);
        assert_eq!(rig.engine.active_voice_count(), 0);
    }

    #[test]
    fn requests_beyond_sixteen_voices_are_not_played_and_existing_voices_keep_playing() {
        // 打鍵の現実的な速さ（30ms 間隔）で17回叩く。キックは0.9秒鳴るので17回目の時点で16音が鳴っている。
        let mut rig = Rig::free_kit();
        let interval = SR as usize * 30 / 1000;
        for i in 0..=MAX_VOICES {
            rig.engine.play("kick", 0, 1.0);
            rig.render(interval);
            assert_eq!(rig.engine.active_voice_count(), (i + 1).min(MAX_VOICES), "{}回目の後", i + 1);
        }

        // 最初の音が鳴り終わる 0.9秒 を過ぎると1音減って15音になる。
        // 17回目が鳴っていた（あるいは最古の音が奪われていた）なら、ここは16音のまま。
        let kick_len = kick::synthesize_kick(SR).len();
        let elapsed = (MAX_VOICES + 1) * interval;
        rig.render(kick_len - elapsed + interval / 2);
        assert_eq!(rig.engine.active_voice_count(), MAX_VOICES - 1);
    }

    #[test]
    fn last_play_time_is_updated_only_for_voices_that_actually_start() {
        let mut rig = Rig::free_kit();
        assert_eq!(rig.engine.last_accepted_play_ms(), None);

        // 16音を鳴らす。受理されたので時刻が入る。
        for _ in 0..MAX_VOICES {
            rig.engine.play("kick", 0, 1.0);
        }
        rig.render(128);
        let accepted = rig.engine.last_accepted_play_ms().expect("16音は鳴っている");
        assert_eq!(rig.engine.active_voice_count(), MAX_VOICES);

        // 17音目は積めるが鳴らない。直近の発音時刻は動かない。
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(rig.engine.play("kick", 0, 1.0));
        rig.render(128);
        assert_eq!(rig.engine.active_voice_count(), MAX_VOICES);
        assert_eq!(rig.engine.last_accepted_play_ms(), Some(accepted), "鳴らなかった打鍵で更新された");
    }

    #[test]
    fn overflowing_the_request_queue_drops_requests_without_blocking() {
        let rig = Rig::free_kit();
        let accepted = (0..REQUEST_QUEUE_CAPACITY + 10).filter(|_| rig.engine.play("kick", 0, 1.0)).count();
        assert_eq!(accepted, REQUEST_QUEUE_CAPACITY);
    }

    #[test]
    fn choke_fade_has_no_step_and_is_removed_by_the_fade_length() {
        let mut rig = Rig::new(test_kit(1.0, 0.0));
        let fade = rig.mixer.fade_samples as usize;

        // a を鳴らし、100サンプル後に同じ組の b（無音の波形）で止める。
        let mut out = Vec::new();
        rig.engine.play("a", 0, 1.0);
        out.extend(rig.render(100));
        rig.engine.play("b", 0, 1.0);
        out.extend(rig.render(fade + 50));

        // 段差: 4ms 以上かけて線形に下げるなら、1サンプルあたりの差は VOICE_GAIN/(4ms 分のサンプル数)
        // 以下（tanh の傾きは1以下）。即切りだと 0.6 近い段差になる。
        let limit = VOICE_GAIN / (SR as f32 * 0.004);
        let diff = max_adjacent_diff(&out);
        assert!(diff <= limit, "隣り合うサンプルの差が大きい（プツッと鳴る）: {diff} > {limit}");
    }

    #[test]
    fn choked_sound_contributes_zero_after_about_five_ms() {
        let mut rig = Rig::new(test_kit(1.0, 0.0));
        let fade = rig.mixer.fade_samples as usize;
        // フェード長は約5msである（仮値の取り違えを検出する）。
        let fade_ms = fade as f32 * 1000.0 / SR as f32;
        assert!((4.0..=6.0).contains(&fade_ms), "フェード長が約5msでない: {fade_ms}ms");

        rig.engine.play("a", 0, 1.0);
        let before = rig.render(100);
        let steady = *before.last().unwrap();
        assert!((steady - VOICE_GAIN.tanh()).abs() < 1e-6, "止める前は a が鳴っている: {steady}");

        rig.engine.play("b", 0, 1.0);
        let after = rig.render(fade + 20);

        // フェードの途中は、鳴っていた値より小さく0より大きい（止まっていない・即切りでもない）。
        let mid = after[fade / 2];
        assert!(mid > 0.0 && mid < steady, "フェードの途中の値がおかしい: {mid}");
        // 約5ms後（フェード長を過ぎた後）は、a の寄与が0。b は無音の波形なので出力は0。
        assert!(after[fade..].iter().all(|s| *s == 0.0), "5ms 後も a の寄与が残っている");
        // a は片付き、b だけが残る。
        assert_eq!(rig.engine.active_voice_count(), 1);
    }

    #[test]
    fn same_sound_retrigger_chokes_the_previous_one() {
        // 開きハイハットどうしのように、同じ音を重ねると前の音が止まる。
        let mut rig = Rig::new(test_kit(1.0, 0.0));
        let fade = rig.mixer.fade_samples as usize;
        rig.engine.play("a", 0, 1.0);
        rig.render(100);
        rig.engine.play("a", 0, 1.0);
        let out = rig.render(fade + 20);
        assert!((out.last().unwrap() - VOICE_GAIN.tanh()).abs() < 1e-6, "1音分の大きさに戻るはず");
        assert_eq!(rig.engine.active_voice_count(), 1);
    }

    #[test]
    fn sounds_in_other_groups_or_no_group_are_not_choked() {
        for other in ["other", "plain"] {
            let mut rig = Rig::new(test_kit(1.0, 0.0));
            let fade = rig.mixer.fade_samples as usize;
            rig.engine.play("a", 0, 1.0);
            rig.render(100);
            rig.engine.play(other, 0, 1.0);
            let out = rig.render(fade + 20);
            let two_voices = (2.0 * VOICE_GAIN).tanh();
            assert!((out.last().unwrap() - two_voices).abs() < 1e-6, "{other} が a を止めている");
            assert_eq!(rig.engine.active_voice_count(), 2);
        }
    }

    #[test]
    fn audio_callback_does_not_allocate_or_free() {
        let mut rig = Rig::new(test_kit(0.5, 0.5));
        // 発音の取り込み・止め合い・16音の上限・鳴り終わりまでを通す。
        for i in 0..20 {
            rig.engine.play(if i % 2 == 0 { "a" } else { "b" }, 3, 0.8);
        }
        rig.engine.play("plain", 0, 1.0);
        rig.engine.play("nothing", 0, 1.0);
        let mut buf = vec![0.0_f32; 256];

        let before = ALLOCATIONS.with(|c| c.get());
        for _ in 0..(SR as usize / 256 + 8) {
            rig.mixer.fill_output(&mut buf, 2);
        }
        let allocations = ALLOCATIONS.with(|c| c.get()) - before;
        assert_eq!(allocations, 0, "音声コールバックの中で確保・解放が起きた");
    }

    // ---- 重なりの扱い（固定倍率0.7＋tanh）の回帰 ----

    // 合算後を clamp するだけだった旧方式は、重なると ±1.0 に張り付いて音割れした
    // （issue #6 原因C）。現実的な最悪ケース＝同時押し4音で、張り付きが起きないこと。
    #[test]
    fn four_simultaneous_voices_do_not_stick_to_full_scale() {
        let mut rig = Rig::free_kit();
        for _ in 0..4 {
            rig.engine.play("kick", 0, 1.0);
        }
        let total = kick::synthesize_kick(SR).len();
        let out = rig.render(total);
        assert_eq!(rig.engine.active_voice_count(), 0, "鳴り終わっている");

        let clipped = out.iter().filter(|s| s.abs() >= 0.999).count();
        let ratio = clipped as f32 / total as f32;
        assert!(ratio < 0.05, "クリップに張り付いたサンプルが多い: {:.1}%", ratio * 100.0);
    }

    // 単発の1音のピークが実用的な大きさに収まっていること（VOICE_GAIN の変更で崩れる）。
    #[test]
    fn single_voice_peak_is_in_usable_range() {
        let mut rig = Rig::free_kit();
        rig.engine.play("kick", 0, 1.0);
        let peak = rig.render(kick::synthesize_kick(SR).len()).iter().fold(0.0_f32, |p, s| p.max(s.abs()));
        assert!((0.5..=0.7).contains(&peak), "単発のピークが実用範囲(0.5〜0.7)から外れている: {peak:.3}");
    }

    // 高速連打（30ms 間隔で16音）でも、tanh の飽和域（合算×0.7 が 2 超）に居続けないこと。
    #[test]
    fn fast_typing_does_not_stay_in_saturation() {
        let kit = build_free_kit(SR);
        let mut pool = VoicePool::new();
        let interval = SR as usize * 30 / 1000;
        let total = SR as usize * 9 / 10;
        let mut saturated = 0usize;
        for i in 0..total {
            if i % interval == 0 && i / interval < MAX_VOICES {
                pool.start(&kit, PlayRequest { sound: 0, variant: 0, volume: 1.0 }, 240);
            }
            if (pool.mix_next_sample(&kit) * VOICE_GAIN).abs() > 2.0 {
                saturated += 1;
            }
        }
        let ratio = saturated as f32 / total as f32;
        assert!(ratio < 0.01, "飽和域に留まるサンプルが多い: {:.1}%", ratio * 100.0);
    }

    // 強弱の幅を最大にして 30ms 間隔で連打しても、頭打ち（tanh の飽和域）に張り付く割合が、
    // 強弱なし（常に音量 1.0）の実測より悪くならない。「超えない」だけでなく、連打の音量が実際に
    // 下がり（最初の1打だけが大きい）、混ざった波形のピークも実際に低いことまで確かめる。
    #[test]
    fn full_width_dynamics_on_30ms_hits_does_not_stick_to_saturation_more_than_without() {
        use crate::dynamics::Dynamics;
        use std::time::{Duration, Instant};

        let kit = build_free_kit(SR);
        let interval = SR as usize * 30 / 1000;
        let total = SR as usize * 9 / 10;
        // 16音を 30ms 間隔で鳴らし、(飽和域のサンプル割合, 混ぜた値のピーク, 実際に使った音量の列) を返す。
        let run = |dynamics: Option<&Dynamics>| {
            let mut pool = VoicePool::new();
            let start = Instant::now();
            let (mut saturated, mut peak, mut volumes) = (0usize, 0.0_f32, Vec::new());
            for i in 0..total {
                if i % interval == 0 && i / interval < MAX_VOICES {
                    let hit = (i / interval) as u64;
                    let volume = dynamics
                        .map_or(1.0, |d| d.next_volume(start + Duration::from_millis(30 * hit)));
                    volumes.push(volume);
                    pool.start(&kit, PlayRequest { sound: 0, variant: 0, volume }, 240);
                }
                let mixed = (pool.mix_next_sample(&kit) * VOICE_GAIN).abs();
                peak = peak.max(mixed);
                if mixed > 2.0 {
                    saturated += 1;
                }
            }
            (saturated as f32 / total as f32, peak, volumes)
        };

        let (base_ratio, base_peak, _) = run(None);
        let dynamics = Dynamics::with_seed(1.0, 3);
        let (dyn_ratio, dyn_peak, volumes) = run(Some(&dynamics));

        assert!(dyn_ratio <= base_ratio, "強弱で張り付きが悪化: {dyn_ratio} > {base_ratio}");
        assert!(dyn_ratio < 0.01, "飽和域に留まるサンプルが多い: {:.1}%", dyn_ratio * 100.0);
        // 最初の1打は最大側、30ms の連打は最小側（最大の約 -10dB = 0.32 前後、揺らぎ込みで 0.4 未満）。
        assert!(volumes[0] > 0.75, "最初の1打が大きくない: {volumes:?}");
        assert!(volumes[1..].iter().all(|v| *v < 0.4), "連打の音量が下がっていない: {volumes:?}");
        assert!(dyn_peak < base_peak * 0.6, "混ざった波形のピークが実際に下がっていない: {dyn_peak} vs {base_peak}");
    }

    // 「同時発音数で割る」方式は重なった瞬間に既存の音が跳ねて「プツッ」と鳴った（PR #7）。
    // 100ms 間隔で2音・3音を重ねても、単発の波形が元々持つ隣接差を超えないこと。
    #[test]
    fn overlapping_voices_do_not_create_larger_step_than_single_voice() {
        let raw = kick::synthesize_kick(SR);
        let single_max_diff = max_adjacent_diff(&raw);

        let mut rig = Rig::free_kit();
        let interval = SR as usize / 10;
        let mut out = Vec::new();
        for _ in 0..3 {
            rig.engine.play("kick", 0, 1.0);
            out.extend(rig.render(interval));
        }
        out.extend(rig.render(raw.len()));
        let overlap_max_diff = max_adjacent_diff(&out);
        assert!(
            overlap_max_diff <= single_max_diff,
            "重なりの瞬間に単発時より大きな段差がある: overlap={overlap_max_diff} single={single_max_diff}"
        );
    }
}

