// 画面のテスト用の簡易 HTTP サーバ。依存なし。
// 対応表（i18n/*.json）の読み込みは fetch を使うため、`file://` では動かない。web/ をここで配る。
// 実物の窓と同じ制限で動かすため、tauri.conf.json の CSP をそのまま応答ヘッダに付ける
// （インラインのスクリプト・スタイル・許可していない通信が混ざると、画面のテストが落ちる）。

const http = require("node:http");
const fs = require("node:fs");
const path = require("node:path");

const WEB_ROOT = path.resolve(__dirname, "../../web");
const CONFIG = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../src-tauri/tauri.conf.json"), "utf8"));
const CSP = CONFIG.app.security.csp;
const PORT = Number(process.env.PORT || 4173);

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
};

const server = http.createServer((req, res) => {
  const url = new URL(req.url, "http://localhost");
  const relative = decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname);
  const file = path.resolve(WEB_ROOT, `.${relative}`);

  // web/ の外は配らない。
  if (!file.startsWith(WEB_ROOT + path.sep)) {
    res.writeHead(403).end();
    return;
  }
  fs.readFile(file, (error, body) => {
    if (error) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, {
      "Content-Type": TYPES[path.extname(file)] || "application/octet-stream",
      "Content-Security-Policy": CSP,
      "Cache-Control": "no-store",
    });
    res.end(body);
  });
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`serving ${WEB_ROOT} on http://127.0.0.1:${PORT}`);
});
