// 画面の起動に失敗したときの最低限の表示。ほかのスクリプトより先に読む。
// 画面は起動が終わるまで隠してある（対応表が入る前に、キーのままの文字を見せないため）。
// 起動中に例外が出たり、一定時間たっても終わらなかったりすると、真っ白のまま止まって見える。
// ここで、対応表に頼らない文言（英語と日本語の直書き）で、次の操作を出す。

(function () {
  const STARTUP_TIMEOUT_MS = 5000;
  const MESSAGE = [
    "drumclack could not start its window. Restart drumclack.",
    "drumclack の画面を起動できませんでした。drumclack を再起動してください。",
  ];

  function showStartupError() {
    if (document.body.dataset.ready === "true") {
      return;
    }
    const el = document.querySelector('[data-testid="startup-error"]');
    if (el) {
      el.replaceChildren(
        ...MESSAGE.map((line) => {
          const span = document.createElement("span");
          span.textContent = line;
          span.className = "startup-error-line";
          return span;
        }),
      );
      el.hidden = false;
    }
    // 隠したままにしない（出せる部分だけでも見せる）。
    document.body.dataset.ready = "true";
  }

  window.addEventListener("error", showStartupError);
  window.addEventListener("unhandledrejection", showStartupError);
  setTimeout(showStartupError, STARTUP_TIMEOUT_MS);
})();
