// 日英の対応表（web/i18n/ja.json・en.json）の検査。ブラウザは使わない。

const { test, expect, readWeb } = require("./helpers");

const ja = JSON.parse(readWeb("i18n/ja.json"));
const en = JSON.parse(readWeb("i18n/en.json"));

/** 片方にしか無い項目（`{ onlyA, onlyB }`）。 */
function diffKeys(a, b) {
  const keysA = Object.keys(a);
  const keysB = Object.keys(b);
  return {
    onlyA: keysA.filter((key) => !(key in b)),
    onlyB: keysB.filter((key) => !(key in a)),
  };
}

function placeholders(text) {
  return (text.match(/\{[A-Za-z]+\}/g) || []).sort();
}

/** HTML と JS が使っている対応表のキー。 */
function usedKeys() {
  const html = readWeb("index.html");
  const keys = new Set();

  for (const match of html.matchAll(/data-i18n="([^"]+)"/g)) keys.add(match[1]);
  for (const match of html.matchAll(/data-i18n-attr="([^"]+)"/g)) {
    for (const pair of match[1].split(";")) keys.add(pair.split(":")[1].trim());
  }
  // JS 側は、キーを文字列リテラルで書く（組み立てない）。`"status.ok"` のような形の文字列をすべて拾う。
  for (const file of ["main.js", "assign.js", "play.js"]) {
    for (const match of readWeb(file).matchAll(/"([a-z]+(?:\.[A-Za-z]+)+)"/g)) keys.add(match[1]);
  }
  return [...keys];
}

test("ja と en の項目の集合が一致する", () => {
  const { onlyA, onlyB } = diffKeys(ja, en);
  expect({ jaOnly: onlyA, enOnly: onlyB }).toEqual({ jaOnly: [], enOnly: [] });
});

test("片方から1項目を消すと、集合の比較が不一致を報告する（検査が緩んでいない）", () => {
  const [removed, ...rest] = Object.keys(en);
  const enWithoutOne = Object.fromEntries(rest.map((key) => [key, en[key]]));

  expect(diffKeys(ja, enWithoutOne)).toEqual({ onlyA: [removed], onlyB: [] });
  expect(diffKeys(enWithoutOne, ja)).toEqual({ onlyA: [], onlyB: [removed] });
});

test("差し込み位置（{名前}）が ja と en で一致する", () => {
  for (const key of Object.keys(ja)) {
    expect(placeholders(en[key] ?? ""), key).toEqual(placeholders(ja[key]));
  }
});

test("画面が使うキーはすべて両方の対応表にあり、使われないキーは残っていない", () => {
  const used = usedKeys();
  expect(used.length).toBeGreaterThan(20); // 取り出しが空振りして素通りしていないこと

  expect(used.filter((key) => !(key in ja))).toEqual([]);
  expect(used.filter((key) => !(key in en))).toEqual([]);
  expect(Object.keys(ja).filter((key) => !used.includes(key))).toEqual([]);
});

test("HTML と main.js に日本語の文言を直書きしていない", () => {
  const withoutComments = (source) =>
    source
      .replace(/<!--[\s\S]*?-->/g, "")
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/(^|\s)\/\/.*$/gm, "")
      .split("\n")
      // 開発者向けのログと例外の文言は画面に出ない。
      .filter((line) => !/console\.error\(|new Error\(/.test(line))
      .join("\n");

  // layouts.js は、キートップの刻印（実物のキーボードに印字された文字）なので対象にしない。
  for (const file of ["index.html", "main.js", "i18n.js", "assign.js", "play.js"]) {
    const found = withoutComments(readWeb(file)).match(/[぀-ヿ一-鿿]+/g);
    expect(found, file).toBeNull();
  }
});
