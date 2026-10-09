use super::local;
use crate::ai;
use crate::engine::Engine;
use library_core::*;
use serde_json::{json, Value};
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
    let template = if let Some(requested) = template {
        let registered: Vec<library_core::templates::Template> =
            serde_json::from_value(state.store.setting("templates")?.unwrap_or(json!([])))?;
        let registered = registered
            .iter()
            .find(|t| requested["id"] == t.id)
            .ok_or_else(|| Failure::new("TEMPLATE_INVALID", "选择的模板没有注册。"))?;
        let mut actual = library_core::templates::inspect(
            std::path::Path::new(&registered.path),
            &registered.sheet,
            registered.header_row,
            registered.required.clone(),
            registered.notes.clone(),
        )?;
        if actual.fingerprint != registered.fingerprint || actual.columns != registered.columns {
            return Err(Failure::new(
                "TEMPLATE_CHANGED",
                "模板已变化，请重新注册后调用 AI。",
            ));
        }
        actual.id = registered.id.clone();
        Some(json!(actual))
    } else {
        None
    };
    let r = ai::classify(&state, &id, template).await;
    state.changed(&app);
    r
}
