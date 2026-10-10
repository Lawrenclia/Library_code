use super::local;
use crate::engine::Engine;
use library_core::{templates::Template, *};
use serde_json::{json, Value};
use tauri::{AppHandle, State, WebviewWindow};

#[tauri::command]
pub(crate) async fn adopt_file(
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
pub(crate) fn templates(window: WebviewWindow, state: State<Engine>) -> Result<Value> {
    local(&window)?;
    Ok(state.store.setting("templates")?.unwrap_or(json!([])))
}
#[tauri::command]
pub(crate) async fn register_template(
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
pub(crate) async fn fill_template(
    window: WebviewWindow,
    app: AppHandle,
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
    let template = library_core::materials::actual_template(template)?;
    let result = task
        .classification
        .as_ref()
        .ok_or_else(|| Failure::new("EVIDENCE_REQUIRED", "先针对实际模板生成有来源的 AI 建议。"))?;
    let sources = library_core::classification::sources(&state.store.root, &task)?;
    library_core::classification::validate_saved(&task, result, &sources, &json!(template))?;
    let frozen = library_core::materials::facts(&state.store.root, &task)?;
    let file = rfd::AsyncFileDialog::new()
        .add_filter("Excel 材料", &["xlsx"])
        .set_file_name(format!("{}-{}.xlsx", template.name, id))
        .save_file()
        .await;
    let Some(file) = file else {
        return Ok(json!({"cancelled":true}));
    };
    // Recheck the original scope inside the shared publisher after the dialog.
    // A failed external copy leaves its managed product available for submission.
    let product = library_core::materials::prepare_template(&state.store, &id, &frozen, &template);
    state.changed(&app);
    let product = product?;
    let exported =
        library_core::materials::export_copy(&state.store, &product, &frozen, file.path());
    state.changed(&app);
    exported
}
