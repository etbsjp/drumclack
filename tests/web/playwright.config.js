const { defineConfig } = require("@playwright/test");

const PORT = 4173;

module.exports = defineConfig({
  testDir: ".",
  testMatch: "*.spec.js",
  fullyParallel: true,
  retries: 0,
  reporter: [["list"]],
  use: {
    // 既定の言語（auto）は OS の言語から決まるので、文言を日本語で確かめるテストは ja-JP の環境で開く。
    locale: "ja-JP",
    baseURL: `http://127.0.0.1:${PORT}`,
    // 実際の窓の最小サイズ（tauri.conf.json の minWidth × minHeight）で確かめる。
    viewport: { width: 900, height: 600 },
  },
  webServer: {
    command: "node serve.js",
    url: `http://127.0.0.1:${PORT}/index.html`,
    reuseExistingServer: false,
    env: { PORT: String(PORT) },
  },
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
});
