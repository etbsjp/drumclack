// 割り当ての区画の検査。Rust の代わりに差し替えた `window.__TAURI__` で、素のブラウザに web/ を開く。
// 文言ではなく data-testid / data-code で要素を選ぶ（言語を切り替えても壊れないように）。

const {
  test,
  expect,
  DEFAULT_SETTINGS,
  openApp,
  updateCalls,
  previewCalls,
  currentSettings,
  allCalls,
  readWeb,
} = require("./helpers");

const id = (page, testId) => page.getByTestId(testId);
const key = (page, code) => page.locator(`[data-testid="keyboard"] [data-code="${code}"]`);

/** 割り当ての区画を開く。`settings` の一部だけ書き換えられる。 */
async function openAssign(page, settings = {}, options = {}) {
  await openApp(page, { settings: { ...DEFAULT_SETTINGS, keyboard_layout: "us", ...settings }, ...options });
  await id(page, "tab-assign").click();
  await expect(id(page, "assign")).toBeVisible();
}

async function pickSound(page, code, choice) {
  await key(page, code).click();
  await id(page, `choice-${choice}`).click();
}

const withOverrides = {
  typing: { groups: { letters: "snare" }, keys: { KeyJ: "kick", Numpad5: "none" } },
};

// ============================================================================
// キーをクリックして音を選ぶ
// ============================================================================

test.describe("キーの音を選ぶ", () => {
  test("クリック→音を選ぶと、正しい引数で更新と試聴が呼ばれ、キーの表示が変わる", async ({ page }) => {
    await openAssign(page);
    await expect(key(page, "KeyA")).toHaveAttribute("data-sound", "hat_closed");
    await expect(key(page, "KeyA")).toHaveAttribute("data-override", "false");

    await pickSound(page, "KeyA", "snare");

    expect(await updateCalls(page)).toEqual([
      { cmd: "update_settings", args: { settings: { typing: { keys: { KeyA: "snare" } } } } },
    ]);
    expect(await previewCalls(page)).toEqual([{ sound: "snare" }]);
    await expect(key(page, "KeyA")).toHaveAttribute("data-sound", "snare");
    await expect(key(page, "KeyA")).toHaveAttribute("data-override", "true");
    await expect(id(page, "key-menu")).toBeHidden();
    await expect(key(page, "KeyA")).toBeFocused();
  });

  test("「無音」を選ぶと更新は送るが、試聴は呼ばない", async ({ page }) => {
    await openAssign(page);
    await pickSound(page, "KeyB", "none");

    expect((await updateCalls(page)).map((call) => call.args)).toEqual([
      { settings: { typing: { keys: { KeyB: "none" } } } },
    ]);
    expect(await previewCalls(page)).toEqual([]);
    await expect(key(page, "KeyB")).toHaveAttribute("data-sound", "none");
  });

  test("「グループに従う」で、上書きの印が消え、グループの音に戻る（その音を試聴する）", async ({ page }) => {
    await openAssign(page, withOverrides);
    await expect(key(page, "KeyJ")).toHaveAttribute("data-override", "true");
    await expect(key(page, "KeyJ")).toHaveAttribute("data-sound", "kick");

    await pickSound(page, "KeyJ", "follow");

    expect((await updateCalls(page)).map((call) => call.args)).toEqual([
      { settings: { typing: { keys: { KeyJ: null } } } },
    ]);
    await expect(key(page, "KeyJ")).toHaveAttribute("data-override", "false");
    // グループ（文字キー）の指定は snare。
    await expect(key(page, "KeyJ")).toHaveAttribute("data-sound", "snare");
    expect(await previewCalls(page)).toEqual([{ sound: "snare" }]);
    expect((await currentSettings(page)).typing.keys).toEqual({ Numpad5: "none" });
  });

  test("上書きしていないキーで「グループに従う」を選んでも、更新は送らない", async ({ page }) => {
    await openAssign(page);
    await pickSound(page, "KeyA", "follow");
    expect(await updateCalls(page)).toEqual([]);
  });

  test("試聴に失敗しても、割り当ての変更は反映される", async ({ page }) => {
    await openAssign(page, {}, { failPreview: true });
    await pickSound(page, "KeyA", "rim");
    await expect(key(page, "KeyA")).toHaveAttribute("data-sound", "rim");
    expect(await updateCalls(page)).toHaveLength(1);
  });

  test("選択肢は8音＋無音＋グループに従うで、今の選び方に印が付く", async ({ page }) => {
    await openAssign(page, withOverrides);
    await key(page, "KeyJ").click();
    const items = id(page, "key-menu").locator("[data-choice]");
    expect(await items.evaluateAll((nodes) => nodes.map((node) => node.dataset.choice))).toEqual([
      "kick",
      "snare",
      "hat_closed",
      "hat_open",
      "clap",
      "rim",
      "tom_low",
      "tom_high",
      "none",
      "follow",
    ]);
    await expect(id(page, "choice-kick")).toHaveAttribute("aria-checked", "true");
    await expect(id(page, "choice-follow")).toHaveAttribute("aria-checked", "false");
  });

  test("選択肢の外をクリックすると閉じる", async ({ page }) => {
    await openAssign(page);
    await key(page, "KeyA").click();
    await expect(id(page, "key-menu")).toBeVisible();
    await page.locator(".assign-heading").first().click();
    await expect(id(page, "key-menu")).toBeHidden();
  });

  test("選択肢は窓の中に収まる（900×600）", async ({ page }) => {
    await openAssign(page);
    for (const code of ["Escape", "Space", "Numpad0", "ArrowRight"]) {
      await key(page, code).click();
      const box = await id(page, "key-menu").boundingBox();
      expect(box.x).toBeGreaterThanOrEqual(0);
      expect(box.y).toBeGreaterThanOrEqual(0);
      expect(box.x + box.width).toBeLessThanOrEqual(900);
      expect(box.y + box.height).toBeLessThanOrEqual(600);
      await page.keyboard.press("Escape");
    }
  });
});

// ============================================================================
// グループ10行の選択欄
// ============================================================================

test.describe("グループの選択欄", () => {
  test("グループを変えると、そのグループのキーだけが変わり、上書きしたキーは変わらない", async ({ page }) => {
    await openAssign(page, { typing: { groups: {}, keys: { KeyJ: "kick" } } });

    await id(page, "group-select-letters").selectOption("snare");

    expect((await updateCalls(page)).map((call) => call.args)).toEqual([
      { settings: { typing: { groups: { letters: "snare" } } } },
    ]);
    await expect(key(page, "KeyB")).toHaveAttribute("data-sound", "snare");
    await expect(key(page, "KeyZ")).toHaveAttribute("data-sound", "snare");
    // 上書きしたキーは変わらない。ほかのグループも変わらない。
    await expect(key(page, "KeyJ")).toHaveAttribute("data-sound", "kick");
    await expect(key(page, "Space")).toHaveAttribute("data-sound", "kick");
    await expect(key(page, "Enter")).toHaveAttribute("data-sound", "snare");
    await expect(key(page, "Digit1")).toHaveAttribute("data-sound", "tom_low");
  });

  test("既定と同じ音へ戻すと、グループの指定そのものを消す（null を送る）", async ({ page }) => {
    await openAssign(page, { typing: { groups: { letters: "snare" }, keys: {} } });
    await id(page, "group-select-letters").selectOption("hat_closed");
    expect((await updateCalls(page)).map((call) => call.args)).toEqual([
      { settings: { typing: { groups: { letters: null } } } },
    ]);
    expect((await currentSettings(page)).typing.groups).toEqual({});
  });

  test("10行を2列×5行で並べ、既定の音に「（既定）」と添える", async ({ page }) => {
    await openAssign(page);
    const selects = id(page, "assign-groups").locator("select");
    await expect(selects).toHaveCount(10);
    const boxes = await selects.evaluateAll((nodes) =>
      nodes.map((node) => {
        const rect = node.getBoundingClientRect();
        return { x: Math.round(rect.x), y: Math.round(rect.y) };
      }),
    );
    expect(new Set(boxes.map((box) => box.x)).size).toBe(2);
    // 5行（同じ行の2列は、縦の位置がほぼ同じ）。
    const rows = new Set(boxes.map((box) => Math.round(box.y / 20)));
    expect(rows.size).toBeLessThanOrEqual(5 + 3);
    expect(await selects.evaluateAll((nodes) => nodes.map((node) => node.value))).toEqual([
      "hat_closed",
      "kick",
      "snare",
      "rim",
      "hat_open",
      "tom_high",
      "tom_low",
      "clap",
      "none",
      "none",
    ]);
    await expect(id(page, "group-select-letters").locator('option[value="hat_closed"]')).toHaveText(
      "閉じハイハット（既定）",
    );
  });

  test("編集する組を切り替えると、絵と選択欄がその組の割り当てになる", async ({ page }) => {
    await openAssign(page);
    await expect(key(page, "KeyA")).toHaveAttribute("data-sound", "hat_closed");

    await id(page, "assign-set-play").click();

    await expect(id(page, "assign-set-play")).toHaveAttribute("aria-checked", "true");
    await expect(key(page, "KeyA")).toHaveAttribute("data-sound", "kick");
    await expect(key(page, "KeyJ")).toHaveAttribute("data-sound", "hat_closed");
    await expect(key(page, "KeyQ")).toHaveAttribute("data-sound", "none");
    await expect(id(page, "group-select-space")).toHaveValue("kick");
    await expect(id(page, "group-select-letters")).toHaveValue("none");

    await pickSound(page, "KeyQ", "tom_high");
    expect((await updateCalls(page)).map((call) => call.args)).toEqual([
      { settings: { play: { keys: { KeyQ: "tom_high" } } } },
    ]);
    expect((await currentSettings(page)).typing).toEqual({ groups: {}, keys: {} });
  });

  test("演奏用で、グループの指定はキーごとの既定より優先され、解除すると既定に戻る", async ({ page }) => {
    await openAssign(page);
    await id(page, "assign-set-play").click();
    await id(page, "group-select-letters").selectOption("clap");
    await expect(key(page, "KeyA")).toHaveAttribute("data-sound", "clap");
    await id(page, "group-select-letters").selectOption("none");
    await expect(key(page, "KeyA")).toHaveAttribute("data-sound", "kick");
  });
});

// ============================================================================
// 個別に変えたキーの一覧
// ============================================================================

test.describe("個別に変えたキー", () => {
  test("件数と一覧を出し、キーごとに解除できる", async ({ page }) => {
    await openAssign(page, withOverrides);
    await expect(id(page, "assign-overrides-title")).toHaveText("個別に変えたキー 2個");
    await expect(id(page, "override-item")).toHaveCount(2);
    await expect(id(page, "override-item").first()).toContainText("J");
    await expect(id(page, "override-item").nth(1)).toContainText("テンキーの 5");

    await id(page, "override-clear-KeyJ").click();

    expect((await updateCalls(page)).map((call) => call.args)).toEqual([
      { settings: { typing: { keys: { KeyJ: null } } } },
    ]);
    await expect(id(page, "assign-overrides-title")).toHaveText("個別に変えたキー 1個");
    await expect(key(page, "KeyJ")).toHaveAttribute("data-override", "false");
  });

  test("0個のときは、無いことを文で伝える", async ({ page }) => {
    await openAssign(page);
    await expect(id(page, "assign-overrides-title")).toHaveText("個別に変えたキー 0個");
    await expect(id(page, "assign-overrides-none")).toBeVisible();
    await expect(id(page, "assign-overrides")).toBeHidden();
  });
});

// ============================================================================
// 既定に戻す（組ごと・確認・取り消す）
// ============================================================================

test.describe("既定に戻す", () => {
  const playOverrides = { play: { groups: { navigation: "tom_low" }, keys: { Space: "hat_open" } } };

  test("対象の組と件数を示して確認し、戻したあと「取り消す」で元に戻る", async ({ page }) => {
    await openAssign(page, { ...withOverrides, ...playOverrides });
    const before = await currentSettings(page);

    await id(page, "assign-reset").click();
    await expect(id(page, "assign-reset-confirm")).toBeVisible();
    await expect(id(page, "assign-reset-text")).toContainText("タイピング用");
    await expect(id(page, "assign-reset-text")).toContainText("1件");
    await expect(id(page, "assign-reset-text")).toContainText("2個");
    await expect(id(page, "assign-reset-text")).toContainText("計 3件");
    // 確認の時点では、まだ何も送っていない。
    expect(await updateCalls(page)).toEqual([]);

    await id(page, "assign-reset-confirm-button").click();

    // 組ごと: 演奏用はそのまま。
    const after = await currentSettings(page);
    expect(after.typing).toEqual({ groups: {}, keys: {} });
    expect(after.play).toEqual(before.play);
    await expect(key(page, "KeyJ")).toHaveAttribute("data-sound", "hat_closed");
    await expect(key(page, "KeyJ")).toHaveAttribute("data-override", "false");
    await expect(id(page, "assign-feedback-text")).toContainText("既定に戻しました");
    await expect(id(page, "assign-undo")).toBeVisible();

    await id(page, "assign-undo").click();

    expect(await currentSettings(page)).toEqual(before);
    await expect(key(page, "KeyJ")).toHaveAttribute("data-sound", "kick");
    await expect(key(page, "KeyJ")).toHaveAttribute("data-override", "true");
    await expect(id(page, "assign-undo")).toBeHidden();
  });

  test("やめるを押すと何も変わらない", async ({ page }) => {
    await openAssign(page, withOverrides);
    await id(page, "assign-reset").click();
    await id(page, "assign-reset-cancel").click();
    await expect(id(page, "assign-reset-confirm")).toBeHidden();
    expect(await updateCalls(page)).toEqual([]);
    await expect(id(page, "assign-reset")).toBeFocused();
  });

  test("既定のままの組では押せず、その旨を文で伝える", async ({ page }) => {
    await openAssign(page);
    await expect(id(page, "assign-reset")).toBeDisabled();
    await expect(id(page, "assign-reset-hint")).toBeVisible();
  });

  test("ほかの変更をしたら「取り消す」は消える", async ({ page }) => {
    await openAssign(page, withOverrides);
    await id(page, "assign-reset").click();
    await id(page, "assign-reset-confirm-button").click();
    await expect(id(page, "assign-undo")).toBeVisible();

    await pickSound(page, "KeyA", "clap");

    await expect(id(page, "assign-undo")).toBeHidden();
  });

  test("演奏用の組だけを戻せる", async ({ page }) => {
    await openAssign(page, { ...withOverrides, ...playOverrides });
    await id(page, "assign-set-play").click();
    await id(page, "assign-reset").click();
    await expect(id(page, "assign-reset-text")).toContainText("演奏用");
    await id(page, "assign-reset-confirm-button").click();

    const after = await currentSettings(page);
    expect(after.play).toEqual({ groups: {}, keys: {} });
    expect(after.typing).toEqual(withOverrides.typing);
  });
});

// ============================================================================
// JIS／US
// ============================================================================

test.describe("JIS と US", () => {
  test("切り替えると絵が変わり、保存される割り当ては変わらない", async ({ page }) => {
    await openAssign(page, withOverrides);
    await expect(id(page, "keyboard")).toHaveAttribute("data-layout", "us");
    await expect(key(page, "IntlYen")).toHaveCount(0);
    await expect(key(page, "KeyJ")).toHaveAttribute("data-override", "true");
    const before = await currentSettings(page);

    await id(page, "assign-layout-jis").click();

    await expect(id(page, "keyboard")).toHaveAttribute("data-layout", "jis");
    await expect(key(page, "IntlYen")).toHaveCount(1);
    await expect(key(page, "Convert")).toHaveCount(1);
    expect((await updateCalls(page)).map((call) => call.args)).toEqual([{ settings: { keyboard_layout: "jis" } }]);
    const after = await currentSettings(page);
    expect(after.typing).toEqual(before.typing);
    expect(after.play).toEqual(before.play);
    // 音の割り当ては code 名で持つので、配列が変わっても同じキーは同じ音。
    await expect(key(page, "KeyJ")).toHaveAttribute("data-sound", "kick");
    await expect(key(page, "KeyJ")).toHaveAttribute("data-override", "true");

    await id(page, "assign-layout-us").click();
    await expect(key(page, "IntlYen")).toHaveCount(0);
  });

  test("移動キーとテンキーは、どちらの配列でも常に描く", async ({ page }) => {
    for (const layout of ["jis", "us"]) {
      await openAssign(page, { keyboard_layout: layout });
      for (const code of ["ArrowUp", "Home", "PageDown", "Numpad0", "NumpadEnter", "NumpadAdd", "NumLock"]) {
        await expect(key(page, code)).toHaveCount(1);
      }
      await page.goto("about:blank");
    }
  });

  test("設定の区画の配列の選択と連動する", async ({ page }) => {
    await openAssign(page);
    await id(page, "assign-layout-jis").click();
    await expect(id(page, "assign-layout-jis")).toHaveAttribute("aria-checked", "true");
    await id(page, "tab-settings").click();
    await expect(id(page, "setting-layout")).toHaveValue("jis");
  });

  test("絵に描いたすべてのキーが、グループの元データにある", async ({ page }) => {
    await openAssign(page);
    const data = JSON.parse(readWeb("key-data.json"));
    const codes = await page.evaluate(() => {
      const { layouts } = window.drumclackLayouts;
      return Object.values(layouts).flatMap((keys) => keys.map((k) => k.code));
    });
    expect(codes.length).toBeGreaterThan(100);
    expect(codes.filter((code) => !(code in data.groups))).toEqual([]);
  });
});

// ============================================================================
// キーボード操作・読み上げ名
// ============================================================================

test.describe("キーボード操作", () => {
  test("Tab で止まるのは絵の中の1か所だけ", async ({ page }) => {
    await openAssign(page);
    const stops = await page.locator('[data-testid="keyboard"] .key[tabindex="0"]').count();
    expect(stops).toBe(1);
  });

  test("矢印キーだけで任意のキーに辿り着き、音を変えられる", async ({ page }) => {
    await openAssign(page);
    await key(page, "KeyA").focus();

    // A → S → D（右へ2回）。
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("ArrowRight");
    await expect(key(page, "KeyD")).toBeFocused();
    // 上の段へ。D の上は E。
    await page.keyboard.press("ArrowUp");
    await expect(key(page, "KeyE")).toBeFocused();

    await page.keyboard.press("Enter");
    await expect(id(page, "key-menu")).toBeVisible();
    // 今の選び方（グループに従う）から、選択肢の間を矢印で動き、Enter で決める。
    await expect(id(page, "choice-follow")).toBeFocused();
    await page.keyboard.press("ArrowDown"); // 先頭（kick）へ回る
    await expect(id(page, "choice-kick")).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(id(page, "choice-snare")).toBeFocused();
    await page.keyboard.press("Enter");

    expect((await updateCalls(page)).map((call) => call.args)).toEqual([
      { settings: { typing: { keys: { KeyE: "snare" } } } },
    ]);
    expect(await previewCalls(page)).toEqual([{ sound: "snare" }]);
    await expect(key(page, "KeyE")).toHaveAttribute("data-sound", "snare");
    await expect(key(page, "KeyE")).toBeFocused();
  });

  test("Esc で選択肢を閉じ、フォーカスを元のキーへ戻す（何も変えない）", async ({ page }) => {
    await openAssign(page);
    await key(page, "KeyS").focus();
    await page.keyboard.press("Enter");
    await expect(id(page, "key-menu")).toBeVisible();
    await page.keyboard.press("ArrowDown");

    await page.keyboard.press("Escape");

    await expect(id(page, "key-menu")).toBeHidden();
    await expect(key(page, "KeyS")).toBeFocused();
    expect(await updateCalls(page)).toEqual([]);
    expect(await previewCalls(page)).toEqual([]);
  });

  test("Space でも選択肢を開ける", async ({ page }) => {
    await openAssign(page);
    await key(page, "KeyS").focus();
    await page.keyboard.press("Space");
    await expect(id(page, "key-menu")).toBeVisible();
  });

  for (const layout of ["us", "jis"]) {
    test(`矢印キーで、${layout.toUpperCase()} のすべてのキーに辿り着ける`, async ({ page }) => {
      await openAssign(page, { keyboard_layout: layout });
      const result = await page.evaluate(() => {
        const buttons = Array.from(document.querySelectorAll('[data-testid="keyboard"] .key'));
        const byCode = new Map(buttons.map((button) => [button.dataset.code, button]));
        const move = (code, direction) => {
          const button = byCode.get(code);
          button.focus();
          button.dispatchEvent(new KeyboardEvent("keydown", { key: direction, bubbles: true, cancelable: true }));
          return document.activeElement.dataset.code;
        };
        const seen = new Set(["KeyA"]);
        const queue = ["KeyA"];
        while (queue.length > 0) {
          const code = queue.shift();
          for (const direction of ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"]) {
            const next = move(code, direction);
            if (next && !seen.has(next)) {
              seen.add(next);
              queue.push(next);
            }
          }
        }
        return { total: buttons.length, unreached: buttons.map((b) => b.dataset.code).filter((c) => !seen.has(c)) };
      });
      expect(result.total).toBeGreaterThan(100);
      expect(result.unreached).toEqual([]);
    });
  }

  test("読み上げ名は「A：閉じハイハット（グループに従う）」の形で、上書きすると「個別に変更」になる", async ({ page }) => {
    await openAssign(page, withOverrides);
    await expect(key(page, "KeyA")).toHaveAttribute("aria-label", "A：スネア（グループに従う）");
    await expect(key(page, "KeyJ")).toHaveAttribute("aria-label", "J：キック（個別に変更）");
    await expect(key(page, "Numpad5")).toHaveAttribute("aria-label", "テンキーの 5：無音（個別に変更）");
    await expect(key(page, "Space")).toHaveAttribute("aria-label", "Space：キック（グループに従う）");

    await id(page, "assign-set-play").click();
    await expect(key(page, "KeyA")).toHaveAttribute("aria-label", "A：キック（既定）");
  });

  test("英語でも同じ形で読み上げられ、音の略号を文字で出す", async ({ page }) => {
    await openAssign(page, { language: "en" });
    await expect(key(page, "KeyA")).toHaveAttribute("aria-label", "A: Closed hi-hat (follows its group)");
    await expect(key(page, "KeyA").locator(".key-sound")).toHaveText("CHH");
    await expect(key(page, "KeyN")).toHaveAttribute("data-sound", "hat_closed");
  });

  test("各キーに音の略号を文字でも出す（色だけに頼らない）", async ({ page }) => {
    await openAssign(page);
    const abbreviations = await page.locator('[data-testid="keyboard"] .key').evaluateAll((nodes) => {
      const bySound = {};
      for (const node of nodes) {
        const text = node.querySelector(".key-sound").textContent;
        (bySound[node.dataset.sound] ||= new Set()).add(text);
      }
      return Object.fromEntries(Object.entries(bySound).map(([sound, set]) => [sound, [...set]]));
    });
    // 音ごとに、空でない同じ1つの略号。音が違えば略号も違う。
    for (const texts of Object.values(abbreviations)) {
      expect(texts).toHaveLength(1);
      expect(texts[0]).not.toBe("");
    }
    const all = Object.values(abbreviations).map((texts) => texts[0]);
    expect(new Set(all).size).toBe(all.length);
    expect(all.length).toBeGreaterThanOrEqual(5);
  });
});

// ============================================================================
// 窓の大きさ（900×600）
// ============================================================================

test.describe("窓の大きさ", () => {
  for (const language of ["ja", "en"]) {
    for (const layout of ["jis", "us"]) {
      test(`900×600 で、選択欄と絵が溢れない（${language}・${layout.toUpperCase()}）`, async ({ page }) => {
        await openAssign(page, { ...withOverrides, language, keyboard_layout: layout });
        const overflow = await page.evaluate(() => {
          const scroller = document.querySelector('[data-testid="keyboard-scroll"]');
          const rect = (el) => el.getBoundingClientRect();
          const selects = Array.from(document.querySelectorAll('[data-testid="assign-groups"] select'));
          const labels = Array.from(document.querySelectorAll('[data-testid="assign-groups"] label'));
          const keys = Array.from(document.querySelectorAll('[data-testid="keyboard"] .key'));
          return {
            page: document.documentElement.scrollWidth - document.documentElement.clientWidth,
            keyboard: scroller.scrollWidth - scroller.clientWidth,
            selectsOutside: selects.filter((el) => rect(el).left < 0 || rect(el).right > window.innerWidth).length,
            labelsClipped: labels.filter((el) => el.scrollWidth > el.clientWidth).length,
            keysOutside: keys.filter((el) => rect(el).right > rect(scroller).right + 0.5).length,
          };
        });
        expect(overflow).toEqual({ page: 0, keyboard: 0, selectsOutside: 0, labelsClipped: 0, keysOutside: 0 });
      });
    }
  }

  test("幅が足りないときは、キーボードの絵だけが横にスクロールする", async ({ page }) => {
    await page.setViewportSize({ width: 640, height: 600 });
    await openAssign(page);
    const result = await page.evaluate(() => {
      const scroller = document.querySelector('[data-testid="keyboard-scroll"]');
      return {
        page: document.documentElement.scrollWidth - document.documentElement.clientWidth,
        keyboard: scroller.scrollWidth - scroller.clientWidth,
        overflowX: getComputedStyle(scroller).overflowX,
        focusable: scroller.tabIndex,
      };
    });
    expect(result.page).toBeLessThanOrEqual(0);
    expect(result.keyboard).toBeGreaterThan(0);
    expect(result.overflowX).toBe("auto");
    // スクロールできるときは、キーボードで枠を操作できる。
    expect(result.focusable).toBe(0);
  });
});

// ============================================================================
// 罠: 打鍵を絵の上で光らせない
// ============================================================================

test.describe("打鍵を画面へ送らない", () => {
  test("この画面を開いている間に押されたキーで、絵が変わらず、命令も出ない", async ({ page }) => {
    await openAssign(page, withOverrides);
    const snapshot = () =>
      page.locator('[data-testid="keyboard"]').evaluate((board) => board.innerHTML);
    const before = await snapshot();
    const callsBefore = (await allCalls(page)).filter((call) => !["get_status", "get_settings"].includes(call.cmd));

    // フォーカスが絵の外にあるまま、いろいろなキーを押す（文字・数字・記号・矢印・Enter・Esc）。
    await id(page, "tab-assign").focus();
    for (const name of ["a", "s", "j", "1", "Space", "Enter", "Escape", "Backspace", "Tab", "Shift"]) {
      await page.keyboard.press(name);
    }
    await page.waitForTimeout(400);

    expect(await snapshot()).toBe(before);
    const callsAfter = (await allCalls(page)).filter((call) => !["get_status", "get_settings"].includes(call.cmd));
    expect(callsAfter).toEqual(callsBefore);
  });

  test("assign.js は document / window でキーを受けず、Rust のイベントも受けない", () => {
    const source = readWeb("assign.js").replace(/\/\/.*$/gm, "");
    expect(source).not.toMatch(/(document|window)\.addEventListener\(\s*["']key/);
    expect(source).not.toMatch(/\.listen\(/);
    expect(source).not.toMatch(/\bonkey(down|up|press)\b/);
    // 絵と選択肢の中だけで受ける。
    expect(source).toMatch(/els\.keyboard\.addEventListener\("keydown"/);
    expect(source).toMatch(/els\.menu\.addEventListener\("keydown"/);
  });
});

// ============================================================================
// 読み込みの失敗・設定を読めないとき
// ============================================================================

test.describe("読み込みの失敗", () => {
  test("元データを読めないときは、割り当ての区画だけが使えず、次の操作を出す", async ({ page }) => {
    await page.route("**/key-data.json", (route) => route.fulfill({ status: 500, body: "" }));
    await openApp(page);
    await id(page, "tab-assign").click();
    await expect(id(page, "assign-load-error")).toBeVisible();
    await expect(id(page, "assign")).toBeHidden();
    // 設定の区画は使える。
    await id(page, "tab-settings").click();
    await expect(id(page, "setting-volume")).toBeEnabled();
  });

  test("設定を読めないときは、絵を操作できない", async ({ page }) => {
    await openApp(page, { failSettings: true });
    await id(page, "tab-assign").click();
    await expect(id(page, "assign")).toHaveAttribute("data-disabled", "true");
  });
});
