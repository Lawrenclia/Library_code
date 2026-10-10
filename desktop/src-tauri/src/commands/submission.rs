use super::local;
use crate::engine::Engine;
use library_core::*;
use serde_json::Value;
use tauri::{AppHandle, State, WebviewWindow};

#[tauri::command]
pub(crate) fn submission_options(
    window: WebviewWindow,
    state: State<Engine>,
    id: String,
) -> Result<Value> {
    local(&window)?;
    library_core::submission::options(&state.store, &id)
}
#[tauri::command]
pub(crate) fn prepare_submission(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
    recipe: String,
    task_revision: i64,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let result = library_core::submission::prepare(&state.store, &id, &recipe, task_revision);
    state.changed(&app);
    result
}
