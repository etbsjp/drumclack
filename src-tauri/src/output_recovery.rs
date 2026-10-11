//! 音の出口（出力ストリーム）の見張りと作り直し。
//!
//! イヤホンの抜き差し・Bluetooth の接続と切断・スリープ復帰・既定の出力先の変更のあとも鳴り続けるよう、
//! 次の3つを見て、出口を作り直す。
//!
//! 1. ストリームのエラー（cpal の `err_fn`）。OS が出口を取り上げたとき。
//! 2. 既定の出力先が、いま使っているものと違う（名前・サンプルレート・チャンネル数）。
//! 3. コールバックが一定時間まったく呼ばれていない（エラーなしで止まる場合の保険）。
//!
//! 作り直しに失敗している間は [`AudioEngine`] の出力状態を「失敗」にし、間隔を空けて再試行する
//! （状態の帯・トレイアイコンの「問題あり」はこの状態を読む）。直ったら自動で「正常」に戻す。
//!
//! キットの合成は、同じサンプルレートの出口に作り直すときはやり直さない。サンプルレートが変わる出口
//! （44.1kHz と 48kHz など）のときだけ、そのレートで合成し直す（再生側でレートを変換しない）。
//!
//! cpal に触れる部分は [`OutputBackend`] の向こう側に置き、ここは cpal を知らない（自動テストで、
//! 実機の音声デバイスが無くても「失敗→再試行→成功」などを確かめられるようにするため）。
//! 打鍵の内容は一切扱わない。

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio::{AudioEngine, Mixer, OutputStatus};
use crate::voices::Kit;

// ---- 調整できる値（すべて仮の値。聞こえ方・復帰の早さに関わるのでここを変えて調整する） ----------------

/// 出口の様子（既定の出力先・コールバックの進み）を見に行く間隔。
/// 短いほど切り替わりに早く気づくが、既定の出力先を問い合わせる回数が増える。
pub const WATCH_INTERVAL: Duration = Duration::from_millis(1000);

/// 作り直しの再試行の間隔の最初の値。失敗するたびに2倍にして [`RETRY_MAX_INTERVAL`] で頭打ちにする。
/// 出口を失った直後の1回目もこの間隔だけ待つ（OS が既定の出力先を切り替え終わるのを待つため）。
pub const RETRY_FIRST_INTERVAL: Duration = Duration::from_millis(500);

/// 再試行の間隔の上限。
pub const RETRY_MAX_INTERVAL: Duration = Duration::from_millis(5000);

/// 作り直した出口がこの時間つづけて正常なら、次に失ったときの再試行の間隔を最初の値に戻す。
/// 作り直した直後にまた失う（エラーを出し続ける出口）ときに、作り直しを忙しく繰り返さないための歯止め。
pub const HEALTHY_RESET_AFTER: Duration = Duration::from_secs(10);

/// コールバックが進んでいない見張りの回数がこの回数つづいたら、止まったとみなして作り直す
/// （`WATCH_INTERVAL` × この回数 ≒ 止まったと判断するまでの秒数）。
pub const STALL_WATCH_COUNT: u32 = 3;

/// 出力先が見つからないまま起動したときに、仮に合成するキットのサンプルレート。
/// 後で見つかった出力先のレートがこれと違えば、そのレートで合成し直す。
pub const FALLBACK_SAMPLE_RATE: u32 = 48_000;

// ---- 出口の情報と、ストリームの健康状態 ----------------------------------------------------------

/// 出力先の情報。見張りは、この3つのどれかが変わったら「別の出口」とみなす（比べ方は
/// [`OutputInfo::is_same_output_as`]）。同じ名前の機器が2台つながっているときの区別はしない（名前しか取れない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputInfo {
    /// 機器名。OS が名前を返せないときは `None`（名前が取れなくても、その出口で鳴らす）。
    pub device_name: Option<String>,
    pub sample_rate: u32,
    pub channels: usize,
}

impl OutputInfo {
    /// 同じ出口とみなせるか。名前は、両方が取れているときだけ比べる。
    /// 名前の取得は一瞬だけ失敗することがあり、それを「別の出力先に変わった」と見ると、
    /// 動いている出口を誤って手放して作り直しを繰り返すため。レートとチャンネル数は常に比べる。
    pub fn is_same_output_as(&self, other: &OutputInfo) -> bool {
        let same_name = match (&self.device_name, &other.device_name) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        };
        same_name && self.sample_rate == other.sample_rate && self.channels == other.channels
    }
}

/// ストリーム1本ぶんの健康状態。音声のスレッドが書き、見張りが読む（原子的な値だけ。ロックなし）。
#[derive(Debug, Default)]
pub struct StreamHealth {
    failed: AtomicBool,
    callbacks: AtomicU64,
}

impl StreamHealth {
    pub fn new() -> Self {
        Self::default()
    }

    /// ストリームのエラーを受けたとき（`err_fn`）に呼ぶ。
    pub fn mark_failed(&self) {
        self.failed.store(true, Ordering::Relaxed);
    }

    pub fn is_failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// 音声コールバックが1回呼ばれるたびに呼ぶ（確保・ロックなし）。
    pub fn note_callback(&self) {
        self.callbacks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn callbacks(&self) -> u64 {
        self.callbacks.load(Ordering::Relaxed)
    }
}

/// 出力の部品（実機では cpal）。見張りが必要とする操作だけを持つ。
pub trait OutputBackend {
    /// 作った出口を保持するもの。落とすと再生が止まる。
    type Stream;

    /// いまの既定の出力先を調べる。見つからないときは理由。
    fn probe(&mut self) -> Result<OutputInfo, String>;

    /// いまの既定の出力先でストリームを作って再生を始める。
    /// 実際に使う出力先が分かった時点で `make_mixer` を呼び、返った [`Mixer`] を音声コールバックへ渡す
    /// （どのキットで鳴らすかは見張りが決める）。コールバックは `health` へ進みを書き、
    /// エラーを受けたら `health` を失敗にする。
    fn open(
        &mut self,
        make_mixer: &mut dyn FnMut(&OutputInfo) -> Mixer,
        health: Arc<StreamHealth>,
    ) -> Result<(OutputInfo, Self::Stream), String>;
}

// ---- 再試行の間隔 ---------------------------------------------------------------------------

/// 再試行の間隔。呼ぶたびに倍になる。
#[derive(Debug)]
struct RetryBackoff {
    next: Duration,
}

impl RetryBackoff {
    fn new() -> Self {
        Self { next: RETRY_FIRST_INTERVAL }
    }

    fn take(&mut self) -> Duration {
        let current = self.next;
        self.next = (self.next * 2).min(RETRY_MAX_INTERVAL);
        current
    }

    fn reset(&mut self) {
        self.next = RETRY_FIRST_INTERVAL;
    }
}

// ---- 見張り ----------------------------------------------------------------------------------

/// 動いている出口。
struct Running<S> {
    /// 落とすと再生が止まるので、使わなくても持ち続ける。
    _stream: S,
    info: OutputInfo,
    health: Arc<StreamHealth>,
    started_at: Instant,
    last_callbacks: u64,
    stalled_watches: u32,
}

/// 出口の見張りと作り直し。専用のスレッドから [`OutputSupervisor::tick`] を呼び続ける。
pub struct OutputSupervisor<B: OutputBackend> {
    backend: B,
    engine: Arc<AudioEngine>,
    build_kit: Box<dyn Fn(u32) -> Kit>,
    /// 出口へ渡すキットと、それを合成したサンプルレート。
    kit: Arc<Kit>,
    kit_sample_rate: u32,
    running: Option<Running<B::Stream>>,
    backoff: RetryBackoff,
    next_attempt_at: Instant,
    next_watch_at: Instant,
    /// 直近にログへ出した失敗の理由。同じ理由を繰り返し出さないために覚える。
    last_logged_failure: Option<String>,
}

impl<B: OutputBackend> OutputSupervisor<B> {
    /// 見張りを作り、最初の出口づくりを1回試す。出力先が見つからなくても `AudioEngine` は作る
    /// （後から出力先が現れたとき、同じエンジンのまま鳴らせるようにするため）。
    /// `build_kit` は指定のサンプルレートでキットを合成する関数（製品では `build_free_kit`）。
    pub fn start(backend: B, build_kit: Box<dyn Fn(u32) -> Kit>, now: Instant) -> Self {
        let mut backend = backend;
        let initial_rate = backend.probe().map(|info| info.sample_rate).unwrap_or(FALLBACK_SAMPLE_RATE);
        let kit = Arc::new(build_kit(initial_rate));
        let engine = Arc::new(AudioEngine::with_shared_kit(kit.clone(), initial_rate));
        let mut supervisor = Self {
            backend,
            engine,
            build_kit,
            kit,
            kit_sample_rate: initial_rate,
            running: None,
            backoff: RetryBackoff::new(),
            next_attempt_at: now,
            next_watch_at: now + WATCH_INTERVAL,
            last_logged_failure: None,
        };
        supervisor.try_open(now);
        supervisor
    }

    /// 発音の窓口。作り直しても同じものを使い続ける。
    pub fn engine(&self) -> Arc<AudioEngine> {
        self.engine.clone()
    }

    /// 出口が動いているか。
    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// 見張りを1回進める。`now` は現在時刻（テストでは進めた時刻を渡す）。
    pub fn tick(&mut self, now: Instant) {
        self.watch(now);
        if self.running.is_none() && now >= self.next_attempt_at {
            self.try_open(now);
        }
    }

    /// [`OutputSupervisor::tick`] を、panic しても見張りが止まらないように包んで呼ぶ（専用スレッドが呼ぶ）。
    /// panic したら、出口を手放して出力状態を「問題あり」にし、次の再試行の時刻を決めて続ける
    /// （panic のまま見張りが死ぬと、状態が「正常」のまま作り直されず無音になるため）。
    /// 戻り値は panic したか。
    pub fn tick_guarded(&mut self, now: Instant) -> bool {
        let panicked = catch_unwind(AssertUnwindSafe(|| self.tick(now))).is_err();
        if panicked {
            eprintln!("[drumclack] 音声出力の見張りで内部エラーが起きました（出口を作り直して続けます）");
            self.running = None;
            self.engine.set_output_status(OutputStatus::Err("音声出力の見張りで内部エラーが起きました。".to_string()));
            self.next_attempt_at = now + self.backoff.take();
        }
        panicked
    }

    /// 動いている出口を見て、失っていれば手放す。
    fn watch(&mut self, now: Instant) {
        let Some(running) = self.running.as_mut() else {
            return;
        };

        // エラーを受けたときは、見張りの間隔を待たずに手放す。
        let mut lost: Option<&'static str> = if running.health.is_failed() { Some("stream_error") } else { None };

        if lost.is_none() && now >= self.next_watch_at {
            self.next_watch_at = now + WATCH_INTERVAL;

            // コールバックが進んでいるか。
            let callbacks = running.health.callbacks();
            if callbacks == running.last_callbacks {
                running.stalled_watches += 1;
            } else {
                running.last_callbacks = callbacks;
                running.stalled_watches = 0;
            }
            if running.stalled_watches >= STALL_WATCH_COUNT {
                lost = Some("callback_stalled");
            } else if let Ok(current) = self.backend.probe() {
                // 既定の出力先が変わったか。調べられないとき（Err）は、動いている出口を手放さない。
                if !current.is_same_output_as(&running.info) {
                    lost = Some("default_output_changed");
                }
            }
        }

        if let Some(reason) = lost {
            // 条件の名前だけをログに出す（打鍵の内容は持っていない）。
            eprintln!("[drumclack] 音声出力を作り直します: {reason}");
            self.lose_output(now);
        }
    }

    /// 動いていた出口を手放し、次の試行の時刻を決める。
    fn lose_output(&mut self, now: Instant) {
        // 古いストリームはここで落とし切る。新しいミキサーを作る（`try_open`）より前に、
        // 古いコールバックが止まっていること。待ち行列の読み手（`RequestQueue::pop`）は
        // 常に1つのコールバックだけ、という前提がこの順序に依っている。
        if let Some(running) = self.running.take() {
            if now.duration_since(running.started_at) >= HEALTHY_RESET_AFTER {
                self.backoff.reset();
            }
        }
        debug_assert!(self.running.is_none());
        self.next_attempt_at = now + self.backoff.take();
    }

    /// 出口を作ってみる。成功すれば出力状態を「正常」に、失敗すれば「失敗」にして次の試行を決める。
    fn try_open(&mut self, now: Instant) {
        // 動いている出口があるまま新しいミキサーを作ると、待ち行列の読み手が2つになる（`lose_output` を参照）。
        // 呼び出し側は出口が無いときだけ呼ぶが、本番ビルドでも読み手が2つにならないよう、ここでも防ぐ。
        if self.running.is_some() {
            return;
        }
        let health = Arc::new(StreamHealth::new());
        let engine = &self.engine;
        let build_kit = &self.build_kit;
        let kit = &mut self.kit;
        let kit_sample_rate = &mut self.kit_sample_rate;

        let mut make_mixer = |info: &OutputInfo| -> Mixer {
            // 同じサンプルレートならキットを使い回す。違うときだけ、そのレートで合成し直す。
            if *kit_sample_rate != info.sample_rate {
                *kit = Arc::new(build_kit(info.sample_rate));
                *kit_sample_rate = info.sample_rate;
            }
            Mixer::for_output(engine.clone(), kit.clone(), info.sample_rate)
        };

        match self.backend.open(&mut make_mixer, health.clone()) {
            Ok((info, stream)) => {
                println!(
                    "[drumclack] audio output ready: device={:?} sample_rate={}Hz channels={}",
                    info.device_name, info.sample_rate, info.channels
                );
                self.engine.set_output_status(OutputStatus::Ok { sample_rate: info.sample_rate });
                self.last_logged_failure = None;
                self.running = Some(Running {
                    _stream: stream,
                    info,
                    health,
                    started_at: now,
                    last_callbacks: 0,
                    stalled_watches: 0,
                });
                self.next_watch_at = now + WATCH_INTERVAL;
            }
            Err(reason) => {
                if self.last_logged_failure.as_deref() != Some(reason.as_str()) {
                    eprintln!("[drumclack] 音声出力を開けませんでした（間隔を空けて再試行します）: {reason}");
                    self.last_logged_failure = Some(reason.clone());
                }
                self.engine.set_output_status(OutputStatus::Err(reason));
                self.next_attempt_at = now + self.backoff.take();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drums::build_free_kit;
    use crate::voices::MAX_VOICES;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    // ---- 実機の音声デバイスの代わりになる偽物 ----

    /// 偽の世界。テストが書き換え、偽のバックエンドが読む。
    struct World {
        /// いまの既定の出力先（見つからないときは Err）。
        default_output: Result<OutputInfo, String>,
        /// あと何回、開くのに失敗させるか。
        open_failures_left: usize,
        /// 開くのを試した回数（失敗も含む）。
        open_attempts: usize,
        /// 開くのに成功した出口の情報（古い順）。
        opened: Vec<OutputInfo>,
        /// 最後に開いた出口のコールバック（本物では cpal のスレッドが持つ）。
        mixer: Option<Mixer>,
        health: Option<Arc<StreamHealth>>,
        /// 生きているストリームの数（落とされたら減る）。
        live_streams: Rc<Cell<usize>>,
        /// 真の間、既定の出力先の問い合わせで panic する。
        panic_on_probe: bool,
    }

    type SharedWorld = Rc<RefCell<World>>;

    struct FakeStream {
        live: Rc<Cell<usize>>,
    }

    impl Drop for FakeStream {
        fn drop(&mut self) {
            self.live.set(self.live.get() - 1);
        }
    }

    struct FakeBackend {
        world: SharedWorld,
    }

    impl OutputBackend for FakeBackend {
        type Stream = FakeStream;

        fn probe(&mut self) -> Result<OutputInfo, String> {
            if self.world.borrow().panic_on_probe {
                panic!("テスト用の panic");
            }
            self.world.borrow().default_output.clone()
        }

        fn open(
            &mut self,
            make_mixer: &mut dyn FnMut(&OutputInfo) -> Mixer,
            health: Arc<StreamHealth>,
        ) -> Result<(OutputInfo, FakeStream), String> {
            let mut world = self.world.borrow_mut();
            world.open_attempts += 1;
            let info = world.default_output.clone()?;
            if world.open_failures_left > 0 {
                world.open_failures_left -= 1;
                return Err("出力ストリームの構築に失敗しました（テスト用）".to_string());
            }
            world.mixer = Some(make_mixer(&info));
            world.health = Some(health);
            world.opened.push(info.clone());
            world.live_streams.set(world.live_streams.get() + 1);
            Ok((info, FakeStream { live: world.live_streams.clone() }))
        }
    }

    fn info(name: &str, rate: u32) -> OutputInfo {
        OutputInfo { device_name: Some(name.to_string()), sample_rate: rate, channels: 2 }
    }

    fn nameless_info(rate: u32) -> OutputInfo {
        OutputInfo { device_name: None, sample_rate: rate, channels: 2 }
    }

    fn new_world(default_output: Result<OutputInfo, String>) -> SharedWorld {
        Rc::new(RefCell::new(World {
            default_output,
            open_failures_left: 0,
            open_attempts: 0,
            opened: Vec::new(),
            mixer: None,
            health: None,
            live_streams: Rc::new(Cell::new(0)),
            panic_on_probe: false,
        }))
    }

    /// 見張りと、キットを合成した回数・レートの記録。
    struct Rig {
        world: SharedWorld,
        supervisor: OutputSupervisor<FakeBackend>,
        synthesized_rates: Rc<RefCell<Vec<u32>>>,
        clock: Instant,
    }

    impl Rig {
        fn start(world: SharedWorld) -> Self {
            let synthesized_rates = Rc::new(RefCell::new(Vec::new()));
            let log = synthesized_rates.clone();
            let clock = Instant::now();
            let supervisor = OutputSupervisor::start(
                FakeBackend { world: world.clone() },
                Box::new(move |rate| {
                    log.borrow_mut().push(rate);
                    build_free_kit(rate)
                }),
                clock,
            );
            Self { world, supervisor, synthesized_rates, clock }
        }

        /// 時計を進めて、見張りを1回進める。
        fn advance(&mut self, by: Duration) {
            self.clock += by;
            self.supervisor.tick(self.clock);
        }

        /// 出口が正常に動いている印として、コールバックを1回進める（本物ではオーディオスレッドの仕事）。
        fn callback_once(&self) {
            self.world.borrow().health.as_ref().unwrap().note_callback();
        }

        fn status(&self) -> OutputStatus {
            self.supervisor.engine().output_status()
        }

        fn synth_count(&self) -> usize {
            self.synthesized_rates.borrow().len()
        }

        /// 最後に開いた出口の Mixer で、`frames` フレーム（モノラル）を作る。
        fn render(&self, frames: usize) -> Vec<f32> {
            let mut world = self.world.borrow_mut();
            let mut buf = vec![0.0_f32; frames];
            world.mixer.as_mut().unwrap().fill_output(&mut buf, 1);
            buf
        }
    }

    // ---- 失敗 → 再試行 → 成功 ----

    #[test]
    fn failed_open_is_retried_with_a_gap_until_it_succeeds_and_the_problem_clears() {
        let world = new_world(Ok(info("内蔵スピーカー", 48_000)));
        world.borrow_mut().open_failures_left = 2;
        let mut rig = Rig::start(world.clone());

        // 起動時の1回目は失敗。状態は「問題あり」。
        assert_eq!(world.borrow().open_attempts, 1);
        assert!(matches!(rig.status(), OutputStatus::Err(_)));
        assert!(!rig.supervisor.is_running());

        // 間隔（最初は 500ms）が来るまでは試さない。
        rig.advance(Duration::from_millis(100));
        assert_eq!(world.borrow().open_attempts, 1, "間隔を空けずに試した");

        // 間隔が来たら2回目。これも失敗する。
        rig.advance(RETRY_FIRST_INTERVAL);
        assert_eq!(world.borrow().open_attempts, 2);
        assert!(matches!(rig.status(), OutputStatus::Err(_)));

        // 次の間隔は倍になる。最初の間隔では試さない。
        rig.advance(RETRY_FIRST_INTERVAL);
        assert_eq!(world.borrow().open_attempts, 2, "間隔が倍になっていない");

        // 3回目で成功し、「問題あり」が自動で消える。
        rig.advance(RETRY_FIRST_INTERVAL);
        assert_eq!(world.borrow().open_attempts, 3);
        assert!(rig.supervisor.is_running());
        assert_eq!(rig.status(), OutputStatus::Ok { sample_rate: 48_000 });
    }

    #[test]
    fn startup_without_any_output_device_recovers_when_a_device_appears() {
        let world = new_world(Err("出力デバイスが見つかりませんでした".to_string()));
        let mut rig = Rig::start(world.clone());
        let engine = rig.supervisor.engine();
        assert!(matches!(rig.status(), OutputStatus::Err(_)));

        // 出力先が無い間は、何度試しても失敗のまま。
        rig.advance(RETRY_FIRST_INTERVAL);
        assert!(matches!(rig.status(), OutputStatus::Err(_)));

        // 出力先が現れたら、次の再試行で正常になり、同じエンジンで鳴る。
        world.borrow_mut().default_output = Ok(info("イヤホン", 48_000));
        rig.advance(RETRY_MAX_INTERVAL);
        assert_eq!(rig.status(), OutputStatus::Ok { sample_rate: 48_000 });
        assert!(Arc::ptr_eq(&engine, &rig.supervisor.engine()));
        // 出口の最初のコールバックは、溜まっていた要求を捨てる。それが済んでから打つ。
        rig.render(1);
        assert!(engine.play("kick", 0, 1.0));
        assert!(rig.render(2_000).iter().any(|s| s.abs() > 0.1), "作り直した出口から音が出ない");
    }

    #[test]
    fn retry_interval_is_capped() {
        let mut backoff = RetryBackoff::new();
        let mut last = Duration::ZERO;
        for _ in 0..10 {
            last = backoff.take();
        }
        assert_eq!(last, RETRY_MAX_INTERVAL);
    }

    // ---- 出口を失ったときの作り直し ----

    #[test]
    fn stream_error_rebuilds_the_output_without_resynthesizing_the_kit() {
        let world = new_world(Ok(info("内蔵スピーカー", 48_000)));
        let mut rig = Rig::start(world.clone());
        let engine = rig.supervisor.engine();
        assert_eq!(rig.synth_count(), 1);

        // OS が出口を取り上げた（cpal の err_fn が呼ばれた）。
        world.borrow().health.as_ref().unwrap().mark_failed();
        rig.advance(Duration::from_millis(1));
        assert!(!rig.supervisor.is_running(), "古い出口を手放していない");
        assert_eq!(world.borrow().live_streams.get(), 0);

        rig.advance(RETRY_FIRST_INTERVAL);
        assert!(rig.supervisor.is_running());
        assert_eq!(world.borrow().opened.len(), 2, "作り直していない");
        assert_eq!(world.borrow().live_streams.get(), 1, "出口が二重にある");
        assert_eq!(rig.status(), OutputStatus::Ok { sample_rate: 48_000 });

        // キットは合成し直さず、エンジンも同じまま、作り直した出口から鳴る。
        assert_eq!(rig.synth_count(), 1, "同じレートなのにキットを合成し直した");
        assert!(Arc::ptr_eq(&engine, &rig.supervisor.engine()));
        rig.render(1);
        assert!(engine.play("snare", 0, 1.0));
        assert!(rig.render(2_000).iter().any(|s| s.abs() > 0.1));
    }

    #[test]
    fn default_output_change_rebuilds_the_output() {
        let world = new_world(Ok(info("内蔵スピーカー", 48_000)));
        let mut rig = Rig::start(world.clone());
        rig.callback_once();

        // 既定の出力先が Bluetooth のヘッドホンに変わった（同じレート）。
        world.borrow_mut().default_output = Ok(info("Bluetooth ヘッドホン", 48_000));
        rig.advance(WATCH_INTERVAL);
        rig.advance(RETRY_FIRST_INTERVAL);

        assert_eq!(world.borrow().opened.len(), 2, "既定の出力先が変わったのに作り直していない");
        assert_eq!(world.borrow().opened[1].device_name.as_deref(), Some("Bluetooth ヘッドホン"));
        assert_eq!(rig.synth_count(), 1, "同じレートなのにキットを合成し直した");
    }

    #[test]
    fn unchanged_healthy_output_is_left_alone() {
        let world = new_world(Ok(info("内蔵スピーカー", 48_000)));
        let mut rig = Rig::start(world.clone());

        for _ in 0..20 {
            rig.callback_once();
            rig.advance(WATCH_INTERVAL);
        }
        assert_eq!(world.borrow().opened.len(), 1, "正常な出口を作り直した");
        assert_eq!(world.borrow().open_attempts, 1);
    }

    #[test]
    fn a_stream_that_silently_stops_calling_back_is_rebuilt() {
        let world = new_world(Ok(info("内蔵スピーカー", 48_000)));
        let mut rig = Rig::start(world.clone());

        // 最初は動いていた（コールバックが進む）。
        rig.callback_once();
        rig.advance(WATCH_INTERVAL);
        assert_eq!(world.borrow().opened.len(), 1);

        // スリープ復帰などで、エラーなしにコールバックが止まる。
        for _ in 0..STALL_WATCH_COUNT {
            rig.advance(WATCH_INTERVAL);
        }
        rig.advance(RETRY_FIRST_INTERVAL);
        assert_eq!(world.borrow().opened.len(), 2, "止まった出口を作り直していない");
    }

    #[test]
    fn failed_probe_does_not_drop_a_working_output() {
        let world = new_world(Ok(info("内蔵スピーカー", 48_000)));
        let mut rig = Rig::start(world.clone());

        // 既定の出力先を調べられない一瞬があっても、鳴っている出口は手放さない。
        world.borrow_mut().default_output = Err("一時的に調べられない".to_string());
        rig.callback_once();
        rig.advance(WATCH_INTERVAL);
        assert!(rig.supervisor.is_running());
        assert_eq!(rig.status(), OutputStatus::Ok { sample_rate: 48_000 });
    }

    #[test]
    fn rapidly_failing_output_backs_off_instead_of_rebuilding_in_a_tight_loop() {
        let world = new_world(Ok(info("内蔵スピーカー", 48_000)));
        let mut rig = Rig::start(world.clone());

        // 作り直しても、すぐにエラーを出し続ける出口。
        let mut attempts_seen = Vec::new();
        for _ in 0..6 {
            world.borrow().health.as_ref().unwrap().mark_failed();
            rig.advance(Duration::from_millis(1));
            // 次の試行まで、間隔を1msずつ進めて、何ms待ったかを測る。
            let mut waited = 0u64;
            while !rig.supervisor.is_running() {
                rig.advance(Duration::from_millis(50));
                waited += 50;
                assert!(waited < 20_000);
            }
            attempts_seen.push(waited);
        }
        assert!(
            attempts_seen.windows(2).any(|w| w[1] > w[0]),
            "作り直しの間隔が広がっていない: {attempts_seen:?}"
        );
    }

    #[test]
    fn an_output_whose_name_cannot_be_read_still_plays_and_is_not_rebuilt_over_and_over() {
        let world = new_world(Ok(nameless_info(48_000)));
        let mut rig = Rig::start(world.clone());

        // 名前が取れない出口でも、開けて鳴る。
        assert!(rig.supervisor.is_running(), "名前が取れないだけで開けない");
        assert_eq!(rig.status(), OutputStatus::Ok { sample_rate: 48_000 });
        rig.render(1);
        assert!(rig.supervisor.engine().play("kick", 0, 1.0));
        assert!(rig.render(2_000).iter().any(|s| s.abs() > 0.1));

        // 見張りが毎回「別の出口」と誤判定して、作り直しを繰り返さない。
        for _ in 0..10 {
            rig.callback_once();
            rig.advance(WATCH_INTERVAL);
        }
        assert_eq!(world.borrow().opened.len(), 1, "名前が取れない出口を作り直し続けた");
    }

    #[test]
    fn a_momentary_name_failure_does_not_drop_a_named_output_but_a_rate_change_still_does() {
        let world = new_world(Ok(info("内蔵スピーカー", 48_000)));
        let mut rig = Rig::start(world.clone());

        // 名前の取得だけが一瞬失敗した（レート・チャンネルは同じ）。手放さない。
        world.borrow_mut().default_output = Ok(nameless_info(48_000));
        rig.callback_once();
        rig.advance(WATCH_INTERVAL);
        assert_eq!(world.borrow().opened.len(), 1);
        assert!(rig.supervisor.is_running());

        // 名前が取れなくても、レートが変われば別の出口として作り直す。
        world.borrow_mut().default_output = Ok(nameless_info(44_100));
        rig.callback_once();
        rig.advance(WATCH_INTERVAL);
        rig.advance(RETRY_FIRST_INTERVAL);
        assert_eq!(world.borrow().opened.len(), 2);
        assert_eq!(world.borrow().opened[1].sample_rate, 44_100);
    }

    #[test]
    fn try_open_does_nothing_while_an_output_is_running() {
        let world = new_world(Ok(info("内蔵スピーカー", 48_000)));
        let mut rig = Rig::start(world.clone());
        let attempts = world.borrow().open_attempts;

        rig.supervisor.try_open(rig.clock);
        assert_eq!(world.borrow().open_attempts, attempts, "動いている出口があるのに作ろうとした");
        assert_eq!(world.borrow().live_streams.get(), 1);
    }

    #[test]
    fn a_panic_in_the_watch_marks_the_output_as_a_problem_and_the_watch_keeps_going() {
        let world = new_world(Ok(info("内蔵スピーカー", 48_000)));
        let mut rig = Rig::start(world.clone());
        rig.callback_once();

        // 見張りの途中で panic する。状態が「正常」のまま残ってはいけない。
        world.borrow_mut().panic_on_probe = true;
        rig.clock += WATCH_INTERVAL;
        assert!(rig.supervisor.tick_guarded(rig.clock), "panic を捕まえていない");
        assert!(matches!(rig.status(), OutputStatus::Err(_)), "panic したのに「問題なし」のまま");
        assert!(!rig.supervisor.is_running());

        // 原因がなくなれば、見張りは続いていて、作り直して正常に戻る。
        world.borrow_mut().panic_on_probe = false;
        rig.clock += RETRY_MAX_INTERVAL;
        assert!(!rig.supervisor.tick_guarded(rig.clock));
        assert!(rig.supervisor.is_running());
        assert_eq!(rig.status(), OutputStatus::Ok { sample_rate: 48_000 });
    }

    // ---- サンプルレートが変わる出力先 ----

    /// 出力を `seconds` 秒ぶん作る。
    fn render_seconds(rig: &Rig, seconds: f32, sample_rate: u32) -> Vec<f32> {
        rig.render((seconds * sample_rate as f32) as usize)
    }

    /// 上向きのゼロ交差の時刻（秒）。
    fn upward_zero_crossings(samples: &[f32], sample_rate: u32) -> Vec<f64> {
        samples
            .windows(2)
            .enumerate()
            .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
            .map(|(i, w)| (i as f64 + (-w[0] / (w[1] - w[0])) as f64) / sample_rate as f64)
            .collect()
    }

    /// 音の長さ（秒）。最後に 1% を超えた時刻。
    fn audible_length_seconds(samples: &[f32], sample_rate: u32, peak: f32) -> f64 {
        let last = samples.iter().rposition(|s| s.abs() > peak * 0.01).unwrap_or(0);
        (last + 1) as f64 / sample_rate as f64
    }

    #[test]
    fn rate_change_resynthesizes_the_kit_at_the_new_rate() {
        let world = new_world(Ok(info("内蔵スピーカー", 44_100)));
        let mut rig = Rig::start(world.clone());
        assert_eq!(*rig.synthesized_rates.borrow(), vec![44_100]);

        // 48kHz の機器に替わる。
        world.borrow_mut().default_output = Ok(info("イヤホン", 48_000));
        rig.callback_once();
        rig.advance(WATCH_INTERVAL);
        rig.advance(RETRY_FIRST_INTERVAL);

        assert_eq!(*rig.synthesized_rates.borrow(), vec![44_100, 48_000]);
        assert_eq!(rig.status(), OutputStatus::Ok { sample_rate: 48_000 });
    }

    /// もう一方のレートの機器で起動してから、`sample_rate` の機器に切り替わった状態の見張り。
    /// 実際の切り替え（レートが変わる出口への作り直し）を通した出口で確かめるために使う。
    fn rig_switched_to(sample_rate: u32) -> Rig {
        let other = if sample_rate == 48_000 { 44_100 } else { 48_000 };
        let world = new_world(Ok(info("機器A", other)));
        let mut rig = Rig::start(world.clone());
        rig.callback_once();
        world.borrow_mut().default_output = Ok(info("機器B", sample_rate));
        rig.advance(WATCH_INTERVAL);
        rig.advance(RETRY_FIRST_INTERVAL);
        assert_eq!(world.borrow().opened.last().unwrap().sample_rate, sample_rate, "切り替わっていない");
        // 作り直した出口の最初のコールバックは、溜まっていた要求を捨てる。それが済んでから打つ。
        rig.render(1);
        rig
    }

    /// `sample_rate` の機器に切り替わったあとの出口でキックを鳴らして、`seconds` 秒ぶんを返す。
    fn play_kick_after_switch_to(sample_rate: u32, seconds: f32) -> Vec<f32> {
        let rig = rig_switched_to(sample_rate);
        assert!(rig.supervisor.engine().play("kick", 0, 1.0));
        render_seconds(&rig, seconds, sample_rate)
    }

    #[test]
    fn kick_has_the_same_pitch_and_length_at_44100_and_48000() {
        let seconds = 0.6;
        let at_44k = play_kick_after_switch_to(44_100, seconds);
        let at_48k = play_kick_after_switch_to(48_000, seconds);

        // 長さ: 音が鳴っている時間（秒）が同じ。
        let length_44k = audible_length_seconds(&at_44k, 44_100, 0.7);
        let length_48k = audible_length_seconds(&at_48k, 48_000, 0.7);
        assert!(length_44k > 0.1, "鳴っていない");
        assert!(
            (length_44k - length_48k).abs() < 0.005,
            "長さが違う: 44.1kHz={length_44k}s 48kHz={length_48k}s"
        );

        // 音の高さ: 周波数が下がっていく途中の山の時刻（秒）が、同じ番目ごとにそろう。
        let crossings_44k = upward_zero_crossings(&at_44k, 44_100);
        let crossings_48k = upward_zero_crossings(&at_48k, 48_000);
        let compared = crossings_44k.len().min(crossings_48k.len()).min(40);
        assert!(compared >= 30, "比べられる山が少なすぎる: {compared}");
        for (i, (a, b)) in crossings_44k.iter().zip(&crossings_48k).take(compared).enumerate() {
            assert!((a - b).abs() < 0.0005, "{i} 番目の山の時刻が違う: 44.1kHz={a}s 48kHz={b}s");
        }
        // 周波数の絶対値も確かめる（2つの出力が同じだけずれていても落ちるように）。
        // 音が落ち着いたあと（0.15秒以降）の周期は、合成の定義（終端 45Hz）から 22ms 前後。
        let settled: Vec<f64> = crossings_48k.iter().copied().filter(|t| *t > 0.15).collect();
        assert!(settled.len() >= 4, "落ち着いたあとの山が少なすぎる");
        let mean_period = (settled[settled.len() - 1] - settled[0]) / (settled.len() - 1) as f64;
        assert!((0.0205..0.0235).contains(&mean_period), "落ち着いたあとの周期がおかしい: {mean_period}s");
    }

    #[test]
    fn every_sound_has_the_same_length_at_44100_and_48000() {
        let names = ["kick", "snare", "hat_closed", "hat_open", "clap", "rim", "tom_low", "tom_high"];
        for name in names {
            let render = |rate: u32| {
                let rig = rig_switched_to(rate);
                assert!(rig.supervisor.engine().play(name, 0, 1.0));
                let out = render_seconds(&rig, 2.0, rate);
                let peak = out.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
                assert!(peak > 0.05, "{name} が鳴っていない");
                audible_length_seconds(&out, rate, peak)
            };
            let (a, b) = (render(44_100), render(48_000));
            assert!((a - b).abs() < 0.02, "{name} の長さが違う: 44.1kHz={a}s 48kHz={b}s");
        }
    }

    // ---- 作り直しの前に溜まった要求 ----

    #[test]
    fn requests_queued_while_the_output_was_down_are_not_played_in_a_burst() {
        let world = new_world(Ok(info("内蔵スピーカー", 48_000)));
        let mut rig = Rig::start(world.clone());
        let engine = rig.supervisor.engine();

        // 出口が死んでいる間に打たれた分。
        world.borrow().health.as_ref().unwrap().mark_failed();
        rig.advance(Duration::from_millis(1));
        for _ in 0..MAX_VOICES {
            engine.play("kick", 0, 1.0);
        }

        rig.advance(RETRY_FIRST_INTERVAL);
        assert!(rig.supervisor.is_running());
        let out = rig.render(2_000);
        assert!(out.iter().all(|s| *s == 0.0), "出口が戻った瞬間に、溜まっていた音が鳴った");
        assert_eq!(engine.active_voice_count(), 0);

        // 戻ったあとの打鍵は鳴る。
        assert!(engine.play("kick", 0, 1.0));
        assert!(rig.render(2_000).iter().any(|s| s.abs() > 0.1));
    }
}
