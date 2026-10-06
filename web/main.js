// 画面の本体。状態の帯（ポーリング）・設定の区画・区画の切り替えを受け持つ。
// Rust との行き来は `bridge.js`、文言は `i18n.js` を必ず通す。
// 設定の更新は「変えた項目だけ」を送る（全体を送り返すと、Rust 側の変更を巻き戻すため）。

const POLL_INTERVAL_MS = 300;

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
};

// ============================================================================
// 状態の帯
// ============================================================================

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
  const permission = view.status && view.status.permission;
  if (permission && permission.state !== "granted") {
    const denied = permission.state === "denied";
    issues.push({
      id: "permission",
      level: denied ? "error" : "warn",
      message: t(denied ? "status.permission.denied" : "status.permission.unknown"),
      action: t("status.permission.action"),
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
  const key = JSON.stringify([i18n.language, state, issues]);
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
    li.dataset.testid = "status-ok";
    li.textContent = t("status.ok");
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
  const permission = status.permission;
  els.detailPermissionRow.hidden = !permission;
  if (permission) {
    const known = ["granted", "denied"].includes(permission.state) ? permission.state : "unknown";
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

async function poll() {
  try {
    view.status = await bridge.getStatus();
    view.statusError = null;
  } catch (err) {
    // 状態の取得に失敗しても、次の操作を先に出す（内部のエラーは詳細として残す）。
    view.statusError = String(err);
  }
  renderStatus();
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
  setSliderValue(els.volume, els.volumeValue, percent(settings.volume));
  setSliderValue(els.dynamics, els.dynamicsValue, percent(settings.dynamics));
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
    }
  });
  return updateQueue;
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
      setSliderValue(slider, output, slider.value);
    });
    slider.addEventListener("change", () => sendUpdate({ [key]: Number(slider.value) / 100 }));
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
