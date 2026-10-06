// 日英の対応表を読み、画面の文言を差し替える（翻訳用のライブラリは使わない）。
//
// - 静的な文言は要素に `data-i18n="キー"` を付ける。属性（aria-label など）は
//   `data-i18n-attr="属性名:キー"`（複数は `;` 区切り）。
// - 動的な文言は `drumclackI18n.t("キー", { 名前: 値 })` を通す。`{名前}` が差し込み位置。
// - キーが対応表に無いときは、キーそのものを出す（抜けに気づけるように）。

(function () {
  const SUPPORTED = ["ja", "en"];
  // `data-i18n-attr` で差し替えてよい属性。onclick など、動作を持つ属性は受け付けない。
  const TRANSLATABLE_ATTRIBUTES = ["aria-label", "title", "placeholder", "alt"];
  const dictionaries = {};
  let current = "ja";

  /** `auto` を OS の言語から決める。日本語なら ja、それ以外は en。 */
  function resolveAuto(osLanguage) {
    return String(osLanguage || "").toLowerCase().startsWith("ja") ? "ja" : "en";
  }

  async function load(lang) {
    if (!dictionaries[lang]) {
      const response = await fetch(`./i18n/${lang}.json`);
      if (!response.ok) {
        throw new Error(`対応表を読めません: ${lang}`);
      }
      dictionaries[lang] = await response.json();
    }
    return dictionaries[lang];
  }

  function t(key, params) {
    const table = dictionaries[current] || {};
    let text = Object.prototype.hasOwnProperty.call(table, key) ? table[key] : key;
    if (params) {
      for (const [name, value] of Object.entries(params)) {
        text = text.split(`{${name}}`).join(String(value));
      }
    }
    return text;
  }

  function apply(root) {
    for (const el of root.querySelectorAll("[data-i18n]")) {
      el.textContent = t(el.dataset.i18n);
    }
    for (const el of root.querySelectorAll("[data-i18n-attr]")) {
      for (const pair of el.dataset.i18nAttr.split(";")) {
        const [attr, key] = pair.split(":");
        if (attr && key && TRANSLATABLE_ATTRIBUTES.includes(attr.trim())) {
          el.setAttribute(attr.trim(), t(key.trim()));
        }
      }
    }
  }

  /** 言語を切り替える。対応表を読めなければ現在の言語のまま（呼び出し側が失敗として扱える）。 */
  async function setLanguage(lang) {
    if (!SUPPORTED.includes(lang)) {
      throw new Error(`知らない言語です: ${lang}`);
    }
    await load(lang);
    current = lang;
    document.documentElement.lang = lang;
    apply(document);
  }

  window.drumclackI18n = {
    SUPPORTED,
    resolveAuto,
    t,
    apply,
    setLanguage,
    get language() {
      return current;
    },
  };
})();
