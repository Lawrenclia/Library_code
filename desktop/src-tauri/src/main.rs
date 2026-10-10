#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod adapters;
mod ai;
#[cfg(feature = "smoke-test")]
mod ai_smoke;
#[cfg(feature = "smoke-test")]
mod alias_smoke;
mod browser;
#[cfg(feature = "smoke-test")]
mod claim_smoke;
mod commands;
#[cfg(feature = "smoke-test")]
mod download_smoke;
mod engine;
#[cfg(feature = "smoke-test")]
mod link_smoke;
#[cfg(feature = "smoke-test")]
mod merge_smoke;
#[cfg(feature = "smoke-test")]
mod metadata_smoke;
#[cfg(feature = "smoke-test")]
mod queue_smoke;
#[cfg(feature = "smoke-test")]
mod restart_smoke;
#[cfg(feature = "smoke-test")]
mod smoke;
use engine::Engine;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            #[cfg(not(feature = "smoke-test"))]
            let root = app.path().app_local_data_dir()?;
            #[cfg(feature = "smoke-test")]
            let root = smoke::workspace_root()?;
            let engine = Engine::new(root).map_err(|e| std::io::Error::other(e.message))?;
            app.manage(engine);
            #[cfg(feature = "smoke-test")]
            smoke::setup(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspace::workspace,
            commands::workspace::import_roster,
            commands::workspace::prepare_input_version,
            commands::workspace::accept_input_version,
            commands::workspace::preview_legacy,
            commands::workspace::migrate_legacy,
            commands::browsers::open_browser,
            commands::tasks::run_queue,
            commands::tasks::pause_queue,
            commands::tasks::resume_queue,
            commands::tasks::cancel_queue,
            commands::tasks::review_task,
            commands::tasks::run_step,
            commands::materials::adopt_file,
            commands::sources::preview_source_file,
            commands::sources::source_file_page,
            commands::sources::attach_source_file,
            commands::workspace::export_report,
            commands::workspace::open_folder,
            commands::models::ai_settings,
            commands::models::save_ai_settings,
            commands::models::classify_task,
            commands::models::run_ai_queue,
            commands::models::resume_ai_queue,
            commands::models::pause_ai_queue,
            commands::models::cancel_ai_queue,
            commands::materials::templates,
            commands::materials::register_template,
            commands::materials::fill_template,
            commands::browsers::browser_result
        ])
        .run(tauri::generate_context!())
        .expect("工作台启动失败");
}
