use super::local;
use crate::engine::Engine;
use library_core::*;
use serde_json::Value;
use tauri::{AppHandle, State, WebviewWindow};

#[tauri::command]
pub(crate) async fn open_browser(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    role: String,
    entry: Option<String>,
) -> Result<()> {
    local(&window)?;
    if let Some(entry) = entry {
        if role != "wos" || entry != "database_directory" {
            return Err(Failure::new("INVALID_CHANNEL", "未注册的数据库访问入口。"));
        }
        let _lease = state.acquire()?;
        state.browser.open(&app, &state.store.root, &role).await?;
        state.browser.database_directory(&app)?;
        state.changed(&app);
        return Ok(());
    }
    state.browser.open(&app, &state.store.root, &role).await?;
    state.changed(&app);
    Ok(())
}
#[tauri::command]
pub(crate) fn browser_result(
    window: WebviewWindow,
    state: State<Engine>,
    request_id: String,
    result: Value,
) -> Result<()> {
    state.browser.reply(&window, &request_id, result)
}
