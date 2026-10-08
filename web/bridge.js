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

    /** 音の名前（`kick` など）を指定して1回鳴らす（試聴。オフ中でも鳴る）。 */
    previewSound(sound) {
      return tauri().core.invoke("preview_sound", { sound });
    },

    /** macOS の入力監視の設定画面を開く（許可を変えるのは利用者）。 */
    openInputMonitoringSettings() {
      return tauri().core.invoke("open_input_monitoring_settings");
    },

    /** アプリ自身を起動し直す。成功すると、この窓ごと終了するので返事は来ない。 */
    restartApp() {
      return tauri().core.invoke("restart_app");
    },

    /**
     * 演奏の区画を開いた／閉じたと Rust に知らせる。窓が最前面のあいだだけ演奏用の割り当てを使い、
     * 返事は「いま演奏用の割り当てを使っているか」（真偽）。
     */
    setPlayViewOpen(open) {
      return tauri().core.invoke("set_play_view_open", { open });
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
