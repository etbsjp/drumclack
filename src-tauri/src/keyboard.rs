//! OS全体からのキー入力を「受信専用」で監視する。
//!
//! 採用方式と理由は README の「実装メモ」節を参照。要点だけ書くと:
//!
//! - macOS: `CGEventTap` を **listen-only**（`kCGEventTapOptionListenOnly`）モードで
//!   直接 FFI で張る（[`macos_tap`] モジュール）。これは他アプリへのイベント伝播を
//!   止めない“傍受のみ”のタップで、必要な権限も「アクセシビリティ」ではなく
//!   「入力監視（Input Monitoring）」のみで済む。
//!   以前は `rdev` クレートを使っていたが、`rdev` はキーイベントごとに
//!   `TSMGetInputSourceProperty` 等の HIToolbox API を内部で呼んでおり、
//!   macOS 15 以降ではメインスレッド以外からの呼び出しで `dispatch_assert_queue_fail`
//!   により停止する事例が報告されたため（README参照）、キーコード→文字列変換を
//!   一切行わない素の `CGEventTap` 直叩きへ切り替えた。
//! - Windows: 引き続き `rdev::listen` を使う。内部で `SetWindowsHookEx(WH_KEYBOARD_LL, ...)`
//!   による低レベルキーボードフックを使い、これも受信専用でイベントを握りつぶさない。
//!
//! いずれの方式でも、扱うのは「キーが押された」というタイミングのみ。
//! どのキーが押されたか（文字・キーコード）は一切保存・表示・ログ出力しない。

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::audio::AudioEngine;
use crate::state::AppState;

/// キー入力監視を開始する（プラットフォームごとの実装は下記の各モジュールへ委譲）。
pub fn spawn_listener(engine: Arc<AudioEngine>, state: Arc<AppState>) {
    #[cfg(target_os = "macos")]
    macos_tap::spawn_listener(engine, state);

    #[cfg(not(target_os = "macos"))]
    rdev_listener::spawn_listener(engine, state);
}

/// キー押下（タイミングのみ）を受けて発音を試みる、プラットフォーム共通の処理。
///
/// `DRUMCLACK_TEST_DELAY_MS` が設定されていれば、その分だけ遅延させてから
/// 発音する（測定用の陽性対照）。呼び出し元のイベントループ／コールバックの
/// スレッドを長時間ブロックしないよう、遅延がある場合は別スレッドへ逃がす。
fn handle_key_down(engine: Arc<AudioEngine>, state: Arc<AppState>) {
    match state.test_delay_ms {
        Some(ms) => {
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(ms));
                trigger(&engine, &state);
            });
        }
        None => trigger(&engine, &state),
    }
}

fn trigger(engine: &AudioEngine, state: &AppState) {
    // 上限（16音）に達している場合、trigger() は何もせず false を返す。
    // その場合も直近発音時刻は更新しない（実際には鳴っていないため）。
    if engine.trigger() {
        state.record_play_now();
    }
}

/// macOS: `CGEventTap` への直接 FFI によるキー監視。
///
/// `rdev` を経由しない理由は本モジュール冒頭のコメント、および README を参照。
/// ここで使う CoreGraphics / CoreFoundation の API はいずれも「タップの作成・
/// 有効化・ランループへの登録」という配管作業のみで、キーコードや文字列を
/// 一切読み取らない（コールバック内でイベント種別が `KeyDown` かどうかだけを見る）。
#[cfg(target_os = "macos")]
mod macos_tap {
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::sync::Arc;

    use super::handle_key_down;
    use crate::audio::AudioEngine;
    use crate::state::AppState;

    // 以降、CoreGraphics/CoreFoundation の型・定数は必要最小限のみを素の FFI で宣言する
    // （`core-graphics` 等の外部クレートは追加せず、`permission.rs` と同じ流儀で
    // フレームワークへ直接リンクする）。

    type CFAllocatorRef = *const c_void;
    type CFMachPortRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFRunLoopRef = *mut c_void;
    type CFStringRef = *const c_void;
    type CFIndex = isize;
    type CgEventRef = *mut c_void;
    type CgEventTapProxy = *mut c_void;
    type CgEventTapLocation = u32;
    type CgEventTapPlacement = u32;
    type CgEventTapOptions = u32;
    type CgEventType = u32;
    type CgEventMask = u64;

    /// セッション内（現在ログイン中のユーザーセッション）のイベントを対象にする。
    /// `kCGHIDEventTap`（より低レベル）は使わない。
    const K_CG_SESSION_EVENT_TAP: CgEventTapLocation = 1;
    const K_CG_HEAD_INSERT_EVENT_TAP: CgEventTapPlacement = 0;
    /// 傍受のみ（他アプリへのイベント伝播を止めない）。要件どおり。
    const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: CgEventTapOptions = 1;
    const K_CG_EVENT_KEY_DOWN: CgEventType = 10;
    /// タイムアウトやユーザー操作でタップが無効化されたことを示す特殊イベント種別。
    const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: CgEventType = 0xFFFF_FFFE;
    const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: CgEventType = 0xFFFF_FFFF;

    type CgEventTapCallBack = extern "C" fn(
        proxy: CgEventTapProxy,
        event_type: CgEventType,
        event: CgEventRef,
        user_info: *mut c_void,
    ) -> CgEventRef;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: CgEventTapLocation,
            place: CgEventTapPlacement,
            options: CgEventTapOptions,
            events_of_interest: CgEventMask,
            callback: CgEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;

        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFMachPortCreateRunLoopSource(
            allocator: CFAllocatorRef,
            port: CFMachPortRef,
            order: CFIndex,
        ) -> CFRunLoopSourceRef;
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopRun();

        static kCFRunLoopCommonModes: CFStringRef;
    }

    /// コールバックへ渡す最小限の共有コンテキスト。押されたキーの情報は含まない。
    /// `tap` はタップ作成後に埋める（無効化通知を受けた際の再有効化に使う）。
    struct TapContext {
        engine: Arc<AudioEngine>,
        state: Arc<AppState>,
        tap: Cell<CFMachPortRef>,
    }

    /// CGEventTap のコールバック本体。
    ///
    /// イベント種別が `KeyDown` かどうかだけを見て発音をトリガーする。
    /// イベントの中身（キーコード等）は一切読み取らない。listen-only タップでは
    /// 戻り値は OS 側で無視されるが、慣例に従い受け取った `event` をそのまま返す。
    extern "C" fn tap_callback(
        _proxy: CgEventTapProxy,
        event_type: CgEventType,
        event: CgEventRef,
        user_info: *mut c_void,
    ) -> CgEventRef {
        // SAFETY: user_info はこのコールバックを登録する直前に
        // `Box::into_raw` した `TapContext` へのポインタで、
        // 監視スレッドが生きている（= プロセスが動いている）間は有効。
        // コールバックは登録元スレッドのランループ上でのみ呼ばれるため、
        // 単一スレッドからのアクセスに限られる（Cell で十分）。
        let ctx = unsafe { &*(user_info as *const TapContext) };

        if event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
            || event_type == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
        {
            // 処理に時間がかかった等でOSがタップを自動的に無効化することがある。
            // 放置すると以降キーを検知できなくなるため、直ちに再有効化する。
            let tap = ctx.tap.get();
            if !tap.is_null() {
                unsafe { CGEventTapEnable(tap, true) };
            }
            return event;
        }

        if event_type != K_CG_EVENT_KEY_DOWN {
            return event;
        }

        handle_key_down(ctx.engine.clone(), ctx.state.clone());
        event
    }

    pub fn spawn_listener(engine: Arc<AudioEngine>, state: Arc<AppState>) {
        std::thread::spawn(move || {
            let ctx = Box::new(TapContext { engine, state, tap: Cell::new(std::ptr::null_mut()) });
            let ctx_ptr = Box::into_raw(ctx);

            let event_mask: CgEventMask = 1u64 << K_CG_EVENT_KEY_DOWN;

            // SAFETY: 渡す関数ポインタ・ユーザーデータのポインタはいずれも
            // このスレッドが生きている間有効であり、CFRunLoopRun() が
            // このスレッドを離れない（無限にイベントループを回す）ため、
            // タップ生存中に ctx_ptr が指す先が解放されることは無い。
            let tap = unsafe {
                CGEventTapCreate(
                    K_CG_SESSION_EVENT_TAP,
                    K_CG_HEAD_INSERT_EVENT_TAP,
                    K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
                    event_mask,
                    tap_callback,
                    ctx_ptr as *mut c_void,
                )
            };

            if tap.is_null() {
                eprintln!(
                    "[drumclack] CGEventTap の作成に失敗しました（入力監視の権限が未許可の可能性があります）"
                );
                // ctx_ptr は誰にも登録されなかったので、ここで責任を持って解放する。
                unsafe {
                    drop(Box::from_raw(ctx_ptr));
                }
                return;
            }

            // SAFETY: ctx_ptr はまだ他スレッドと共有されていない、このスレッド
            // だけが所有するポインタ。コールバックが呼ばれるのは後段の
            // CFRunLoopRun() 開始後（＝このタイミングでの書き込みは安全に先行する）。
            unsafe {
                (*ctx_ptr).tap.set(tap);
            }

            unsafe {
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                let run_loop = CFRunLoopGetCurrent();
                CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, true);
                // このスレッドのランループを回し続ける。CGEventTap のコールバックは
                // このランループ上で呼ばれるため、戻ってくることはない
                // （＝以降のコードは実行されない。ctx_ptr はプロセス終了までリークし
                // 続けるが、アプリの生存期間＝プロセス生存期間なので実害は無い）。
                CFRunLoopRun();
            }
        });
    }
}

/// Windows: `rdev::listen`（内部で `SetWindowsHookEx(WH_KEYBOARD_LL, ...)`）による
/// 低レベルキーボードフック。macOSのような HIToolbox 由来の制約は無いため、
/// 従来どおり `rdev` を利用する。
#[cfg(not(target_os = "macos"))]
mod rdev_listener {
    use std::sync::Arc;
    use std::thread;

    use rdev::EventType;

    use super::handle_key_down;
    use crate::audio::AudioEngine;
    use crate::state::AppState;

    /// `rdev::listen` はブロッキング呼び出し（内部でOSのイベントループを回す）
    /// のため、専用スレッドを立てて実行する。権限が無い環境では `listen` 自体が
    /// エラーを返すか、あるいはイベントが一切届かない状態になるが、
    /// いずれの場合もアプリ本体はクラッシュしない。
    pub fn spawn_listener(engine: Arc<AudioEngine>, state: Arc<AppState>) {
        thread::spawn(move || {
            let callback = move |event: rdev::Event| {
                // 押されたキーの種類（Key の中身）には一切触れない。
                // イベントの種別が KeyPress かどうかだけを見て、押下タイミングとして扱う。
                if !matches!(event.event_type, EventType::KeyPress(_)) {
                    return;
                }
                handle_key_down(engine.clone(), state.clone());
            };

            if let Err(err) = rdev::listen(callback) {
                eprintln!(
                    "[drumclack] キー入力の監視を開始できませんでした（入力監視の権限が未許可の可能性があります）: {err:?}"
                );
            }
        });
    }
}
