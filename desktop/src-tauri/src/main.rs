#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod ai;
#[cfg(feature = "smoke-test")]
mod alias_smoke;
mod browser;
#[cfg(feature = "smoke-test")]
mod download_smoke;
mod engine;
#[cfg(feature = "smoke-test")]
mod metadata_smoke;
#[cfg(feature = "smoke-test")]
mod queue_smoke;
#[cfg(feature = "smoke-test")]
mod restart_smoke;
#[cfg(feature = "smoke-test")]
mod smoke;
use engine::Engine;
use library_core::{files, templates::Template, *};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, State, WebviewWindow};

fn local(w: &WebviewWindow) -> Result<()> {
    if w.label() != "main" {
        return Err(Failure::new(
            "PERMISSION_DENIED",
            "只能从工作台执行此操作。",
        ));
    }
    let u = w.url().map_err(Failure::storage)?;
    if !matches!(u.scheme(), "tauri" | "http" | "https")
        || (!matches!(
            u.host_str(),
            Some("tauri.localhost" | "127.0.0.1" | "localhost")
        ) && u.scheme() != "tauri")
    {
        return Err(Failure::new("PERMISSION_DENIED", "页面不属于本地工作台。"));
    }
    Ok(())
}
#[tauri::command]
fn workspace(window: WebviewWindow, app: AppHandle, state: State<Engine>) -> Result<Value> {
    local(&window)?;
    state.snapshot(&app)
}
#[tauri::command]
async fn import_roster(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Excel 名单", &["xlsx"])
        .pick_file()
        .await;
    let Some(file) = file else {
        return Ok(json!({"cancelled":true}));
    };
    let (records, hash) = files::read_roster(file.path())?;
    let archive = files::archive_roster(&state.store.root, file.path(), &hash)?;
    let count = state.store.import_from(records, hash, &archive)?;
    state.changed(&app);
    Ok(json!({"count":count}))
}
#[tauri::command]
async fn prepare_input_version(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    state.prepare_input_version(&app, &id).await
}
#[tauri::command]
async fn accept_input_version(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
    proposal_id: String,
    source: String,
    proof: String,
    approved: bool,
) -> Result<Task> {
    local(&window)?;
    if !approved {
        return Err(Failure::new("REVIEW_REQUIRED", "先确认本次新名单版本。"));
    }
    let _lease = state.acquire()?;
    state
        .accept_input_version(&app, &id, &proposal_id, &source, &proof)
        .await
}
#[tauri::command]
async fn preview_legacy(window: WebviewWindow, state: State<'_, Engine>) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let Some(folder) = rfd::AsyncFileDialog::new()
        .set_title("选择含 list.xlsx 和 runtime 的旧版目录")
        .pick_folder()
        .await
    else {
        return Ok(json!({"cancelled":true}));
    };
    let plan = library_core::legacy::inspect(folder.path())?;
    Ok(serde_json::to_value(plan.preview)?)
}
#[tauri::command]
fn migrate_legacy(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    root: String,
    fingerprint: String,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let plan = library_core::legacy::inspect(std::path::Path::new(&root))?;
    if plan.preview.fingerprint != fingerprint {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "预览后旧数据发生变化，请重新读取迁移预览。",
        ));
    }
    let result = state.store.apply_legacy(&plan)?;
    state.changed(&app);
    Ok(result)
}
#[tauri::command]
async fn open_browser(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    role: String,
) -> Result<()> {
    local(&window)?;
    state.browser.open(&app, &state.store.root, &role).await?;
    state.changed(&app);
    Ok(())
}
#[tauri::command]
async fn run_queue(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    owner: String,
    retry_skipped: bool,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire()?;
    let r = state.queue(&app, &owner, retry_skipped).await;
    state.changed(&app);
    r
}
#[tauri::command]
fn pause_queue(window: WebviewWindow, app: AppHandle, state: State<Engine>) -> Result<()> {
    local(&window)?;
    state.store.request_download_pause()?;
    state.pause.store(true, Ordering::SeqCst);
    state.changed(&app);
    Ok(())
}
#[tauri::command]
async fn resume_queue(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire()?;
    let result = state.resume_queue(&app, &id).await;
    state.changed(&app);
    result
}
#[tauri::command]
fn cancel_queue(
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
fn review_task(
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
async fn run_step(
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
async fn adopt_file(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let file = rfd::AsyncFileDialog::new()
        .add_filter("WOS 完整记录", &["txt"])
        .pick_file()
        .await;
    let Some(f) = file else {
        return Ok(json!({"cancelled":true}));
    };
    let t = state.attach(
        &id,
        f.path(),
        "WOS 本地原始导出".into(),
        "手动导入，来源链接待补充".into(),
    )?;
    state.changed(&app);
    Ok(json!(t))
}
#[tauri::command]
async fn export_report(window: WebviewWindow, state: State<'_, Engine>) -> Result<Value> {
    local(&window)?;
    let path = rfd::AsyncFileDialog::new()
        .add_filter("Excel", &["xlsx"])
        .set_file_name("任务与来源.xlsx")
        .save_file()
        .await;
    let Some(path) = path else {
        return Ok(json!({"cancelled":true}));
    };
    let (tasks, queues) = state.store.report_snapshot()?;
    files::export_report_with_queues(&tasks, &queues, path.path())?;
    Ok(json!({"path":path.path().to_string_lossy()}))
}
#[tauri::command]
fn open_folder(window: WebviewWindow, state: State<Engine>) -> Result<()> {
    local(&window)?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer.exe")
            .arg(&state.store.root)
            .creation_flags(0x08000000)
            .spawn()?;
    }
    Ok(())
}
#[tauri::command]
fn ai_settings(window: WebviewWindow, state: State<Engine>) -> Result<Value> {
    local(&window)?;
    ai::settings(&state)
}
#[tauri::command]
fn save_ai_settings(
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
async fn classify_task(
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
#[tauri::command]
fn templates(window: WebviewWindow, state: State<Engine>) -> Result<Value> {
    local(&window)?;
    Ok(state.store.setting("templates")?.unwrap_or(json!([])))
}
#[tauri::command]
async fn register_template(
    window: WebviewWindow,
    state: State<'_, Engine>,
    sheet: String,
    header_row: u32,
    required: Vec<String>,
    notes: String,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let file = rfd::AsyncFileDialog::new()
        .add_filter("导入模板", &["xlsx"])
        .pick_file()
        .await;
    let Some(file) = file else {
        return Ok(json!({"cancelled":true}));
    };
    let mut template =
        library_core::templates::inspect(file.path(), &sheet, header_row, required, notes)?;
    let folder = state.store.root.join("templates");
    std::fs::create_dir_all(&folder)?;
    let owned = folder.join(format!("{}.xlsx", template.fingerprint));
    if !owned.exists() {
        std::fs::copy(file.path(), &owned)?;
    }
    template.path = owned.to_string_lossy().into();
    let mut saved: Vec<Template> =
        serde_json::from_value(state.store.setting("templates")?.unwrap_or(json!([])))?;
    saved.retain(|t| t.id != template.id);
    saved.push(template.clone());
    state.store.set_setting("templates", json!(saved))?;
    Ok(json!(template))
}
#[tauri::command]
async fn fill_template(
    window: WebviewWindow,
    state: State<'_, Engine>,
    id: String,
    template_id: String,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let task = state.store.task(&id)?;
    let schemas: Vec<Template> =
        serde_json::from_value(state.store.setting("templates")?.unwrap_or(json!([])))?;
    let template = schemas
        .iter()
        .find(|t| t.id == template_id)
        .ok_or_else(|| Failure::new("TEMPLATE_INVALID", "模板不存在。"))?;
    let classification = task
        .classification
        .ok_or_else(|| Failure::new("EVIDENCE_REQUIRED", "先调用 AI 并核对有来源的模板字段。"))?;
    if classification["template_id"].as_str() != Some(&template.id) {
        return Err(Failure::new(
            "TEMPLATE_CHANGED",
            "AI 填写建议属于其他模板，请针对当前模板重新生成。",
        ));
    }
    let fields: std::collections::BTreeMap<String, String> = classification["fields"]
        .as_object()
        .ok_or_else(|| Failure::new("AI_RESULT_INVALID", "缺少已核验字段。"))?
        .iter()
        .map(|(k, v)| {
            Ok((
                k.clone(),
                v["value"]
                    .as_str()
                    .ok_or_else(|| Failure::new("AI_RESULT_INVALID", "字段格式无效。"))?
                    .into(),
            ))
        })
        .collect::<Result<_>>()?;
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Excel 材料", &["xlsx"])
        .set_file_name(format!("{}-{}.xlsx", template.name, id))
        .save_file()
        .await;
    let Some(file) = file else {
        return Ok(json!({"cancelled":true}));
    };
    let missing = library_core::templates::write(template, &fields, file.path())?;
    let audit = state.store.root.join("materials");
    std::fs::create_dir_all(&audit)?;
    let provenance = json!({"sa_id":id,"template_id":template.id,"template_hash":template.fingerprint,"output":file.path().to_string_lossy(),"output_hash":hash(&std::fs::read(file.path())?),"fields":classification["fields"],"evidence":task.evidence,"missing":missing});
    std::fs::write(
        audit.join(format!("{}.json", uuid::Uuid::new_v4())),
        serde_json::to_vec_pretty(&provenance)?,
    )?;
    Ok(json!({"path":file.path().to_string_lossy(),"missing":missing,"ready":missing.is_empty()}))
}
#[tauri::command]
fn browser_result(
    window: WebviewWindow,
    state: State<Engine>,
    request_id: String,
    result: Value,
) -> Result<()> {
    state.browser.reply(&window, &request_id, result)
}
fn main() {
    tauri::Builder::default()
        .setup(|app| {
            #[cfg(not(feature = "smoke-test"))]
            let root = app.path().app_local_data_dir()?;
            #[cfg(feature = "smoke-test")]
            let root = smoke::workspace_root()?;
            let engine = Engine::new(root).map_err(|e| std::io::Error::other(e.message))?;
            app.manage(engine);
            #[cfg(feature = "smoke-test")]
            smoke::setup(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            workspace,
            import_roster,
            prepare_input_version,
            accept_input_version,
            preview_legacy,
            migrate_legacy,
            open_browser,
            run_queue,
            pause_queue,
            resume_queue,
            cancel_queue,
            review_task,
            run_step,
            adopt_file,
            export_report,
            open_folder,
            ai_settings,
            save_ai_settings,
            classify_task,
            templates,
            register_template,
            fill_template,
            browser_result
        ])
        .run(tauri::generate_context!())
        .expect("工作台启动失败");
}
