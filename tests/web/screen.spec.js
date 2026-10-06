// 画面の挙動の検査。Rust の代わりに差し替えた `window.__TAURI__` で、素のブラウザに web/ を開く。
// 文言ではなく data-testid で要素を選ぶ（言語を切り替えても壊れないように）。

const {
  test,
  expect,
  SAMPLE_SETTINGS,
  DEFAULT_SETTINGS,
  STATUS_OK,
  STATUS_PERMISSION_DENIED,
  STATUS_PERMISSION_UNKNOWN,
  STATUS_AUDIO_FAILED,
  openApp,
  updateCalls,
  allCalls,
  setStatus,
  readWeb,
} = require("./helpers");

const ja = JSON.parse(readWeb("i18n/ja.json"));
const en = JSON.parse(readWeb("i18n/en.json"));

const id = (page, testId) => page.getByTestId(testId);

// ============================================================================
// 窓の骨格
// ============================================================================

test.describe("窓の骨格", () => {
  test("設定／割り当て／演奏の3区画があり、押した区画だけが出る", async ({ page }) => {
    await openApp(page);

    const names = ["settings", "assign", "play"];
    for (const name of names) {
      await expect(id(page, `tab-${name}`)).toBeVisible();
    }
    await expect(id(page, "panel-settings")).toBeVisible();
    await expect(id(page, "panel-assign")).toBeHidden();

    for (const name of ["assign", "play", "settings"]) {
      await id(page, `tab-${name}`).click();
      for (const other of names) {
        const shown = other === name;
        await expect(id(page, `panel-${other}`)).toBeVisible({ visible: shown });
        await expect(id(page, `tab-${other}`)).toHaveAttribute("aria-selected", String(shown));
      }
    }
  });

  test("区画の切り替えは矢印キーでもできる", async ({ page }) => {
    await openApp(page);
    await id(page, "tab-settings").focus();

    await page.keyboard.press("ArrowRight");
    await expect(id(page, "panel-assign")).toBeVisible();
    await expect(id(page, "tab-assign")).toBeFocused();

    await page.keyboard.press("End");
    await expect(id(page, "panel-play")).toBeVisible();
    await page.keyboard.press("ArrowRight");
    await expect(id(page, "panel-settings")).toBeVisible();
  });

  test("最小の窓（900×600）で横スクロールが出ない", async ({ page }) => {
    await openApp(page);
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(overflow).toBeLessThanOrEqual(0);
  });
});

// ============================================================================
// 状態の帯（未許可・音声デバイス失敗・正常の3通り）
// ============================================================================

test.describe("状態の帯", () => {
  test("正常: 1行だけで、問題の行も次の操作も出ない", async ({ page }) => {
    await openApp(page, { status: STATUS_OK });

    await expect(id(page, "status-band")).toHaveAttribute("data-state", "ok");
    await expect(id(page, "status-ok")).toHaveText(ja["status.ok"]);
    await expect(page.locator('[data-testid="status-issues"] > li')).toHaveCount(1);
    await expect(page.locator(".action-hint")).toHaveCount(0);
  });

  test("未許可: 何が起きたかと次の操作を出し、帯は error になる", async ({ page }) => {
    await openApp(page, { status: STATUS_PERMISSION_DENIED });

    await expect(id(page, "status-band")).toHaveAttribute("data-state", "error");
    const issue = id(page, "status-issue-permission");
    await expect(issue).toContainText(ja["status.permission.denied"]);
    await expect(issue).toContainText(ja["status.permission.action"]);
    await expect(id(page, "status-ok")).toHaveCount(0);
    await expect(id(page, "status-issue-audio")).toHaveCount(0);
  });

  test("許可の状態が分からない: warn になり、次の操作を出す", async ({ page }) => {
    await openApp(page, { status: STATUS_PERMISSION_UNKNOWN });

    await expect(id(page, "status-band")).toHaveAttribute("data-state", "warn");
    await expect(id(page, "status-issue-permission")).toContainText(ja["status.permission.unknown"]);
    await expect(id(page, "status-issue-permission")).toContainText(ja["status.permission.action"]);
  });

  test("音声デバイス失敗: 次の操作を先に、内部のエラー文字列は詳細として弱く出す", async ({ page }) => {
    await openApp(page, { status: STATUS_AUDIO_FAILED });

    await expect(id(page, "status-band")).toHaveAttribute("data-state", "error");
    const issue = id(page, "status-issue-audio");
    await expect(issue).toContainText(ja["status.audio.failed"]);
    await expect(issue.locator(".action-hint")).toHaveText(ja["status.audio.action"]);
    await expect(issue.locator(".detail-text")).toContainText("no output device (test)");

    // 並び順: 次の操作が、内部のエラー文字列より先。
    const order = await issue.locator("span").evaluateAll((spans) => spans.map((span) => span.className));
    expect(order).toEqual(["issue-message", "action-hint", "detail-text"]);
  });

  test("状態の取得に失敗: 次の操作つきで error を出す", async ({ page }) => {
    await openApp(page, { statusError: true });

    await expect(id(page, "status-band")).toHaveAttribute("data-state", "error");
    await expect(id(page, "status-issue-fetch")).toContainText(ja["status.fetch.action"]);
    await expect(id(page, "status-issue-fetch")).toContainText("status unavailable (test)");
  });

  test("未許可と音声デバイス失敗が重なったら、問題を2つとも出す", async ({ page }) => {
    await openApp(page, { status: { ...STATUS_PERMISSION_DENIED, audio: STATUS_AUDIO_FAILED.audio } });

    await expect(id(page, "status-issue-permission")).toBeVisible();
    await expect(id(page, "status-issue-audio")).toBeVisible();
  });

  test("状態が戻ったら、問題の行が消えて正常の1行になる", async ({ page }) => {
    await openApp(page, { status: STATUS_AUDIO_FAILED });
    await expect(id(page, "status-issue-audio")).toBeVisible();

    await setStatus(page, STATUS_OK);
    await expect(id(page, "status-ok")).toBeVisible();
    await expect(id(page, "status-issue-audio")).toHaveCount(0);
    await expect(id(page, "status-band")).toHaveAttribute("data-state", "ok");
  });

  test("直近の発音と同時発音数は、畳んだ「詳細」の中にある", async ({ page }) => {
    const playedAt = new Date(2026, 9, 6, 15, 4, 5).getTime();
    await openApp(page, { status: { ...STATUS_OK, last_play_ms: playedAt, active_voices: 3 } });

    const details = id(page, "status-details");
    await expect(details).not.toHaveAttribute("open", "");
    await expect(id(page, "detail-voices")).toBeHidden();

    await details.locator("summary").click();
    await expect(id(page, "detail-voices")).toHaveText("3 / 16");
    await expect(id(page, "detail-last-play")).toContainText("15:04:05");
    await expect(id(page, "detail-audio")).toHaveText(ja["status.details.audio.ok"]);
  });

  test("権限の概念が無い環境（Windows）では、権限の行を出さない", async ({ page }) => {
    await openApp(page, { status: { ...STATUS_OK, platform: "windows", permission: null } });

    await expect(id(page, "status-ok")).toBeVisible();
    await id(page, "status-details").locator("summary").click();
    await expect(id(page, "detail-permission-row")).toBeHidden();
  });

  test("テストモードのときだけ、その旨の帯を出す", async ({ page }) => {
    await openApp(page, { status: { ...STATUS_OK, test_delay_ms: 150 } });
    await expect(id(page, "test-mode")).toContainText("150");

    await setStatus(page, STATUS_OK);
    await expect(id(page, "test-mode")).toBeHidden();
  });
});

// ============================================================================
// 設定の区画
// ============================================================================

test.describe("設定の区画: 表示", () => {
  test("設定ファイルの見本（Rust 側のテストと同じファイル）の値が、そのまま画面に出る", async ({ page }) => {
    await openApp(page, { settings: SAMPLE_SETTINGS });

    await expect(id(page, "setting-enabled")).not.toBeChecked(); // enabled: false
    await expect(id(page, "setting-volume")).toHaveValue("50"); // volume: 0.5
    await expect(id(page, "setting-volume-value")).toHaveText("50%");
    await expect(id(page, "setting-dynamics")).toHaveValue("25"); // dynamics: 0.25
    await expect(id(page, "setting-dynamics-value")).toHaveText("25%");
    await expect(id(page, "setting-language")).toHaveValue("en");
    await expect(id(page, "setting-layout")).toHaveValue("us");

    // language: "en" なので文言も英語。画面を開いただけでは何も保存しない。
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(id(page, "tab-settings")).toHaveText(en["nav.settings"]);
    expect(await updateCalls(page)).toEqual([]);
  });

  test("起動時に呼ぶのは get_settings と get_status だけ", async ({ page }) => {
    await openApp(page);
    await expect(id(page, "status-ok")).toBeVisible();

    const names = new Set((await allCalls(page)).map((call) => call.cmd));
    expect([...names].sort()).toEqual(["get_settings", "get_status"]);
  });
});

test.describe("設定の区画: 操作すると、変えた項目だけの更新命令が呼ばれる", () => {
  // [説明, 操作, 期待する update_settings の引数]
  const cases = [
    ["オン／オフ", (page) => id(page, "setting-enabled").uncheck(), { enabled: false }],
    ["音量", (page) => id(page, "setting-volume").fill("35"), { volume: 0.35 }],
    ["強弱の幅", (page) => id(page, "setting-dynamics").fill("0"), { dynamics: 0 }],
    ["言語", (page) => id(page, "setting-language").selectOption("en"), { language: "en" }],
    ["キーボードの配列", (page) => id(page, "setting-layout").selectOption("jis"), { keyboard_layout: "jis" }],
  ];

  for (const [name, operate, expectedPatch] of cases) {
    test(name, async ({ page }) => {
      await openApp(page);
      await operate(page);

      // 命令名と、引数の形（`settings` に差分）まで確かめる。全体は送り返さない。
      await expect.poll(() => updateCalls(page)).toEqual([
        { cmd: "update_settings", args: { settings: expectedPatch } },
      ]);
    });
  }

  test("オンへ戻す操作は enabled: true だけを送る", async ({ page }) => {
    await openApp(page, { settings: SAMPLE_SETTINGS });
    await id(page, "setting-enabled").check();

    await expect.poll(() => updateCalls(page)).toEqual([
      { cmd: "update_settings", args: { settings: { enabled: true } } },
    ]);
  });

  test("スライダーの表示は動かすと変わり、保存は確定（change）の1回だけ", async ({ page }) => {
    await openApp(page);
    const slider = id(page, "setting-volume");

    // input だけが起きた（まだ確定していない）間は、表示だけが変わり、命令は呼ばれない。
    await slider.evaluate((el) => {
      el.value = "10";
      el.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await expect(id(page, "setting-volume-value")).toHaveText("10%");
    expect(await updateCalls(page)).toEqual([]);

    await slider.evaluate((el) => el.dispatchEvent(new Event("change", { bubbles: true })));
    await expect.poll(() => updateCalls(page)).toEqual([
      { cmd: "update_settings", args: { settings: { volume: 0.1 } } },
    ]);
  });

  test("連続して操作しても、前の操作の値が巻き戻らない", async ({ page }) => {
    await openApp(page);

    await id(page, "setting-volume").fill("20");
    await id(page, "setting-dynamics").fill("90");
    await id(page, "setting-layout").selectOption("us");

    await expect.poll(async () => (await updateCalls(page)).length).toBe(3);
    await expect(id(page, "setting-volume")).toHaveValue("20");
    await expect(id(page, "setting-dynamics")).toHaveValue("90");
    await expect(id(page, "setting-layout")).toHaveValue("us");
    expect((await updateCalls(page)).map((call) => Object.keys(call.args.settings))).toEqual([
      ["volume"],
      ["dynamics"],
      ["keyboard_layout"],
    ]);
  });
});

test.describe("設定の区画: 言語", () => {
  test("言語を切り替えると、文言と lang 属性が変わり、testid で選んだ要素はそのまま使える", async ({ page }) => {
    await openApp(page, { settings: { ...DEFAULT_SETTINGS, language: "ja" } });
    await expect(page.locator("html")).toHaveAttribute("lang", "ja");
    await expect(id(page, "tab-settings")).toHaveText(ja["nav.settings"]);

    await id(page, "setting-language").selectOption("en");

    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(id(page, "tab-settings")).toHaveText(en["nav.settings"]);
    await expect(id(page, "status-ok")).toHaveText(en["status.ok"]);
    // 文言の要素は data-i18n で選べる。
    await expect(page.locator('[data-i18n="nav.play"]')).toHaveText(en["nav.play"]);

    await id(page, "setting-language").selectOption("ja");
    await expect(page.locator("html")).toHaveAttribute("lang", "ja");
    await expect(page.locator('[data-i18n="nav.play"]')).toHaveText(ja["nav.play"]);
  });

  test("言語を切り替えると、出ている異常の文言も新しい言語になる", async ({ page }) => {
    await openApp(page, {
      settings: { ...DEFAULT_SETTINGS, language: "ja" },
      status: STATUS_AUDIO_FAILED,
      recovered: true,
    });
    await expect(id(page, "status-issue-audio")).toContainText(ja["status.audio.failed"]);

    await id(page, "setting-language").selectOption("en");

    await expect(id(page, "status-issue-audio")).toContainText(en["status.audio.failed"]);
    await expect(id(page, "status-issue-audio")).toContainText(en["status.audio.action"]);
    await expect(id(page, "settings-recovered-text")).toHaveText(en["settings.recovered"]);
  });

  test("auto: OS の言語が日本語なら日本語・JIS、それ以外は英語・US。選ぶまで何も保存しない", async ({ browser }) => {
    for (const [locale, lang, layout] of [
      ["ja-JP", "ja", "jis"],
      ["en-US", "en", "us"],
      ["fr-FR", "en", "us"],
    ]) {
      const context = await browser.newContext({ locale, baseURL: "http://127.0.0.1:4173" });
      const page = await context.newPage();
      await openApp(page); // 既定は language: auto / keyboard_layout: auto

      await expect(page.locator("html"), locale).toHaveAttribute("lang", lang);
      await expect(page.locator("html"), locale).toHaveAttribute("data-keyboard-layout", layout);
      // 画面には「自動」のまま出る（推測した値を勝手に選んだことにしない）。
      await expect(id(page, "setting-language")).toHaveValue("auto");
      await expect(id(page, "setting-layout")).toHaveValue("auto");
      expect(await updateCalls(page), locale).toEqual([]);
      await context.close();
    }
  });

  test("配列を選んだあとは、OS の言語に関係なく選んだ値を使う", async ({ browser }) => {
    const context = await browser.newContext({ locale: "ja-JP", baseURL: "http://127.0.0.1:4173" });
    const page = await context.newPage();
    await openApp(page, { settings: { ...DEFAULT_SETTINGS, keyboard_layout: "us" } });

    await expect(page.locator("html")).toHaveAttribute("data-keyboard-layout", "us");
    await id(page, "setting-layout").selectOption("jis");
    await expect(page.locator("html")).toHaveAttribute("data-keyboard-layout", "jis");
    await context.close();
  });
});

test.describe("設定の区画: 失敗の表示", () => {
  test("保存に失敗したら、その旨と次の操作を出す。成功したら消える", async ({ page }) => {
    await openApp(page, { saveFailed: true });

    await expect(id(page, "settings-error")).toHaveText(ja["settings.saveFailed"]);
    await expect(id(page, "settings-error")).toHaveAttribute("role", "alert");

    await page.evaluate(() => {
      window.__fake.cfg.saveFailed = false;
    });
    await id(page, "setting-volume").fill("42");
    await expect(id(page, "settings-error")).toBeHidden();
  });

  test("更新の命令が失敗したら、画面の値を最後に確かめられた設定へ戻し、その旨を出す", async ({ page }) => {
    await openApp(page, { failUpdate: true });
    await expect(id(page, "setting-volume")).toHaveValue("80");

    await id(page, "setting-volume").fill("10");

    await expect(id(page, "settings-error")).toHaveText(ja["settings.updateFailed"]);
    await expect(id(page, "setting-volume")).toHaveValue("80");
    await expect(id(page, "setting-volume-value")).toHaveText("80%");
  });

  test("退避していない通常の起動では、その1行を出さない", async ({ page }) => {
    await openApp(page);
    await expect(id(page, "settings-recovered")).toBeHidden();
    await expect(id(page, "settings-error")).toBeHidden();
  });

  test("壊れた設定ファイルを退避して既定で起動したことを、1行で出す", async ({ page }) => {
    await openApp(page, { recovered: true });

    await expect(id(page, "settings-recovered-text")).toHaveText(ja["settings.recovered"]);
    await expect(id(page, "settings-recovered-text")).toContainText("settings.broken.json");
  });

  test("設定を取得できないときは、操作部品を止めて次の操作を出す", async ({ page }) => {
    await openApp(page, { failSettings: true });

    await expect(id(page, "settings-error")).toHaveText(ja["settings.loadFailed"]);
    await expect(id(page, "setting-volume")).toBeDisabled();
    await expect(id(page, "setting-language")).toBeDisabled();
    expect(await updateCalls(page)).toEqual([]);
  });
});

// ============================================================================
// レビュー指摘への対応（帯の詳細・通知の位置・幅・起動失敗など）
// ============================================================================

test.describe("レビュー指摘への対応", () => {
  test("「詳細」には開閉の目印（▸／▾）があり、押せる範囲が文字より広い", async ({ page }) => {
    await openApp(page);
    const summary = id(page, "status-details").locator("summary");
    const marker = () => summary.evaluate((el) => getComputedStyle(el, "::before").content);

    expect(await marker()).toContain("\u25B8");
    await summary.click();
    expect(await marker()).toContain("\u25BE");

    const padding = await summary.evaluate((el) => parseFloat(getComputedStyle(el).paddingTop));
    expect(padding).toBeGreaterThanOrEqual(4);
  });

  test("内部のエラー文字列は「エラー内容:」（英語は Error:）と書き、「詳細」と紛れない", async ({ page }) => {
    await openApp(page, { status: STATUS_AUDIO_FAILED });
    const detail = id(page, "status-issue-audio").locator(".detail-text");
    await expect(detail).toHaveText("エラー内容: no output device (test)");

    await openApp(page, { status: STATUS_AUDIO_FAILED, settings: { ...DEFAULT_SETTINGS, language: "en" } });
    await expect(detail).toHaveText("Error: no output device (test)");
  });

  test("画面の幅を制限しない（広い窓では .app が窓いっぱいに広がる）", async ({ page }) => {
    await page.setViewportSize({ width: 1600, height: 900 });
    await openApp(page);
    const width = await page.locator(".app").evaluate((el) => el.getBoundingClientRect().width);
    expect(width).toBeGreaterThan(1500);
    // 設定のフォームは読みやすい幅に収まる。
    const form = await id(page, "settings-form").evaluate((el) => el.getBoundingClientRect().width);
    expect(form).toBeLessThanOrEqual(640);
  });

  test("保存失敗と「既定で起動した」の通知は、どの区画を開いていても帯の下・タブの上に見える", async ({ page }) => {
    await openApp(page, { saveFailed: true, recovered: true });

    for (const tab of ["assign", "play", "settings"]) {
      await id(page, `tab-${tab}`).click();
      await expect(id(page, "settings-error")).toBeVisible();
      await expect(id(page, "settings-recovered")).toBeVisible();
    }

    // 設定の区画の中には置かない。位置は帯より下、タブより上。
    await expect(id(page, "panel-settings").locator('[data-testid="settings-error"]')).toHaveCount(0);
    await expect(id(page, "panel-settings").locator('[data-testid="settings-recovered"]')).toHaveCount(0);
    const y = (testId) => id(page, testId).evaluate((el) => el.getBoundingClientRect().top);
    expect(await y("settings-error")).toBeGreaterThan(await y("status-band"));
    expect(await y("settings-error")).toBeLessThan(await y("tab-settings"));
    expect(await y("settings-recovered")).toBeLessThan(await y("tab-settings"));
  });

  test("保存失敗の文は、事実と操作の2文だけ", async ({ page }) => {
    await openApp(page, { saveFailed: true });
    const text = await id(page, "settings-error").innerText();
    expect(text.split("。").filter(Boolean)).toHaveLength(2);
    expect(text).toContain("書き込めるか確認してください");
  });

  test("「既定の設定で起動しました」の行は、閉じるボタンで消せる", async ({ page }) => {
    await openApp(page, { recovered: true });
    await expect(id(page, "settings-recovered")).toBeVisible();

    await id(page, "settings-recovered-dismiss").click();
    await expect(id(page, "settings-recovered")).toBeHidden();

    // 設定を変えても、閉じた行は戻らない。
    await id(page, "setting-volume").fill("30");
    await expect.poll(() => updateCalls(page)).toHaveLength(1);
    await expect(id(page, "settings-recovered")).toBeHidden();
  });

  test("配列の「自動」の選択肢に、今どちらで描くかを出す", async ({ browser }) => {
    for (const [locale, shown] of [
      ["ja-JP", "自動（今は JIS）"],
      ["en-US", "Auto (currently US)"],
    ]) {
      const context = await browser.newContext({ locale, baseURL: "http://127.0.0.1:4173" });
      const page = await context.newPage();
      await openApp(page);
      await expect(id(page, "setting-layout").locator('option[value="auto"]'), locale).toHaveText(shown);
      await context.close();
    }
  });

  test("音量・強弱のスライダーは、読み上げ用の値（aria-valuetext）が「80%」の形で追従する", async ({ page }) => {
    await openApp(page);
    await expect(id(page, "setting-volume")).toHaveAttribute("aria-valuetext", "80%");

    await id(page, "setting-volume").fill("35");
    await expect(id(page, "setting-volume")).toHaveAttribute("aria-valuetext", "35%");
    await expect(id(page, "setting-dynamics")).toHaveAttribute("aria-valuetext", "60%");
  });

  test("起動中に例外が出たら、真っ白にせず次の操作を出す", async ({ page }) => {
    page.allowPageErrors = true;
    await openApp(page, { malformedSettings: true, noWait: true });

    // タイムアウト（5秒）を待たずに、例外を拾ってすぐ出ること。
    await expect(id(page, "startup-error")).toBeVisible({ timeout: 2000 });
    await expect(id(page, "startup-error")).toContainText("Restart drumclack");
    await expect(id(page, "startup-error")).toContainText("再起動してください");
  });

  test("起動が終わらないまま一定時間たったら、同じ表示を出す", async ({ page }) => {
    await page.clock.install();
    await openApp(page, { hangSettings: true });
    await expect(id(page, "startup-error")).toBeHidden();

    await page.clock.fastForward(6000);
    await expect(id(page, "startup-error")).toBeVisible();
  });

  test("起動に成功したときは、起動失敗の表示を出さない", async ({ page }) => {
    await openApp(page);
    await expect(id(page, "startup-error")).toBeHidden();
  });

  test("data-i18n-attr は許可した属性（aria-label など）だけを差し替え、onclick などは受け付けない", async ({ page }) => {
    await openApp(page);
    const result = await page.evaluate(() => {
      const el = document.createElement("div");
      el.dataset.i18nAttr = "onclick:nav.play;aria-label:nav.play;onfocus:nav.play";
      document.body.appendChild(el);
      window.drumclackI18n.apply(document.body);
      return { onclick: el.getAttribute("onclick"), onfocus: el.getAttribute("onfocus"), label: el.getAttribute("aria-label") };
    });
    expect(result).toEqual({ onclick: null, onfocus: null, label: ja["nav.play"] });
  });
});

// ============================================================================
// 配色（明るい配色・暗い配色の両方で WCAG AA 相当）
// ============================================================================

/** ページ内で、文字と実効の背景色のコントラスト比を全部測る。 */
function measureContrast(page) {
  return page.evaluate(() => {
    const parse = (value) => {
      const m = value.match(/rgba?\(([^)]+)\)/);
      const [r, g, b, a = 1] = m[1].split(/[ ,/]+/).filter(Boolean).map(Number);
      return { r, g, b, a };
    };
    const luminance = ({ r, g, b }) => {
      const channel = (v) => {
        const s = v / 255;
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
      };
      return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
    };
    const ratio = (a, b) => {
      const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
      return (hi + 0.05) / (lo + 0.05);
    };
    const backgroundOf = (el) => {
      for (let node = el; node; node = node.parentElement) {
        const color = parse(getComputedStyle(node).backgroundColor);
        if (color.a > 0) return color;
      }
      return parse(getComputedStyle(document.body).backgroundColor);
    };
    const toRgb = (cssColor) => {
      const probe = document.createElement("span");
      probe.style.color = cssColor;
      document.body.appendChild(probe);
      const value = parse(getComputedStyle(probe).color);
      probe.remove();
      return value;
    };

    const text = [];
    for (const el of document.querySelectorAll("body *")) {
      const own = Array.from(el.childNodes).some((n) => n.nodeType === 3 && n.textContent.trim());
      const hasBox = el.getClientRects().length > 0 && getComputedStyle(el).visibility !== "hidden";
      if (!own || !hasBox) continue;
      const style = getComputedStyle(el);
      text.push({
        what: `${el.tagName.toLowerCase()}[${el.dataset.testid || el.className || ""}]`,
        ratio: ratio(parse(style.color), backgroundOf(el)),
      });
    }

    const root = getComputedStyle(document.documentElement);
    const bg = toRgb(root.getPropertyValue("--bg"));
    const nonText = ["--border", "--accent", "--ok-fg", "--warn-fg", "--error-fg"].map((name) => ({
      what: name,
      ratio: ratio(toRgb(root.getPropertyValue(name)), bg),
    }));
    return { text, nonText };
  });
}

test.describe("配色のコントラスト", () => {
  const states = {
    正常: { status: STATUS_OK },
    "未許可＋音声デバイス失敗＋詳細": {
      status: { ...STATUS_PERMISSION_DENIED, audio: STATUS_AUDIO_FAILED.audio, test_delay_ms: 100 },
      recovered: true,
      saveFailed: true,
    },
    "許可の状態が不明": { status: STATUS_PERMISSION_UNKNOWN },
  };

  for (const scheme of ["light", "dark"]) {
    for (const [name, options] of Object.entries(states)) {
      test(`${scheme}・${name}: 文字は 4.5:1、枠と状態の色は 3:1 以上`, async ({ page }) => {
        await page.emulateMedia({ colorScheme: scheme });
        await openApp(page, options);
        await id(page, "status-details").locator("summary").click();

        const { text, nonText } = await measureContrast(page);
        expect(text.length).toBeGreaterThan(10);
        for (const item of text) {
          expect(item.ratio, `文字 ${item.what}`).toBeGreaterThanOrEqual(4.5);
        }
        for (const item of nonText) {
          expect(item.ratio, `色 ${item.what}`).toBeGreaterThanOrEqual(3);
        }
      });
    }
  }
});

// ============================================================================
// 橋渡し・ビルド工程なし
// ============================================================================

test.describe("橋渡しとビルド工程", () => {
  test("画面のスクリプトは __TAURI__ に直接触らない（橋渡しを通す）", async () => {
    for (const file of ["main.js", "i18n.js"]) {
      expect(readWeb(file), file).not.toMatch(/__TAURI__/);
    }
    expect(readWeb("bridge.js")).toMatch(/__TAURI__/);
  });

  test("画面が呼ぶ命令は、許可している3つだけ", async ({ page }) => {
    await openApp(page);
    await id(page, "setting-volume").fill("30");
    await id(page, "setting-layout").selectOption("us");
    await expect.poll(async () => (await updateCalls(page)).length).toBe(2);

    const names = new Set((await allCalls(page)).map((call) => call.cmd));
    expect([...names].sort()).toEqual(["get_settings", "get_status", "update_settings"]);
  });
});

// ============================================================================
// 常駐（メニューバー／トレイ）との接点
// ============================================================================

test.describe("常駐との接点", () => {
  test("窓を閉じても終了しないことを、窓の中の文で伝える（日英とも）", async ({ page }) => {
    await openApp(page, { settings: { ...DEFAULT_SETTINGS, language: "ja" } });
    await expect(id(page, "resident-note")).toBeVisible();
    await expect(id(page, "resident-note")).toHaveText(ja["app.resident"]);

    await id(page, "setting-language").selectOption("en");
    await expect(id(page, "resident-note")).toHaveText(en["app.resident"]);
    expect(ja["app.resident"]).not.toBe(en["app.resident"]);
    // 短い脚注であること（長い説明にしない）。
    expect(ja["app.resident"].length).toBeLessThanOrEqual(60);
    expect(en["app.resident"].length).toBeLessThanOrEqual(130);
  });

  test("常駐の文は、状態の帯より下に置く（見出しの直下で警告に先立たない）", async ({ page }) => {
    await openApp(page);
    const noteIsAfterBand = await page.evaluate(() => {
      const band = document.querySelector('[data-testid="status-band"]');
      const note = document.querySelector('[data-testid="resident-note"]');
      return Boolean(band.compareDocumentPosition(note) & Node.DOCUMENT_POSITION_FOLLOWING);
    });
    expect(noteIsAfterBand).toBe(true);
  });

  test("Rust が呼ぶ入口で、演奏の区画だけが開く。知らない名前は何も変えない", async ({ page }) => {
    await openApp(page);
    await expect(id(page, "panel-settings")).toBeVisible();

    await page.evaluate(() => window.drumclackShowSection("nope"));
    await expect(id(page, "panel-settings")).toBeVisible();

    await page.evaluate(() => window.drumclackShowSection("play"));
    await expect(id(page, "panel-play")).toBeVisible();
    await expect(id(page, "panel-settings")).toBeHidden();
    await expect(id(page, "tab-play")).toHaveAttribute("aria-selected", "true");

    await page.evaluate(() => window.drumclackShowSection("settings"));
    await expect(id(page, "panel-settings")).toBeVisible();
    await expect(id(page, "panel-play")).toBeHidden();
  });
});
