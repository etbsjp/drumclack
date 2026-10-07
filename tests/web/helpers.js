// 画面のテストの共通部品。Rust の代わりに `window.__TAURI__`（bridge.js が呼ぶ先）を差し替え、
// 画面から出た命令の名前と引数を記録する。web/ のコードには手を入れない。

const fs = require("node:fs");
const path = require("node:path");
const base = require("@playwright/test");

const WEB_ROOT = path.resolve(__dirname, "../../web");

/**
 * 設定ファイルの見本（Rust 側の設定のテストと同じファイル）。
 * Rust の `Settings` を JSON にしたものと同じ形なので、そのまま `get_settings` の `settings` に使える。
 */
const SAMPLE_SETTINGS = JSON.parse(
  fs.readFileSync(path.resolve(__dirname, "../fixtures/settings.sample.json"), "utf8"),
);

/** 何も選んでいない起動直後の設定（Rust の `Settings::default()` と同じ値）。 */
const DEFAULT_SETTINGS = {
  version: 1,
  kit: "808",
  enabled: true,
  volume: 0.8,
  dynamics: 0.6,
  language: "auto",
  keyboard_layout: "auto",
  first_sound_done: false,
  typing: { groups: {}, keys: {} },
  play: { groups: {}, keys: {} },
};

const STATUS_OK = {
  running: true,
  platform: "macos",
  permission: { state: "granted" },
  audio: { ok: true, message: "初期化済み（サンプルレート 48000 Hz）" },
  // 鳴ったことがある状態（正常の1行になる）。まだ鳴っていない状態は STATUS_NO_SOUND_YET。
  last_play_ms: new Date(2026, 9, 6, 15, 0, 0).getTime(),
  active_voices: 0,
  max_voices: 16,
  test_delay_ms: null,
  key_events_seen: false,
};

const STATUS_NO_SOUND_YET = { ...STATUS_OK, last_play_ms: null };

const STATUS_PERMISSION_DENIED = { ...STATUS_OK, permission: { state: "denied" } };
const STATUS_PERMISSION_UNKNOWN = { ...STATUS_OK, permission: { state: "unknown" } };
const STATUS_AUDIO_FAILED = {
  ...STATUS_OK,
  audio: { ok: false, message: "no output device (test)" },
};

/**
 * 画面を開く。`options` で Rust の返事を決める。
 *  - settings: get_settings が返す設定
 *  - status: get_status が返す状態 / statusError: 状態の取得を失敗させる
 *  - recovered / saveFailed: 設定のスナップショットの印
 *  - failSettings / failUpdate: get_settings / update_settings を失敗させる
 *  - failOpenSettings / failRestart: 入力監視の設定を開く命令 / 起動し直す命令を失敗させる
 *  - malformedSettings: get_settings が（設定を含まない）壊れた返事を返す
 *  - hangSettings: get_settings が返事をしない
 *  - noWait: 画面が起動し終わる（data-ready）のを待たずに返す
 */
async function openApp(page, options = {}) {
  const config = {
    settings: DEFAULT_SETTINGS,
    status: STATUS_OK,
    statusError: false,
    recovered: false,
    saveFailed: false,
    failSettings: false,
    failUpdate: false,
    failOpenSettings: false,
    failRestart: false,
    failPreview: false,
    malformedSettings: false,
    hangSettings: false,
    ...options,
  };

  await page.addInitScript((cfg) => {
    const fake = { cfg, calls: [], settings: JSON.parse(JSON.stringify(cfg.settings)) };
    window.__fake = fake;

    // 本物と同じく、返すたびに別のオブジェクトにする（Rust との間は JSON で渡るため）。
    const snapshot = () => ({
      settings: JSON.parse(JSON.stringify(fake.settings)),
      recovered_from_broken: fake.cfg.recovered,
      save_failed: fake.cfg.saveFailed,
    });

    window.__TAURI__ = {
      core: {
        invoke: async (cmd, args) => {
          fake.calls.push({ cmd, args: args === undefined ? null : JSON.parse(JSON.stringify(args)) });
          if (cmd === "get_status") {
            if (fake.cfg.statusError) throw new Error("status unavailable (test)");
            return fake.cfg.status;
          }
          if (cmd === "get_settings") {
            if (fake.cfg.failSettings) throw new Error("settings unavailable (test)");
            if (fake.cfg.hangSettings) return new Promise(() => {});
            if (fake.cfg.malformedSettings) return {};
            return snapshot();
          }
          if (cmd === "update_settings") {
            if (fake.cfg.failUpdate) throw new Error("update refused (test)");
            // Rust の差分の重ね方を真似る。最上位の項目は置き換え、割り当て（typing / play）は
            // groups / keys の項目単位で重ね、値が null の項目は上書きを消す。
            for (const [name, value] of Object.entries(args.settings)) {
              if ((name === "typing" || name === "play") && value && typeof value === "object") {
                const section = fake.settings[name];
                for (const part of ["groups", "keys"]) {
                  for (const [item, sound] of Object.entries(value[part] || {})) {
                    if (sound === null) delete section[part][item];
                    else section[part][item] = sound;
                  }
                }
              } else {
                fake.settings[name] = value;
              }
            }
            return snapshot();
          }
          if (cmd === "preview_sound") {
            if (fake.cfg.failPreview) throw new Error("preview refused (test)");
            return null;
          }
          if (cmd === "open_input_monitoring_settings") {
            if (fake.cfg.failOpenSettings) throw new Error("open refused (test)");
            return null;
          }
          if (cmd === "restart_app") {
            if (fake.cfg.failRestart) throw new Error("restart refused (test)");
            return null;
          }
          throw new Error(`知らない命令: ${cmd}`);
        },
      },
      event: { listen: async () => () => {} },
    };
  }, config);

  await page.goto("/index.html");
  if (!config.hangSettings && !config.noWait) {
    await page.locator('body[data-ready="true"]').waitFor();
  }
}

/** 画面から出た `update_settings` の呼び出し（命令名と引数）。 */
function updateCalls(page) {
  return page.evaluate(() => window.__fake.calls.filter((call) => call.cmd === "update_settings"));
}

/** 画面から出た `preview_sound` の呼び出し（引数）。 */
function previewCalls(page) {
  return page.evaluate(() =>
    window.__fake.calls.filter((call) => call.cmd === "preview_sound").map((call) => call.args),
  );
}

/** 今 Rust 側（差し替え）が持っている設定。 */
function currentSettings(page) {
  return page.evaluate(() => JSON.parse(JSON.stringify(window.__fake.settings)));
}

function allCalls(page) {
  return page.evaluate(() => window.__fake.calls);
}

/** 以後の `get_status` が返す内容を変える。 */
function setStatus(page, status) {
  return page.evaluate((next) => {
    window.__fake.cfg.statusError = false;
    window.__fake.cfg.status = next;
  }, status);
}

/** Rust 側で設定が変わったことにする（メニュー／トレイの切り替え、初めて鳴った印など）。 */
function changeSettingsOnRustSide(page, patch) {
  return page.evaluate((next) => Object.assign(window.__fake.settings, next), patch);
}

function readWeb(relativePath) {
  return fs.readFileSync(path.join(WEB_ROOT, relativePath), "utf8");
}

/**
 * 実物の窓と同じ CSP で配っているので、CSP 違反やスクリプトの例外は、画面の不具合として落とす。
 * （意図して失敗させるテストが出す console.error は対象にしない。）
 */
const test = base.test.extend({
  page: async ({ page }, use) => {
    const problems = [];
    // 起動の失敗を意図して起こすテストだけが、例外を許可する（page.allowPageErrors = true）。
    page.allowPageErrors = false;
    page.on("pageerror", (error) => {
      if (!page.allowPageErrors) problems.push(`pageerror: ${error.message}`);
    });
    page.on("console", (message) => {
      const text = message.text();
      if (message.type() === "error" && /Content Security Policy|Refused to/i.test(text)) {
        problems.push(`csp: ${text}`);
      }
    });
    await use(page);
    base.expect(problems).toEqual([]);
  },
});

module.exports = {
  test,
  expect: base.expect,
  WEB_ROOT,
  SAMPLE_SETTINGS,
  DEFAULT_SETTINGS,
  STATUS_OK,
  STATUS_NO_SOUND_YET,
  STATUS_PERMISSION_DENIED,
  STATUS_PERMISSION_UNKNOWN,
  STATUS_AUDIO_FAILED,
  openApp,
  updateCalls,
  previewCalls,
  currentSettings,
  allCalls,
  setStatus,
  changeSettingsOnRustSide,
  readWeb,
};
