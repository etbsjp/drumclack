//! 設定ファイルの読み書きと、画面へ公開する設定の取得・更新。
//!
//! 保存先は OS 標準の設定フォルダ（呼び出し側が決めて渡す）。読み書きは Rust 側が正本で、
//! 画面から設定に触れるのは `get_settings` と `update_settings`（差分の更新）だけ（`main.rs`）。

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use serde_json::Value;

use crate::assignment::LiveAssignments;
use crate::audio::AudioEngine;
use crate::settings::{Settings, SETTINGS_VERSION};

pub const SETTINGS_FILE_NAME: &str = "settings.json";
pub const BROKEN_FILE_NAME: &str = "settings.broken.json";
const TEMP_FILE_NAME: &str = "settings.json.tmp";

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
/// 新しい版のファイルを残す名前の候補の数（`settings.v2.json`、`settings.v2.1.json` …）。
const FUTURE_COPY_CANDIDATES: u32 = 10;
const BROKEN_PREVIOUS_FILE_NAME: &str = "settings.broken.json.1";

/// 設定ファイルを読んだ結果。
#[derive(Debug, PartialEq)]
pub struct LoadOutcome {
    pub settings: Settings,
    /// 壊れたファイルを `settings.broken.json` へ退避して、既定で起動したか（退避が成功したときだけ true）。
    pub recovered_from_broken: bool,
    /// 元のファイルを残せていないため、保存してはいけない状態か（読み取りの IO エラー、退避の失敗）。
    pub save_blocked: bool,
    /// このアプリより新しい版のファイルを読んだときの、その版。初回の保存前に元ファイルを残す。
    pub future_version: Option<u32>,
}

impl LoadOutcome {
    fn defaults() -> Self {
        Self { settings: Settings::default(), recovered_from_broken: false, save_blocked: false, future_version: None }
    }
}

/// 起動時に設定を読む。ファイルが無ければ既定。壊れていれば退避して既定。
/// 壊れているのではなく読めないだけ（権限など）のときは、退避せず既定で動き、保存を止める。
pub fn load(dir: &Path) -> LoadOutcome {
    let path = dir.join(SETTINGS_FILE_NAME);

    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return LoadOutcome::defaults(),
        Err(_) => return LoadOutcome { save_blocked: true, ..LoadOutcome::defaults() },
    };

    // Windows のメモ帳などが付ける UTF-8 の BOM は、中身が正しければ壊れ扱いにしない。
    let body = bytes.strip_prefix(UTF8_BOM).unwrap_or(&bytes);
    let parsed = std::str::from_utf8(body).ok().and_then(|text| {
        let value: Value = serde_json::from_str(text).ok()?;
        let settings = Settings::from_value(&value)?;
        let version = value.get("version").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok());
        Some((settings, version))
    });
    match parsed {
        Some((settings, version)) => LoadOutcome {
            settings,
            future_version: version.filter(|version| *version > SETTINGS_VERSION),
            ..LoadOutcome::defaults()
        },
        None => recover_from_broken(dir),
    }
}

/// 壊れたファイルを退避して既定の設定を返す。退避先に前の退避があれば、先に `.1` へ寄せる（世代は1つ）。
/// 退避できなかったときは、元のファイルを残すため保存を止める。
fn recover_from_broken(dir: &Path) -> LoadOutcome {
    let path = dir.join(SETTINGS_FILE_NAME);
    let broken_path = dir.join(BROKEN_FILE_NAME);

    let set_aside = (|| {
        if broken_path.exists() {
            fs::rename(&broken_path, dir.join(BROKEN_PREVIOUS_FILE_NAME))?;
        }
        // 名前の付け替えができない環境では、コピーだけでも元の内容を残す。
        fs::rename(&path, &broken_path).or_else(|_| fs::copy(&path, &broken_path).map(|_| ()))
    })();

    let succeeded = set_aside.is_ok();
    LoadOutcome { recovered_from_broken: succeeded, save_blocked: !succeeded, ..LoadOutcome::defaults() }
}

/// 新しい版のファイルを、書き換える前に `settings.v{n}.json` へ残す。同名が既にあれば上書きせず、
/// `settings.v{n}.1.json` 以降の空いている名前へ残す。空きが無ければ失敗（呼び出し側は保存しない）。
fn keep_future_version(dir: &Path, version: u32) -> io::Result<()> {
    let mut source = File::open(dir.join(SETTINGS_FILE_NAME))?;
    for attempt in 0..FUTURE_COPY_CANDIDATES {
        let name = match attempt {
            0 => format!("settings.v{version}.json"),
            n => format!("settings.v{version}.{n}.json"),
        };
        // create_new: 既にあるファイルは決して書き換えない。
        match fs::OpenOptions::new().write(true).create_new(true).open(dir.join(name)) {
            Ok(mut destination) => {
                io::copy(&mut source, &mut destination)?;
                return destination.sync_all();
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free name to keep the newer-version file"))
}

/// 設定を保存する（別名で書いてから置き換える）。保存先のフォルダが無ければ作る。
pub fn save(dir: &Path, settings: &Settings) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    write_atomic_with(&dir.join(SETTINGS_FILE_NAME), settings.to_json_string().as_bytes(), |file, bytes| {
        file.write_all(bytes)
    })
}

/// 同じフォルダの別名ファイルへ書いてから、保存先へ置き換える。
/// 書き込みの途中で失敗しても、元のファイルは変わらない（別名のファイルは消す）。
/// `write_body` は書き込み本体で、テストが途中失敗を模すために差し替える。
fn write_atomic_with(
    path: &Path,
    bytes: &[u8],
    write_body: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let temp_path = path.with_file_name(TEMP_FILE_NAME);

    let written = (|| {
        let mut file = File::create(&temp_path)?;
        write_body(&mut file, bytes)?;
        // 置き換えた後に電源が落ちても、空のファイルが残らないようディスクへ出し切る。
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, path)
    })();

    if written.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    written
}

/// 画面へ返す設定の状態。
#[derive(Debug, Clone, Serialize)]
pub struct SettingsSnapshot {
    pub settings: Settings,
    /// 起動時に読めないファイルを退避したか（画面に1行出す）。退避先は `settings.broken.json`。
    pub recovered_from_broken: bool,
    /// 直近の保存に失敗したか。失敗しても、この起動中は更新した値で動き続ける。
    pub save_failed: bool,
}

struct Current {
    settings: Settings,
    save_failed: bool,
    /// 初回の保存前に元ファイルを残す、新しい版の番号。残せたら `None` に戻す。
    future_version_to_keep: Option<u32>,
}

/// 現在の設定を持ち、更新を保存と音量へ反映する。
pub struct SettingsStore {
    /// 保存先のフォルダ。設定フォルダが分からないときは `None`（保存せず、この起動中だけ動く）。
    dir: Option<PathBuf>,
    engine: Option<Arc<AudioEngine>>,
    /// キー監視が読む、今の割り当てとオン／オフ。設定が変わるたびに差し替える。
    assignments: Arc<LiveAssignments>,
    recovered_from_broken: bool,
    /// 元のファイルを残せていないため、この起動中は保存しない。
    save_blocked: bool,
    current: Mutex<Current>,
}

impl SettingsStore {
    /// 設定を読み込み、音量・割り当て・オン／オフを鳴らす側へ反映して起動する。
    /// キー監視が読む割り当ては、この store が持つもの（`open_with` で外から渡すこともできる）。
    #[cfg(test)]
    pub fn open(dir: Option<PathBuf>, engine: Option<Arc<AudioEngine>>) -> Self {
        Self::open_with(dir, engine, Arc::new(LiveAssignments::new()))
    }

    /// `open` と同じで、反映先の割り当て（キー監視が読むもの）を渡す。
    pub fn open_with(
        dir: Option<PathBuf>,
        engine: Option<Arc<AudioEngine>>,
        assignments: Arc<LiveAssignments>,
    ) -> Self {
        let outcome = match &dir {
            Some(dir) => load(dir),
            None => LoadOutcome::defaults(),
        };
        let store = Self {
            dir,
            engine,
            assignments,
            recovered_from_broken: outcome.recovered_from_broken,
            save_blocked: outcome.save_blocked,
            current: Mutex::new(Current {
                settings: outcome.settings,
                save_failed: outcome.save_blocked,
                future_version_to_keep: outcome.future_version,
            }),
        };
        store.apply_to_engine(&store.lock().settings);
        store
    }

    fn lock(&self) -> MutexGuard<'_, Current> {
        // 別スレッドが異常終了していても、設定の読み書きは続けられるようにする。
        self.current.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn apply_to_engine(&self, settings: &Settings) {
        if let Some(engine) = &self.engine {
            engine.set_master_volume(settings.volume);
        }
        self.assignments.apply(settings);
    }

    fn snapshot(&self, current: &Current) -> SettingsSnapshot {
        SettingsSnapshot {
            settings: current.settings.clone(),
            recovered_from_broken: self.recovered_from_broken,
            save_failed: current.save_failed,
        }
    }

    pub fn get(&self) -> SettingsSnapshot {
        let current = self.lock();
        self.snapshot(&current)
    }

    /// 画面から届いた差分を今の設定に重ねる。送られた項目だけを変え、送られなかった項目は今の値のまま。
    /// 割り当ては項目単位で重ね、値が `null` の項目は上書きを消す（`Settings::merged`）。
    /// 範囲外の値や知らない項目は読み取りと同じ規則で整え、即時に保存して、鳴らす側へ反映する。
    /// 整えた設定全体を返す。
    ///
    /// 差分の重ねも保存も、設定のロックを持ったまま行う。画面とRust側の更新が重なっても、
    /// 片方が他方の変更を巻き戻さない。
    pub fn update(&self, patch: &Value) -> Result<SettingsSnapshot, String> {
        let mut current = self.lock();
        let settings = current.settings.merged(patch).ok_or("設定の形式が正しくありません")?;

        self.apply_to_engine(&settings);
        current.save_failed = match &self.dir {
            Some(_) if self.save_blocked => true,
            Some(dir) => {
                // 新しい版のファイルは、初めて書き換える前に残す。残せなければ保存しない。
                let kept = match current.future_version_to_keep {
                    Some(version) => keep_future_version(dir, version).is_ok(),
                    None => true,
                };
                if kept {
                    current.future_version_to_keep = None;
                }
                !kept || save(dir, &settings).is_err()
            }
            None => true,
        };
        current.settings = settings;
        Ok(self.snapshot(&current))
    }

    /// Rust 側（メニューのオン／オフなど）から、オン／オフを変える。画面の更新と同じ差分の経路を通る。
    // 呼び出し元（メニュー）は後の issue（#17）。それまで本体からは呼ばれない。
    #[allow(dead_code)]
    pub fn set_enabled(&self, enabled: bool) -> SettingsSnapshot {
        self.update(&serde_json::json!({ "enabled": enabled })).expect("オブジェクトの差分は必ず重ねられる")
    }

    /// Rust 側から「初めて音が鳴った」印を付ける。画面の更新と同じ差分の経路を通る。
    // 呼び出し元は後の issue（#17）。それまで本体からは呼ばれない。
    #[allow(dead_code)]
    pub fn mark_first_sound_done(&self) -> SettingsSnapshot {
        self.update(&serde_json::json!({ "first_sound_done": true }))
            .expect("オブジェクトの差分は必ず重ねられる")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drums::build_free_kit;
    use crate::settings::{KeyGroup, Sound, DEFAULT_VOLUME};
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    const SAMPLE_JSON: &str = include_str!("../../tests/fixtures/settings.sample.json");

    /// テストごとの一時フォルダ。終わったら消す。
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "drumclack-settings-test-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_engine() -> Arc<AudioEngine> {
        Arc::new(AudioEngine::new(build_free_kit(48_000), 48_000))
    }

    #[test]
    fn missing_file_starts_with_defaults_without_a_broken_notice() {
        let dir = TempDir::new();
        let outcome = load(dir.path());
        assert_eq!(outcome, LoadOutcome::defaults());
        assert!(!dir.path().join(BROKEN_FILE_NAME).exists());
    }

    #[test]
    fn saved_settings_read_back_the_same() {
        let dir = TempDir::new();
        let sample = Settings::from_json_str(SAMPLE_JSON).unwrap();

        // 保存先のフォルダが無くても作って書ける。
        let nested = dir.path().join("jp.etbs.drumclack");
        save(&nested, &sample).unwrap();

        let outcome = load(&nested);
        assert_eq!(outcome.settings, sample);
        assert!(!outcome.recovered_from_broken);
        assert!(!nested.join(TEMP_FILE_NAME).exists(), "別名のファイルが残っていない");
    }

    #[test]
    fn untouched_settings_are_saved_with_empty_groups_and_keys() {
        let dir = TempDir::new();
        save(dir.path(), &Settings::default()).unwrap();

        let text = fs::read_to_string(dir.path().join(SETTINGS_FILE_NAME)).unwrap();
        let written: Value = serde_json::from_str(&text).unwrap();
        for section in ["typing", "play"] {
            assert_eq!(written[section]["groups"], json!({}));
            assert_eq!(written[section]["keys"], json!({}));
        }
    }

    #[test]
    fn broken_file_is_moved_aside_and_the_app_starts_with_defaults() {
        // 書き込み途中の切り詰め・別のファイル・空・文字コード違いなど、実際に起こる壊れ方。
        let broken_contents: [&[u8]; 5] = [
            b"{\"version\": 1, \"volume\": 0.5, \"typing\": {\"gro",
            b"{ not json at all",
            b"",
            b"[1, 2, 3]",
            &[0xff, 0xfe, 0x00, 0x7b, 0x80],
        ];
        for original in broken_contents {
            let dir = TempDir::new();
            fs::write(dir.path().join(SETTINGS_FILE_NAME), original).unwrap();

            let outcome = load(dir.path());

            assert_eq!(outcome.settings, Settings::default(), "{original:?}");
            assert!(outcome.recovered_from_broken, "{original:?}");
            assert_eq!(fs::read(dir.path().join(BROKEN_FILE_NAME)).unwrap(), original, "{original:?}");
            assert!(!dir.path().join(SETTINGS_FILE_NAME).exists(), "{original:?}");
        }
    }

    #[test]
    fn failure_in_the_middle_of_a_write_leaves_the_original_file_untouched() {
        let dir = TempDir::new();
        let sample = Settings::from_json_str(SAMPLE_JSON).unwrap();
        save(dir.path(), &sample).unwrap();
        let path = dir.path().join(SETTINGS_FILE_NAME);
        let original = fs::read(&path).unwrap();

        // 新しい内容の半分まで書いたところで失敗（ディスク満杯などの模擬）。
        let mut changed = sample.clone();
        changed.volume = 0.1;
        changed.typing.keys.insert("KeyA".to_string(), Sound::Rim);
        let new_bytes = changed.to_json_string().into_bytes();
        let result = write_atomic_with(&path, &new_bytes, |file, bytes| {
            file.write_all(&bytes[..bytes.len() / 2])?;
            Err(io::Error::new(io::ErrorKind::Other, "disk full"))
        });

        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), original, "元のファイルが変わっていない");
        assert!(!dir.path().join(TEMP_FILE_NAME).exists(), "途中まで書いた別名のファイルが残っていない");
        assert_eq!(load(dir.path()).settings, sample);
    }

    #[test]
    fn file_with_unknown_items_loads_and_keeps_the_other_items() {
        let dir = TempDir::new();
        let text = r#"{
            "volume": 0.3, "mystery": [1, 2],
            "typing": {"groups": {"letters": "snare", "ghost_group": "kick", "space": "ghost_sound"},
                       "keys": {"KeyJ": "kick", "KeyNope": "kick"}}
        }"#;
        fs::write(dir.path().join(SETTINGS_FILE_NAME), text).unwrap();

        let outcome = load(dir.path());

        assert!(!outcome.recovered_from_broken);
        assert_eq!(outcome.settings.volume, 0.3);
        assert_eq!(outcome.settings.typing.groups.len(), 1);
        assert_eq!(outcome.settings.typing.groups[&KeyGroup::Letters], Sound::Snare);
        assert_eq!(outcome.settings.typing.keys.len(), 1);
        assert_eq!(outcome.settings.typing.keys["KeyJ"], Sound::Kick);
        assert!(!dir.path().join(BROKEN_FILE_NAME).exists());
    }

    #[test]
    fn startup_applies_the_saved_volume_to_the_master_volume() {
        let dir = TempDir::new();
        fs::write(dir.path().join(SETTINGS_FILE_NAME), r#"{"volume": 0.25}"#).unwrap();
        let engine = test_engine();
        assert_eq!(engine.master_volume(), 1.0);

        let _store = SettingsStore::open(Some(dir.path().to_path_buf()), Some(engine.clone()));

        assert_eq!(engine.master_volume(), 0.25);
    }

    #[test]
    fn startup_without_a_file_uses_the_default_volume() {
        let dir = TempDir::new();
        let engine = test_engine();
        let _store = SettingsStore::open(Some(dir.path().to_path_buf()), Some(engine.clone()));
        assert_eq!(engine.master_volume(), DEFAULT_VOLUME);
    }

    #[test]
    fn update_rounds_volume_saves_immediately_and_reaches_the_master_volume() {
        for (given, expected) in [(999, 1.0), (-5, 0.0), (1000, 1.0)] {
            let dir = TempDir::new();
            let engine = test_engine();
            let store = SettingsStore::open(Some(dir.path().to_path_buf()), Some(engine.clone()));

            let snapshot = store.update(&json!({"volume": given, "enabled": false})).unwrap();

            assert_eq!(snapshot.settings.volume, expected, "{given}");
            assert!(!snapshot.save_failed);
            assert_eq!(engine.master_volume(), expected, "{given}");
            // 保存済みで、開き直しても同じ。
            let reopened = SettingsStore::open(Some(dir.path().to_path_buf()), None);
            assert_eq!(reopened.get().settings, snapshot.settings, "{given}");
            assert!(!reopened.get().settings.enabled);
        }
    }

    #[test]
    fn update_with_a_non_object_is_rejected_and_changes_nothing() {
        let dir = TempDir::new();
        let engine = test_engine();
        let store = SettingsStore::open(Some(dir.path().to_path_buf()), Some(engine.clone()));

        assert!(store.update(&json!([1, 2])).is_err());

        assert_eq!(store.get().settings, Settings::default());
        assert_eq!(engine.master_volume(), DEFAULT_VOLUME);
        assert!(!dir.path().join(SETTINGS_FILE_NAME).exists());
    }

    #[test]
    fn failed_save_is_reported_but_the_session_keeps_the_new_values() {
        let dir = TempDir::new();
        // 保存先のフォルダの位置にファイルを置いて、保存を失敗させる。
        let blocked = dir.path().join("blocked");
        fs::write(&blocked, b"a file, not a folder").unwrap();
        let engine = test_engine();
        let store = SettingsStore::open(Some(blocked), Some(engine.clone()));

        let snapshot = store.update(&json!({"volume": 0.4})).unwrap();

        assert!(snapshot.save_failed);
        assert_eq!(snapshot.settings.volume, 0.4);
        assert_eq!(engine.master_volume(), 0.4);
    }

    #[test]
    fn snapshot_reports_that_a_broken_file_was_set_aside() {
        let dir = TempDir::new();
        fs::write(dir.path().join(SETTINGS_FILE_NAME), "{ broken").unwrap();

        let store = SettingsStore::open(Some(dir.path().to_path_buf()), None);

        let snapshot = store.get();
        assert!(snapshot.recovered_from_broken);
        assert_eq!(snapshot.settings, Settings::default());
        // 画面へ渡す JSON にも出る。
        assert_eq!(serde_json::to_value(&snapshot).unwrap()["recovered_from_broken"], json!(true));
    }

    #[test]
    fn when_setting_aside_fails_the_original_is_kept_and_saving_stops() {
        let dir = TempDir::new();
        let original = b"{ broken but precious";
        fs::write(dir.path().join(SETTINGS_FILE_NAME), original).unwrap();
        // 前の退避があり、それを寄せる先（.1）が中身のあるフォルダで塞がっている。
        fs::write(dir.path().join(BROKEN_FILE_NAME), b"older").unwrap();
        let blocker = dir.path().join(BROKEN_PREVIOUS_FILE_NAME);
        fs::create_dir_all(&blocker).unwrap();
        fs::write(blocker.join("x"), b"x").unwrap();

        let outcome = load(dir.path());
        assert!(!outcome.recovered_from_broken, "退避できていないのに退避したことにしない");
        assert!(outcome.save_blocked);

        let store = SettingsStore::open(Some(dir.path().to_path_buf()), None);
        let snapshot = store.update(&json!({"volume": 0.4})).unwrap();
        assert!(snapshot.save_failed);
        assert_eq!(snapshot.settings.volume, 0.4);
        assert_eq!(fs::read(dir.path().join(SETTINGS_FILE_NAME)).unwrap(), original, "元のファイルを上書きしない");
        assert_eq!(fs::read(dir.path().join(BROKEN_FILE_NAME)).unwrap(), b"older");
    }

    #[test]
    fn unreadable_file_is_not_set_aside_and_saving_stops() {
        let dir = TempDir::new();
        // 読もうとするとエラーになる（ファイルの位置にフォルダがある）。壊れた中身ではない。
        let path = dir.path().join(SETTINGS_FILE_NAME);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("inside"), b"keep me").unwrap();

        let outcome = load(dir.path());
        assert_eq!(outcome.settings, Settings::default());
        assert!(!outcome.recovered_from_broken);
        assert!(outcome.save_blocked);
        assert!(!dir.path().join(BROKEN_FILE_NAME).exists(), "退避していない");
        assert!(path.join("inside").exists());

        let store = SettingsStore::open(Some(dir.path().to_path_buf()), None);
        assert!(store.get().save_failed);
        assert!(store.update(&json!({"volume": 0.4})).unwrap().save_failed);
        assert!(path.is_dir(), "保存で置き換えていない");
    }

    #[test]
    fn newer_version_file_is_copied_aside_before_the_first_save() {
        let dir = TempDir::new();
        let original = br#"{"version": 2, "volume": 0.3, "from_the_future": {"a": 1}}"#;
        fs::write(dir.path().join(SETTINGS_FILE_NAME), original).unwrap();
        let store = SettingsStore::open(Some(dir.path().to_path_buf()), None);
        assert_eq!(store.get().settings.volume, 0.3);
        assert!(!store.get().recovered_from_broken);

        let snapshot = store.update(&json!({"volume": 0.6})).unwrap();

        assert!(!snapshot.save_failed);
        assert_eq!(fs::read(dir.path().join("settings.v2.json")).unwrap(), original);
        let rewritten: Value =
            serde_json::from_slice(&fs::read(dir.path().join(SETTINGS_FILE_NAME)).unwrap()).unwrap();
        assert_eq!(rewritten["version"], json!(1));

        // 2回目以降の保存で、残したコピーを上書きしない。
        store.update(&json!({"volume": 0.7})).unwrap();
        assert_eq!(fs::read(dir.path().join("settings.v2.json")).unwrap(), original);
    }

    #[test]
    fn newer_version_file_is_not_overwritten_when_the_copy_fails() {
        let dir = TempDir::new();
        let original = br#"{"version": 3, "volume": 0.3}"#;
        fs::write(dir.path().join(SETTINGS_FILE_NAME), original).unwrap();
        // コピー先の候補がすべて塞がっている。
        fs::create_dir_all(dir.path().join("settings.v3.json")).unwrap();
        for n in 1..FUTURE_COPY_CANDIDATES {
            fs::create_dir_all(dir.path().join(format!("settings.v3.{n}.json"))).unwrap();
        }
        let store = SettingsStore::open(Some(dir.path().to_path_buf()), None);

        let snapshot = store.update(&json!({"volume": 0.6})).unwrap();

        assert!(snapshot.save_failed);
        assert_eq!(fs::read(dir.path().join(SETTINGS_FILE_NAME)).unwrap(), original);
    }

    #[test]
    fn existing_kept_copy_is_never_overwritten() {
        let dir = TempDir::new();
        fs::write(dir.path().join("settings.v2.json"), b"kept earlier").unwrap();
        let original = br#"{"version": 2, "volume": 0.3}"#;
        fs::write(dir.path().join(SETTINGS_FILE_NAME), original).unwrap();
        let store = SettingsStore::open(Some(dir.path().to_path_buf()), None);

        let snapshot = store.update(&json!({"volume": 0.6})).unwrap();

        assert!(!snapshot.save_failed);
        assert_eq!(fs::read(dir.path().join("settings.v2.json")).unwrap(), b"kept earlier");
        assert_eq!(fs::read(dir.path().join("settings.v2.1.json")).unwrap(), original);
    }

    #[test]
    fn file_saved_with_a_utf8_bom_is_read_normally() {
        let dir = TempDir::new();
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"volume": 0.3, "typing": {"keys": {"KeyJ": "kick"}}}"#);
        fs::write(dir.path().join(SETTINGS_FILE_NAME), &bytes).unwrap();

        let outcome = load(dir.path());

        assert!(!outcome.recovered_from_broken);
        assert!(!outcome.save_blocked);
        assert_eq!(outcome.settings.volume, 0.3);
        assert_eq!(outcome.settings.typing.keys["KeyJ"], Sound::Kick);
        assert!(!dir.path().join(BROKEN_FILE_NAME).exists());
        assert_eq!(fs::read(dir.path().join(SETTINGS_FILE_NAME)).unwrap(), bytes);
    }

    #[test]
    fn previous_broken_file_is_kept_as_dot_one() {
        let dir = TempDir::new();
        fs::write(dir.path().join(SETTINGS_FILE_NAME), b"{ first broken").unwrap();
        assert!(load(dir.path()).recovered_from_broken);
        fs::write(dir.path().join(SETTINGS_FILE_NAME), b"{ second broken").unwrap();

        let outcome = load(dir.path());

        assert!(outcome.recovered_from_broken);
        assert!(!outcome.save_blocked);
        assert_eq!(fs::read(dir.path().join(BROKEN_FILE_NAME)).unwrap(), b"{ second broken");
        assert_eq!(fs::read(dir.path().join(BROKEN_PREVIOUS_FILE_NAME)).unwrap(), b"{ first broken");
    }

    #[test]
    fn screen_can_call_only_the_four_listed_commands() {
        // 画面から呼べる命令の一覧（権限）を、増減したらここが落ちるようにする。
        let capability: Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let mut permissions: Vec<&str> =
            capability["permissions"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
        permissions.sort_unstable();
        assert_eq!(
            permissions,
            ["allow-get-settings", "allow-get-status", "allow-preview-sound", "allow-update-settings"]
        );

        // build.rs の命令の一覧（空白・改行を除いて比べる）。
        let build_script: String =
            include_str!("../build.rs").chars().filter(|c| !c.is_whitespace()).collect();
        assert!(build_script.contains(r#".commands(&["get_status","get_settings","update_settings","preview_sound",])"#));
    }

    #[test]
    fn volume_only_update_leaves_every_other_item_as_it_was() {
        let dir = TempDir::new();
        // 音量以外をすべて既定から変えておく（割り当て・言語・オン／オフ・配列・印）。
        fs::write(dir.path().join(SETTINGS_FILE_NAME), SAMPLE_JSON).unwrap();
        let store = SettingsStore::open(Some(dir.path().to_path_buf()), Some(test_engine()));
        let before = store.get().settings;
        assert!(!before.enabled && before.first_sound_done);
        assert_ne!(before.typing, Settings::default().typing);

        let snapshot = store.update(&json!({"volume": 0.3})).unwrap();

        let after = snapshot.settings;
        assert_eq!(after.volume, 0.3);
        assert_eq!(Settings { volume: before.volume, ..after.clone() }, before, "音量以外は変わらない");
        // 保存されたファイルも同じ。
        assert_eq!(SettingsStore::open(Some(dir.path().to_path_buf()), None).get().settings, after);
    }

    #[test]
    fn partial_updates_do_not_reset_the_items_they_do_not_mention() {
        let store = SettingsStore::open(None, None);
        store.update(&json!({"language": "en", "dynamics": 0.2, "keyboard_layout": "us"})).unwrap();
        store.update(&json!({"typing": {"keys": {"KeyJ": "kick"}}})).unwrap();

        let snapshot = store.update(&json!({"volume": 0.1, "play": {"groups": {"letters": "clap"}}})).unwrap();

        let settings = snapshot.settings;
        assert_eq!(settings.language, crate::settings::Language::En);
        assert_eq!(settings.dynamics, 0.2);
        assert_eq!(settings.keyboard_layout, crate::settings::KeyboardLayout::Us);
        assert_eq!(settings.typing.keys["KeyJ"], Sound::Kick);
        assert_eq!(settings.play.groups[&KeyGroup::Letters], Sound::Clap);
    }

    #[test]
    fn wrong_typed_or_unknown_items_in_an_update_keep_the_current_value() {
        let store = SettingsStore::open(None, None);
        store.update(&json!({"volume": 0.3, "enabled": false, "language": "ja"})).unwrap();

        let settings = store
            .update(&json!({"volume": "loud", "enabled": 1, "language": "klingon", "kit": "nope", "mystery": 1}))
            .unwrap()
            .settings;

        assert_eq!(settings.volume, 0.3);
        assert!(!settings.enabled);
        assert_eq!(settings.language, crate::settings::Language::Ja);
        assert_eq!(settings.kit, crate::settings::DEFAULT_KIT);
    }

    #[test]
    fn rust_side_changes_go_through_the_same_diff_and_are_not_undone_by_a_stale_full_copy() {
        let dir = TempDir::new();
        let store = SettingsStore::open(Some(dir.path().to_path_buf()), Some(test_engine()));
        store.update(&json!({"language": "en", "typing": {"keys": {"KeyJ": "kick"}}})).unwrap();

        // 画面が設定を読んだあとに、Rust 側がオフにして、初めて鳴った印を付ける。
        store.set_enabled(false);
        store.mark_first_sound_done();
        assert!(!store.get().settings.enabled);
        assert!(store.get().settings.first_sound_done);

        // 画面は音量だけ変える。巻き戻らない。
        let settings = store.update(&json!({"volume": 0.2})).unwrap().settings;
        assert!(!settings.enabled);
        assert!(settings.first_sound_done);
        assert_eq!(settings.language, crate::settings::Language::En);
        assert_eq!(settings.typing.keys["KeyJ"], Sound::Kick);

        // Rust 側の変更も、画面の差分も、保存されている。
        let reopened = SettingsStore::open(Some(dir.path().to_path_buf()), None).get().settings;
        assert_eq!(reopened, settings);
    }

    #[test]
    fn null_removes_only_that_override_for_groups_and_keys() {
        let dir = TempDir::new();
        let store = SettingsStore::open(Some(dir.path().to_path_buf()), None);
        store
            .update(&json!({
                "typing": {
                    "groups": {"letters": "snare", "space": "clap"},
                    "keys": {"KeyJ": "kick", "KeyK": "rim", "Numpad5": "none"}
                },
                "play": {"keys": {"KeyJ": "tom_low"}}
            }))
            .unwrap();

        let settings = store
            .update(&json!({"typing": {"groups": {"letters": null}, "keys": {"KeyJ": null}}}))
            .unwrap()
            .settings;

        assert_eq!(settings.typing.groups, std::collections::BTreeMap::from([(KeyGroup::Space, Sound::Clap)]));
        assert_eq!(
            settings.typing.keys,
            std::collections::BTreeMap::from([("KeyK".to_string(), Sound::Rim), ("Numpad5".to_string(), Sound::None)])
        );
        assert_eq!(settings.play.keys["KeyJ"], Sound::TomLow, "演奏用の同名キーは別の組なので残る");

        // 保存にも反映される。存在しない項目に null を送っても何も起きない。
        let reopened = SettingsStore::open(Some(dir.path().to_path_buf()), None);
        assert_eq!(reopened.get().settings, settings);
        let unchanged = store.update(&json!({"typing": {"keys": {"KeyZ": null}, "groups": {"enter": null}}})).unwrap();
        assert_eq!(unchanged.settings, settings);
    }

    #[test]
    fn newer_version_copy_is_made_once_and_not_again_on_later_saves() {
        let dir = TempDir::new();
        let original = br#"{"version": 2, "volume": 0.3}"#;
        fs::write(dir.path().join(SETTINGS_FILE_NAME), original).unwrap();
        let store = SettingsStore::open(Some(dir.path().to_path_buf()), None);

        for volume in [0.6, 0.7, 0.8, 0.9] {
            assert!(!store.update(&json!({ "volume": volume })).unwrap().save_failed);
        }

        let mut files: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort_unstable();
        assert_eq!(files, ["settings.json", "settings.v2.json"], "コピーは1つだけ（2回目以降は作らない）");
        assert_eq!(fs::read(dir.path().join("settings.v2.json")).unwrap(), original);
    }
}
