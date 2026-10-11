fn main() {
    // 画面へ公開する命令の一覧。足したら capabilities/default.json にも許可を足す
    // （片方だけだと、画面からその命令が呼べない）。
    let app_commands = tauri_build::AppManifest::new().commands(&[
        "get_status",
        "get_settings",
        "update_settings",
        "preview_sound",
        "open_input_monitoring_settings",
        "restart_app",
        "set_play_view_open",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(app_commands))
        .expect("tauri-build に失敗しました");
}
