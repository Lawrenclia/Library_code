use super::local;
use crate::engine::Engine;
use library_core::{
    source_files::{self, Draft, ReadOptions, Selection},
    *,
};
use serde_json::{json, Value};
use tauri::{AppHandle, State, WebviewWindow};
fn drafts(state: &Engine) -> Result<Vec<Draft>> {
    Ok(serde_json::from_value(
        state.store.setting("source_previews")?.unwrap_or(json!([])),
    )?)
}
pub(super) fn remember(state: &Engine, draft: Draft) -> Result<()> {
    let mut pending = drafts(state)?;
    pending.push(draft);
    state.store.set_setting("source_previews", json!(pending))
}
fn resolve(state: &Engine, id: &str, preview: Option<&str>) -> Result<Draft> {
    drafts(state)?
        .into_iter()
        .rev()
        .find(|d| d.task_id == id && preview.is_none_or(|p| d.id == p))
        .ok_or_else(|| {
            Failure::new(
                "SOURCE_MISSING",
                "本条没有已保存的来源预览，请选择原始导出文件。",
            )
        })
}
#[tauri::command]
pub(crate) async fn preview_source_file(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
    channel: String,
    options: ReadOptions,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let formats = source_files::channel_formats(&channel)?;
    let refs: Vec<_> = formats.iter().map(String::as_str).collect();
    let task = state.store.task(&id)?;
    let Some(file) = rfd::AsyncFileDialog::new()
        .add_filter("数据库原始导出", &refs)
        .pick_file()
        .await
    else {
        return Ok(json!({"cancelled":true}));
    };
    let draft = source_files::prepare(&state.store.root, &task, &channel, file.path(), options)?;
    let document = source_files::check_draft(&state.store.root, &task, &draft)?;
    let result = document.page(&draft, None, 0, 0)?;
    remember(&state, draft)?;
    state.changed(&app);
    Ok(result)
}
#[tauri::command]
pub(crate) fn source_file_page(
    window: WebviewWindow,
    state: State<Engine>,
    id: String,
    preview_id: Option<String>,
    sheet: Option<String>,
    header_row: u32,
    page: u32,
) -> Result<Value> {
    local(&window)?;
    let draft = resolve(&state, &id, preview_id.as_deref())?;
    let task = state.store.task(&id)?;
    let document = source_files::check_draft(&state.store.root, &task, &draft)?;
    document.page(&draft, sheet.as_deref(), header_row, page)
}
#[tauri::command]
pub(crate) fn attach_source_file(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
    preview_id: String,
    selection: Selection,
    source_url: String,
    note: String,
    confirmed: bool,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    let mut task = state.store.task(&id)?;
    if task.stage == Stage::Unknown
        || task.stage == Stage::Completed
        || !state.store.unresolved(&id)?.is_empty()
    {
        return Err(Failure::new(
            "INVALID_TRANSITION",
            "已有未确认操作或任务已完成，不能新增绑定来源。",
        ));
    }
    if state.store.pending_input(&id)?.is_some() {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "先核对名单版本，再绑定原始来源。",
        ));
    }
    let draft = resolve(&state, &id, Some(&preview_id))?;
    let evidence = source_files::bind(
        &state.store.root,
        &task,
        &draft,
        selection,
        source_url,
        note,
        confirmed,
    )?;
    task.evidence.push(evidence);
    task.classification = None;
    state.store.save(&mut task, "original_source_attached")?;
    state.changed(&app);
    Ok(json!({"task_revision":task.revision,"stage":task.stage,"attached":true}))
}
