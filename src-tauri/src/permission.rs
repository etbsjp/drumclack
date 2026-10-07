//! macOS の「入力監視（Input Monitoring）」権限の確認。
//!
//! ここでは **確認のみ** を行い、システム設定の変更や権限の強制付与は一切行わない。
//! 権限が無い場合、キー入力の監視（`CGEventTap`）はイベントを受け取れない（＝キーを打っても
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

/// 入力監視の設定画面を開く（macOS）。シェルを通さず、`open` に URL を引数として渡す。
/// 設定を変えるのは利用者で、このアプリは画面を開くだけ。
#[cfg(target_os = "macos")]
pub fn open_input_monitoring_settings() -> Result<(), String> {
    let status = std::process::Command::new("open")
        .arg(INPUT_MONITORING_SETTINGS_URL)
        .status()
        .map_err(|error| format!("open を起動できませんでした: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("open が失敗しました: {status}"))
    }
}

/// macOS 以外には入力監視の設定が無い。
#[cfg(not(target_os = "macos"))]
pub fn open_input_monitoring_settings() -> Result<(), String> {
    Err("この OS には入力監視の設定がありません".to_string())
}

/// システム設定の「プライバシーとセキュリティ > 入力監視」を直接開く URL。
#[cfg(target_os = "macos")]
const INPUT_MONITORING_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
