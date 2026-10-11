//! 画面表示用の状態管理。
//!
//! ここに集める情報はすべて「今どうなっているか」の状態のみで、
//! 押されたキーの内容や文字列など、入力内容そのものは一切保持しない。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::assignment::LiveAssignments;
use crate::audio::AudioEngine;
use crate::permission;

/// 音声デバイス（音の出口）の状態。起動時の結果だけでなく、出口の作り直しの成否も表す
/// （`audio.rs` の `OutputStatus`。成功ならいま使っているサンプルレート、失敗なら原因の要約文言）。
pub use crate::audio::OutputStatus as AudioInitStatus;

/// アプリ全体で共有する状態。
pub struct AppState {
    /// 起動時に一度だけ判定する、Windows/macOS 等のプラットフォーム種別。
    pub platform: &'static str,
    /// 音声エンジンが無いときの音声デバイスの状態。エンジンがあるときは、エンジンが持つ
    /// 最新の状態（出口の作り直しの結果）を読む（[`AppState::audio_status`]）。
    audio_init: AudioInitStatus,
    /// 音声合成・再生エンジン。出力先が使えなくても作る（出口は見張りが作り直す）。
    /// キー入力スレッドと状態表示コマンドの双方から参照するため `Arc` で共有する。
    pub audio_engine: Option<Arc<AudioEngine>>,
    /// `DRUMCLACK_TEST_DELAY_MS` の値（テスト用の遅延発音、測定の陽性対照）。
    pub test_delay_ms: Option<u64>,
    /// キー監視が読む、今の割り当てとオン／オフ。設定の変更で差し替わる（`SettingsStore` が書く）。
    pub assignments: Arc<LiveAssignments>,
    /// キー監視に、OS からキーのイベントが1回でも届いたか（届いた事実だけ。どのキーかは持たない）。
    /// 入力監視の許可は起動時の判定が起動後の変化を反映しないため、実際に届いたことで補う。
    key_events_seen: AtomicBool,
}

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
            test_delay_ms,
            assignments: Arc::new(LiveAssignments::new()),
            key_events_seen: AtomicBool::new(false),
        }
    }

    /// キー監視が OS からイベントを受け取ったと記録する（キーの内容は渡さない）。
    pub fn note_key_event_received(&self) {
        // 毎打鍵で呼ばれるので、すでに真なら書き込まない。
        if !self.key_events_seen.load(Ordering::Relaxed) {
            self.key_events_seen.store(true, Ordering::Relaxed);
        }
    }

    /// キー監視にイベントが届いたことがあるか。起動後ずっと真のままなので、許可が後から取り消されても
    /// 偽に戻らない（取り消しは検知できない既知の制約。次に起動し直すと OS の判定に戻る）。
    pub fn key_events_seen(&self) -> bool {
        self.key_events_seen.load(Ordering::Relaxed)
    }

    /// 入力監視の許可が足りているか。Mac は許可が下りていなければ偽。Windows には許可の概念が無いので常に真。
    pub fn input_permission_ok(&self) -> bool {
        self.platform != "macos" || permission::check_input_monitoring() == "granted"
    }

    /// 音声デバイスの今の状態。出口を失って作り直している間は失敗、直れば自動で成功に戻る。
    pub fn audio_status(&self) -> AudioInitStatus {
        match &self.audio_engine {
            Some(engine) => engine.output_status(),
            None => self.audio_init.clone(),
        }
    }

    /// 音声デバイスが使えているか（起動時の初期化に成功し、いまも出口が生きている）。
    pub fn audio_ok(&self) -> bool {
        matches!(self.audio_status(), AudioInitStatus::Ok { .. })
    }

    /// 直近に実際に鳴り始めた時刻（UNIX epoch ミリ秒）。まだ一度も鳴っていなければ None。
    /// 16音の上限で鳴らなかった打鍵は含まない（音声コールバックが受理した発音だけ）。
    pub fn last_play_ms(&self) -> Option<u64> {
        self.audio_engine.as_ref().and_then(|e| e.last_accepted_play_ms())
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
    /// キー監視にイベントが届いたか。真なら、権限の判定が未許可でも実際には許可されている。
    pub key_events_seen: bool,
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

    let audio = match &state.audio_status() {
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
        key_events_seen: state.key_events_seen(),
    }
}
