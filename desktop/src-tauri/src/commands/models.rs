use super::local;
use crate::ai;
use crate::engine::Engine;
use library_core::*;
use serde_json::Value;
use tauri::{AppHandle, State, WebviewWindow};

#[tauri::command]
pub(crate) fn ai_settings(window: WebviewWindow, state: State<Engine>) -> Result<Value> {
    local(&window)?;
    ai::settings(&state)
}
#[tauri::command]
pub(crate) fn save_ai_settings(
    window: WebviewWindow,
    state: State<Engine>,
    base: String,
    model: String,
    key: String,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire()?;
    ai::save(&state, base, model, key)
}

#[tauri::command]
pub(crate) async fn preview_legacy_api_key(
    window: WebviewWindow,
    state: State<'_, Engine>,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("选择旧版 runtime/model_api_key.dpapi")
        .add_filter("旧版加密密钥", &["dpapi"])
        .pick_file()
        .await
    else {
        return Ok(serde_json::json!({"cancelled":true}));
    };
    crate::legacy_key::preview(&state, file.path())
}
#[tauri::command]
pub(crate) fn import_legacy_api_key(
    window: WebviewWindow,
    state: State<Engine>,
    path: String,
    fingerprint: String,
    base: String,
    model: String,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    crate::legacy_key::import(
        &state,
        std::path::Path::new(&path),
        &fingerprint,
        &base,
        &model,
    )
}
#[tauri::command]
pub(crate) async fn classify_task(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
    template: Option<Value>,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let template = ai::resolve_template(
        &state,
        template.as_ref().map(|t| t["id"].as_str().unwrap_or("")),
    )?;
    let r = ai::classify(&state, &id, template).await;
    state.changed(&app);
    r
}

#[tauri::command]
pub(crate) async fn run_ai_queue(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    owner: String,
    zero_only: bool,
    template_id: Option<String>,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire_ai()?;
    let config = ai::public_config(&state)?;
    let template = ai::resolve_template(&state, template_id.as_deref())?;
    let queue = state
        .store
        .start_ai_queue(&owner, zero_only, config, template)?;
    state.changed(&app);
    let result = ai::queue_loop(&state, &app, &queue.id).await;
    state.changed(&app);
    result
}
#[tauri::command]
pub(crate) async fn resume_ai_queue(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire_ai()?;
    if state.store.ai_queue(&id)?.status == library_core::queue::QueueStatus::Running {
        state.store.recover_ai_queue()?;
    }
    let queue = state.store.ai_queue(&id)?;
    if !queue.status.unfinished() {
        state.changed(&app);
        return Ok(());
    }
    let config = ai::public_config(&state)?;
    let template = ai::resolve_template(
        &state,
        queue.template.as_ref().and_then(|t| t["id"].as_str()),
    )?;
    state
        .store
        .resume_ai_queue(&id, &config, template.as_ref())?;
    state.changed(&app);
    let result = ai::queue_loop(&state, &app, &id).await;
    state.changed(&app);
    result
}
#[tauri::command]
pub(crate) fn pause_ai_queue(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
) -> Result<()> {
    local(&window)?;
    state.store.request_ai_pause()?;
    state.pause.store(true, std::sync::atomic::Ordering::SeqCst);
    state.changed(&app);
    Ok(())
}
#[tauri::command]
pub(crate) fn cancel_ai_queue(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire()?;
    state.store.cancel_ai_queue(&id)?;
    state.changed(&app);
    Ok(())
}
