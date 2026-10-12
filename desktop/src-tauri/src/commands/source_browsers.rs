use super::{local, sources::remember};
use crate::engine::Engine;
use library_core::{
    source_downloads::Site,
    source_files::{self, ReadOptions},
    *,
};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

fn cnki_progress(app: &AppHandle, id: &str, phase: &str, message: &str) {
    let _ = app.emit_to(
        "main",
        "source-acquisition-progress",
        json!({"id":id,"channel":"cnki","phase":phase,"message":message}),
    );
}

fn cnki_unchanged(state: &Engine, task: &Task) -> Result<()> {
    if state.pause.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(Failure::new(
            "PAUSED",
            "CNKI 获取已暂停，保留来源窗口和已保存文件。",
        ));
    }
    let latest = state.store.task(&task.id)?;
    if latest.revision != task.revision
        || latest.input_hash != task.input_hash
        || json!(latest.record) != json!(task.record)
        || state.store.pending_input(&task.id)?.is_some()
        || !state.store.unresolved(&task.id)?.is_empty()
    {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "获取期间名单或任务范围变化，请重新获取。",
        ));
    }
    Ok(())
}

async fn cnki_navigation(
    app: &AppHandle,
    state: &Engine,
    task: &Task,
    label: &str,
    target: &url::Url,
) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        cnki_unchanged(state, task)?;
        let window = app
            .get_webview_window(label)
            .ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "CNKI 来源窗口已关闭。"))?;
        let actual = window.url().map_err(Failure::storage)?;
        if cnki::is_site(&actual) && actual.path().starts_with("/verify/") {
            return Err(Failure::new(
                "AUTH_REQUIRED",
                "CNKI 已转入安全验证页，请在内置窗口完成验证码后继续；没有重新提交检索。",
            ));
        }
        if actual.origin() == target.origin()
            && actual.path() == target.path()
            && state
                .browser
                .source_loaded
                .lock()
                .map_err(Failure::storage)?
                .get(label)
                .is_some_and(|url| url == actual.as_str())
            && target
                .query_pairs()
                .all(|(k, v)| actual.query_pairs().any(|(ak, av)| ak == k && av == v))
        {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(Failure::new(
                if cnki::is_site(&actual) { "PAGE_TIMEOUT" } else { "AUTH_REQUIRED" },
                "CNKI 目标页面尚未加载完成，请在内置窗口核对网络、登录或验证后继续；未判定为无记录。",
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

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
    if !["search", "capture", "auto"].contains(&action.as_str()) {
        return Err(Failure::new(
            "INVALID_ACTION",
            "请选择 CNKI 检索、自动获取或当前详情题录。",
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
    let mut selected_label = label;
    let mut search = Value::Null;
    if action == "search" || action == "auto" {
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
        cnki_progress(&app, &id, "search", "正在打开名单题名检索页…");
        let opened = state
            .browser
            .open_source(&app, &state.store, current.clone())?;
        let label = opened["label"]
            .as_str()
            .ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "来源窗口未返回。"))?;
        let query = cnki::title_search(&entry, &task.record.title)?;
        state
            .browser
            .source_loaded
            .lock()
            .map_err(Failure::storage)?
            .remove(label);
        app.get_webview_window(label)
            .ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "来源窗口已关闭。"))?
            .navigate(query.clone())
            .map_err(Failure::storage)?;
        if action == "search" {
            return Ok(opened);
        }
        selected_label = Some(label.to_owned());
        cnki_navigation(&app, &state, &task, label, &query).await?;
        cnki_progress(&app, &id, "results", "等待完整结果并核对唯一精确题名…");
        search = state
            .browser
            .execute(
                &app,
                label,
                "cnki_results",
                json!({"title":task.record.title}),
                30,
            )
            .await?;
        cnki_unchanged(&state, &task)?;
        let target = cnki::select_result(&task, &search)?;
        if !current
            .site
            .download_origins
            .contains(&target.origin().ascii_serialization())
        {
            return Err(Failure::new(
                "PAGE_UNSUPPORTED",
                "目标详情来源未登记，请核对并保存实际下载来源。",
            ));
        }
        cnki_progress(&app, &id, "detail", "已找到唯一精确题名，正在打开详情…");
        state
            .browser
            .source_loaded
            .lock()
            .map_err(Failure::storage)?
            .remove(label);
        app.get_webview_window(label)
            .ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "来源窗口已关闭。"))?
            .navigate(target.clone())
            .map_err(Failure::storage)?;
        cnki_navigation(&app, &state, &task, label, &target).await?;
    }
    let label = selected_label
        .ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "请选择本篇 CNKI 详情窗口。"))?;
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
    cnki_unchanged(&state, &task)?;
    cnki_progress(&app, &id, "export", "正在获取完整题录，保存原始响应与来源…");
    let mut response = state
        .browser
        .execute(
            &app,
            &label,
            "cnki_capture",
            if action == "auto" {
                json!({"title":task.record.title})
            } else {
                json!({})
            },
            35,
        )
        .await?;
    cnki_unchanged(&state, &task)?;
    response["search"] = search;
    let (draft, acquisition) = cnki::capture(&state.store.root, &task, &response)?;
    let document = source_files::check_draft(&state.store.root, &task, &draft)?;
    let mut page = document.page(&draft, None, 1, 0)?;
    page["acquisition"] = acquisition;
    remember(&state, draft)?;
    cnki_progress(
        &app,
        &id,
        "saved",
        "完整题录已保存，请选择记录并核对重要缺项。",
    );
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
        json!({"sites":sites,"flows":state.store.source_flows()?,"source_queue":state.store.source_queue()?,"acquisitions":state.store.source_acquisitions(&id)?,"downloads":state.store.source_downloads(&id)?,"windows":state.browser.source_window_states(&app,&task,&sites)?}),
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
        cnki_excel::normalize_for_channel(
            &state.store.root,
            &task,
            &receipt.session.site.channel,
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
