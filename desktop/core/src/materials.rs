//! Local, source-bound material products. No API calls or platform transitions.
use crate::{templates::Template, *};
use calamine::Reader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Product {
    pub sa_id: String,
    pub recipe: String,
    pub kind: String,
    pub path: String,
    pub sha256: String,
    pub audit: String,
    pub validation: Value,
    pub reused: bool,
}
fn invalid(message: &str) -> Failure {
    Failure::new("MATERIAL_CHANGED", message)
}
fn atomic_json(path: &Path, value: &Value) -> Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}
fn directory(root: &Path, recipe: &str) -> Result<PathBuf> {
    let base = root.join("materials").join("products");
    fs::create_dir_all(&base)?;
    let original = root.canonicalize()?;
    let real = base.canonicalize()?;
    if !real.starts_with(&original) {
        return Err(invalid("材料目录不在工作目录内。"));
    }
    let folder = base.join(recipe);
    fs::create_dir_all(&folder)?;
    if !folder.canonicalize()?.starts_with(&real) {
        return Err(invalid("材料版本目录已被重定向。"));
    }
    Ok(folder)
}
fn read_json(path: &Path) -> Result<Value> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 32 * 1024 * 1024 {
        return Err(invalid("材料记录不是有效原文件。"));
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn same_file(path: &Path, digest: &str) -> Result<bool> {
    let meta = fs::symlink_metadata(path)?;
    Ok(meta.is_file()
        && !meta.file_type().is_symlink()
        && meta.len() <= 32 * 1024 * 1024
        && hash(&fs::read(path)?) == digest)
}
pub fn template_semantics(template: &Value) -> Value {
    let mut result = serde_json::Map::new();
    for key in [
        "id",
        "fingerprint",
        "sheet",
        "header_row",
        "columns",
        "headers",
        "required",
        "notes",
        "field_rules",
    ] {
        result.insert(key.into(), template[key].clone());
    }
    if let Some(notes) = template["notes"].as_str() {
        let marker = "\n模板原文格式要求：\n";
        let parts: Vec<_> = notes.split(marker).collect();
        if parts.len() > 2 && parts[1..].iter().all(|p| *p == parts[1]) {
            result.insert(
                "notes".into(),
                json!(format!("{}{}{}", parts[0], marker, parts[1])),
            );
        }
    }
    Value::Object(result)
}
pub fn same_template(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => template_semantics(a) == template_semantics(b),
        _ => false,
    }
}
pub fn actual_template(saved: &Template) -> Result<Template> {
    let mut actual = templates::inspect(
        Path::new(&saved.path),
        &saved.sheet,
        saved.header_row,
        saved.required.clone(),
        saved.notes.clone(),
    )?;
    actual.id = saved.id.clone();
    actual.name = saved.name.clone();
    // inspect appends original instruction text; registration already owns it.
    actual.notes = saved.notes.clone();
    if template_semantics(&json!(actual)) != template_semantics(&json!(saved)) {
        return Err(Failure::new(
            "TEMPLATE_CHANGED",
            "实际模板与注册版本不同，请重新注册。",
        ));
    }
    Ok(actual)
}
pub fn facts(root: &Path, task: &Task) -> Result<Value> {
    let sources = classification::sources(root, task)?;
    Ok(
        json!({"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),"record_hash":hash(json!(task.record).to_string().as_bytes()),"source_hash":ai_queue::source_hash(&sources)?,"classification_hash":task.classification.as_ref().map(|v|hash(v.to_string().as_bytes())),"artifact_hash":task.artifact.as_ref().map(|a|hash(json!(a).to_string().as_bytes()))}),
    )
}
fn eligible(store: &Store, task: &Task) -> Result<()> {
    if task.record.done
        || task.record.skipped
        || task.stage == Stage::Completed
        || task.stage == Stage::Unknown
        || store.pending_input(&task.id)?.is_some()
        || !store.unresolved(&task.id)?.is_empty()
    {
        return Err(invalid(
            "任务已完成、跳过或有待确认版本/平台操作，未整理材料。",
        ));
    }
    Ok(())
}
fn check_template_original(task: &Task) -> Result<()> {
    if let Some(artifact) = &task.artifact {
        if !artifact.identity_confirmed {
            return Err(Failure::new(
                "IDENTITY_CONFLICT",
                "已有原始导出身份待核验，不能改用模板绕过核验。",
            ));
        }
        let payload = json!({"sa_id":task.id,"instructions":format!("SA补充-{}",task.id),"candidate":artifact.candidate,"contentSha":artifact.candidate.sha256});
        files::read_import_archive(task, &payload)?;
    }
    Ok(())
}
fn commit_audit(store: &Store, task: &mut Task, audit: &Value, path: &Path) -> Result<()> {
    if !task.evidence.iter().any(|e| {
        e.kind == "material_validation"
            && serde_json::from_str::<Value>(&e.text)
                .is_ok_and(|a| a["recipe"] == audit["recipe"] && a == *audit)
    }) {
        task.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "material_validation".into(),
            source: path.to_string_lossy().into(),
            text: audit.to_string(),
            created: now(),
        });
        store.save(task, "material_product_saved")?;
    }
    Ok(())
}
pub fn verify_product(store: &Store, product: &Product, frozen: &Value) -> Result<()> {
    let task = store.task(&product.sa_id)?;
    eligible(store, &task)?;
    if facts(&store.root, &task)? != *frozen
        || product.recipe.len() != 64
        || !product.recipe.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid("材料结果与原范围事实不符。"));
    }
    let folder = directory(&store.root, &product.recipe)?;
    let audit_path = folder.join("来源.json");
    let audit = read_json(&audit_path)?;
    let expected_kind = if audit["plan"]["kind"] == "original" {
        "original"
    } else if audit["validation"]["ready"] == true {
        "field_valid"
    } else {
        "draft"
    };
    if product.audit != audit_path.to_string_lossy()
        || product.kind != expected_kind
        || audit["recipe"] != product.recipe
        || audit["output"] != product.path
        || audit["output_hash"] != product.sha256
        || audit["validation"] != product.validation
        || audit["plan"]["facts"] != *frozen
        || !same_file(Path::new(&product.path), &product.sha256)?
        || !task.evidence.iter().any(|e| {
            e.kind == "material_validation"
                && serde_json::from_str::<Value>(&e.text).is_ok_and(|a| a == audit)
        })
    {
        return Err(invalid("材料字节、来源回执或保存审计不符。"));
    }
    Ok(())
}
/// Publish a prepared product, or adopt the exact receipt after a local crash.
pub fn prepare(
    store: &Store,
    id: &str,
    frozen: &Value,
    schema: Option<&Template>,
) -> Result<Product> {
    prepare_product(store, id, frozen, schema, false)
}
/// An explicitly requested template uses the same durable product publication.
/// Automatic batches still prefer original database exports.
pub fn prepare_template(
    store: &Store,
    id: &str,
    frozen: &Value,
    schema: &Template,
) -> Result<Product> {
    prepare_product(store, id, frozen, Some(schema), true)
}
/// Export a user copy while retaining the managed product as the authority.
pub fn export_copy(
    store: &Store,
    product: &Product,
    frozen: &Value,
    destination: &Path,
) -> Result<Value> {
    verify_product(store, product, frozen)?;
    if product.kind == "original"
        || destination
            .extension()
            .and_then(|v| v.to_str())
            .map(str::to_lowercase)
            .as_deref()
            != Some("xlsx")
    {
        return Err(Failure::new(
            "FILE_INVALID",
            "模板副本需要保存为 Excel 文件。",
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("保存目录无效。"))?
        .canonicalize()?;
    if parent.starts_with(store.root.canonicalize()?) {
        return Err(invalid(
            "请选择工作目录之外的位置导出副本；应用目录内的原材料已自动保存。",
        ));
    }
    if destination.exists() {
        let meta = fs::symlink_metadata(destination)?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(invalid("副本保存位置不是普通文件。"));
        }
    }
    let bytes = fs::read(&product.path)?;
    if hash(&bytes) != product.sha256 {
        return Err(invalid("管理文件在导出前变化，未复制。"));
    }
    fs::write(destination, &bytes)?;
    if !same_file(destination, &product.sha256)? {
        return Err(invalid("导出副本与管理文件不一致，未登记成功。"));
    }
    let mut task = store.task(&product.sa_id)?;
    verify_product(store, product, frozen)?;
    task.evidence.push(Evidence {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "material_export".into(),
        source: destination.to_string_lossy().into(),
        text: json!({"schema":"material_export_v1","product":product,"copy":destination.to_string_lossy(),"copy_hash":product.sha256,"facts":frozen,"platform_verified":false}).to_string(),
        created: now(),
    });
    store.save(&mut task, "material_copy_exported")?;
    Ok(
        json!({"path":destination.to_string_lossy(),"managed_path":product.path,"recipe":product.recipe,"audit":product.audit,"sha256":product.sha256,"task_revision":task.revision,"missing":product.validation["missing"],"invalid":product.validation["invalid"],"requires_review":product.validation["requires_review"],"ready":product.validation["ready"]}),
    )
}
fn prepare_product(
    store: &Store,
    id: &str,
    frozen: &Value,
    schema: Option<&Template>,
    explicit_template: bool,
) -> Result<Product> {
    let mut task = store.task(id)?;
    eligible(store, &task)?;
    if facts(&store.root, &task)? != *frozen {
        return Err(invalid(
            "原范围的名单、来源、建议或原始文件变化，未覆盖旧材料。",
        ));
    }
    let sources = classification::sources(&store.root, &task)?;
    if explicit_template {
        check_template_original(&task)?;
    }
    let (kind, fields, template, original) = if let Some(artifact) =
        task.artifact.as_ref().filter(|_| !explicit_template)
    {
        let payload = json!({"sa_id":id,"instructions":format!("SA补充-{id}"),"candidate":artifact.candidate,"contentSha":artifact.candidate.sha256});
        let bytes = files::read_import_archive(&task, &payload)?;
        if !artifact.identity_confirmed {
            return Err(Failure::new(
                "IDENTITY_CONFLICT",
                "已有原始导出尚未核验身份，保留原文件；不改用模板绕过核验。",
            ));
        }
        ("original", BTreeMap::new(), None, Some(bytes))
    } else {
        let saved = schema.ok_or_else(|| {
            Failure::new(
                "EVIDENCE_REQUIRED",
                "缺少原始导出或对应模板的已保存 AI 建议。",
            )
        })?;
        let actual = actual_template(saved)?;
        let result = task.classification.as_ref().ok_or_else(|| {
            Failure::new("EVIDENCE_REQUIRED", "先针对实际模板生成有来源的 AI 建议。")
        })?;
        classification::validate_saved(&task, result, &sources, &json!(actual))?;
        let complete = task
            .evidence
            .iter()
            .filter(|e| e.kind == "ai_classification")
            .filter_map(|e| serde_json::from_str::<Value>(&e.text).ok())
            .any(|a| {
                a["result"] == *result
                    && a["sources"] == json!(sources)
                    && template_semantics(&a["template"]) == template_semantics(&json!(actual))
                    && a["review_required"] == true
                    && a["platform_verified"] == false
            });
        if !complete {
            return Err(Failure::new(
                "AI_RESULT_INVALID",
                "建议缺少对应当前来源和模板的完整审计，请重新生成。",
            ));
        }
        let fields = result["fields"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v["value"].as_str().unwrap().to_string()))
            .collect();
        ("template", fields, Some(actual), None)
    };
    let plan = json!({"schema":"material_recipe_v1","sa_id":id,"record":task.record,"facts":frozen,"kind":kind,"template":template,"fields":task.classification.as_ref().map(|v|&v["fields"]),"sources":sources,"platform_verified":false});
    let recipe = hash(plan.to_string().as_bytes());
    let folder = directory(&store.root, &recipe)?;
    let intent = folder.join("intent.json");
    if intent.exists() {
        if read_json(&intent)? != plan {
            return Err(invalid("原材料意图已变化，不能覆盖。"));
        }
    } else {
        atomic_json(&intent, &plan)?;
    }
    let output = folder.join(if kind == "original" {
        "原始导出.txt"
    } else {
        "模板材料.xlsx"
    });
    let receipt = folder.join("prepared.json");
    let reused = receipt.exists();
    let prepared = if reused {
        let value = read_json(&receipt)?;
        if value["plan"] != plan || value["recipe"] != recipe {
            return Err(invalid("原材料回执与当前版本不符。"));
        }
        value
    } else {
        if output.exists() {
            return Err(invalid("材料文件已有但缺少完整回执，保留原文件等待核对。"));
        }
        let staging = format!("{}.staging", uuid::Uuid::new_v4());
        let temporary = folder.join(&staging);
        let validation = if let Some(bytes) = &original {
            fs::write(&temporary, bytes)?;
            json!({"missing":[],"invalid":[],"requires_review":[],"ready":true})
        } else {
            json!(templates::write_checked(
                template.as_ref().unwrap(),
                &fields,
                &temporary
            )?)
        };
        let value = json!({"schema":"material_prepared_v1","recipe":recipe,"plan":plan,"staging":staging,"sha256":hash(&fs::read(&temporary)?),"validation":validation});
        atomic_json(&receipt, &value)?;
        value
    };
    let expected_validation = if kind == "original" {
        json!({"missing":[],"invalid":[],"requires_review":[],"ready":true})
    } else {
        json!(templates::inspect_fields(
            template.as_ref().unwrap(),
            &fields
        )?)
    };
    if prepared["schema"] != "material_prepared_v1"
        || prepared["validation"] != expected_validation
        || original
            .as_ref()
            .is_some_and(|bytes| prepared["sha256"] != hash(bytes))
    {
        return Err(invalid("材料回执的校验结果或原始字节不符，未采纳。"));
    }
    // Revalidate original facts and actual template after generation, before publishing.
    task = store.task(id)?;
    eligible(store, &task)?;
    if facts(&store.root, &task)? != *frozen {
        return Err(invalid("材料生成期间原资料变化，文件尚未采纳。"));
    }
    if let Some(schema) = &template {
        actual_template(schema)?;
        check_template_original(&task)?;
    }
    let digest = prepared["sha256"]
        .as_str()
        .ok_or_else(|| invalid("材料哈希缺失。"))?;
    if output.exists() {
        if !same_file(&output, digest)? {
            return Err(invalid("原材料字节已变化，不会覆盖或当作有效材料。"));
        }
    } else {
        let name = prepared["staging"]
            .as_str()
            .ok_or_else(|| invalid("材料暂存名缺失。"))?;
        if Path::new(name).file_name().and_then(|p| p.to_str()) != Some(name)
            || !name.ends_with(".staging")
        {
            return Err(invalid("材料暂存路径无效。"));
        }
        let staging = folder.join(name);
        if !same_file(&staging, digest)? {
            return Err(invalid("原材料暂存字节已变化。"));
        }
        verify_cells(&staging, template.as_ref(), &fields, &expected_validation)?;
        fs::rename(staging, &output)?;
    }
    verify_cells(&output, template.as_ref(), &fields, &expected_validation)?;
    let audit = folder.join("来源.json");
    let provenance = json!({"schema":"template_material_v1","recipe":recipe,"sa_id":id,"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),"classification_hash":frozen["classification_hash"],"template_id":template.as_ref().map(|t|t.id.as_str()),"template_hash":template.as_ref().map(|t|t.fingerprint.as_str()),"output":output.to_string_lossy(),"output_hash":digest,"fields":plan["fields"],"evidence":sources,"validation":prepared["validation"],"plan":plan,"invalid_fields_omitted":true,"platform_verified":false});
    if audit.exists() {
        if read_json(&audit)? != provenance {
            return Err(invalid("原来源记录已变化，不能覆盖。"));
        }
    } else {
        atomic_json(&audit, &provenance)?;
    }
    commit_audit(store, &mut task, &provenance, &audit)?;
    Ok(Product {
        sa_id: id.into(),
        recipe,
        kind: if kind == "original" {
            "original"
        } else if prepared["validation"]["ready"] == true {
            "field_valid"
        } else {
            "draft"
        }
        .into(),
        path: output.to_string_lossy().into(),
        sha256: digest.into(),
        audit: audit.to_string_lossy().into(),
        validation: prepared["validation"].clone(),
        reused,
    })
}
fn verify_cells(
    path: &Path,
    template: Option<&Template>,
    fields: &BTreeMap<String, String>,
    validation: &Value,
) -> Result<()> {
    let Some(template) = template else {
        return Ok(());
    };
    let mut book =
        calamine::Xlsx::new(std::io::Cursor::new(fs::read(path)?)).map_err(Failure::storage)?;
    let sheet = book
        .worksheet_range(&template.sheet)
        .map_err(Failure::storage)?;
    for (column, name) in template.columns.iter().enumerate() {
        let omitted = validation["missing"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(name)))
            || validation["invalid"]
                .as_array()
                .is_some_and(|a| a.iter().any(|p| p["column"] == *name));
        let expected = if omitted {
            ""
        } else {
            fields.get(name).map(String::as_str).unwrap_or("")
        };
        let actual = sheet
            .get_value((template.header_row + 1, column as u32))
            .map(ToString::to_string)
            .unwrap_or_default();
        if actual != expected {
            return Err(invalid("材料实际单元格与原范围有来源字段不符。"));
        }
    }
    Ok(())
}
