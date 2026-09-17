//! 画面表示用の状態管理。
//!
//! ここに集める情報はすべて「今どうなっているか」の状態のみで、
//! 押されたキーの内容や文字列など、入力内容そのものは一切保持しない。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::audio::AudioEngine;
use crate::permission;

/// 音声デバイス初期化の結果。
#[derive(Debug, Clone)]
pub enum AudioInitStatus {
    /// 初期化成功。サンプルレートとバッファ設定を保持し、画面表示にも使う。
    Ok { sample_rate: u32 },
    /// 初期化失敗。原因の要約文言を保持する。
    Err(String),
}

/// アプリ全体で共有する状態。
pub struct AppState {
    /// 起動時に一度だけ判定する、Windows/macOS 等のプラットフォーム種別。
    pub platform: &'static str,
    /// 音声デバイス初期化の成否。起動後に一度だけセットされ、以後は不変。
    pub audio_init: AudioInitStatus,
    /// 音声合成・再生エンジン（初期化に成功した場合のみ Some）。
    /// キー入力スレッドと状態表示コマンドの双方から参照するため `Arc` で共有する。
    pub audio_engine: Option<Arc<AudioEngine>>,
    /// 直近に発音した時刻（UNIX epoch ミリ秒）。まだ一度も鳴っていなければ None。
    last_play_ms: AtomicU64,
    /// `DRUMCLACK_TEST_DELAY_MS` の値（テスト用の遅延発音、測定の陽性対照）。
    pub test_delay_ms: Option<u64>,
}

/// `last_play_ms` の「未発音」を表す番兵値。
const NOT_PLAYED_YET: u64 = 0;

impl AppState {
    pub fn new(audio_init: AudioInitStatus, audio_engine: Option<Arc<AudioEngine>>) -> Self {
        let platform = if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "other"
        };

        let test_delay_ms = std::env::var("DRUMCLACK_TEST_DELAY_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok());

        Self {
            platform,
            audio_init,
            audio_engine,
            last_play_ms: AtomicU64::new(NOT_PLAYED_YET),
            test_delay_ms,
        }
    }

    /// 発音した瞬間に呼ぶ。押されたキーの種類は受け取らず、時刻のみ記録する。
    pub fn record_play_now(&self) {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(NOT_PLAYED_YET)
            // 0 は「未発音」の番兵値と衝突しうるが、1970年時点なので実運用上は無視できる。
            .max(1);
        self.last_play_ms.store(ms, Ordering::Relaxed);
    }

    pub fn last_play_ms(&self) -> Option<u64> {
        match self.last_play_ms.load(Ordering::Relaxed) {
            NOT_PLAYED_YET => None,
            ms => Some(ms),
        }
    }
}

/// フロントエンドへ返す状態のスナップショット。
#[derive(Debug, Clone, serde::Serialize)]
pub struct StatusSnapshot {
    pub running: bool,
    pub platform: &'static str,
    pub permission: Option<PermissionSnapshot>,
    pub audio: AudioSnapshot,
    pub last_play_ms: Option<u64>,
    pub active_voices: usize,
    pub max_voices: usize,
    pub test_delay_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PermissionSnapshot {
    /// "granted" | "denied" | "unknown"
    pub state: &'static str,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AudioSnapshot {
    pub ok: bool,
    pub message: String,
}

/// 現在の状態一式を集めてスナップショットを作る（Tauri コマンドから呼ばれる）。
pub fn build_snapshot(state: &AppState) -> StatusSnapshot {
    let permission = if state.platform == "macos" {
        Some(PermissionSnapshot { state: permission::check_input_monitoring() })
    } else {
        // Windows / その他: 権限の概念自体が無いので項目自体を出さない。
        None
    };

    let audio = match &state.audio_init {
        AudioInitStatus::Ok { sample_rate } => AudioSnapshot {
            ok: true,
            message: format!("初期化済み（サンプルレート {sample_rate} Hz）"),
        },
        AudioInitStatus::Err(reason) => AudioSnapshot { ok: false, message: reason.clone() },
    };

    let active_voices = state.audio_engine.as_ref().map(|e| e.active_voice_count()).unwrap_or(0);

    StatusSnapshot {
        running: true,
        platform: state.platform,
        permission,
        audio,
        last_play_ms: state.last_play_ms(),
        active_voices,
        max_voices: crate::voices::MAX_VOICES,
        test_delay_ms: state.test_delay_ms,
    }
}
