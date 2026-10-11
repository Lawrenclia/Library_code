use super::{local, sources::remember};
use crate::engine::Engine;
use library_core::{
    source_downloads::Session,
    source_files,
    source_flows::{self, Flow},
    *,
};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

fn progress(app: &AppHandle, id: &str, channel: &str, phase: &str, message: &str) {
    let _ = app.emit_to(
        "main",
        "source-queue-changed",
        json!({"id":id,"channel":channel}),
    );
    let _ = app.emit_to(
        "main",
        "source-acquisition-progress",
        json!({"id":id,"channel":channel,"phase":phase,"message":message}),
    );
}
fn unchanged(state: &Engine, task: &Task, session: &Session, flow: &Flow) -> Result<()> {
    if state.pause.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(Failure::new(
            "PAUSED",
            "来源获取已暂停，保留原窗口和下载记录。",
        ));
    }
    let current = state
        .store
        .source_session(&task.id, &session.site.channel)?;
    if task.input_hash != current.input_hash
        || json!(task.record) != json!(current.record)
        || json!(session.site) != json!(current.site)
        || json!(state.store.source_flow(&session.site)?) != json!(flow)
    {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "名单或渠道流程变化，本次停止。",
        ));
    }
    Ok(())
}
async fn loaded(
    app: &AppHandle,
    state: &Engine,
    task: &Task,
    session: &Session,
    flow: &Flow,
    label: &str,
    target: Option<&url::Url>,
) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
    loop {
        unchanged(state, task, session, flow)?;
        let w = app
            .get_webview_window(label)
            .ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "本篇来源窗口关闭。"))?;
        let u = w.url().map_err(Failure::storage)?;
        let in_scope = source_flows::allowed(&session.site, u.as_str()).is_ok();
        if in_scope
            && target.is_none_or(|t| {
                t.origin() == u.origin()
                    && t.path() == u.path()
                    && t.query_pairs()
                        .all(|(k, v)| u.query_pairs().any(|(uk, uv)| uk == k && uv == v))
            })
            && state
                .browser
                .source_loaded
                .lock()
                .map_err(Failure::storage)?
                .get(label)
                .is_some_and(|s| s == u.as_str())
        {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(Failure::new(
                if in_scope {
                    "PAGE_TIMEOUT"
                } else {
                    "AUTH_REQUIRED"
                },
                "来源页尚未加载或仍在登录，请在窗口处理后继续。未判定为无记录。",
            ));
        }
        // Some databases navigate inside a single-page app without a native
        // Finished event. A nonce-bound read-only reply can confirm readiness.
        if in_scope
            && target.is_none_or(|t| {
                t.origin() == u.origin()
                    && t.path() == u.path()
                    && t.query_pairs()
                        .all(|(k, v)| u.query_pairs().any(|(uk, uv)| uk == k && uv == v))
            })
        {
            match state
                .browser
                .execute(app, label, "source_ping", payload(task, session, flow), 3)
                .await
            {
                Ok(p) if p["complete"] == true && p["page_url"] == u.as_str() => return Ok(()),
                Err(e)
                    if ["AUTH_REQUIRED", "SOURCE_SERVICE_UNAVAILABLE"]
                        .contains(&e.code.as_str()) =>
                {
                    return Err(e)
                }
                _ => (),
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}
fn payload(task: &Task, session: &Session, flow: &Flow) -> Value {
    let formats = catalog::channels()
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == session.site.channel)
        .unwrap()["formats"]
        .clone();
    json!({"title":task.record.title,"doi":task.record.doi,"site":session.site,"flow":flow,"formats":formats})
}
fn page_preview(state: &Engine, task: &Task, session: &Session, data: &Value) -> Result<Value> {
    let (draft, acquisition) = source_flows::capture(&state.store.root, &session.site, task, data)?;
    let doc = source_files::check_draft(&state.store.root, task, &draft)?;
    let mut page = doc.page(&draft, None, 1, 0)?;
    page["acquisition"] = acquisition;
    state
        .store
        .remember_source_capture(session, &draft, &page["acquisition"])?;
    remember(state, draft)?;
    Ok(page)
}
fn native_info(receipt: &source_downloads::Receipt, draft: &source_files::Draft) -> Value {
    json!({"mode":"native_source_download","source_url":receipt.page_url,"response_path":receipt.path,"response_sha256":receipt.sha256,"saved_path":draft.path,"missing":[]})
}
#[tauri::command]
pub(crate) fn save_source_flow(
    window: WebviewWindow,
    state: State<Engine>,
    flow: Flow,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    Ok(json!(state.store.save_source_flow(flow)?))
}
#[tauri::command]
pub(crate) fn recheck_source_export(
    window: WebviewWindow,
    state: State<Engine>,
    id: String,
    operation_id: String,
    download_id: Option<String>,
) -> Result<Value> {
    local(&window)?;
    state.store.task(&id)?;
    state
        .store
        .recheck_source_export(&id, &operation_id, download_id.as_deref())
}
#[tauri::command]
pub(crate) async fn acquire_source(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
    channel: String,
    action: String,
    label: Option<String>,
) -> Result<Value> {
    local(&window)?;
    if !["auto", "capture", "export"].contains(&action.as_str()) {
        return Err(Failure::new(
            "INVALID_ACTION",
            "请选择自动获取、详情来源或原始导出。",
        ));
    }
    let lease = state.acquire()?;
    let current = state.store.source_session(&id, &channel)?;
    let mut task = state.store.task(&id)?;
    let prior = state.store.source_acquisitions(&id)?;
    if prior.iter().any(|p| {
        p["session"]["site"]["channel"] == channel
            && ["export_pending", "downloading", "unknown"]
                .contains(&p["state"].as_str().unwrap_or(""))
    }) && action != "capture"
    {
        return Err(Failure::new(
            "SOURCE_EXPORT_UNKNOWN",
            "本渠道原导出结果未确认，请先检查原下载结果；不会再次检索并导出。",
        ));
    }
    if action == "auto" {
        if let Some(p) = prior.iter().find(|p| {
            p["state"] == "completed"
                && p["session"]["site"] == json!(current.site)
                && p["session"]["record"] == json!(task.record)
                && p["session"]["input_hash"] == task.input_hash
        }) {
            let receipt = state
                .store
                .source_download_for_preview(&id, p["receipt_id"].as_str().unwrap_or(""))?;
            let mut draft = source_files::prepare(
                &state.store.root,
                &task,
                &channel,
                std::path::Path::new(&receipt.path),
                Default::default(),
            )?;
            draft.original_name = receipt.original_name.clone();
            let doc = source_files::check_draft(&state.store.root, &task, &draft)?;
            let mut page = doc.page(
                &draft,
                None,
                if draft.options.tagged_format.is_some() {
                    1
                } else {
                    0
                },
                0,
            )?;
            page["acquisition"] = native_info(&receipt, &draft);
            remember(&state, draft)?;
            return Ok(page);
        }
    }
    let label = match label {
        Some(l) => l,
        None => state
            .browser
            .open_source(&app, &state.store, current.clone())?["label"]
            .as_str()
            .ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "没有本篇来源窗口。"))?
            .to_string(),
    };
    let session = state
        .browser
        .source_windows
        .lock()
        .map_err(Failure::storage)?
        .get(&label)
        .cloned()
        .ok_or_else(|| Failure::new("PERMISSION_DENIED", "未登记的本篇来源窗口。"))?;
    if session.record.sa_id != id || session.site.channel != channel {
        return Err(Failure::new(
            "PERMISSION_DENIED",
            "所选来源窗口不属于本篇渠道。",
        ));
    }
    let flow = state.store.source_flow(&session.site)?;
    unchanged(&state, &task, &session, &flow)?;
    let mut cmd = payload(&task, &session, &flow);
    cmd["manual_identity"] = json!(action != "auto");
    loaded(&app, &state, &task, &session, &flow, &label, None).await?;
    let mut search = Value::Null;
    if action == "auto" {
        progress(&app, &id, &channel, "search", "正在按本篇题名检索…");
        if !flow.search_url.is_empty() {
            let target = source_flows::search_url(&flow, &session.site, &task)?;
            state
                .browser
                .source_loaded
                .lock()
                .map_err(Failure::storage)?
                .remove(&label);
            app.get_webview_window(&label)
                .unwrap()
                .navigate(target.clone())
                .map_err(Failure::storage)?;
            loaded(&app, &state, &task, &session, &flow, &label, Some(&target)).await?;
            cmd["query_url"] = json!(app
                .get_webview_window(&label)
                .unwrap()
                .url()
                .map_err(Failure::storage)?
                .as_str());
        } else {
            let result = state
                .browser
                .execute(&app, &label, "source_search", cmd.clone(), 8)
                .await;
            // Navigation may close the reply channel after the one search click.
            // Observe the new page once; never repeat the search submission.
            match result {
                Ok(_) => (),
                Err(e) if ["PAGE_TIMEOUT", "BROWSER_DISCONNECTED"].contains(&e.code.as_str()) => (),
                Err(e) => return Err(e),
            };
            loaded(&app, &state, &task, &session, &flow, &label, None).await?;
        }
        progress(
            &app,
            &id,
            &channel,
            "results",
            "正在核对完整结果和唯一精确题名…",
        );
        search = state
            .browser
            .execute(&app, &label, "source_results", cmd.clone(), 30)
            .await?;
        state.store.record_source_search(&task, &channel, &search)?;
        task = state.store.task(&id)?;
        unchanged(&state, &task, &session, &flow)?;
        let target = source_flows::select_result(&session.site, &task, &search)?;
        state
            .browser
            .source_loaded
            .lock()
            .map_err(Failure::storage)?
            .remove(&label);
        app.get_webview_window(&label)
            .unwrap()
            .navigate(target.clone())
            .map_err(Failure::storage)?;
        loaded(&app, &state, &task, &session, &flow, &label, Some(&target)).await?;
    }
    progress(
        &app,
        &id,
        &channel,
        "detail",
        "正在核对详情身份并保存完整来源字段…",
    );
    let mut data = state
        .browser
        .execute(&app, &label, "source_capture", cmd.clone(), 15)
        .await?;
    unchanged(&state, &task, &session, &flow)?;
    data["search"] = search;
    if action == "capture" {
        let page = page_preview(&state, &task, &session, &data)?;
        state.changed(&app);
        return Ok(page);
    }
    // Preserve the fallback before any native export is issued. It remains
    // factual input and cannot masquerade as the database's original file.
    let _saved_fallback = page_preview(&state, &task, &session, &data)?;
    let op = state.store.arm_source_export(&session, &flow, &data)?;
    let op_id = op["id"].as_str().unwrap().to_string();
    state
        .browser
        .arm_source_export(&label, &session, &op_id, lease)?;
    progress(
        &app,
        &id,
        &channel,
        "export",
        "正在选择原始格式并等待原生下载完成…",
    );
    let issued = state
        .browser
        .execute(&app, &label, "source_export", cmd, 35)
        .await;
    if let Err(e) = &issued {
        // No click is the only case where failure is known. A click without its
        // native receipt remains unknown, including modal/format failures.
        if e.submitted == Some(false) {
            let _ = state
                .store
                .source_export_update(&op_id, "failed", None, Some(e));
            state
                .browser
                .finish_source_export_watch(&label, &state.store, None);
            let mut fallback = page_preview(&state, &state.store.task(&id)?, &session, &data)?;
            fallback["acquisition"]["original_export_error"] = json!(e);
            state.changed(&app);
            return Ok(fallback);
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let result: Result<Value> = async {
        loop {
            unchanged(&state, &task, &session, &flow)?;
            let status = state
                .store
                .source_acquisitions(&id)?
                .into_iter()
                .find(|o| o["id"] == op_id)
                .ok_or_else(|| Failure::new("SOURCE_EXPORT_UNKNOWN", "原导出记录缺失。"))?;
            if status["state"] == "completed" {
                let receipt = state.store.source_download_for_preview(
                    &id,
                    status["receipt_id"].as_str().unwrap_or(""),
                )?;
                let latest = state.store.task(&id)?;
                let mut draft = source_files::prepare(
                    &state.store.root,
                    &latest,
                    &channel,
                    std::path::Path::new(&receipt.path),
                    Default::default(),
                )?;
                draft.original_name = receipt.original_name.clone();
                let doc = source_files::check_draft(&state.store.root, &latest, &draft)?;
                let mut page = doc.page(
                    &draft,
                    None,
                    if draft.options.tagged_format.is_some() {
                        1
                    } else {
                        0
                    },
                    0,
                )?;
                page["acquisition"] = native_info(&receipt, &draft);
                remember(&state, draft)?;
                return Ok(page);
            }
            if ["failed", "unknown"].contains(&status["state"].as_str().unwrap_or("")) {
                return Err(Failure::new(
                    "SOURCE_EXPORT_UNKNOWN",
                    "原导出未确认完成，请检查原下载结果；已保存详情来源供整理。",
                ));
            }
            if std::time::Instant::now() >= deadline {
                return Err(Failure::new(
                    "SOURCE_EXPORT_UNKNOWN",
                    "原导出等待结束，结果尚未确认。请检查原下载，不自动再导出。",
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }
    .await;
    match result {
        Ok(page) => {
            state
                .browser
                .finish_source_export_watch(&label, &state.store, None);
            progress(
                &app,
                &id,
                &channel,
                "saved",
                "原始文件已保存，请选择单篇记录并核对重要字段。",
            );
            state.changed(&app);
            Ok(page)
        }
        Err(e) => {
            state
                .browser
                .finish_source_export_watch(&label, &state.store, Some(&e));
            let mut fallback = page_preview(&state, &state.store.task(&id)?, &session, &data)?;
            fallback["acquisition"]["original_export_error"] = json!(e);
            fallback["acquisition"]["operation_id"] = json!(op_id);
            state.changed(&app);
            Ok(fallback)
        }
    }
}

#[tauri::command]
pub(crate) fn preview_source_capture(
    window: WebviewWindow,
    state: State<Engine>,
    id: String,
    operation_id: String,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let (draft, acquisition) = state.store.captured_source(&id, &operation_id)?;
    let task = state.store.task(&id)?;
    let doc = source_files::check_draft(&state.store.root, &task, &draft)?;
    let mut page = doc.page(&draft, None, 1, 0)?;
    page["acquisition"] = acquisition;
    remember(&state, draft)?;
    Ok(page)
}
#[tauri::command]
pub(crate) fn cancel_source_export(
    window: WebviewWindow,
    state: State<Engine>,
    id: String,
    operation_id: String,
    note: String,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let op = state
        .store
        .source_acquisitions(&id)?
        .into_iter()
        .find(|o| o["id"] == operation_id)
        .ok_or_else(|| Failure::new("SOURCE_FLOW_INVALID", "原导出不属于本任务。"))?;
    if state
        .browser
        .source_windows
        .lock()
        .map_err(Failure::storage)?
        .values()
        .any(|s| json!(s.id) == op["session"]["id"])
    {
        return Err(Failure::new(
            "BUSY",
            "先关闭原来源会话的所有窗口，再核对并取消原导出记录。",
        ));
    }
    state.store.cancel_source_export(&id, &operation_id, &note)
}
#[tauri::command]
pub(crate) async fn source_queue_control(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    action: String,
    owner: Option<String>,
    channel: Option<String>,
) -> Result<Value> {
    local(&window)?;
    let mut q = if action == "start" {
        let _lease = state.acquire()?;
        state.store.create_source_queue(
            owner.as_deref().unwrap_or(""),
            channel
                .as_deref()
                .ok_or_else(|| Failure::new("INVALID_CHANNEL", "请选择来源渠道。"))?,
        )?
    } else {
        state.store.source_queue_control(&action)?
    };
    if ["pause", "cancel"].contains(&action.as_str()) {
        state.changed(&app);
        return Ok(q);
    }
    let entries = q["entries"].as_array().unwrap().clone();
    let channel = q["channel"].as_str().unwrap().to_string();
    let queue_id = q["id"].clone();
    let _ = app.emit_to("main", "source-queue-changed", json!({"queue_id":queue_id}));
    while (q["index"].as_u64().unwrap_or(0) as usize) < entries.len() {
        let latest = state
            .store
            .source_queue()?
            .ok_or_else(|| Failure::new("INPUT_CHANGED", "原来源队列缺失。"))?;
        if latest["id"] != queue_id {
            return Err(Failure::new("INPUT_CHANGED", "来源队列版本变化。"));
        }
        if latest["pause_requested"] == true {
            q["status"] = json!("paused");
            break;
        }
        let index = q["index"].as_u64().unwrap() as usize;
        let entry = &entries[index];
        let id = entry["id"].as_str().unwrap();
        progress(
            &app,
            id,
            &channel,
            "queue",
            &format!("来源队列：第 {} / {} 篇", index + 1, entries.len()),
        );
        let check = (|| -> Result<()> {
            let task = state.store.task(id)?;
            let session = state.store.source_session(id, &channel)?;
            if task.input_hash != entry["input_hash"]
                || json!(task.record) != entry["record"]
                || json!(session.site) != entry["site"]
                || json!(state.store.source_flow(&session.site)?) != entry["flow"]
            {
                return Err(Failure::new(
                    "INPUT_CHANGED",
                    "名单或渠道流程已变化，原队列暂停。",
                ));
            }
            Ok(())
        })();
        let before_windows: std::collections::HashSet<_> = state
            .browser
            .source_windows
            .lock()
            .map_err(Failure::storage)?
            .keys()
            .cloned()
            .collect();
        let result = match check {
            Err(e) => Err(e),
            Ok(()) => {
                acquire_source(
                    window.clone(),
                    app.clone(),
                    state.clone(),
                    id.into(),
                    channel.clone(),
                    "auto".into(),
                    None,
                )
                .await
            }
        };
        let (blocked, summary) = match result {
            Ok(page) => {
                let error = page["acquisition"]["original_export_error"].clone();
                let blocked = [
                    "AUTH_REQUIRED",
                    "SOURCE_SERVICE_UNAVAILABLE",
                    "SOURCE_EXPORT_UNKNOWN",
                    "INPUT_CHANGED",
                    "BROWSER_DISCONNECTED",
                    "PAUSED",
                    "PAGE_TIMEOUT",
                    "BUSY",
                ]
                .contains(&error["code"].as_str().unwrap_or(""));
                (
                    blocked,
                    json!({"id":id,"state":if error.is_null(){"saved"}else{"detail_saved"},"draft":page["draft"],"acquisition":page["acquisition"],"error":error}),
                )
            }
            Err(e) => {
                let blocked = [
                    "AUTH_REQUIRED",
                    "SOURCE_SERVICE_UNAVAILABLE",
                    "SOURCE_EXPORT_UNKNOWN",
                    "INPUT_CHANGED",
                    "BROWSER_DISCONNECTED",
                    "PAUSED",
                    "PAGE_TIMEOUT",
                    "BUSY",
                ]
                .contains(&e.code.as_str());
                (blocked, json!({"id":id,"state":"failed","error":e}))
            }
        };
        q["results"].as_array_mut().unwrap().push(summary);
        if blocked {
            q["status"] = json!("blocked");
            break;
        }
        // Only close windows this queue created. Keep the blocking page open
        // for login/recovery; do not accumulate a browser for every paper.
        let owned: Vec<_> = state
            .browser
            .source_windows
            .lock()
            .map_err(Failure::storage)?
            .iter()
            .filter(|(label, s)| {
                !before_windows.contains(*label)
                    && s.record.sa_id == id
                    && s.site.channel == channel
            })
            .map(|(label, _)| label.clone())
            .collect();
        for label in owned {
            if let Some(w) = app.get_webview_window(&label) {
                let _ = w.close();
            }
        }
        q["index"] = json!(index + 1);
        state.store.save_source_queue_progress(&mut q)?;
        let _ = app.emit_to("main", "source-queue-changed", json!({"queue_id":queue_id}));
        state.changed(&app);
    }
    if q["index"].as_u64().unwrap_or(0) as usize >= entries.len() {
        q["status"] = json!("completed");
    }
    state.store.save_source_queue_progress(&mut q)?;
    let _ = app.emit_to("main", "source-queue-changed", json!({"queue_id":queue_id}));
    state.changed(&app);
    Ok(q)
}
