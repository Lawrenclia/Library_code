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
pub(crate) async fn classify_task(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
    template: Option<Value>,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let r = ai::classify(&state, &id, template).await;
    state.changed(&app);
    r
}
