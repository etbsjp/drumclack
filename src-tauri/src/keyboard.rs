//! OS全体からのキー入力を「受信専用」で監視する。
//!
//! 採用方式と理由は README の「実装メモ」節を参照。要点だけ書くと:
//!
//! - macOS: `CGEventTap` を **listen-only**（`kCGEventTapOptionListenOnly`）モードで
//!   直接 FFI で張る（[`macos_tap`] モジュール）。これは他アプリへのイベント伝播を
//!   止めない“傍受のみ”のタップで、必要な権限も「アクセシビリティ」ではなく
//!   「入力監視（Input Monitoring）」のみで済む。
//!   `rdev` は使わない。`rdev` はキーイベントごとに `TSMGetInputSourceProperty` 等の
//!   HIToolbox API を内部で呼び、macOS 15 以降ではメインスレッド以外からの呼び出しで
//!   `dispatch_assert_queue_fail` により停止する事例が報告されたため（README参照）。
//!   読むのはキーコード（物理位置）とリピートの印・修飾フラグだけで、文字や入力ソースを
//!   引く API は一切呼ばない。
//! - Windows: 低レベルキーボードフック（`WH_KEYBOARD_LL`）を直接呼ぶ
//!   （[`windows_hook`] モジュール）。読むのは走査コードと拡張キーの印・離しの印だけ
//!   （仮想キーコードは配列・NumLock に依存するので読まない）。マウスのフックは張らず、
//!   イベントも握りつぶさない。
//!
//! キーの位置は音を選ぶことにだけ使い、保存も記録もしない。位置を表す型
//! （[`KeyPosition`]）は表示・ログ出力・保存の機能を持たず、[`sound_name_for`] の外へ出さない。

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::audio::AudioEngine;
use crate::key_position::KeyPosition;
use crate::key_tracker::{KeyEvent, KeyTracker};
use crate::state::AppState;

/// キー入力監視を開始する（プラットフォームごとの実装は下記の各モジュールへ委譲）。
pub fn spawn_listener(engine: Arc<AudioEngine>, state: Arc<AppState>) {
    #[cfg(target_os = "macos")]
    macos_tap::spawn_listener(engine, state);

    #[cfg(target_os = "windows")]
    windows_hook::spawn_listener(engine, state);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (engine, state);
        eprintln!("[drumclack] この OS ではキー入力の監視に対応していません");
    }
}

/// キーの位置を音の名前に変える、唯一の場所。キーの位置はここで使い切る。
///
/// この issue ではどのキーでもキックのまま。割り当て（グループ・個別の上書き）は後続の issue で入れる。
fn sound_name_for(_position: KeyPosition) -> &'static str {
    "kick"
}

/// 監視から届いたイベントを状態機械に通し、新しく押されたときだけ発音を試みる。
fn dispatch(
    tracker: &mut KeyTracker,
    event: KeyEvent,
    engine: &Arc<AudioEngine>,
    state: &Arc<AppState>,
) {
    if let Some(position) = tracker.handle(event) {
        handle_key_down(engine.clone(), state.clone(), sound_name_for(position));
    }
}

/// キー押下を受けて発音を試みる、プラットフォーム共通の処理。
///
/// `DRUMCLACK_TEST_DELAY_MS` が設定されていれば、その分だけ遅延させてから
/// 発音する（測定用の陽性対照）。呼び出し元のイベントループ／コールバックの
/// スレッドを長時間ブロックしないよう、遅延がある場合は別スレッドへ逃がす。
fn handle_key_down(engine: Arc<AudioEngine>, state: Arc<AppState>, sound_name: &'static str) {
    match state.test_delay_ms {
        Some(ms) => {
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(ms));
                trigger(&engine, sound_name);
            });
        }
        None => trigger(&engine, sound_name),
    }
}

fn trigger(engine: &AudioEngine, sound_name: &str) {
    // 直近の発音時刻は、音声コールバックが実際に鳴らした時点でエンジン側が記録する
    // （16音の上限で鳴らなかった打鍵は記録されない）。
    engine.play_varied(sound_name, 1.0);
}

/// macOS: `CGEventTap` への直接 FFI によるキー監視。
///
/// ここで使う CoreGraphics / CoreFoundation の API は、タップの配管と、イベントから
/// キーコード・リピートの印・修飾フラグという数値を読むものだけ。文字列や入力ソースを
/// 引く API（`CGEventKeyboardGetUnicodeString`、TIS 系など）は呼ばない。
#[cfg(target_os = "macos")]
mod macos_tap {
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::sync::Arc;

    use super::dispatch;
    use crate::audio::AudioEngine;
    use crate::key_position::KeyPosition;
    use crate::key_tables::{mac_keycode_to_position, mac_modifier_change, ModifierAction};
    use crate::key_tracker::{KeyEvent, KeyTracker};
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
    type CgEventField = u32;
    type CgEventFlags = u64;

    /// セッション内（現在ログイン中のユーザーセッション）のイベントを対象にする。
    /// `kCGHIDEventTap`（より低レベル）は使わない。
    const K_CG_SESSION_EVENT_TAP: CgEventTapLocation = 1;
    const K_CG_HEAD_INSERT_EVENT_TAP: CgEventTapPlacement = 0;
    /// 傍受のみ（他アプリへのイベント伝播を止めない）。要件どおり。
    const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: CgEventTapOptions = 1;
    const K_CG_EVENT_KEY_DOWN: CgEventType = 10;
    const K_CG_EVENT_KEY_UP: CgEventType = 11;
    /// 修飾キー（Shift・Cmd 等）の変化は、通常の押下とは別のイベントで届く。
    const K_CG_EVENT_FLAGS_CHANGED: CgEventType = 12;
    /// タイムアウトやユーザー操作でタップが無効化されたことを示す特殊イベント種別。
    const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: CgEventType = 0xFFFF_FFFE;
    const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: CgEventType = 0xFFFF_FFFF;
    /// OS が付けるキーリピートの印（0 以外ならリピート）。
    const K_CG_KEYBOARD_EVENT_AUTOREPEAT: CgEventField = 8;
    /// 物理キーの仮想キーコード（配列・入力ソースに依存しない）。
    const K_CG_KEYBOARD_EVENT_KEYCODE: CgEventField = 9;

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
        fn CGEventGetIntegerValueField(event: CgEventRef, field: CgEventField) -> i64;
        fn CGEventGetFlags(event: CgEventRef) -> CgEventFlags;
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

    /// コールバックへ渡す共有コンテキスト。押下中のキーの集合（`tracker`）は
    /// このスレッドの中だけで使い、外へ出さない。
    /// `tap` はタップ作成後に埋める（無効化通知を受けた際の再有効化に使う）。
    struct TapContext {
        engine: Arc<AudioEngine>,
        state: Arc<AppState>,
        tracker: RefCell<KeyTracker>,
        tap: Cell<CFMachPortRef>,
    }

    /// CGEventTap のコールバック本体。
    ///
    /// イベント種別に応じて、キーコード・リピートの印・修飾フラグだけを読み、
    /// 状態機械へ渡す。listen-only タップでは戻り値は OS 側で無視されるが、
    /// 慣例に従い受け取った `event` をそのまま返す。
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
        // 単一スレッドからのアクセスに限られる（Cell / RefCell で十分）。
        let ctx = unsafe { &*(user_info as *const TapContext) };

        if event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
            || event_type == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
        {
            // 止められている間のキーの離しは届かない。押下中の集合を空にしないと、
            // その間に離したキーが「押されたまま」扱いになり、二度と鳴らなくなる。
            ctx.tracker.borrow_mut().handle(KeyEvent::MonitoringInterrupted);
            // 放置すると以降キーを検知できなくなるため、直ちに再有効化する。
            let tap = ctx.tap.get();
            if !tap.is_null() {
                unsafe { CGEventTapEnable(tap, true) };
            }
            return event;
        }

        match event_type {
            K_CG_EVENT_KEY_DOWN | K_CG_EVENT_KEY_UP => {
                let Some(position) = read_position(event) else {
                    return event;
                };
                let key_event = if event_type == K_CG_EVENT_KEY_DOWN {
                    // SAFETY: event はコールバック中のみ有効な有効なイベント。
                    let autorepeat =
                        unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_AUTOREPEAT) } != 0;
                    KeyEvent::Down { position, autorepeat }
                } else {
                    KeyEvent::Up { position }
                };
                dispatch_event(ctx, key_event);
            }
            K_CG_EVENT_FLAGS_CHANGED => {
                // SAFETY: 同上。
                let keycode = unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) };
                let flags = unsafe { CGEventGetFlags(event) };
                let change = u16::try_from(keycode)
                    .ok()
                    .and_then(|keycode| mac_modifier_change(keycode, flags));
                if let Some((position, action)) = change {
                    modifier_events(position, action, |e| dispatch_event(ctx, e));
                }
            }
            _ => {}
        }
        event
    }

    fn read_position(event: CgEventRef) -> Option<KeyPosition> {
        // SAFETY: event はコールバック中のみ有効な有効なイベント。
        let keycode = unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) };
        mac_keycode_to_position(u16::try_from(keycode).ok()?)
    }

    fn dispatch_event(ctx: &TapContext, event: KeyEvent) {
        dispatch(&mut ctx.tracker.borrow_mut(), event, &ctx.engine, &ctx.state);
    }

    /// 修飾キーの変化を、通常キーと同じ押下・離しのイベントへ直す。
    fn modifier_events(position: KeyPosition, action: ModifierAction, mut emit: impl FnMut(KeyEvent)) {
        match action {
            ModifierAction::Down => emit(KeyEvent::Down { position, autorepeat: false }),
            ModifierAction::Up => emit(KeyEvent::Up { position }),
            // CapsLock は離しが届かないので、押して即離した扱いにして集合に残さない。
            ModifierAction::Pressed => {
                emit(KeyEvent::Down { position, autorepeat: false });
                emit(KeyEvent::Up { position });
            }
        }
    }

    pub fn spawn_listener(engine: Arc<AudioEngine>, state: Arc<AppState>) {
        std::thread::spawn(move || {
            let ctx = Box::new(TapContext {
                engine,
                state,
                tracker: RefCell::new(KeyTracker::new()),
                tap: Cell::new(std::ptr::null_mut()),
            });
            let ctx_ptr = Box::into_raw(ctx);

            let event_mask: CgEventMask = (1u64 << K_CG_EVENT_KEY_DOWN)
                | (1u64 << K_CG_EVENT_KEY_UP)
                | (1u64 << K_CG_EVENT_FLAGS_CHANGED);

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

/// Windows: 低レベルキーボードフック（`WH_KEYBOARD_LL`）の直接呼び出し。
///
/// `rdev` を使わない理由は、打鍵のたびに内部で文字を作ること、マウスのフックも同時に張ること、
/// 取れるキーが配列依存でテンキーの Enter を区別できないこと。ここでは `windows-sys` 等も足さず、
/// macOS 側と同じく必要な API だけを素の FFI で宣言する。
/// フックはキーボードだけで、イベントは常に次のフックへ渡す（握りつぶさない）。
#[cfg(target_os = "windows")]
mod windows_hook {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::sync::Arc;

    use super::dispatch;
    use crate::audio::AudioEngine;
    use crate::key_tables::windows_scancode_to_position;
    use crate::key_tracker::{KeyEvent, KeyTracker};
    use crate::state::AppState;

    const WH_KEYBOARD_LL: i32 = 13;
    const HC_ACTION: i32 = 0;
    /// 拡張キー（テンキー Enter・右 Ctrl・本体の矢印など）の印。
    const LLKHF_EXTENDED: u32 = 0x01;
    /// 離したときに立つ印。
    const LLKHF_UP: u32 = 0x80;

    /// `KBDLLHOOKSTRUCT`。仮想キーコードは配列・NumLock に依存するため読まない
    /// （配置を合わせるためだけに持つ）。
    #[repr(C)]
    struct KbdLlHookStruct {
        _vk_code: u32,
        scan_code: u32,
        flags: u32,
        _time: u32,
        _extra_info: usize,
    }

    #[repr(C)]
    struct Msg {
        hwnd: *mut c_void,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        pt_x: i32,
        pt_y: i32,
    }

    type HookProc = unsafe extern "system" fn(code: i32, w_param: usize, l_param: isize) -> isize;

    #[link(name = "user32")]
    extern "system" {
        fn SetWindowsHookExW(
            id_hook: i32,
            proc: HookProc,
            module: *mut c_void,
            thread_id: u32,
        ) -> *mut c_void;
        fn CallNextHookEx(hook: *mut c_void, code: i32, w_param: usize, l_param: isize) -> isize;
        fn GetMessageW(msg: *mut Msg, hwnd: *mut c_void, filter_min: u32, filter_max: u32) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(module_name: *const u16) -> *mut c_void;
    }

    struct HookContext {
        engine: Arc<AudioEngine>,
        state: Arc<AppState>,
        tracker: KeyTracker,
    }

    thread_local! {
        // フックのコールバックは、フックを張ったこのスレッドでしか呼ばれない。
        static HOOK_CONTEXT: RefCell<Option<HookContext>> = const { RefCell::new(None) };
    }

    unsafe extern "system" fn hook_proc(code: i32, w_param: usize, l_param: isize) -> isize {
        if code == HC_ACTION {
            // SAFETY: HC_ACTION のとき l_param は有効な KBDLLHOOKSTRUCT を指す。
            let info = unsafe { &*(l_param as *const KbdLlHookStruct) };
            let position =
                windows_scancode_to_position(info.scan_code, info.flags & LLKHF_EXTENDED != 0);
            if let Some(position) = position {
                // Windows のフックにはリピートの印が無い。押下中の集合で判定する。
                let key_event = if info.flags & LLKHF_UP != 0 {
                    KeyEvent::Up { position }
                } else {
                    KeyEvent::Down { position, autorepeat: false }
                };
                HOOK_CONTEXT.with(|cell| {
                    if let Some(ctx) = cell.borrow_mut().as_mut() {
                        dispatch(&mut ctx.tracker, key_event, &ctx.engine, &ctx.state);
                    }
                });
            }
        }
        // イベントは握りつぶさず、必ず次のフックへ渡す。
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param) }
    }

    pub fn spawn_listener(engine: Arc<AudioEngine>, state: Arc<AppState>) {
        std::thread::spawn(move || {
            HOOK_CONTEXT.with(|cell| {
                *cell.borrow_mut() = Some(HookContext { engine, state, tracker: KeyTracker::new() });
            });

            // SAFETY: hook_proc はプロセス生存中有効な関数。モジュールハンドルは自プロセスのもの。
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    hook_proc,
                    GetModuleHandleW(std::ptr::null()),
                    0,
                )
            };
            if hook.is_null() {
                eprintln!("[drumclack] キー入力の監視を開始できませんでした");
                return;
            }

            // 低レベルフックは、フックを張ったスレッドでメッセージを取り出し続けている間だけ呼ばれる。
            // SAFETY: msg は書き込み先として有効な領域。
            let mut msg: Msg = unsafe { std::mem::zeroed() };
            while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {}
        });
    }
}
