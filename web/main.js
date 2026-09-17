// 状態表示のみを行うフロントエンド。ユーザー操作は一切受け付けない
// （画面はポーリングして読み取り表示するだけ）。

const POLL_INTERVAL_MS = 300;

const els = {
  permissionRow: document.getElementById("row-permission"),
  permissionValue: document.getElementById("permission-value"),
  audioValue: document.getElementById("audio-value"),
  lastPlayValue: document.getElementById("last-play-value"),
  voicesValue: document.getElementById("voices-value"),
  testMode: document.getElementById("test-mode"),
};

function setStateClass(el, state) {
  el.classList.remove("state-ok", "state-warn", "state-error");
  if (state) {
    el.classList.add(state);
  }
}

function renderPermission(permission) {
  if (!permission) {
    // Windows など、この項目自体が不要なプラットフォームでは行ごと隠す。
    els.permissionRow.hidden = true;
    return;
  }

  els.permissionRow.hidden = false;
  els.permissionValue.innerHTML = "";

  const text = document.createElement("span");

  if (permission.state === "granted") {
    text.textContent = "許可済み";
    setStateClass(els.permissionValue, "state-ok");
  } else if (permission.state === "denied") {
    text.textContent = "未許可のため、キー入力を検知できません。";
    setStateClass(els.permissionValue, "state-error");
  } else {
    text.textContent = "確認できませんでした。";
    setStateClass(els.permissionValue, "state-warn");
  }

  els.permissionValue.appendChild(text);

  if (permission.state !== "granted") {
    const hint = document.createElement("span");
    hint.className = "action-hint";
    hint.textContent =
      "次の操作: システム設定 > プライバシーとセキュリティ > 入力監視 で drumclack を許可してください。";
    els.permissionValue.appendChild(hint);
  }
}

function renderAudio(audio) {
  els.audioValue.innerHTML = "";
  setStateClass(els.audioValue, audio.ok ? "state-ok" : "state-error");

  if (audio.ok) {
    const text = document.createElement("span");
    text.textContent = `準備完了（${audio.message}）`;
    els.audioValue.appendChild(text);
    return;
  }

  // 失敗時は、内部エラー文字列をそのまま出すのではなく、次に何をすればいいかを先に示す。
  // 内部エラー文字列は「詳細」として下に残す。
  const hint = document.createElement("span");
  hint.textContent = "次の操作: 他のアプリで使用中でないか確認し、drumclack を再起動してください。";
  els.audioValue.appendChild(hint);

  const detail = document.createElement("span");
  detail.className = "detail-text";
  detail.textContent = `詳細: ${audio.message}`;
  els.audioValue.appendChild(detail);
}

function renderLastPlay(lastPlayMs) {
  if (lastPlayMs == null) {
    els.lastPlayValue.textContent = "待機中";
    return;
  }
  const date = new Date(lastPlayMs);
  els.lastPlayValue.textContent = `${date.toLocaleTimeString("ja-JP", { hour12: false })} に発音`;
}

function renderVoices(active, max) {
  els.voicesValue.textContent = `${active} / ${max}`;
  setStateClass(els.voicesValue, active >= max ? "state-warn" : null);
}

function renderTestMode(testDelayMs) {
  if (testDelayMs == null) {
    els.testMode.hidden = true;
    return;
  }
  els.testMode.hidden = false;
  els.testMode.textContent = `テストモード: 遅延 ${testDelayMs}ms`;
}

function render(status) {
  renderPermission(status.permission);
  renderAudio(status.audio);
  renderLastPlay(status.last_play_ms);
  renderVoices(status.active_voices, status.max_voices);
  renderTestMode(status.test_delay_ms);
}

async function poll() {
  try {
    const invoke = window.__TAURI__.core.invoke;
    const status = await invoke("get_status");
    render(status);
  } catch (err) {
    // 状態取得自体に失敗した場合も、次の行動を先に示す（内部エラーは詳細として残す）。
    els.audioValue.innerHTML = "";
    setStateClass(els.audioValue, "state-error");

    const hint = document.createElement("span");
    hint.textContent = "次の操作: drumclack を再起動してください。";
    els.audioValue.appendChild(hint);

    const detail = document.createElement("span");
    detail.className = "detail-text";
    detail.textContent = `詳細: 状態の取得に失敗しました（${err}）`;
    els.audioValue.appendChild(detail);
  } finally {
    setTimeout(poll, POLL_INTERVAL_MS);
  }
}

poll();
