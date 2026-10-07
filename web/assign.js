// 割り当ての区画。キーボードの絵・グループ10行の選択欄・個別の上書き・「既定に戻す」を受け持つ。
// Rust との行き来は `bridge.js`（設定の更新は main.js の sendUpdate 経由）、文言は `i18n.js` を通す。
//
// 守ること（設計の正本 #9）:
// - 設定の更新は「変えた項目だけ」を送る（`{ typing: { keys: { KeyA: "kick" } } }`。null で上書きを消す）。
// - 打鍵を画面へ送る経路は作らない。この画面を開いている間に押されたキーを、絵の上で光らせない。
//   キーボードのイベントを受けるのは、絵・選択肢の中にフォーカスがあるときの矢印／Esc だけ
//   （document / window では受けない）。Rust のイベントも listen しない。
// - キーの位置は、音を選ぶことにだけ使う。保存するのは `code` 名と音の名前だけ。

(function () {
  const SOUNDS = ["kick", "snare", "hat_closed", "hat_open", "clap", "rim", "tom_low", "tom_high"];
  const GROUPS = [
    "letters",
    "space",
    "enter",
    "delete",
    "symbols",
    "digits_even",
    "digits_odd",
    "control",
    "navigation",
    "modifiers",
  ];
  const NONE = "none";

  // 対応表のキーは、検査（i18n.spec.js）が拾えるよう、文字列リテラルで並べる（組み立てない）。
  const SOUND_NAME = {
    kick: "sound.kick",
    snare: "sound.snare",
    hat_closed: "sound.hatClosed",
    hat_open: "sound.hatOpen",
    clap: "sound.clap",
    rim: "sound.rim",
    tom_low: "sound.tomLow",
    tom_high: "sound.tomHigh",
    none: "sound.none",
  };
  const SOUND_ABBR = {
    kick: "sound.kick.abbr",
    snare: "sound.snare.abbr",
    hat_closed: "sound.hatClosed.abbr",
    hat_open: "sound.hatOpen.abbr",
    clap: "sound.clap.abbr",
    rim: "sound.rim.abbr",
    tom_low: "sound.tomLow.abbr",
    tom_high: "sound.tomHigh.abbr",
    none: "sound.none.abbr",
  };
  const GROUP_NAME = {
    letters: "group.letters",
    space: "group.space",
    enter: "group.enter",
    delete: "group.delete",
    symbols: "group.symbols",
    digits_even: "group.digitsEven",
    digits_odd: "group.digitsOdd",
    control: "group.control",
    navigation: "group.navigation",
    modifiers: "group.modifiers",
  };
  const SOURCE_NAME = {
    group: "assign.source.group",
    override: "assign.source.override",
    default: "assign.source.default",
  };
  const SET_NAME = { typing: "assign.set.typing", play: "assign.set.play" };

  /** 最初に Tab で入ったとき、フォーカスが止まるキー。 */
  const FIRST_KEY = "KeyA";
  /** 隣のキーとして数えるのに必要な、重なりの長さ（キー1個ぶんを1とする）。 */
  const MIN_OVERLAP = 0.3;
  const EPSILON = 0.01;

  const state = {
    ctx: null,
    data: null, // key-data.json（グループと既定の音）
    loadError: false,
    set: "typing", // 編集している組（画面だけの状態。保存しない）
    layout: null, // 今描いている配列（"jis" | "us"）
    keyEls: new Map(), // code → ボタン
    geometry: [], // 今の配列のキーの位置（矢印キーでの移動に使う）
    current: FIRST_KEY, // 矢印キーで動くキー（Tab の止まり先）
    menu: null, // 開いている選択肢 { code }
    confirming: false, // 「既定に戻す」の確認を出しているか
    feedback: null, // { type: "reset" | "undone", set, count }
    undo: null, // 「取り消す」で戻す内容 { set, groups, keys, count }
    groupsSignature: "",
  };

  const $ = (testId) => document.querySelector(`[data-testid="${testId}"]`);
  const els = {};

  const t = (key, params) => state.ctx.i18n.t(key, params);

  function element(tag, className, attrs) {
    const el = document.createElement(tag);
    if (className) el.className = className;
    if (attrs) {
      for (const [name, value] of Object.entries(attrs)) {
        if (name === "text") el.textContent = value;
        else el.setAttribute(name, value);
      }
    }
    return el;
  }

  // ==========================================================================
  // 割り当ての読み取り（Rust の assignment.rs と同じ優先順位）
  // ==========================================================================

  function assignmentsOf(set) {
    const snapshot = state.ctx.getSnapshot();
    const section = snapshot && snapshot.settings && snapshot.settings[set];
    return {
      groups: knownEntries((section && section.groups) || {}, (name) => GROUPS.includes(name)),
      keys: knownEntries((section && section.keys) || {}, (name) => state.data && name in state.data.groups),
    };
  }

  /**
   * 知らない音の名前・グループ名・キー名の項目は無視する（その項目だけ既定の表示になる）。
   * Rust も読み取り時に同じ扱いだが、手編集や将来の変更で届いても、画面全体が落ちないようにする。
   */
  function knownEntries(values, isKnownName) {
    const known = {};
    for (const [name, sound] of Object.entries(values)) {
      if (isKnownName(name) && (sound === NONE || SOUNDS.includes(sound))) {
        known[name] = sound;
      }
    }
    return known;
  }

  function groupDefault(set, group) {
    return state.data.groupDefaults[set][group];
  }

  /** グループの今の音（グループの指定 ＞ 既定）。 */
  function groupSound(set, group, assignments) {
    return assignments.groups[group] || groupDefault(set, group);
  }

  /** キーの音と、その決まり方。個別の上書き ＞ グループの指定 ＞ キーごとの既定 ＞ グループの既定。 */
  function resolveKey(set, code, assignments, ignoreOverride) {
    const group = state.data.groups[code];
    if (!ignoreOverride && assignments.keys[code]) {
      return { sound: assignments.keys[code], source: "override" };
    }
    if (assignments.groups[group]) {
      return { sound: assignments.groups[group], source: "group" };
    }
    const keyDefault = state.data.keyDefaults[set][code];
    if (keyDefault) {
      return { sound: keyDefault, source: "default" };
    }
    return { sound: groupDefault(set, group), source: "group" };
  }

  function keyDisplayName(code) {
    const { layouts, NAMES } = window.drumclackLayouts;
    let label = NAMES[code];
    if (!label) {
      for (const keys of Object.values(layouts)) {
        const found = keys.find((key) => key.code === code);
        if (found) {
          label = found.label;
          break;
        }
      }
    }
    label = label || code;
    return code.startsWith("Numpad") ? t("assign.key.numpad", { key: label }) : label;
  }

  const soundName = (sound) => t(SOUND_NAME[sound]);

  // ==========================================================================
  // キーボードの絵
  // ==========================================================================

  function buildKeyboard(layout) {
    const { layouts, width, height } = window.drumclackLayouts;
    const keys = layouts[layout];
    state.layout = layout;
    state.keyEls = new Map();
    state.geometry = keys.map((key) => ({ code: key.code, x: key.x, y: key.y, w: key.w, h: key.h }));
    if (!state.geometry.some((key) => key.code === state.current)) {
      state.current = FIRST_KEY;
    }

    const board = els.keyboard;
    board.dataset.layout = layout;
    board.style.setProperty("--board-w", String(width));
    board.style.setProperty("--board-h", String(height));
    board.replaceChildren();
    for (const key of keys) {
      const button = element("button", "key", {
        type: "button",
        tabindex: "-1",
        "aria-haspopup": "menu",
        "aria-expanded": "false",
      });
      button.dataset.code = key.code;
      button.dataset.testid = `key-${key.code}`;
      button.style.setProperty("--x", String(key.x));
      button.style.setProperty("--y", String(key.y));
      button.style.setProperty("--w", String(key.w));
      button.style.setProperty("--h", String(key.h));
      button.append(element("span", "key-label", { text: key.label }), element("span", "key-sound"));
      board.appendChild(button);
      state.keyEls.set(key.code, button);
    }
    setCurrent(state.current);
  }

  function setCurrent(code) {
    const previous = state.keyEls.get(state.current);
    if (previous) previous.tabIndex = -1;
    state.current = code;
    const next = state.keyEls.get(code);
    if (next) next.tabIndex = 0;
  }

  function renderKeys(set, assignments) {
    for (const [code, button] of state.keyEls) {
      const info = resolveKey(set, code, assignments, false);
      button.dataset.sound = info.sound;
      button.dataset.override = String(info.source === "override");
      button.querySelector(".key-sound").textContent = t(SOUND_ABBR[info.sound]);
      button.setAttribute(
        "aria-label",
        t("assign.key.label", {
          key: keyDisplayName(code),
          sound: soundName(info.sound),
          source: t(SOURCE_NAME[info.source]),
        }),
      );
    }
  }

  /** 横にあふれたときだけ、絵の枠を Tab で止まれる（スクロールできる）ようにする。 */
  function updateScroller() {
    const scroller = els.scroller;
    if (scroller.scrollWidth > scroller.clientWidth) {
      scroller.tabIndex = 0;
    } else {
      scroller.removeAttribute("tabindex");
    }
  }

  // 矢印キーでの移動。絵の上の位置関係で、その向きにいちばん近いキーへ動く。
  function overlap(aStart, aSize, bStart, bSize) {
    return Math.min(aStart + aSize, bStart + bSize) - Math.max(aStart, bStart);
  }

  function neighbor(fromCode, direction) {
    const from = state.geometry.find((key) => key.code === fromCode);
    if (!from) return null;
    const horizontal = direction === "ArrowLeft" || direction === "ArrowRight";
    let best = null;
    let bestGap = Infinity;
    let bestOff = Infinity;
    for (const key of state.geometry) {
      if (key === from) continue;
      let gap;
      let off;
      if (horizontal) {
        if (overlap(from.y, from.h, key.y, key.h) < MIN_OVERLAP) continue;
        gap = direction === "ArrowRight" ? key.x - (from.x + from.w) : from.x - (key.x + key.w);
        off = Math.abs(key.y + key.h / 2 - (from.y + from.h / 2));
      } else {
        if (overlap(from.x, from.w, key.x, key.w) < MIN_OVERLAP) continue;
        gap = direction === "ArrowDown" ? key.y - (from.y + from.h) : from.y - (key.y + key.h);
        off = Math.abs(key.x + key.w / 2 - (from.x + from.w / 2));
      }
      if (gap < -EPSILON) continue;
      if (gap < bestGap - EPSILON || (Math.abs(gap - bestGap) <= EPSILON && off < bestOff)) {
        best = key;
        bestGap = gap;
        bestOff = off;
      }
    }
    return best && best.code;
  }

  function onBoardKeydown(event) {
    const button = event.target.closest && event.target.closest(".key");
    if (!button || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
    event.preventDefault();
    const next = neighbor(button.dataset.code, event.key);
    if (next) state.keyEls.get(next).focus();
  }

  // ==========================================================================
  // 選択肢（キーをクリック／Enter で開く）
  // ==========================================================================

  function menuChoices(code) {
    const set = state.set;
    const assignments = assignmentsOf(set);
    const currentOverride = assignments.keys[code] || null;
    const followed = resolveKey(set, code, assignments, true);
    const choices = SOUNDS.concat([NONE]).map((sound) => ({
      id: sound,
      sound,
      text: soundName(sound),
      checked: currentOverride === sound,
    }));
    choices.push({
      id: "follow",
      sound: followed.sound,
      text: t("assign.choice.follow", { sound: soundName(followed.sound) }),
      checked: currentOverride == null,
    });
    return choices;
  }

  function openMenu(code) {
    if (!state.ctx.getSnapshot()) return;
    closeMenu(false);
    state.menu = { code };
    state.keyEls.get(code).setAttribute("aria-expanded", "true");
    renderMenu();
    positionMenu(state.keyEls.get(code));
    const checked = els.menu.querySelector('[aria-checked="true"]');
    (checked || els.menu.querySelector("button")).focus();
    document.addEventListener("pointerdown", onOutsidePointerDown, true);
  }

  function closeMenu(restoreFocus) {
    if (!state.menu) return;
    const button = state.keyEls.get(state.menu.code);
    state.menu = null;
    if (button) button.setAttribute("aria-expanded", "false");
    els.menu.hidden = true;
    els.menu.replaceChildren();
    document.removeEventListener("pointerdown", onOutsidePointerDown, true);
    if (restoreFocus && button) button.focus();
  }

  function onOutsidePointerDown(event) {
    if (state.menu && !els.menu.contains(event.target)) closeMenu(false);
  }

  function renderMenu() {
    if (!state.menu) return;
    const { code } = state.menu;
    els.menu.setAttribute("aria-label", t("assign.menu.label", { key: keyDisplayName(code) }));
    els.menu.hidden = false;
    const focusedId =
      document.activeElement && els.menu.contains(document.activeElement)
        ? document.activeElement.dataset.choice
        : null;
    els.menu.replaceChildren();
    for (const choice of menuChoices(code)) {
      const item = element("button", "menu-item", {
        type: "button",
        role: "menuitemradio",
        "aria-checked": String(choice.checked),
        tabindex: "-1",
      });
      item.dataset.choice = choice.id;
      item.dataset.testid = `choice-${choice.id}`;
      item.dataset.sound = choice.sound;
      item.append(
        element("span", "chip", { text: t(SOUND_ABBR[choice.sound]), "aria-hidden": "true" }),
        element("span", "menu-item-text", { text: choice.text }),
      );
      item.addEventListener("click", () => choose(code, choice.id));
      els.menu.appendChild(item);
    }
    if (focusedId) {
      const again = els.menu.querySelector(`[data-choice="${focusedId}"]`);
      if (again) again.focus();
    }
  }

  function positionMenu(anchor) {
    const menu = els.menu;
    const rect = anchor.getBoundingClientRect();
    const margin = 8;
    const width = menu.offsetWidth;
    const height = menu.offsetHeight;
    const left = Math.min(Math.max(margin, rect.left), window.innerWidth - width - margin);
    // 下に出して収まらなければ上へ。それでも収まらなければ、窓に収まる位置へ寄せる（中は縦にスクロール）。
    let top = rect.bottom + 4;
    if (top + height > window.innerHeight - margin) {
      top = rect.top - height - 4;
    }
    top = Math.min(Math.max(margin, top), Math.max(margin, window.innerHeight - height - margin));
    menu.style.setProperty("--menu-left", `${left}px`);
    menu.style.setProperty("--menu-top", `${top}px`);
  }

  function onMenuKeydown(event) {
    const items = Array.from(els.menu.querySelectorAll(".menu-item"));
    const index = items.indexOf(document.activeElement);
    if (event.key === "Escape" || event.key === "Tab") {
      event.preventDefault();
      closeMenu(true);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      items[(index + 1) % items.length].focus();
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      items[(index - 1 + items.length) % items.length].focus();
    } else if (event.key === "Home") {
      event.preventDefault();
      items[0].focus();
    } else if (event.key === "End") {
      event.preventDefault();
      items[items.length - 1].focus();
    }
  }

  /** 選んだ音を、そのキーの個別の上書きにする（「グループに従う」は上書きを消す）。選んだらすぐ試聴する。 */
  function choose(code, choiceId) {
    const set = state.set;
    const assignments = assignmentsOf(set);
    const follow = choiceId === "follow";
    const sound = follow ? resolveKey(set, code, assignments, true).sound : choiceId;

    closeMenu(true);
    clearFeedback();
    if (!follow || assignments.keys[code] != null) {
      state.ctx.sendUpdate({ [set]: { keys: { [code]: follow ? null : choiceId } } });
    }
    preview(sound);
    render();
  }

  function preview(sound) {
    // 無音は鳴らさない。試聴できなくても（音声デバイスが無いなど）割り当ての変更は止めない。
    if (sound === NONE) return;
    Promise.resolve()
      .then(() => state.ctx.bridge.previewSound(sound))
      .catch((err) => {
        console.error("試聴できませんでした", err);
      });
  }

  // ==========================================================================
  // グループ10行の選択欄（2列×5行）
  // ==========================================================================

  function buildGroups() {
    els.groups.replaceChildren();
    for (const group of GROUPS) {
      const item = element("li", "group-row");
      item.dataset.group = group;
      const selectId = `group-select-${group}`;
      const label = element("label", "group-label", { for: selectId });
      label.dataset.testid = `group-label-${group}`;
      const chip = element("span", "chip", { "aria-hidden": "true" });
      chip.dataset.testid = `group-chip-${group}`;
      const select = element("select", "group-select", { id: selectId });
      select.dataset.testid = selectId;
      select.dataset.group = group;
      select.addEventListener("change", () => onGroupChange(group, select.value));
      item.append(chip, label, select);
      els.groups.appendChild(item);
    }
  }

  function renderGroups(set, assignments) {
    // 言語か組が変わったときだけ、選択肢の文言を作り直す（既定の印が組ごとに違うため）。
    const signature = `${state.ctx.i18n.language}:${set}`;
    const rebuild = signature !== state.groupsSignature;
    state.groupsSignature = signature;

    for (const row of els.groups.children) {
      const group = row.dataset.group;
      const select = row.querySelector("select");
      const current = groupSound(set, group, assignments);
      if (rebuild) {
        row.querySelector("label").textContent = t(GROUP_NAME[group]);
        select.replaceChildren();
        for (const sound of SOUNDS.concat([NONE])) {
          select.appendChild(
            element("option", null, {
              value: sound,
              text:
                sound === groupDefault(set, group)
                  ? t("assign.groups.default", { sound: soundName(sound) })
                  : soundName(sound),
            }),
          );
        }
      }
      select.value = current;
      row.dataset.changed = String(Boolean(assignments.groups[group]));
      const chip = row.querySelector(".chip");
      chip.dataset.sound = current;
      chip.textContent = t(SOUND_ABBR[current]);
    }
  }

  function onGroupChange(group, value) {
    const set = state.set;
    clearFeedback();
    // 既定と同じ音に戻したときは、上書きそのものを消す（保存するのは既定から変えた分だけ）。
    const patch = value === groupDefault(set, group) ? null : value;
    state.ctx.sendUpdate({ [set]: { groups: { [group]: patch } } });
    render();
  }

  // ==========================================================================
  // 個別に変えたキーの一覧
  // ==========================================================================

  function overrideCodes(assignments) {
    const order = state.geometry.map((key) => key.code);
    const rank = (code) => {
      const index = order.indexOf(code);
      return index < 0 ? Infinity : index;
    };
    return Object.keys(assignments.keys)
      .filter((code) => state.data.groups[code])
      .sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
  }

  function renderOverrides(set, assignments) {
    const codes = overrideCodes(assignments);
    els.overridesTitle.textContent = t("assign.overrides.title", { count: codes.length });
    els.overridesNone.hidden = codes.length > 0;
    els.overrides.hidden = codes.length === 0;

    const focusedCode =
      document.activeElement && els.overrides.contains(document.activeElement)
        ? document.activeElement.dataset.clear
        : null;
    els.overrides.replaceChildren();
    for (const code of codes) {
      const sound = assignments.keys[code];
      const name = keyDisplayName(code);
      const item = element("li", "override-item");
      item.dataset.testid = "override-item";
      item.dataset.code = code;
      const chip = element("span", "chip", { text: t(SOUND_ABBR[sound]), "aria-hidden": "true" });
      chip.dataset.sound = sound;
      const clear = element("button", "issue-button", {
        type: "button",
        text: t("assign.overrides.clear"),
        "aria-label": t("assign.overrides.clear.label", { key: name }),
      });
      clear.dataset.testid = `override-clear-${code}`;
      clear.dataset.clear = code;
      clear.addEventListener("click", () => {
        clearFeedback();
        state.ctx.sendUpdate({ [set]: { keys: { [code]: null } } });
        // 一覧から消えるので、焦点が body に落ちないよう、絵の同じキーへ戻す。
        const button = state.keyEls.get(code);
        if (button) button.focus();
        render();
      });
      item.append(
        chip,
        element("span", "override-key", { text: name }),
        element("span", "override-sound", { text: soundName(sound) }),
        clear,
      );
      els.overrides.appendChild(item);
    }
    if (focusedCode) {
      const again = els.overrides.querySelector(`[data-clear="${focusedCode}"]`);
      if (again) again.focus();
    }
  }

  // ==========================================================================
  // 編集する組・配列の切り替え
  // ==========================================================================

  function bindSegmented(group, onSelect) {
    const buttons = Array.from(group.querySelectorAll('[role="radio"]'));
    for (const button of buttons) {
      button.addEventListener("click", () => onSelect(button));
      button.addEventListener("keydown", (event) => {
        const index = buttons.indexOf(button);
        let next = null;
        if (event.key === "ArrowRight" || event.key === "ArrowDown") next = buttons[(index + 1) % buttons.length];
        if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
          next = buttons[(index - 1 + buttons.length) % buttons.length];
        }
        if (next) {
          event.preventDefault();
          onSelect(next);
          next.focus();
        }
      });
    }
    return buttons;
  }

  function setSegmented(buttons, selected) {
    for (const button of buttons) {
      const on = button === selected;
      button.setAttribute("aria-checked", String(on));
      button.tabIndex = on ? 0 : -1;
    }
  }

  function selectSet(set) {
    if (set === state.set) return;
    closeMenu(false);
    state.set = set;
    clearFeedback();
    render();
  }

  // ==========================================================================
  // 既定に戻す（組ごと・確認つき・直後に取り消せる）
  // ==========================================================================

  function resetCounts(assignments) {
    const groups = Object.keys(assignments.groups).length;
    const keys = Object.keys(assignments.keys).length;
    return { groups, keys, count: groups + keys };
  }

  function clearFeedback() {
    state.feedback = null;
    state.undo = null;
    state.confirming = false;
  }

  function startReset() {
    const { count } = resetCounts(assignmentsOf(state.set));
    if (count === 0 || !state.ctx.getSnapshot()) return;
    closeMenu(false);
    clearFeedback();
    state.confirming = true;
    render();
    els.resetCancel.focus();
  }

  function cancelReset() {
    state.confirming = false;
    render();
    els.reset.focus();
  }

  function confirmReset() {
    const set = state.set;
    const assignments = assignmentsOf(set);
    const { count } = resetCounts(assignments);
    const clearAll = (values) => Object.fromEntries(Object.keys(values).map((name) => [name, null]));
    state.undo = { set, groups: { ...assignments.groups }, keys: { ...assignments.keys }, count };
    state.feedback = { type: "reset", set, count };
    state.confirming = false;
    state.ctx.sendUpdate({ [set]: { groups: clearAll(assignments.groups), keys: clearAll(assignments.keys) } });
    render();
    els.undo.focus();
  }

  function undoReset() {
    const undo = state.undo;
    if (!undo) return;
    state.ctx.sendUpdate({ [undo.set]: { groups: undo.groups, keys: undo.keys } });
    state.undo = null;
    state.feedback = { type: "undone", set: undo.set, count: undo.count };
    render();
    els.reset.focus();
  }

  function renderReset(set, assignments) {
    const { groups, keys, count } = resetCounts(assignments);
    const setName = t(SET_NAME[set]);
    els.reset.disabled = count === 0;
    els.resetHint.hidden = count !== 0 || state.feedback != null;
    els.resetHint.textContent = t("assign.reset.none", { set: setName });

    els.hint.hidden = state.confirming || state.feedback != null;
    els.confirm.hidden = !state.confirming;
    if (state.confirming) {
      els.confirmText.textContent = t("assign.reset.confirm", { set: setName, groups, keys, count });
    }

    const feedback = state.feedback;
    els.feedback.dataset.hasContent = String(feedback != null);
    els.feedbackText.textContent = feedback
      ? t(feedback.type === "reset" ? "assign.reset.done" : "assign.undo.done", {
          set: t(SET_NAME[feedback.set]),
          count: feedback.count,
        })
      : "";
    els.undo.hidden = !(feedback && feedback.type === "reset" && state.undo);
  }

  // ==========================================================================
  // 描き直し
  // ==========================================================================

  function render() {
    if (!state.ctx) return;
    els.loadError.hidden = !state.loadError;
    els.root.hidden = state.loadError || !state.data;
    if (!state.data) return;

    const snapshot = state.ctx.getSnapshot();
    const settings = snapshot && snapshot.settings;
    els.root.dataset.disabled = String(settings == null);
    const layout = state.ctx.resolveLayout(settings ? settings.keyboard_layout : "auto");
    if (layout !== state.layout) {
      buildKeyboard(layout);
    }

    const assignments = assignmentsOf(state.set);
    setSegmented(
      els.setButtons,
      els.setButtons.find((button) => button.dataset.set === state.set),
    );
    setSegmented(
      els.layoutButtons,
      els.layoutButtons.find((button) => button.dataset.layout === layout),
    );
    renderKeys(state.set, assignments);
    renderGroups(state.set, assignments);
    renderOverrides(state.set, assignments);
    renderReset(state.set, assignments);
    renderMenu();
    updateScroller();
  }

  // ==========================================================================
  // 起動
  // ==========================================================================

  async function loadData() {
    const response = await fetch("./key-data.json");
    if (!response.ok) {
      throw new Error(`key-data.json を読めません: ${response.status}`);
    }
    return response.json();
  }

  /**
   * ctx: { bridge, i18n, getSnapshot(), resolveLayout(setting), sendUpdate(patch) }
   * 読み込みに失敗しても例外は投げない（割り当ての区画だけが使えない表示になる）。
   */
  async function init(ctx) {
    state.ctx = ctx;
    els.root = $("assign");
    els.loadError = $("assign-load-error");
    els.keyboard = $("keyboard");
    els.scroller = $("keyboard-scroll");
    els.menu = $("key-menu");
    els.groups = $("assign-groups");
    els.overridesTitle = $("assign-overrides-title");
    els.overrides = $("assign-overrides");
    els.overridesNone = $("assign-overrides-none");
    els.reset = $("assign-reset");
    els.resetHint = $("assign-reset-hint");
    els.hint = $("assign-hint");
    els.confirm = $("assign-reset-confirm");
    els.confirmText = $("assign-reset-text");
    els.resetCancel = $("assign-reset-cancel");
    els.feedback = $("assign-feedback");
    els.feedbackText = $("assign-feedback-text");
    els.undo = $("assign-undo");

    els.setButtons = bindSegmented($("assign-set"), (button) => selectSet(button.dataset.set));
    els.layoutButtons = bindSegmented($("assign-layout"), (button) => {
      // 配列は絵の見た目だけ。割り当て（code 名と音）は変えない。
      if (button.dataset.layout !== state.layout) {
        state.ctx.sendUpdate({ keyboard_layout: button.dataset.layout });
      }
    });

    els.keyboard.addEventListener("keydown", onBoardKeydown);
    els.keyboard.addEventListener("focusin", (event) => {
      const button = event.target.closest && event.target.closest(".key");
      if (button) setCurrent(button.dataset.code);
    });
    els.keyboard.addEventListener("click", (event) => {
      const button = event.target.closest && event.target.closest(".key");
      if (button) openMenu(button.dataset.code);
    });
    els.menu.addEventListener("keydown", onMenuKeydown);
    els.reset.addEventListener("click", startReset);
    $("assign-reset-confirm-button").addEventListener("click", confirmReset);
    els.resetCancel.addEventListener("click", cancelReset);
    els.confirm.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        cancelReset();
      }
    });
    els.undo.addEventListener("click", undoReset);
    window.addEventListener("resize", () => {
      closeMenu(false);
      updateScroller();
    });

    buildGroups();
    try {
      state.data = await loadData();
    } catch (err) {
      console.error("割り当ての画面の元データを読み込めませんでした", err);
      state.loadError = true;
    }
    // 描くのは、対応表を読み込んだあとに main.js が呼ぶ render から（読み込み前に描くと、文言がキーのままになる）。
  }

  /** この区画の失敗を、設定の区画など、ほかの部分へ広げない。 */
  function contained(fn) {
    return (...args) => {
      try {
        return fn(...args);
      } catch (err) {
        console.error("割り当ての区画で問題が起きました", err);
        return undefined;
      }
    };
  }

  window.drumclackAssign = {
    init: async (ctx) => {
      try {
        await init(ctx);
      } catch (err) {
        console.error("割り当ての区画を起動できませんでした", err);
        state.loadError = true;
        contained(render)();
      }
    },
    render: contained(render),
    /** 区画を切り替えるとき（開いている選択肢を閉じる）。 */
    close: contained(() => closeMenu(false)),
  };
})();
