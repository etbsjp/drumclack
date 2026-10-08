// 演奏の区画の検査。Rust の代わりに差し替えた `window.__TAURI__` で、素のブラウザに web/ を開く。
// Rust から届くイベントは、helpers.js の emitFromRust で「届いたことにする」。
// 文言ではなく data-testid で要素を選ぶ（言語を切り替えても壊れないように）。

const {
  test,
  expect,
  DEFAULT_SETTINGS,
  openApp,
  updateCalls,
  previewCalls,
  allCalls,
  emitFromRust,
  setRustWindowFocus,
  readWeb,
} = require("./helpers");

const ja = JSON.parse(readWeb("i18n/ja.json"));
const en = JSON.parse(readWeb("i18n/en.json"));

const id = (page, testId) => page.getByTestId(testId);

/** このテストが使う演奏用の配置（既定の配置とは別。既定の配置を決め直しても、このテストは変わらない）。 */
const TEST_LAYOUT = {
  KeyA: "kick",
  Space: "kick",
  KeyS: "snare",
  KeyD: "rim",
  KeyF: "clap",
  KeyJ: "hat_closed",
  KeyK: "hat_open",
  KeyL: "tom_high",
  Semicolon: "tom_low",
};
const SOUNDS = ["kick", "snare", "hat_closed", "hat_open", "clap", "rim", "tom_low", "tom_high"];

/** 演奏の区画を開く。 */
async function openPlay(page, settings = {}, options = {}) {
  const play = settings.play || { groups: {}, keys: TEST_LAYOUT };
  await openApp(page, { settings: { ...DEFAULT_SETTINGS, ...settings, play }, ...options });
  await id(page, "tab-play").click();
  await expect(id(page, "play")).toBeVisible();
}

/** Rust に知らせた「演奏の画面の開閉」の呼び出し（引数の open の並び）。 */
async function openCalls(page) {
  return (await allCalls(page)).filter((call) => call.cmd === "set_play_view_open").map((call) => call.args.open);
}

/** パッドを実際のマウスで押す（pointerdown → pointerup → click の順で届く）。 */
async function pressPad(page, sound) {
  await id(page, `pad-${sound}`).hover();
  await page.mouse.down();
  await page.mouse.up();
}

// ============================================================================
// パッド8個
// ============================================================================

test.describe("パッド", () => {
  test("8音ぶんのパッドがあり、演奏用の割り当てのキーが書いてある", async ({ page }) => {
    await openPlay(page);

    for (const sound of SOUNDS) {
      await expect(id(page, `pad-${sound}`)).toBeVisible();
    }
    await expect(page.locator('[data-testid="pads"] .pad')).toHaveCount(8);

    // 上の配置（キックは A とスペースの2つ）。
    const keys = async (sound) => id(page, `pad-keys-${sound}`).locator(".pad-key").allTextContents();
    expect(await keys("kick")).toEqual(["A", "Space"]);
    expect(await keys("snare")).toEqual(["S"]);
    expect(await keys("rim")).toEqual(["D"]);
    expect(await keys("clap")).toEqual(["F"]);
    expect(await keys("hat_closed")).toEqual(["J"]);
    expect(await keys("hat_open")).toEqual(["K"]);
    expect(await keys("tom_high")).toEqual(["L"]);
    expect(await keys("tom_low")).toEqual([";"]);
  });

  test("割り当てを変えると、パッドのキーも変わる（多いときは「ほか N 個」にまとめる）", async ({ page }) => {
    await openPlay(page, { play: { groups: { letters: "kick", space: "kick" }, keys: { KeyJ: "snare" } } });

    // 文字キー全体がキック（J を除く25個）＋スペースで、先頭3つだけ出して残りは個数にする。
    await expect(id(page, "pad-keys-kick").locator(".pad-key")).toHaveCount(3);
    await expect(id(page, "pad-keys-kick").locator(".pad-more")).toHaveText(ja["play.pad.more"].replace("{count}", "23"));
    await expect(id(page, "pad-keys-snare").locator(".pad-key")).toHaveText(["J"]);
    // どのキーも割り当てていない音は、そう書く。
    await expect(id(page, "pad-keys-hat_closed").locator(".pad-more")).toHaveText(ja["play.pad.noKey"]);
  });

  test("パッドを押すと（pointerdown）、その音の試聴が1回呼ばれ、パッドが光る", async ({ page }) => {
    await openPlay(page);

    await pressPad(page, "snare");

    expect(await previewCalls(page)).toEqual([{ sound: "snare" }]);
    await expect(id(page, "pad-snare")).toHaveAttribute("data-lit", "true");
    // 少しして消える。
    await expect(id(page, "pad-snare")).toHaveAttribute("data-lit", "false");
  });

  test("鳴らすのは pointerdown だけ。click だけ（キーボードの操作など）では鳴らない", async ({ page }) => {
    await openPlay(page);

    await id(page, "pad-kick").evaluate((button) => button.click());
    // マウスの右ボタンでも鳴らない。
    await id(page, "pad-kick").click({ button: "right" });

    expect(await previewCalls(page)).toEqual([]);
  });

  test("試聴に失敗しても、画面は止まらない", async ({ page }) => {
    await openPlay(page, {}, { failPreview: true });
    page.on("console", () => {});

    await pressPad(page, "kick");

    await expect(id(page, "pad-kick")).toHaveAttribute("data-lit", "true");
    await expect(id(page, "play-mode")).toBeVisible();
  });
});

// ============================================================================
// 発音の通知（Rust → 画面）
// ============================================================================

test.describe("発音の通知", () => {
  test("鳴った音の名前が届くと、その音のパッドだけが光る（試聴は呼ばない）", async ({ page }) => {
    await openPlay(page);

    await emitFromRust(page, "drum-played", "hat_open");

    await expect(id(page, "pad-hat_open")).toHaveAttribute("data-lit", "true");
    for (const sound of SOUNDS.filter((sound) => sound !== "hat_open")) {
      await expect(id(page, `pad-${sound}`)).toHaveAttribute("data-lit", "false");
    }
    expect(await previewCalls(page)).toEqual([]);
    await expect(id(page, "pad-hat_open")).toHaveAttribute("data-lit", "false");
  });

  test("知らない名前や文字列でない値は無視する", async ({ page }) => {
    await openPlay(page);

    for (const value of ["no_such_sound", "", null, 42, { sound: "kick" }, ["kick"]]) {
      await emitFromRust(page, "drum-played", value);
    }

    for (const sound of SOUNDS) {
      await expect(id(page, `pad-${sound}`)).toHaveAttribute("data-lit", "false");
    }
  });

  test("演奏の区画を開いていないときに届いた通知では光らない", async ({ page }) => {
    await openPlay(page);
    await id(page, "tab-settings").click();

    await emitFromRust(page, "drum-played", "kick");
    await id(page, "tab-play").click();

    await expect(id(page, "pad-kick")).toHaveAttribute("data-lit", "false");
  });

  test("画面が受け取るイベントは、鳴った音の名前と、演奏用の割り当てを使うかの2つだけ", async ({ page }) => {
    await openPlay(page);

    const names = await page.evaluate(() => Object.keys(window.__fake.listeners).sort());
    expect(names).toEqual(["drum-played", "play-mode-changed"]);
  });

  test("光ったことは読み上げない（パッドは読み上げ領域の外で、状態の文も打鍵では変わらない）", async ({ page }) => {
    await openPlay(page);

    const insideLiveRegion = await id(page, "pads").evaluate(
      (el) => el.closest('[aria-live], [role="status"], [role="alert"], [role="log"]') !== null,
    );
    expect(insideLiveRegion).toBe(false);

    const before = await id(page, "play-status").innerText();
    for (const sound of SOUNDS) {
      await emitFromRust(page, "drum-played", sound);
    }
    expect(await id(page, "play-status").innerText()).toBe(before);
  });
});

// ============================================================================
// 画面側のキーの既定動作を止める
// ============================================================================

test.describe("キーの既定動作", () => {
  test("スペースや Enter を押しても、フォーカス中のパッドは押されない", async ({ page }) => {
    await openPlay(page);
    await id(page, "pad-kick").focus();
    await expect(id(page, "pad-kick")).toBeFocused();
    // パッドが「押された」ことは click の発生で分かる（止めていなければ、ブラウザが Space・Enter を click に変える）。
    await id(page, "pad-kick").evaluate((button) => {
      window.__padClicks = 0;
      button.addEventListener("click", () => (window.__padClicks += 1));
    });

    await page.keyboard.press("Space");
    await page.keyboard.press("Enter");

    expect(await page.evaluate(() => window.__padClicks)).toBe(0);
    expect(await previewCalls(page)).toEqual([]);
    await expect(id(page, "pad-kick")).toHaveAttribute("data-lit", "false");
  });

  test("演奏の区画では keydown を既定動作ごと止め、どこにも伝えない", async ({ page }) => {
    await openPlay(page);
    await id(page, "pad-snare").focus();
    // 一番外側（window の capture）で見る。止めるのは document の capture なので、その手前で見える。
    await page.evaluate(() => {
      window.__keys = [];
      window.addEventListener("keydown", (event) => window.__keys.push(event), true);
      window.__reached = 0;
      document.body.addEventListener("keydown", () => (window.__reached += 1));
    });

    for (const key of ["Space", "Enter", "KeyA", "Tab", "ArrowRight", "Escape", "Backspace"]) {
      await page.keyboard.press(key);
    }

    const result = await page.evaluate(() => ({
      prevented: window.__keys.map((event) => event.defaultPrevented),
      reached: window.__reached,
    }));
    expect(result.prevented).toHaveLength(7);
    expect(result.prevented.every(Boolean)).toBe(true);
    expect(result.reached).toBe(0);
    // Tab でも焦点は動かない。
    await expect(id(page, "pad-snare")).toBeFocused();
  });

  test("ほかの区画に移ると、キーは今までどおり使える", async ({ page }) => {
    await openPlay(page);
    await id(page, "tab-settings").click();
    await id(page, "tab-settings").focus();
    await page.evaluate(() => {
      window.__reached = 0;
      document.body.addEventListener("keydown", () => (window.__reached += 1));
    });

    await page.keyboard.press("ArrowRight");

    await expect(id(page, "panel-assign")).toBeVisible();
    expect(await page.evaluate(() => window.__reached)).toBeGreaterThan(0);
  });

  test("演奏の区画に入力欄を置いていない", async ({ page }) => {
    await openPlay(page);
    const fields = await id(page, "panel-play").locator("input, textarea, select, [contenteditable]").count();
    expect(fields).toBe(0);
  });
});

// ============================================================================
// 状態の表示（いま演奏用／タイピング用）
// ============================================================================

test.describe("状態の表示", () => {
  test("演奏の区画を開くと Rust に知らせ、演奏用と表示する。閉じると知らせて、タイピング用に戻る", async ({ page }) => {
    await openPlay(page);

    await expect(id(page, "play-mode")).toHaveText(ja["play.mode.play"]);
    await expect(id(page, "play-status")).toHaveAttribute("data-mode", "play");
    await expect(id(page, "play-reason")).toBeHidden();
    await expect(id(page, "pads")).toHaveAttribute("data-dimmed", "false");
    expect(await openCalls(page)).toEqual([true]);

    await id(page, "tab-settings").click();
    expect(await openCalls(page)).toEqual([true, false]);
    await id(page, "tab-play").click();
    await expect(id(page, "play-mode")).toHaveText(ja["play.mode.play"]);
    expect(await openCalls(page)).toEqual([true, false, true]);
  });

  test("演奏の区画に入るまでは、Rust に何も知らせない", async ({ page }) => {
    await openApp(page);
    await id(page, "tab-assign").click();
    // 演奏の区画に入るまで、「開いた」はもちろん「閉じた」とも知らせない（もともと閉じている）。
    expect(await openCalls(page)).toEqual([]);
  });

  test("窓が背面になると、タイピング用と表示して、パッドを薄くし、理由を書く。前面に戻ると演奏用に戻る", async ({ page }) => {
    await openPlay(page);
    await expect(id(page, "play-mode")).toHaveText(ja["play.mode.play"]);

    // 別のアプリに切り替えた（Rust が窓のフォーカスを外し、知らせが届く）。
    await setRustWindowFocus(page, false);

    await expect(id(page, "play-mode")).toHaveText(ja["play.mode.typing"]);
    await expect(id(page, "play-status")).toHaveAttribute("data-mode", "typing");
    await expect(id(page, "pads")).toHaveAttribute("data-dimmed", "true");
    await expect(id(page, "play-reason")).toBeVisible();
    await expect(id(page, "play-reason")).toHaveText(ja["play.reason.background"]);
    const opacity = await id(page, "pad-kick").evaluate((el) => Number(getComputedStyle(el).opacity));
    expect(opacity).toBeLessThan(1);

    await setRustWindowFocus(page, true);

    await expect(id(page, "play-mode")).toHaveText(ja["play.mode.play"]);
    await expect(id(page, "pads")).toHaveAttribute("data-dimmed", "false");
    await expect(id(page, "play-reason")).toBeHidden();
  });

  test("窓のフォーカスが外れたら、知らせを待たずにタイピング用の表示にし、戻ったら Rust に聞き直す", async ({ page }) => {
    await openPlay(page);
    await expect(id(page, "play-mode")).toHaveText(ja["play.mode.play"]);
    const asked = (await openCalls(page)).length;

    await page.evaluate(() => window.dispatchEvent(new Event("blur")));
    await expect(id(page, "play-mode")).toHaveText(ja["play.mode.typing"]);
    // 窓が背面のときに光っていたパッドは消える。
    await expect(id(page, "pads")).toHaveAttribute("data-dimmed", "true");

    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await expect(id(page, "play-mode")).toHaveText(ja["play.mode.play"]);
    expect((await openCalls(page)).length).toBe(asked + 1);
  });

  test("言語を切り替えると、演奏の区画の文言も替わる", async ({ page }) => {
    await openPlay(page, { language: "en" });

    await expect(id(page, "play-mode")).toHaveText(en["play.mode.play"]);
    await expect(id(page, "pad-snare")).toContainText("Snare");
    await expect(id(page, "pad-snare")).toHaveAttribute(
      "aria-label",
      en["play.pad.label"].replace("{sound}", "Snare").replace("{keys}", "S"),
    );

    await id(page, "tab-settings").click();
    await id(page, "setting-language").selectOption("ja");
    await id(page, "tab-play").click();
    await expect(id(page, "play-mode")).toHaveText(ja["play.mode.play"]);
    await expect(id(page, "pad-snare")).toContainText("スネア");
  });

  test("オフのあいだは「オンにする」を出し、押すと enabled: true だけを送る", async ({ page }) => {
    await openPlay(page, { enabled: false });

    await expect(id(page, "play-off")).toBeVisible();
    await expect(id(page, "play-turn-on")).toHaveText(ja["play.turnOn"]);
    // オフでも、パッドの試し聴きはできる。
    await pressPad(page, "kick");
    expect(await previewCalls(page)).toEqual([{ sound: "kick" }]);

    await id(page, "play-turn-on").click();

    await expect.poll(async () => (await updateCalls(page)).map((call) => call.args.settings)).toEqual([
      { enabled: true },
    ]);
    await expect(id(page, "play-off")).toBeHidden();
  });

  test("オンのときは「オンにする」を出さない", async ({ page }) => {
    await openPlay(page);
    await expect(id(page, "play-off")).toBeHidden();
  });
});

// ============================================================================
// 光る表現（OS の「動きを減らす」設定）
// ============================================================================

test.describe("動きを減らす設定", () => {
  async function litStyle(page) {
    await emitFromRust(page, "drum-played", "kick");
    await expect(id(page, "pad-kick")).toHaveAttribute("data-lit", "true");
    return id(page, "pad-kick").evaluate((el) => {
      const style = getComputedStyle(el);
      return { transform: style.transform, transition: style.transitionDuration, shadow: style.boxShadow };
    });
  }

  test("通常は縮む動きと遷移がつく", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await openPlay(page);

    const style = await litStyle(page);

    expect(style.transform).not.toBe("none");
    expect(style.transition).not.toBe("0s");
  });

  test("「動きを減らす」では、動きと遷移を止めて、枠と影の変化だけで光る", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await openPlay(page);

    const style = await litStyle(page);

    expect(style.transform).toBe("none");
    expect(style.transition).toBe("0s");
    // 光ったことは、動きがなくても影の変化で分かる。
    expect(style.shadow).not.toBe("none");
  });
});

// ============================================================================
// 守ること（設計の正本 #9）
// ============================================================================

test.describe("守ること", () => {
  const source = () => readWeb("play.js").replace(/\/\/.*$/gm, "");

  test("Rust へイベントを送らない。受け取るのは決めた2つだけ", async () => {
    expect(source()).not.toMatch(/\.emit\(/);
    const listened = [...source().matchAll(/\.listen\(\s*([A-Z_]+)/g)].map((match) => match[1]);
    expect(listened.sort()).toEqual(["EVENT_MODE_CHANGED", "EVENT_SOUND_PLAYED"]);
  });

  test("音の名前を保存せず、ログにも出さない", async () => {
    const code = source();
    expect(code).not.toMatch(/localStorage|sessionStorage|indexedDB|document\.cookie/);
    expect(code).not.toMatch(/console\.(log|info|debug|warn)\(/);
    // エラーのログに、受け取った音の名前を渡さない。
    for (const line of code.split("\n").filter((line) => /console\.error\(/.test(line))) {
      expect(line).not.toMatch(/sound|name/i);
    }
  });

  test("パッドを鳴らすのは pointerdown だけ", async () => {
    const code = source();
    expect(code).toMatch(/addEventListener\("pointerdown"/);
    expect(code).not.toMatch(/addEventListener\("(click|keydown|keyup|keypress)",[^)]*preview/);
    expect(code).not.toMatch(/\bonclick\b/);
  });
});
