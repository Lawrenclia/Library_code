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
    let mut task = state.store.task(&id)?;
    let schemas: Vec<Template> =
        serde_json::from_value(state.store.setting("templates")?.unwrap_or(json!([])))?;
    let template = schemas
        .iter()
        .find(|t| t.id == template_id)
        .ok_or_else(|| Failure::new("TEMPLATE_INVALID", "模板不存在。"))?;
    let template = library_core::materials::actual_template(template)?;
    let classification = task
        .classification
        .clone()
        .ok_or_else(|| Failure::new("EVIDENCE_REQUIRED", "先调用 AI 并核对有来源的模板字段。"))?;
    if classification["template_id"].as_str() != Some(&template.id) {
        return Err(Failure::new(
            "TEMPLATE_CHANGED",
            "AI 填写建议属于其他模板，请针对当前模板重新生成。",
        ));
    }
    let sources = library_core::classification::sources(&state.store.root, &task)?;
    library_core::classification::validate_saved(
        &task,
        &classification,
        &sources,
        &json!(template),
    )?;
    if !task
        .evidence
        .iter()
        .filter(|e| e.kind == "ai_classification")
        .filter_map(|e| serde_json::from_str::<Value>(&e.text).ok())
        .any(|a| {
            a["result"] == classification
                && a["sources"] == json!(sources)
                && library_core::materials::same_template(a.get("template"), Some(&json!(template)))
                && a["review_required"] == true
                && a["platform_verified"] == false
        })
    {
        return Err(Failure::new(
            "AI_RESULT_INVALID",
            "建议缺少对应实际模板和当前来源的完整审计。",
        ));
    }
    let frozen = library_core::materials::facts(&state.store.root, &task)?;
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
    task = state.store.task(&id)?;
    if library_core::materials::facts(&state.store.root, &task)? != frozen {
        return Err(Failure::new(
            "MATERIAL_CHANGED",
            "选择保存位置期间，原论文资料变化，未导出。",
        ));
    }
    library_core::source_files::verified_evidence(&state.store.root, &task)?;
    let validation = library_core::templates::write_checked(&template, &fields, file.path())?;
    let audit = state.store.root.join("materials");
    std::fs::create_dir_all(&audit)?;
    let source_evidence = sources;
    let provenance = json!({"schema":"template_material_v1","sa_id":id,"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),"classification_hash":hash(classification.to_string().as_bytes()),"template_id":template.id,"template_hash":template.fingerprint,"output":file.path().to_string_lossy(),"output_hash":hash(&std::fs::read(file.path())?),"fields":classification["fields"],"evidence":source_evidence,"validation":validation,"missing":validation.missing,"invalid_fields_omitted":true});
    let audit_path = audit.join(format!("{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&audit_path, serde_json::to_vec_pretty(&provenance)?)?;
    task.evidence.push(Evidence {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "material_validation".into(),
        source: audit_path.to_string_lossy().into(),
        text: provenance.to_string(),
        created: now(),
    });
    state.store.save(&mut task, "material_exported")?;
    state.changed(&app);
    Ok(
        json!({"path":file.path().to_string_lossy(),"task_revision":task.revision,"missing":validation.missing,"invalid":validation.invalid,"requires_review":validation.requires_review,"ready":validation.ready}),
    )
}
