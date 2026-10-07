// 画面の本体。状態の帯（ポーリング）・設定の区画・区画の切り替えを受け持つ。
// Rust との行き来は `bridge.js`、文言は `i18n.js` を必ず通す。
// 設定の更新は「変えた項目だけ」を送る（全体を送り返すと、Rust 側の変更を巻き戻すため）。

const POLL_INTERVAL_MS = 300;
// 設定は、状態の取得 N 回に1回取り直す（メニュー／トレイで変えた設定を、数秒以内に窓へ映すため）。
const SETTINGS_REFRESH_EVERY_POLLS = 3;

const bridge = window.drumclackBridge;
const i18n = window.drumclackI18n;
const t = i18n.t;

const $ = (testId) => document.querySelector(`[data-testid="${testId}"]`);

const els = {
  band: $("status-band"),
  issues: $("status-issues"),
  detailPermissionRow: $("detail-permission-row"),
  detailPermission: $("detail-permission"),
  detailAudio: $("detail-audio"),
  detailLastPlay: $("detail-last-play"),
  detailVoices: $("detail-voices"),
  testMode: $("test-mode"),
  recovered: $("settings-recovered"),
  recoveredText: $("settings-recovered-text"),
  recoveredDismiss: $("settings-recovered-dismiss"),
  settingsError: $("settings-error"),
  settingsForm: $("settings-form"),
  enabled: $("setting-enabled"),
  volume: $("setting-volume"),
  volumeValue: $("setting-volume-value"),
  dynamics: $("setting-dynamics"),
  dynamicsValue: $("setting-dynamics-value"),
  language: $("setting-language"),
  layout: $("setting-layout"),
};

// 今の画面の状態。描き直しのたびに Rust へ聞き直さなくていいように持つ。
const view = {
  settings: null, // 直近に Rust が返した設定のスナップショット
  recoveredDismissed: false, // 「既定の設定で起動しました」の行を閉じたか（この起動中だけ）
  settingsError: null, // "load" | "update" | null（保存の失敗は snapshot.save_failed から出す）
  status: null, // 直近の状態のスナップショット
  statusError: null, // 状態の取得に失敗したときのエラー文字列
  issuesKey: "",
  guideShown: false, // 発音の案内（許可の手順・何かキーを押す）をこの窓で出したか。鳴ったときの「完了」の表示に使う
  actionError: null, // "open" | "restart" | null（案内のボタンを押して失敗したとき）
  restarting: false, // 「起動し直す」を押したあと
  settingsVersion: 0, // 画面から設定を送るたびに増やす。古い取り直しの結果を捨てる目印
  pendingUpdates: 0, // Rust へ送っている最中の更新の数
  editing: new Set(), // 操作の途中の項目（動かしている最中のスライダー）。取り直した値で上書きしない
};

// 案内のボタン。状態の帯は中身が変わったときだけ作り直すので、押したときの動きは名前で引く。
const ACTIONS = {
  openSettings: async () => {
    try {
      await bridge.openInputMonitoringSettings();
      view.actionError = null;
    } catch (err) {
      console.error("入力監視の設定を開けませんでした", err);
      view.actionError = "open";
    }
    renderStatus();
  },
  restart: async () => {
    view.restarting = true;
    view.actionError = null;
    renderStatus();
    try {
      await bridge.restartApp();
    } catch (err) {
      console.error("起動し直せませんでした", err);
      view.restarting = false;
      view.actionError = "restart";
      renderStatus();
    }
  },
};

// ============================================================================
// 状態の帯
// ============================================================================

/**
 * 入力監視の許可の状態。OS の判定は起動時のまま変わらないので、キーのイベントが実際に届いていれば
 * 許可済みとみなす（OS の判定が未許可でも、届いたなら許可されている）。Windows など許可が無い OS は null。
 */
function effectivePermissionState() {
  const permission = view.status && view.status.permission;
  if (!permission) {
    return null;
  }
  return view.status.key_events_seen ? "granted" : permission.state;
}

/** 案内のボタンが失敗したときの、自分で行う方法の文。 */
function actionErrorText() {
  if (view.actionError === "open") return t("status.permission.failed.open");
  if (view.actionError === "restart") return t("status.permission.failed.restart");
  return null;
}

/** 一度でも鳴ったか。保存された印（以前の起動で鳴った）か、今回の起動で鳴った記録があれば真。 */
function hasSoundedBefore() {
  const saved = view.settings && view.settings.settings.first_sound_done === true;
  return saved || (view.status != null && view.status.last_play_ms != null);
}

/** 今の状態から、帯に出す問題の一覧を作る（正常なら空）。 */
function collectIssues() {
  if (view.statusError != null) {
    return [
      {
        id: "fetch",
        level: "error",
        message: t("status.fetch.failed"),
        action: t("status.fetch.action"),
        detail: t("status.fetch.detail", { message: view.statusError }),
      },
    ];
  }

  const issues = [];
  const permissionState = effectivePermissionState();
  const permissionMissing = permissionState != null && permissionState !== "granted";
  if (permissionMissing) {
    const denied = permissionState === "denied";
    issues.push({
      id: "permission",
      level: denied ? "error" : "warn",
      message: t(denied ? "status.permission.denied" : "status.permission.unknown"),
      action: t("status.permission.steps"),
      // 許可 → 起動し直す → キーを押す、の3段。ボタンは名前（ACTIONS）で引く。
      steps: [
        { text: t("status.permission.stepOpen"), button: { name: "openSettings", label: t("status.permission.stepOpen.button") } },
        {
          text: t("status.permission.stepRestart"),
          button: { name: "restart", label: t("status.permission.stepRestart.button"), disabled: view.restarting },
        },
        { text: t("status.permission.stepPress") },
      ],
      note: t("status.permission.reset"),
      detail: actionErrorText(),
    });
  }

  const audio = view.status && view.status.audio;
  if (audio && !audio.ok) {
    issues.push({
      id: "audio",
      level: "error",
      message: t("status.audio.failed"),
      action: t("status.audio.action"),
      // Rust が返す内部のエラー文字列。次の操作より弱い「詳細」として残す。
      detail: t("status.audio.detail", { message: audio.message }),
    });
  }

  // 許可が足りている（許可の無い OS を含む）のに、まだ一度も鳴っていない。次の操作は「キーを押す」だけ。
  if (view.status && !permissionMissing && !(audio && !audio.ok) && !hasSoundedBefore()) {
    issues.push({
      id: "first-sound",
      level: "warn",
      message: t("status.firstSound.message"),
      action: t("status.firstSound.action"),
    });
  }
  return issues;
}

function issueElement(issue) {
  const li = document.createElement("li");
  li.className = `status-issue state-${issue.level}`;
  li.dataset.testid = `status-issue-${issue.id}`;

  const message = document.createElement("span");
  message.className = "issue-message";
  message.textContent = issue.message;
  li.appendChild(message);

  if (issue.action) {
    const action = document.createElement("span");
    action.className = "action-hint";
    action.textContent = issue.action;
    li.appendChild(action);
  }
  if (issue.steps) {
    const list = document.createElement("ol");
    list.className = "issue-steps";
    for (const step of issue.steps) {
      const item = document.createElement("li");
      const text = document.createElement("span");
      text.textContent = step.text;
      item.appendChild(text);
      if (step.button) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "issue-button";
        button.dataset.testid = `status-action-${step.button.name}`;
        button.textContent = step.button.label;
        button.disabled = step.button.disabled === true;
        button.addEventListener("click", ACTIONS[step.button.name]);
        item.appendChild(button);
      }
      list.appendChild(item);
    }
    li.appendChild(list);
  }
  if (issue.note) {
    const note = document.createElement("span");
    note.className = "issue-note";
    note.dataset.testid = `status-note-${issue.id}`;
    note.textContent = issue.note;
    li.appendChild(note);
  }
  if (issue.detail) {
    const detail = document.createElement("span");
    detail.className = "detail-text";
    detail.textContent = issue.detail;
    li.appendChild(detail);
  }
  return li;
}

/** 正常時は1行、異常時は問題ごとに「何が起きたか」と「次の操作」を出す。 */
function renderBand() {
  const checking = view.status == null && view.statusError == null;
  const issues = collectIssues();
  // 案内を出したあとに鳴ったら、正常の文の代わりに「鳴りました」を出す（この窓を開いている間）。
  if (issues.some((issue) => issue.id === "permission" || issue.id === "first-sound")) {
    view.guideShown = true;
  }
  const showSoundDone = issues.length === 0 && view.guideShown;

  let state = "ok";
  if (checking) {
    state = "checking";
  } else if (issues.some((issue) => issue.level === "error")) {
    state = "error";
  } else if (issues.length > 0) {
    state = "warn";
  }
  els.band.dataset.state = state;

  // 読み上げ（aria-live）が毎回走らないよう、中身が変わったときだけ作り直す。
  const key = JSON.stringify([i18n.language, state, issues, showSoundDone]);
  if (key === view.issuesKey) {
    return;
  }
  view.issuesKey = key;
  els.issues.replaceChildren();

  if (checking) {
    const li = document.createElement("li");
    li.className = "status-issue";
    li.textContent = t("status.details.checking");
    els.issues.appendChild(li);
  } else if (issues.length === 0) {
    const li = document.createElement("li");
    li.className = "status-issue state-ok";
    li.dataset.testid = showSoundDone ? "status-first-sound-done" : "status-ok";
    li.textContent = t(showSoundDone ? "status.firstSound.done" : "status.ok");
    els.issues.appendChild(li);
  } else {
    for (const issue of issues) {
      els.issues.appendChild(issueElement(issue));
    }
  }
}

function setStateClass(el, state) {
  el.classList.remove("state-ok", "state-warn", "state-error");
  if (state) {
    el.classList.add(state);
  }
}

function renderDetails() {
  const status = view.status;
  if (!status) {
    for (const el of [els.detailAudio, els.detailLastPlay, els.detailVoices]) {
      el.textContent = t("status.details.checking");
    }
    els.detailPermissionRow.hidden = true;
    return;
  }

  // Windows など、権限の概念が無いプラットフォームでは行ごと隠す。
  const permissionState = effectivePermissionState();
  els.detailPermissionRow.hidden = permissionState == null;
  if (permissionState != null) {
    const known = ["granted", "denied"].includes(permissionState) ? permissionState : "unknown";
    const label = {
      granted: t("status.details.permission.granted"),
      denied: t("status.details.permission.denied"),
      unknown: t("status.details.permission.unknown"),
    };
    const level = { granted: "state-ok", denied: "state-error", unknown: "state-warn" };
    els.detailPermission.textContent = label[known];
    setStateClass(els.detailPermission, level[known]);
  }

  els.detailAudio.textContent = t(status.audio.ok ? "status.details.audio.ok" : "status.details.audio.failed");
  setStateClass(els.detailAudio, status.audio.ok ? "state-ok" : "state-error");

  if (status.last_play_ms == null) {
    els.detailLastPlay.textContent = t("status.details.lastPlay.idle");
  } else {
    const locale = i18n.language === "ja" ? "ja-JP" : "en-US";
    const time = new Date(status.last_play_ms).toLocaleTimeString(locale, { hour12: false });
    els.detailLastPlay.textContent = t("status.details.lastPlay.at", { time });
  }

  els.detailVoices.textContent = `${status.active_voices} / ${status.max_voices}`;
  setStateClass(els.detailVoices, status.active_voices >= status.max_voices ? "state-warn" : null);

  if (status.test_delay_ms == null) {
    els.testMode.hidden = true;
  } else {
    els.testMode.hidden = false;
    els.testMode.textContent = t("status.testMode", { ms: status.test_delay_ms });
  }
}

function renderStatus() {
  renderBand();
  renderDetails();
}

let pollCount = 0;

async function poll() {
  try {
    view.status = await bridge.getStatus();
    view.statusError = null;
  } catch (err) {
    // 状態の取得に失敗しても、次の操作を先に出す（内部のエラーは詳細として残す）。
    view.statusError = String(err);
  }
  renderStatus();
  pollCount += 1;
  if (pollCount % SETTINGS_REFRESH_EVERY_POLLS === 0) {
    await refreshSettings();
  }
  setTimeout(poll, POLL_INTERVAL_MS);
}

// ============================================================================
// 設定の区画
// ============================================================================

function percent(unit) {
  return Math.round(unit * 100);
}

/** 設定の言語（auto 含む）から、実際に使う言語を決める。 */
function resolveLanguage(setting) {
  return setting === "auto" ? i18n.resolveAuto(navigator.language) : setting;
}

/** 配列の `auto` は、OS の言語が日本語なら JIS、それ以外は US として描く（保存するのは選んだ値だけ）。 */
function resolveLayout(setting) {
  if (setting === "auto") {
    return i18n.resolveAuto(navigator.language) === "ja" ? "jis" : "us";
  }
  return setting;
}

function renderNotices() {
  const snapshot = view.settings;

  els.recovered.hidden = !(snapshot && snapshot.recovered_from_broken) || view.recoveredDismissed;
  if (!els.recovered.hidden) {
    els.recoveredText.textContent = t("settings.recovered");
  }

  let errorKey = null;
  if (view.settingsError) {
    errorKey = view.settingsError === "load" ? "settings.loadFailed" : "settings.updateFailed";
  } else if (snapshot && snapshot.save_failed) {
    errorKey = "settings.saveFailed";
  }
  els.settingsError.hidden = errorKey == null;
  if (errorKey) {
    els.settingsError.textContent = t(errorKey);
  }
}

function setSliderValue(slider, output, value) {
  slider.value = String(value);
  output.textContent = `${slider.value}%`;
  slider.setAttribute("aria-valuetext", `${slider.value}%`);
}

function renderControls() {
  const snapshot = view.settings;

  // 配列の「自動」は、今どちらで描くかを添える（「自動」だけでは、何が選ばれているか分からない）。
  const autoOption = els.layout.querySelector('option[value="auto"]');
  autoOption.textContent = t("settings.layout.auto", { layout: resolveLayout("auto").toUpperCase() });

  for (const fieldset of els.settingsForm.querySelectorAll("fieldset")) {
    fieldset.disabled = snapshot == null;
  }
  if (!snapshot) {
    return;
  }

  const settings = snapshot.settings;
  els.enabled.checked = settings.enabled;
  if (!view.editing.has("volume")) {
    setSliderValue(els.volume, els.volumeValue, percent(settings.volume));
  }
  if (!view.editing.has("dynamics")) {
    setSliderValue(els.dynamics, els.dynamicsValue, percent(settings.dynamics));
  }
  els.language.value = settings.language;
  els.layout.value = settings.keyboard_layout;
  document.documentElement.dataset.keyboardLayout = resolveLayout(settings.keyboard_layout);
}

/** 設定のスナップショットを画面へ反映する。言語が変わったら対応表も読み替える。 */
async function applySettings(snapshot) {
  view.settings = snapshot;
  const lang = resolveLanguage(snapshot.settings.language);
  if (lang !== i18n.language) {
    try {
      await i18n.setLanguage(lang);
    } catch (err) {
      // 対応表を読めなければ今の言語のまま。設定の保存とは別の問題なので、画面は止めない。
      console.error("言語を切り替えられませんでした", err);
    }
  }
  renderControls();
  renderNotices();
  renderStatus();
}

// Rust への更新は1本ずつ順に送る。続けて操作しても、古い応答で新しい値を巻き戻さない。
let updateQueue = Promise.resolve();

/** 変えた項目だけを Rust へ送る。 */
function sendUpdate(patch) {
  // 送っている間と、その直後に始まる取り直しの結果は、古い値を含むので使わない。
  view.settingsVersion += 1;
  view.pendingUpdates += 1;
  updateQueue = updateQueue.then(async () => {
    try {
      const snapshot = await bridge.updateSettings(patch);
      view.settingsError = null;
      await applySettings(snapshot);
    } catch (err) {
      console.error("設定を更新できませんでした", err);
      // 保存されたのか分からない値は、画面に残さない。最後に確かめられた設定へ戻す。
      view.settingsError = "update";
      renderControls();
      renderNotices();
    } finally {
      view.pendingUpdates -= 1;
      view.settingsVersion += 1;
    }
  });
  return updateQueue;
}

/**
 * Rust 側で変わった設定（メニュー／トレイのオン／オフ、初めて鳴った印など）を取り込む。
 * 次の場合は取り込まない: 画面から送っている最中／取得中に画面から送った（取得した値が古い）／
 * 取得した設定が今の画面と同じ。操作の途中のスライダーは、取り込んでも上書きしない（renderControls）。
 */
async function refreshSettings() {
  if (view.pendingUpdates > 0) {
    return;
  }
  const version = view.settingsVersion;
  let snapshot;
  try {
    snapshot = await bridge.getSettings();
  } catch (err) {
    // 取り直しの失敗は黙って次に回す（起動時の読み込みの失敗とは別。画面には出さない）。
    return;
  }
  if (!snapshot || !snapshot.settings || version !== view.settingsVersion || view.pendingUpdates > 0) {
    return;
  }
  if (JSON.stringify(snapshot) === JSON.stringify(view.settings)) {
    return;
  }
  await applySettings(snapshot);
}

function bindSettings() {
  // Enter での送信（ページの再読み込み）は使わない。変更は操作のたびに即時に保存する。
  els.settingsForm.addEventListener("submit", (event) => event.preventDefault());

  els.recoveredDismiss.addEventListener("click", () => {
    view.recoveredDismissed = true;
    renderNotices();
  });

  els.enabled.addEventListener("change", () => sendUpdate({ enabled: els.enabled.checked }));

  // スライダーは、動かしている間は表示だけ更新し、手を離したとき（change）に1回だけ保存する。
  for (const [slider, output, key] of [
    [els.volume, els.volumeValue, "volume"],
    [els.dynamics, els.dynamicsValue, "dynamics"],
  ]) {
    slider.addEventListener("input", () => {
      view.editing.add(key);
      setSliderValue(slider, output, slider.value);
    });
    slider.addEventListener("change", () => {
      // 手を離したので操作の途中ではない。送っている間は取り直しも止まり、返ってきた値で描き直される。
      view.editing.delete(key);
      sendUpdate({ [key]: Number(slider.value) / 100 });
    });
  }

  els.language.addEventListener("change", () => sendUpdate({ language: els.language.value }));
  els.layout.addEventListener("change", () => sendUpdate({ keyboard_layout: els.layout.value }));
}

// ============================================================================
// 区画の切り替え（設定／割り当て／演奏）
// ============================================================================

function bindTabs() {
  const tabs = Array.from(document.querySelectorAll('[role="tab"]'));

  function select(tab) {
    for (const other of tabs) {
      const selected = other === tab;
      other.setAttribute("aria-selected", String(selected));
      other.tabIndex = selected ? 0 : -1;
      document.getElementById(other.getAttribute("aria-controls")).hidden = !selected;
    }
  }

  // メニューバー／トレイの「演奏モードを開く」から、Rust が窓を出したあとに呼ぶ入口。
  // 区画の名前（settings / assign / play）で切り替えるだけ。知らない名前は無視する。
  window.drumclackShowSection = (name) => {
    const tab = tabs.find((candidate) => candidate.dataset.testid === `tab-${name}`);
    if (tab) select(tab);
  };

  for (const tab of tabs) {
    tab.addEventListener("click", () => select(tab));
    tab.addEventListener("keydown", (event) => {
      const index = tabs.indexOf(tab);
      let next = null;
      if (event.key === "ArrowRight") next = tabs[(index + 1) % tabs.length];
      if (event.key === "ArrowLeft") next = tabs[(index - 1 + tabs.length) % tabs.length];
      if (event.key === "Home") next = tabs[0];
      if (event.key === "End") next = tabs[tabs.length - 1];
      if (next) {
        event.preventDefault();
        select(next);
        next.focus();
      }
    });
  }
}

// ============================================================================
// 起動
// ============================================================================

async function start() {
  bindTabs();
  bindSettings();

  let snapshot = null;
  try {
    snapshot = await bridge.getSettings();
  } catch (err) {
    console.error("設定を読み込めませんでした", err);
    view.settingsError = "load";
  }

  // 設定が読めなくても、OS の言語で文言は出す。
  const lang = resolveLanguage(snapshot ? snapshot.settings.language : "auto");
  try {
    await i18n.setLanguage(lang);
  } catch (err) {
    console.error("対応表を読み込めませんでした", err);
  }

  if (snapshot) {
    view.settings = snapshot;
  }
  renderControls();
  renderNotices();
  renderStatus();
  document.body.dataset.ready = "true";

  poll();
}

start();
