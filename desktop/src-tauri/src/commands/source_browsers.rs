use super::{local, sources::remember};
use crate::engine::Engine;
use library_core::{
    source_downloads::Site,
    source_files::{self, ReadOptions},
    *,
};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State, WebviewWindow};

/// User initiated CNKI search / metadata acquisition in its own authenticated webview.
#[tauri::command]
pub(crate) async fn cnki_source(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
    action: String,
    label: Option<String>,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    if action != "search" && action != "capture" {
        return Err(Failure::new(
            "INVALID_ACTION",
            "请选择 CNKI 检索或获取题录。",
        ));
    }
    if !state
        .store
        .source_sites()?
        .iter()
        .any(|s| s.channel == "cnki")
    {
        state.store.save_source_site(Site {
            channel: "cnki".into(),
            entry_url: cnki::ENTRY.into(),
            download_origins: vec!["https://kns8.cnki.net".into()],
        })?;
    }
    let current = state.store.source_session(&id, "cnki")?;
    let task = state.store.task(&id)?;
    if action == "search" {
        if task.record.title.trim().is_empty() {
            return Err(Failure::new("INPUT_INVALID", "本篇名单缺少题名。"));
        }
        let entry = url::Url::parse(&current.site.entry_url).map_err(Failure::storage)?;
        if !cnki::is_site(&entry) {
            return Err(Failure::new(
                "PAGE_UNSUPPORTED",
                "机构代理入口请使用‘打开来源窗口’并在网页检索，原始下载仍由工作台接收。",
            ));
        }
        let opened = state.browser.open_source(&app, &state.store, current)?;
        let label = opened["label"]
            .as_str()
            .ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "来源窗口未返回。"))?;
        let mut query = entry
            .join("/kns8s/defaultresult/index")
            .map_err(Failure::storage)?;
        query
            .query_pairs_mut()
            .append_pair("kw", &task.record.title)
            .append_pair("korder", "SU");
        app.get_webview_window(label)
            .ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "来源窗口已关闭。"))?
            .navigate(query)
            .map_err(Failure::storage)?;
        return Ok(opened);
    }
    let label =
        label.ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "请选择本篇 CNKI 详情窗口。"))?;
    let session = state
        .browser
        .source_windows
        .lock()
        .map_err(Failure::storage)?
        .get(&label)
        .cloned()
        .ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "来源窗口未登记或已关闭。"))?;
    if session.site.channel != "cnki"
        || session.input_hash != task.input_hash
        || json!(session.record) != json!(task.record)
        || json!(session.site) != json!(current.site)
    {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "窗口属于旧名单或旧入口，请重新打开本篇窗口。",
        ));
    }
    let response = state
        .browser
        .execute(&app, &label, "cnki_capture", json!({}), 35)
        .await?;
    let latest = state.store.task(&id)?;
    if latest.revision != task.revision
        || latest.input_hash != task.input_hash
        || json!(latest.record) != json!(task.record)
    {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "获取期间名单变化，请重新获取。",
        ));
    }
    let (draft, acquisition) = cnki::capture(&state.store.root, &task, &response)?;
    let document = source_files::check_draft(&state.store.root, &task, &draft)?;
    let mut page = document.page(&draft, None, 1, 0)?;
    page["acquisition"] = acquisition;
    remember(&state, draft)?;
    state.changed(&app);
    Ok(page)
}

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
    resave: Option<bool>,
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
    let mut draft = if resave.unwrap_or(false) {
        if receipt.session.site.channel != "cnki" {
            return Err(Failure::new("SOURCE_INVALID", "另存仅用于 CNKI Excel。"));
        }
        cnki_excel::normalize(
            &state.store.root,
            &task,
            std::path::Path::new(&receipt.path),
            &receipt.original_name,
            &options.encoding,
        )?
    } else {
        source_files::prepare(
            &state.store.root,
            &task,
            &receipt.session.site.channel,
            std::path::Path::new(&receipt.path),
            options,
        )?
    };
    let original_hash = draft
        .resave
        .as_ref()
        .map(|r| r.original_sha256.as_str())
        .unwrap_or(&draft.sha256);
    if original_hash != receipt.sha256.as_deref().unwrap_or("") {
        return Err(Failure::new(
            "SOURCE_DOWNLOAD_INVALID",
            "读取期间原始下载变化。",
        ));
    }
    if draft.resave.is_none() {
        draft.original_name = receipt.original_name;
    }
    let document = source_files::check_draft(&state.store.root, &task, &draft)?;
    let page = document.page(
        &draft,
        None,
        if draft.options.tagged_format.is_some() {
            1
        } else {
            0
        },
        0,
    )?;
    remember(&state, draft)?;
    state.changed(&app);
    Ok(page)
}
