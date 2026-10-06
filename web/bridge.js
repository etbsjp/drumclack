// Rust との橋渡し。画面から Rust の命令を呼ぶ・Rust のイベントを受け取るのは、必ずここを通す。
// 画面のほかのファイルは `window.__TAURI__` に直接触らない。
//
// 公開する命令は capabilities/default.json に許可を書いたものだけ。足すときは
// build.rs の一覧・capabilities・ここの3か所を同じ変更で揃える。
// 画面のテストは `window.__TAURI__` を差し替えて、ここから出る呼び出しの名前と引数を確かめる。

(function () {
  function tauri() {
    const api = window.__TAURI__;
    if (!api || !api.core) {
      throw new Error("Tauri の API が見つかりません");
    }
    return api;
  }

  window.drumclackBridge = {
    /** 状態のスナップショット（権限・音声デバイス・直近の発音・同時発音数）。 */
    getStatus() {
      return tauri().core.invoke("get_status");
    },

    /** 設定のスナップショット `{ settings, recovered_from_broken, save_failed }`。 */
    getSettings() {
      return tauri().core.invoke("get_settings");
    },

    /**
     * 設定を更新する。`patch` は変えた項目だけ（全体を送り返さない）。
     * Rust は整えた設定全体のスナップショットを返す。
     */
    updateSettings(patch) {
      return tauri().core.invoke("update_settings", { settings: patch });
    },

    /**
     * Rust からのイベントを受け取る。解除する関数を返す。
     * 使うイベントを足すときは、capabilities に `core:event:allow-listen` も足す。
     */
    async listen(eventName, handler) {
      return tauri().event.listen(eventName, (event) => handler(event.payload));
    },
  };
})();
