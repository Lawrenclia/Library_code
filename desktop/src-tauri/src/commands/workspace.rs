use super::local;
use crate::engine::Engine;
use library_core::{files, *};
use serde_json::{json, Value};
use tauri::{AppHandle, State, WebviewWindow};

#[tauri::command]
pub(crate) fn workspace(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
) -> Result<Value> {
    local(&window)?;
    state.snapshot(&app)
}
#[tauri::command]
pub(crate) async fn import_roster(
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
pub(crate) async fn prepare_input_version(
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
pub(crate) async fn accept_input_version(
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
pub(crate) async fn preview_legacy(
    window: WebviewWindow,
    state: State<'_, Engine>,
) -> Result<Value> {
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
pub(crate) fn migrate_legacy(
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
pub(crate) async fn export_report(
    window: WebviewWindow,
    state: State<'_, Engine>,
) -> Result<Value> {
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
    files::export_report_with_runs(
        &tasks,
        &queues,
        &state.store.legacy_snapshots()?,
        &state.store.ai_queues()?,
        path.path(),
    )?;
    Ok(json!({"path":path.path().to_string_lossy()}))
}
#[tauri::command]
pub(crate) fn open_folder(window: WebviewWindow, state: State<Engine>) -> Result<()> {
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
