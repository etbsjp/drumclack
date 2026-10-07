// キーボードの絵の配置（JIS と US）。割り当ての画面（assign.js）が読む。
//
// ここにあるのは「どの位置のキーを、どこに、どの大きさで描くか」だけ。音の割り当ては持たない。
// キーは Web 標準の `code` 名で書く（文字には変換しない）。キートップの刻印（`label`）は、
// 実物のキーボードに印字されている文字そのもので、画面の文言ではないため対応表には入れない
// （日本語配列の「英数」「かな」などもここ）。
//
// 座標の単位は「キー1個ぶん」。`x` `y` は左上、`w` `h` は幅と高さ。移動キーとテンキーは常に描く。

(function () {
  const MAIN_WIDTH = 15;
  const CLUSTER_GAP = 0.5;
  const NAV_X = MAIN_WIDTH + CLUSTER_GAP; // 移動キー
  const PAD_X = NAV_X + 3 + CLUSTER_GAP; // テンキー
  const ROW_GAP = 0.4; // ファンクション行と、その下の段の間
  const rowY = (row) => (row === 0 ? 0 : row + ROW_GAP);

  // 読み上げ・一覧で使う名前（刻印だけでは何のキーか分からないもの）。
  const NAMES = {
    Backquote: "Backquote",
    ShiftLeft: "Left Shift",
    ShiftRight: "Right Shift",
    ControlLeft: "Left Ctrl",
    ControlRight: "Right Ctrl",
    AltLeft: "Left Alt/Option",
    AltRight: "Right Alt/Option",
    MetaLeft: "Left Cmd/Win",
    MetaRight: "Right Cmd/Win",
    ContextMenu: "Menu",
    ArrowUp: "Arrow Up",
    ArrowDown: "Arrow Down",
    ArrowLeft: "Arrow Left",
    ArrowRight: "Arrow Right",
    PageUp: "Page Up",
    PageDown: "Page Down",
    PrintScreen: "Print Screen",
    ScrollLock: "Scroll Lock",
    NumLock: "Num Lock",
    CapsLock: "Caps Lock",
    Minus: "-",
    Equal: "=",
    Semicolon: ";",
    Comma: ",",
    Period: ".",
    Slash: "/",
    Backslash: "\\",
    BracketLeft: "[",
    BracketRight: "]",
    Quote: "'",
    IntlYen: "¥",
    IntlRo: "_",
    NumpadAdd: "+",
    NumpadSubtract: "-",
    NumpadMultiply: "*",
    NumpadDivide: "/",
    NumpadDecimal: ".",
    NumpadEnter: "Enter",
  };

  /**
   * 1段ぶんを並べる。要素は `[code, 刻印, 幅?, 高さ?]` か、数（その幅の空き）。
   */
  function row(keys, x0, y, tokens) {
    let x = x0;
    for (const token of tokens) {
      if (typeof token === "number") {
        x += token;
        continue;
      }
      const [code, label, w = 1, h = 1] = token;
      keys.push({ code, label, x, y, w, h });
      x += w;
    }
  }

  const letters = (codes) => codes.split("").map((c) => [`Key${c}`, c]);
  const digits = () => [1, 2, 3, 4, 5, 6, 7, 8, 9, 0].map((d) => [`Digit${d}`, String(d)]);
  const fKeys = (from, to) =>
    Array.from({ length: to - from + 1 }, (_, i) => [`F${from + i}`, `F${from + i}`]);

  /** 移動キーとテンキー（JIS と US で共通）。 */
  function addClusters(keys) {
    row(keys, NAV_X, rowY(0), [["PrintScreen", "PrtSc"], ["ScrollLock", "ScrLk"], ["Pause", "Pause"]]);
    row(keys, NAV_X, rowY(1), [["Insert", "Ins"], ["Home", "Home"], ["PageUp", "PgUp"]]);
    row(keys, NAV_X, rowY(2), [["Delete", "Del"], ["End", "End"], ["PageDown", "PgDn"]]);
    row(keys, NAV_X, rowY(4), [1, ["ArrowUp", "↑"]]);
    row(keys, NAV_X, rowY(5), [["ArrowLeft", "←"], ["ArrowDown", "↓"], ["ArrowRight", "→"]]);

    row(keys, PAD_X, rowY(1), [["NumLock", "Num"], ["NumpadDivide", "/"], ["NumpadMultiply", "*"], ["NumpadSubtract", "-"]]);
    row(keys, PAD_X, rowY(2), [["Numpad7", "7"], ["Numpad8", "8"], ["Numpad9", "9"], ["NumpadAdd", "+", 1, 2]]);
    row(keys, PAD_X, rowY(3), [["Numpad4", "4"], ["Numpad5", "5"], ["Numpad6", "6"]]);
    row(keys, PAD_X, rowY(4), [["Numpad1", "1"], ["Numpad2", "2"], ["Numpad3", "3"], ["NumpadEnter", "Enter", 1, 2]]);
    row(keys, PAD_X, rowY(5), [["Numpad0", "0", 2], ["NumpadDecimal", "."]]);
  }

  function us() {
    const keys = [];
    row(keys, 0, rowY(0), [["Escape", "Esc"], 0.5, ...fKeys(1, 4), 0.5, ...fKeys(5, 8), 0.5, ...fKeys(9, 12)]);
    row(keys, 0, rowY(1), [
      ["Backquote", "`"], ...digits(), ["Minus", "-"], ["Equal", "="], ["Backspace", "Backspace", 2],
    ]);
    row(keys, 0, rowY(2), [
      ["Tab", "Tab", 1.5], ...letters("QWERTYUIOP"), ["BracketLeft", "["], ["BracketRight", "]"], ["Backslash", "\\", 1.5],
    ]);
    row(keys, 0, rowY(3), [
      ["CapsLock", "Caps", 1.75], ...letters("ASDFGHJKL"), ["Semicolon", ";"], ["Quote", "'"], ["Enter", "Enter", 2.25],
    ]);
    row(keys, 0, rowY(4), [
      ["ShiftLeft", "Shift", 2.25], ...letters("ZXCVBNM"), ["Comma", ","], ["Period", "."], ["Slash", "/"], ["ShiftRight", "Shift", 2.75],
    ]);
    row(keys, 0, rowY(5), [
      ["ControlLeft", "Ctrl", 1.25], ["MetaLeft", "Cmd/Win", 1.25], ["AltLeft", "Alt/Opt", 1.25],
      ["Space", "Space", 6.25],
      ["AltRight", "Alt/Opt", 1.25], ["MetaRight", "Cmd/Win", 1.25], ["ContextMenu", "Menu", 1.25], ["ControlRight", "Ctrl", 1.25],
    ]);
    addClusters(keys);
    return keys;
  }

  function jis() {
    const keys = [];
    row(keys, 0, rowY(0), [["Escape", "Esc"], 0.5, ...fKeys(1, 4), 0.5, ...fKeys(5, 8), 0.5, ...fKeys(9, 12)]);
    row(keys, 0, rowY(1), [
      ["Backquote", "半/全"], ...digits(), ["Minus", "-"], ["Equal", "^"], ["IntlYen", "¥"], ["Backspace", "BS"],
    ]);
    // Enter は2段にまたがる大きなキー（実物の L 字を長方形で近似する）。
    row(keys, 0, rowY(2), [
      ["Tab", "Tab", 1.5], ...letters("QWERTYUIOP"), ["BracketLeft", "@"], ["BracketRight", "["], ["Enter", "Enter", 1.5, 2],
    ]);
    row(keys, 0, rowY(3), [
      ["CapsLock", "英数", 1.5], ...letters("ASDFGHJKL"), ["Semicolon", ";"], ["Quote", ":"], ["Backslash", "]"],
    ]);
    row(keys, 0, rowY(4), [
      ["ShiftLeft", "Shift", 2.25], ...letters("ZXCVBNM"), ["Comma", ","], ["Period", "."], ["Slash", "/"], ["IntlRo", "_"], ["ShiftRight", "Shift", 1.75],
    ]);
    row(keys, 0, rowY(5), [
      ["ControlLeft", "Ctrl", 1.25], ["MetaLeft", "Cmd/Win", 1.25], ["AltLeft", "Alt/Opt", 1.25],
      ["NonConvert", "無変換", 1.25], ["Space", "Space", 3.75], ["Convert", "変換", 1.25], ["KanaMode", "かな", 1.25],
      ["MetaRight", "Cmd/Win", 1.25], ["ContextMenu", "Menu", 1.25], ["ControlRight", "Ctrl", 1.25],
    ]);
    addClusters(keys);
    return keys;
  }

  window.drumclackLayouts = {
    NAMES,
    layouts: { jis: jis(), us: us() },
    /** 絵の全体の大きさ（キー1個ぶんの単位）。 */
    width: PAD_X + 4,
    height: rowY(5) + 1,
  };
})();
