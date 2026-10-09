fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "workspace",
            "import_roster",
            "prepare_input_version",
            "accept_input_version",
            "preview_legacy",
            "migrate_legacy",
            "open_browser",
            "run_queue",
            "pause_queue",
            "review_task",
            "run_step",
            "adopt_file",
            "export_report",
            "open_folder",
            "ai_settings",
            "save_ai_settings",
            "classify_task",
            "templates",
            "register_template",
            "fill_template",
            "browser_result",
        ]),
    ))
    .expect("Tauri build failed");
}
