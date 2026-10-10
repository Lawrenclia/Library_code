use super::local;
use crate::engine::Engine;
use library_core::*;
use serde_json::Value;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State, WebviewWindow};

#[tauri::command]
pub(crate) async fn run_queue(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    owner: String,
    retry_skipped: bool,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire_download()?;
    let r = state.queue(&app, &owner, retry_skipped).await;
    state.changed(&app);
    r
}
#[tauri::command]
pub(crate) fn pause_queue(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
) -> Result<()> {
    local(&window)?;
    state.store.request_download_pause()?;
    state.store.request_ai_pause()?;
    state.store.request_material_pause()?;
    state.pause.store(true, Ordering::SeqCst);
    state.changed(&app);
    Ok(())
}
#[tauri::command]
pub(crate) async fn resume_queue(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire_download()?;
    let result = state.resume_queue(&app, &id).await;
    state.changed(&app);
    result
}
#[tauri::command]
pub(crate) fn cancel_queue(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire()?;
    state.store.cancel_download_queue(&id)?;
    state.changed(&app);
    Ok(())
}
#[tauri::command]
pub(crate) fn review_task(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
    review: Review,
    source: String,
    proof: String,
) -> Result<Task> {
    local(&window)?;
    let _lease = state.acquire()?;
    let t = state.review(&id, review, source, proof)?;
    state.changed(&app);
    Ok(t)
}
#[tauri::command]
pub(crate) async fn run_step(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
    action: String,
    approved: bool,
    extra: Value,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let r = state.step(&app, &id, &action, approved, extra).await;
    state.changed(&app);
    r
}

#[tauri::command]
pub(crate) fn record_search_scope(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
    revision: i64,
    input_hash: String,
    observation: library_core::search_scopes::Observation,
) -> Result<Evidence> {
    local(&window)?;
    let _lease = state.acquire()?;
    let result = state
        .store
        .record_search_scope(&id, revision, &input_hash, observation)?;
    state.changed(&app);
    Ok(result)
}
