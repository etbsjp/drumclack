//! macOS の「入力監視（Input Monitoring）」権限の確認。
//!
//! ここでは **確認のみ** を行い、システム設定の変更や権限の強制付与は一切行わない。
//! 権限が無い場合、rdev の `listen()` はイベントを受け取れない（＝キーを打っても
//! 音が鳴らない）状態になるが、アプリ自体はクラッシュせず、画面に次の行動
//! （システム設定でどこを開けばよいか）を案内する。

#[cfg(target_os = "macos")]
mod macos {
    // IOKit (IOHIDLib.h) の IOHIDCheckAccess は macOS 10.15+ で公開されている
    // 権限確認 API。プロンプトは出さず、現在の許可状態だけを読み取れる。
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDCheckAccess(request_type: u32) -> u32;
    }

    // IOHIDRequestType のうち、キー入力等の「受信（listen）」を表す値。
    const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;

    // IOHIDAccessType の値。
    const K_IOHID_ACCESS_TYPE_GRANTED: u32 = 0;
    const K_IOHID_ACCESS_TYPE_DENIED: u32 = 1;
    // それ以外（2: Unknown 等）はすべて「不明」として扱う。

    pub fn check() -> &'static str {
        // SAFETY: IOHIDCheckAccess は引数を検証するだけの読み取り専用 API で、
        // 副作用（システム設定の変更やダイアログ表示）を持たない。
        let result = unsafe { IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) };
        match result {
            K_IOHID_ACCESS_TYPE_GRANTED => "granted",
            K_IOHID_ACCESS_TYPE_DENIED => "denied",
            _ => "unknown",
        }
    }
}

/// 入力監視権限の状態を返す（`"granted"` | `"denied"` | `"unknown"`）。
/// macOS 以外では呼び出さない想定（[`crate::state::build_snapshot`] 側で分岐済み）。
#[cfg(target_os = "macos")]
pub fn check_input_monitoring() -> &'static str {
    macos::check()
}

#[cfg(not(target_os = "macos"))]
pub fn check_input_monitoring() -> &'static str {
    "unknown"
}
