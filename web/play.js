// 演奏の区画。パッド8個・「いま演奏用／タイピング用」の表示・オフ中の「オンにする」を受け持つ。
// Rust との行き来は `bridge.js`、文言は `i18n.js` を通す。
//
// 守ること（設計の正本 #9）:
// - パッドは `pointerdown` でだけ鳴らす（click では鳴らさない。キーボードでは鳴らさない）。
// - 演奏の区画では keydown をすべて既定動作ごと止める。既定でスペース＝キックなので、止めないと
//   フォーカス中のパッドが押されて二重に鳴る。入力欄は置かない。打鍵そのものは Rust 側の監視が受ける。
// - Rust から受け取るのは「鳴った音の名前」と「演奏用の割り当てを使っているか」だけ。
//   音の名前は保存せず、ログにも出さない。キーの位置・時刻は画面に届かない。
// - 光る表現は OS の「動きを減らす」に従う（CSS）。1打ごとの読み上げはしない（パッドを aria-live に入れない）。

(function () {
  const SOUNDS = ["kick", "snare", "hat_closed", "hat_open", "clap", "rim", "tom_low", "tom_high"];
  /** パッドが光っている長さ。 */
  const LIT_MS = 140;
  /** パッドに並べる割り当てキーの数。超えた分は「ほか N 個」にまとめる。 */
  const MAX_KEYS_ON_PAD = 3;
  // Rust 側（src-tauri/src/play_mode.rs）と同じ名前にそろえる。
  const EVENT_SOUND_PLAYED = "drum-played";
  const EVENT_MODE_CHANGED = "play-mode-changed";

  const state = {
    ctx: null,
    open: false, // 演奏の区画を開いているか
    playing: false, // Rust が演奏用の割り当てを使っているか（区画が開いていて、窓が最前面）
    pads: new Map(), // 音の名前 → { button, keys, timer }
    built: false,
  };

  const $ = (testId) => document.querySelector(`[data-testid="${testId}"]`);
  const els = {};

  const t = (key, params) => state.ctx.i18n.t(key, params);

  // ==========================================================================
  // Rust との橋渡し
  // ==========================================================================

  /** 演奏の区画を開いた／閉じたと Rust に知らせ、演奏用の割り当てを使っているかを受け取る。 */
  async function syncOpenToRust() {
    const wanted = state.open;
    try {
      const playing = await window.drumclackBridge.setPlayViewOpen(wanted);
      // 返事を待つ間に区画を移っていたら、古い返事は捨てる（最後に知らせた値が正）。
      if (state.open === wanted) {
        state.playing = wanted && playing === true;
      }
    } catch (err) {
      console.error("演奏の画面の開閉を Rust に知らせられませんでした", err);
      state.playing = false;
    }
    render();
  }

  function setOpen(open) {
    if (state.open === open) {
      // 開いたままもう一度呼ばれた（メニューの「演奏モードを開く」など）。状態だけ取り直す。
      if (open) syncOpenToRust();
      return;
    }
    state.open = open;
    if (!open) {
      state.playing = false;
      clearLit();
    }
    render();
    syncOpenToRust();
  }

  // ==========================================================================
  // パッド
  // ==========================================================================

  function buildPads() {
    const list = $("pads");
    for (const sound of SOUNDS) {
      const item = document.createElement("li");
      item.className = "pad-cell";

      const button = document.createElement("button");
      button.type = "button";
      button.className = "pad";
      button.dataset.testid = `pad-${sound}`;
      button.dataset.sound = sound;
      button.dataset.lit = "false";

      const name = document.createElement("span");
      name.className = "pad-name";
      const keys = document.createElement("span");
      keys.className = "pad-keys";
      keys.dataset.testid = `pad-keys-${sound}`;
      button.append(name, keys);

      // 鳴らすのは pointerdown だけ。キーボードの操作（Enter・Space の click）では鳴らさない。
      button.addEventListener("pointerdown", (event) => {
        if (event.pointerType === "mouse" && event.button !== 0) return;
        light(sound);
        preview(sound);
      });

      item.appendChild(button);
      list.appendChild(item);
      state.pads.set(sound, { button, name, keys, timer: null });
    }
    state.built = true;
  }

  function preview(sound) {
    // 試聴できなくても（音声デバイスが無いなど）画面は止めない。
    Promise.resolve()
      .then(() => window.drumclackBridge.previewSound(sound))
      .catch((err) => {
        console.error("試聴できませんでした", err);
      });
  }

  /** パッドを光らせる。連打では、光る時間を延ばし直す。 */
  function light(sound) {
    const pad = state.pads.get(sound);
    if (!pad) return;
    pad.button.dataset.lit = "true";
    clearTimeout(pad.timer);
    pad.timer = setTimeout(() => {
      pad.button.dataset.lit = "false";
    }, LIT_MS);
  }

  function clearLit() {
    for (const pad of state.pads.values()) {
      clearTimeout(pad.timer);
      pad.button.dataset.lit = "false";
    }
  }

  /** Rust から届いた「鳴った音の名前」。知らない名前・区画が閉じているときは無視する。 */
  function onSoundPlayed(sound) {
    if (!state.open || typeof sound !== "string") return;
    light(sound);
  }

  function onModeChanged(playing) {
    state.playing = state.open && playing === true;
    if (!state.playing) clearLit();
    render();
  }

  // ==========================================================================
  // 表示
  // ==========================================================================

  function isEnabled() {
    const snapshot = state.ctx.getSnapshot();
    // 設定を読めていないあいだは、オフとは言わない。
    return !(snapshot && snapshot.settings && snapshot.settings.enabled === false);
  }

  function renderPads() {
    const assign = state.ctx.assign;
    const keysBySound = assign.keysBySound("play");
    for (const [sound, pad] of state.pads) {
      const keys = keysBySound[sound] || [];
      const shown = keys.slice(0, MAX_KEYS_ON_PAD);
      const rest = keys.length - shown.length;

      pad.name.textContent = assign.soundLabel(sound);
      pad.keys.replaceChildren();
      for (const key of shown) {
        const cap = document.createElement("kbd");
        cap.className = "pad-key";
        cap.textContent = key;
        pad.keys.appendChild(cap);
      }
      if (rest > 0) {
        const more = document.createElement("span");
        more.className = "pad-more";
        more.textContent = t("play.pad.more", { count: rest });
        pad.keys.appendChild(more);
      }
      if (keys.length === 0) {
        const none = document.createElement("span");
        none.className = "pad-more";
        none.textContent = t("play.pad.noKey");
        pad.keys.appendChild(none);
      }
      // 読み上げは「音の名前。割り当てキー」。光ったことは読み上げない。
      pad.button.setAttribute(
        "aria-label",
        keys.length === 0
          ? t("play.pad.label.noKey", { sound: assign.soundLabel(sound) })
          : t("play.pad.label", { sound: assign.soundLabel(sound), keys: keys.join(", ") }),
      );
    }
  }

  function render() {
    if (!state.ctx || !state.built) return;

    const playing = state.playing;
    const enabled = isEnabled();

    // 「いま演奏用／タイピング用」は常に出す。窓が背面のときは、パッドを薄くして理由を書く。
    els.status.dataset.mode = playing ? "play" : "typing";
    els.mode.textContent = t(playing ? "play.mode.play" : "play.mode.typing");
    const background = state.open && !playing;
    els.reason.hidden = !background;
    els.reason.textContent = background ? t("play.reason.background") : "";
    els.pads.dataset.dimmed = String(background);

    // オフ中は、キー入力では鳴らない。「オンにする」を出す（パッドの試聴はオフ中でも鳴る）。
    els.off.hidden = enabled;

    renderPads();
  }

  // ==========================================================================
  // 起動
  // ==========================================================================

  function bindKeys() {
    // 演奏の区画が開いている間は、キーのイベントを画面で一切使わせない（既定動作ごと止める）。
    // capture で最初に受けて止めるので、フォーカスがどこにあっても（パッド・タブ）届かない。
    document.addEventListener(
      "keydown",
      (event) => {
        if (!state.open) return;
        event.preventDefault();
        event.stopPropagation();
      },
      true,
    );
  }

  async function init(ctx) {
    state.ctx = ctx;
    els.status = $("play-status");
    els.mode = $("play-mode");
    els.reason = $("play-reason");
    els.off = $("play-off");
    els.turnOn = $("play-turn-on");
    els.pads = $("pads");

    buildPads();
    bindKeys();
    els.turnOn.addEventListener("click", () => ctx.sendUpdate({ enabled: true }));

    // 窓の前後が変わったとき、Rust に今の状態を聞き直す（知らせが届かなかったときの保険）。
    window.addEventListener("focus", () => {
      if (state.open) syncOpenToRust();
    });
    window.addEventListener("blur", () => {
      if (!state.open) return;
      state.playing = false;
      clearLit();
      render();
    });

    // 受け取るのはイベントだけ（Rust へ送るイベントの権限は無い）。失敗しても、パッドのクリックは使える。
    try {
      await window.drumclackBridge.listen(EVENT_SOUND_PLAYED, onSoundPlayed);
      await window.drumclackBridge.listen(EVENT_MODE_CHANGED, onModeChanged);
    } catch (err) {
      console.error("Rust からの知らせを受け取れません", err);
    }
    render();
  }

  window.drumclackPlay = {
    init: async (ctx) => {
      try {
        await init(ctx);
      } catch (err) {
        console.error("演奏の区画を起動できませんでした", err);
      }
    },
    render: () => {
      try {
        render();
      } catch (err) {
        console.error("演奏の区画で問題が起きました", err);
      }
    },
    /** 演奏の区画を開いた／閉じた（main.js の区画の切り替えから呼ぶ）。 */
    setOpen,
  };
})();
