use super::{local, sources::remember};
use crate::engine::Engine;
use library_core::{
    source_downloads::Site,
    source_files::{self, ReadOptions},
    *,
};
use serde_json::{json, Value};
use tauri::{AppHandle, State, WebviewWindow};

#[tauri::command]
pub(crate) fn source_browser_state(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
) -> Result<Value> {
    local(&window)?;
    let task = state.store.task(&id)?;
    let sites = state.store.source_sites()?;
    Ok(
        json!({"sites":sites,"downloads":state.store.source_downloads(&id)?,"windows":state.browser.source_window_states(&app,&task,&sites)?}),
    )
}
#[tauri::command]
pub(crate) fn save_source_site(
    window: WebviewWindow,
    state: State<Engine>,
    site: Site,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    Ok(json!(state.store.save_source_site(site)?))
}
#[tauri::command]
pub(crate) fn open_source_browser(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
    channel: String,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let session = state.store.source_session(&id, &channel)?;
    state.browser.open_source(&app, &state.store, session)
}
#[tauri::command]
pub(crate) fn focus_source_browser(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
    label: String,
) -> Result<Value> {
    local(&window)?;
    state.store.task(&id)?;
    state.browser.manage_source_window(&app, &id, &label, false)
}
#[tauri::command]
pub(crate) fn close_source_browser(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
    label: String,
) -> Result<Value> {
    local(&window)?;
    state.store.task(&id)?;
    state.browser.manage_source_window(&app, &id, &label, true)
}
#[tauri::command]
pub(crate) fn preview_source_download(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
    download_id: String,
    options: ReadOptions,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    if state.store.pending_input(&id)?.is_some() || !state.store.unresolved(&id)?.is_empty() {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "先处理名单版本或待确认操作。",
        ));
    }
    let receipt = state.store.source_download_for_preview(&id, &download_id)?;
    let task = state.store.task(&id)?;
    let mut draft = source_files::prepare(
        &state.store.root,
        &task,
        &receipt.session.site.channel,
        std::path::Path::new(&receipt.path),
        options,
    )?;
    if draft.sha256 != receipt.sha256.as_deref().unwrap_or("") {
        return Err(Failure::new(
            "SOURCE_DOWNLOAD_INVALID",
            "读取期间原始下载变化。",
        ));
    }
    draft.original_name = receipt.original_name;
    let document = source_files::check_draft(&state.store.root, &task, &draft)?;
    let page = document.page(&draft, None, 0, 0)?;
    remember(&state, draft)?;
    state.changed(&app);
    Ok(page)
}
