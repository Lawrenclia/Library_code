//! Local submission handoff. Material selection is independent of platform writes.
use crate::{
    materials::{self, Product},
    *,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Packet {
    pub id: String,
    pub sa_id: String,
    pub input_hash: String,
    pub record: Record,
    pub facts: Value,
    pub material: Product,
    pub channel: String,
    pub channel_label: String,
    pub work_type: Option<String>,
    pub organisation: String,
    pub instructions: String,
    pub created: u64,
}
fn fail(message: &str) -> Failure {
    Failure::new("SUBMISSION_CHANGED", message)
}
fn eligible(store: &Store, task: &Task) -> Result<()> {
    if task.record.matches != 0
        || task.record.done
        || task.record.skipped
        || matches!(task.stage, Stage::Unknown | Stage::Completed)
        || store.pending_input(&task.id)?.is_some()
        || !store.unresolved(&task.id)?.is_empty()
    {
        return Err(fail("只为未完成且没有待确认操作的零匹配任务选择提交材料。"));
    }
    Ok(())
}
fn editable(store: &Store, task: &Task) -> Result<()> {
    eligible(store, task)?;
    if task.batch.is_some()
        || task.batch_recheck.is_some()
        || matches!(
            task.stage,
            Stage::Uploaded | Stage::Imported | Stage::Pushed | Stage::Claimed
        )
    {
        return Err(Failure::new("INVALID_TRANSITION","已经发出上传或保留原批次，提交材料已锁定；先回读原操作，不能换文件或重新核验为其他材料。"));
    }
    Ok(())
}
fn product(audit: &Value, evidence: &Evidence) -> Option<Product> {
    if audit["schema"] != "template_material_v1" {
        return None;
    }
    Some(Product {
        sa_id: audit["sa_id"].as_str()?.into(),
        recipe: audit["recipe"].as_str()?.into(),
        kind: if audit["plan"]["kind"] == "original" {
            "original"
        } else if audit["plan"]["kind"] == "original_source" {
            "original_pending"
        } else if audit["validation"]["ready"] == true {
            "field_valid"
        } else {
            "draft"
        }
        .into(),
        path: audit["output"].as_str()?.into(),
        sha256: audit["output_hash"].as_str()?.into(),
        audit: evidence.source.clone(),
        validation: audit["validation"].clone(),
        reused: true,
    })
}
pub fn current(store: &Store, id: &str) -> Result<Option<Packet>> {
    let task = store.task(id)?;
    let Some(evidence) = task
        .evidence
        .iter()
        .rev()
        .find(|e| e.kind == "submission_packet")
    else {
        return Ok(None);
    };
    let saved: Value = serde_json::from_str(&evidence.text)?;
    if saved["schema"] != "submission_packet_v1" {
        return Err(fail("提交材料记录格式不支持。"));
    }
    Ok(Some(serde_json::from_value(saved["packet"].clone())?))
}
pub fn validate(store: &Store, task: &Task, packet: &Packet) -> Result<()> {
    if packet.sa_id != task.id
        || packet.input_hash != task.input_hash
        || json!(packet.record) != json!(task.record)
        || packet.instructions != format!("SA补充-{}", task.id)
        || packet.organisation != "上海交通大学"
        || packet.facts != materials::facts(&store.root, task)?
    {
        return Err(fail(
            "提交材料的原名单、来源、建议或说明变化，请重新选择当前版本。",
        ));
    }
    materials::verify_product(store, &packet.material, &packet.facts)?;
    if packet.material.kind == "draft"
        || (packet.material.kind != "original_pending"
            && packet.material.validation["ready"] != true)
    {
        return Err(Failure::new(
            "INCOMPLETE_METADATA",
            "当前材料仍是草稿，不能上传。",
        ));
    }
    let expected_channel = material_channel(store, &packet.material)?;
    if packet.channel != expected_channel
        || packet.channel_label != channel_label(&expected_channel)?
        || packet.material.sa_id != task.id
    {
        return Err(fail("文件与原提交渠道或论文不对应。"));
    }
    if packet.material.kind == "original" {
        let original = task
            .artifact
            .as_ref()
            .ok_or_else(|| fail("原始导出已缺失。"))?;
        if !original.identity_confirmed || original.candidate.sha256 != packet.material.sha256 {
            return Err(fail("所选原始导出与当前核验归档不一致。"));
        }
    } else if packet.material.kind != "original_pending" {
        let kind = task
            .classification
            .as_ref()
            .and_then(|v| v["type"].as_str());
        if packet.work_type.as_deref() != kind || kind.is_none() {
            return Err(Failure::new(
                "REVIEW_REQUIRED",
                "模板材料需要已保存的成果类型建议，请核对实际类型。",
            ));
        }
    }
    Ok(())
}
fn material_channel(store: &Store, material: &Product) -> Result<String> {
    if let Some(export) = materials::original_source(store, material)? {
        Ok(export.receipt.channel)
    } else if material.kind == "original" {
        Ok("wos_txt".into())
    } else {
        Ok("general".into())
    }
}
fn channel_label(channel: &str) -> Result<String> {
    catalog::channels()
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == channel)
        .and_then(|c| c["label"].as_str())
        .map(str::to_string)
        .ok_or_else(|| fail("材料的原始渠道未登记。"))
}
fn source_location(export: &source_files::OriginalExport) -> String {
    let s = &export.receipt.selection;
    if export.receipt.format == "txt" {
        format!("{} · 第 {} 至 {} 行", s.sheet, s.row, s.end_row)
    } else {
        format!("{} · 第 {} 行", s.sheet, s.row)
    }
}
fn gates(store: &Store, task: &Task, packet: &Packet) -> Vec<Failure> {
    let mut issues = vec![];
    if let Err(e) = eligible(store, task) {
        issues.push(e);
    }
    if let Err(e) = validate(store, task, packet) {
        issues.push(e);
    }
    if let Err(e) = material_ready(store, task, packet) {
        issues.push(e);
    }
    let review = task.review.as_ref();
    if task.route != Route::Missing
        || !review
            .is_some_and(|r| r.library_checked && r.identity_confirmed && r.affiliation_confirmed)
    {
        issues.push(Failure::new(
            "REVIEW_REQUIRED",
            "先完成身份、交大归属与机构库缺失核验。",
        ));
    } else if let Err(e) = library::assert_absent(task) {
        issues.push(e);
    }
    if !matches!(
        task.stage,
        Stage::Ready | Stage::Downloaded | Stage::AwaitingReview
    ) {
        issues.push(Failure::new(
            "INVALID_TRANSITION",
            "当前业务阶段不能新上传；已有平台结果应先回读。",
        ));
    }
    issues
}
fn prepared(store: &Store, task: &Task, packet: &Packet) -> Value {
    let issues = gates(store, task, packet);
    json!({"packet":packet,"task_revision":task.revision,"can_upload":issues.is_empty(),"issues":issues,"platform_verified":false})
}
pub(crate) fn bundle_scope(
    store: &Store,
    id: &str,
    packet_id: &str,
    revision: i64,
) -> Result<(Task, Packet, Value)> {
    let task = store.task(id)?;
    eligible(store, &task)?;
    if task.revision != revision {
        return Err(fail("资料已更新，请刷新当前材料后导出。"));
    }
    let packet = current(store, id)?.ok_or_else(|| fail("先明确选择并保存本篇提交材料。"))?;
    if packet.id != packet_id {
        return Err(fail("所选提交材料已变化，请刷新后导出。"));
    }
    validate(store, &task, &packet)?;
    let status = prepared(store, &task, &packet);
    Ok((task, packet, status))
}
pub fn options(store: &Store, id: &str) -> Result<Value> {
    let task = store.task(id)?;
    let facts = materials::facts(&store.root, &task)?;
    let mut choices = vec![];
    if let Some(original) = &task.artifact {
        choices.push(json!({"recipe":"original-export","kind":"original","path":original.path,"sha256":original.candidate.sha256,"usable":original.identity_confirmed,"issue":if original.identity_confirmed {Value::Null} else {json!(Failure::new("IDENTITY_CONFLICT","原始导出身份待核验。"))}}));
    }
    for export in source_files::original_exports(&store.root, &task)? {
        choices.push(json!({"recipe":format!("source-export:{}",export.evidence_id),"kind":"original_pending","path":export.receipt.archive_path,"sha256":export.receipt.sha256,"channel":export.receipt.channel,"channel_label":channel_label(&export.receipt.channel)?,"source_title":export.receipt.title,"source_location":source_location(&export),"source_name":export.receipt.original_name,"source_url":export.receipt.source_url,"usable":true,"issue":Value::Null,"notice":"可按原格式归档并保存提交信息；导出范围、渠道字段和身份归属仍需核验。"}));
    }
    let mut seen = HashSet::new();
    for e in task
        .evidence
        .iter()
        .rev()
        .filter(|e| e.kind == "material_validation")
    {
        let Ok(audit) = serde_json::from_str::<Value>(&e.text) else {
            continue;
        };
        let Some(p) = product(&audit, e) else {
            continue;
        };
        if !seen.insert(p.recipe.clone()) {
            continue;
        }
        let error = materials::verify_product(store, &p, &facts)
            .err()
            .or_else(|| {
                (p.kind == "draft")
                    .then(|| Failure::new("INCOMPLETE_METADATA", "材料仍有缺项或未核对要求。"))
            });
        let original = if error.is_none() {
            materials::original_source(store, &p)?
        } else {
            None
        };
        choices.push(json!({"recipe":p.recipe,"kind":p.kind,"path":p.path,"sha256":p.sha256,"usable":error.is_none(),"issue":error,"validation":p.validation,"channel":original.as_ref().map(|o|o.receipt.channel.as_str()),"source_title":original.as_ref().map(|o|o.receipt.title.as_str()),"source_location":original.as_ref().map(source_location),"source_name":original.as_ref().map(|o|o.receipt.original_name.as_str()),"source_url":original.as_ref().map(|o|o.receipt.source_url.as_str())}));
    }
    let selected = current(store, id)?;
    Ok(
        json!({"sa_id":id,"task_revision":task.revision,"choices":choices,"prepared":selected.as_ref().map(|p|prepared(store,&task,p))}),
    )
}
pub fn prepare(store: &Store, id: &str, recipe: &str, expected_revision: i64) -> Result<Value> {
    let mut task = store.task(id)?;
    editable(store, &task)?;
    if task.revision != expected_revision {
        return Err(fail("论文资料已更新，请刷新可用材料后选择。"));
    }
    let facts = materials::facts(&store.root, &task)?;
    let material = if let Some(evidence_id) = recipe.strip_prefix("source-export:") {
        materials::prepare_original_source(store, id, &facts, evidence_id)?
    } else if recipe == "original-export" {
        materials::prepare(store, id, &facts, None)?
    } else {
        task.evidence
            .iter()
            .rev()
            .filter(|e| e.kind == "material_validation")
            .filter_map(|e| {
                serde_json::from_str::<Value>(&e.text)
                    .ok()
                    .and_then(|v| product(&v, e))
            })
            .find(|p| p.recipe == recipe)
            .ok_or_else(|| fail("材料不属于当前论文，或缺少保存来源。"))?
    };
    // Material preparation may save its own audit; reload optimistic revision.
    task = store.task(id)?;
    let channel = material_channel(store, &material)?;
    let packet = Packet {
        id: uuid::Uuid::new_v4().to_string(),
        sa_id: id.into(),
        input_hash: task.input_hash.clone(),
        record: task.record.clone(),
        facts,
        material,
        channel_label: channel_label(&channel)?,
        channel,
        work_type: task
            .classification
            .as_ref()
            .and_then(|v| v["type"].as_str())
            .map(str::to_string),
        organisation: "上海交通大学".into(),
        instructions: format!("SA补充-{id}"),
        created: now(),
    };
    validate(store, &task, &packet)?;
    if let Some(old) = current(store, id)? {
        if validate(store, &task, &old).is_ok()
            && old.material.recipe == packet.material.recipe
            && old.channel == packet.channel
        {
            return Ok(prepared(store, &task, &old));
        }
    }
    task.evidence.push(Evidence {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "submission_packet".into(),
        source: packet.material.audit.clone(),
        text: json!({"schema":"submission_packet_v1","packet":packet,"platform_verified":false})
            .to_string(),
        created: now(),
    });
    store.save(&mut task, "submission_prepared")?;
    Ok(prepared(store, &task, &packet))
}
/// Supported institution channels are separate from external source containers.
pub fn supported_channel(channel: &str) -> bool {
    matches!(
        channel,
        "general"
            | "wos_excel"
            | "wos_txt"
            | "cnki"
            | "cscd"
            | "cssci"
            | "wanfang"
            | "ei"
            | "vip"
            | "incopat"
    )
}
fn material_ready(store: &Store, task: &Task, packet: &Packet) -> Result<()> {
    if !supported_channel(&packet.channel) {
        return Err(Failure::new(
            "CHANNEL_UNSUPPORTED",
            "此来源没有对应机构库导入入口，请使用实际平台模板。",
        ));
    }
    if packet.material.kind == "original" {
        return task.import_ready();
    }
    let extension = Path::new(&packet.material.path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let channel = catalog::channels()
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == packet.channel)
        .cloned()
        .unwrap();
    if !channel["formats"]
        .as_array()
        .unwrap()
        .contains(&json!(extension))
    {
        return Err(Failure::new(
            "FILE_INVALID",
            "材料扩展名不属于该机构库导入渠道。",
        ));
    }
    if let Some(export) = materials::original_source(store, &packet.material)? {
        source_files::assert_single_export(&export)?;
    }
    let digest = hash(json!(packet).to_string().as_bytes());
    if !task.evidence.iter().any(|e| {
        e.kind == "submission_review"
            && serde_json::from_str::<Value>(&e.text).is_ok_and(|v| {
                v["packet_hash"] == digest
                    && v["format_confirmed"] == true
                    && v["fields_complete"] == true
                    && v["single_record"] == true
                    && v["proof"].as_str().is_some_and(|s| !s.trim().is_empty())
            })
    }) {
        return Err(Failure::new(
            "REVIEW_REQUIRED",
            "请核对单篇导出、实际渠道格式和必要字段，再保存材料核验。",
        ));
    }
    Ok(())
}
pub fn review_material(
    store: &Store,
    id: &str,
    packet_id: &str,
    revision: i64,
    proof: &str,
    format: bool,
    complete: bool,
) -> Result<Value> {
    let mut task = store.task(id)?;
    editable(store, &task)?;
    if task.revision != revision {
        return Err(fail("任务变化，请刷新材料。"));
    }
    let packet = current(store, id)?.ok_or_else(|| fail("先选择本篇提交材料。"))?;
    if packet.id != packet_id {
        return Err(fail("提交材料已更换。"));
    }
    validate(store, &task, &packet)?;
    if !format || !complete || proof.trim().is_empty() || proof.chars().count() > 2000 {
        return Err(Failure::new(
            "REVIEW_REQUIRED",
            "确认实际渠道格式与必要字段，填写具体核对依据。",
        ));
    }
    if let Some(export) = materials::original_source(store, &packet.material)? {
        source_files::assert_single_export(&export)?;
    }
    task.evidence.push(Evidence { id:uuid::Uuid::new_v4().to_string(), kind:"submission_review".into(), source:packet.material.audit.clone(),
        text:json!({"packet_hash":hash(json!(packet).to_string().as_bytes()),"format_confirmed":true,"fields_complete":true,"single_record":true,"proof":proof.trim(),"platform_verified":false}).to_string(), created:now() });
    store.save(&mut task, "submission_review")?;
    Ok(prepared(store, &task, &packet))
}
pub fn check_upload(store: &Store, task: &Task) -> Result<()> {
    if let Some(packet) = current(store, &task.id)? {
        eligible(store, task)?;
        validate(store, task, &packet)?;
        material_ready(store, task, &packet)?;
        if task.route != Route::Missing
            || !task.review.as_ref().is_some_and(|r| {
                r.library_checked && r.identity_confirmed && r.affiliation_confirmed
            })
            || task.batch_recheck.is_some()
        {
            return Err(Failure::new(
                "REVIEW_REQUIRED",
                "先核对文献身份、交大署名、本库缺失和旧批次。",
            ));
        }
        library::assert_absent(task)?;
        Ok(())
    } else {
        task.import_ready()
    }
}
/// Frozen write payload, shared by upload, import, push and their recovery.
pub fn payload(store: &Store, task: &Task) -> Result<Value> {
    if let Some(packet) = current(store, &task.id)? {
        let source = materials::original_source(store, &packet.material)?;
        let mut candidate = if packet.material.kind == "original" {
            task.artifact
                .as_ref()
                .ok_or_else(|| fail("原始导出缺失。"))?
                .candidate
                .clone()
        } else {
            let audit: Value = serde_json::from_slice(&std::fs::read(&packet.material.audit)?)?;
            let fields: std::collections::BTreeMap<String, String> = if let Some(source) = &source {
                source
                    .receipt
                    .fields
                    .iter()
                    .map(|f| (f.label.clone(), f.value.clone()))
                    .collect()
            } else {
                serde_json::from_value(audit["plan"]["fields"].clone())?
            };
            let field = |aliases: &[&str]| -> String {
                fields
                    .iter()
                    .find(|(k, v)| {
                        !v.trim().is_empty()
                            && aliases
                                .iter()
                                .any(|a| norm(k.trim_start_matches('*')) == norm(a))
                    })
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            Candidate {
                title: source
                    .as_ref()
                    .map(|s| s.receipt.title.clone())
                    .unwrap_or_else(|| field(&["题名", "标题", "论文题名", "文献题名", "title"])),
                doi: normalized_doi(
                    &source
                        .as_ref()
                        .map(|s| s.receipt.doi.clone())
                        .unwrap_or_else(|| field(&["DOI"])),
                ),
                wos: normalized_wos(
                    &source
                        .as_ref()
                        .map(|s| s.receipt.wos.clone())
                        .unwrap_or_else(|| field(&["WOS记录号", "WOS ID", "wosId"])),
                ),
                authors: field(&["作者", "作者姓名", "Authors", "%A", "A1"]),
                year: field(&["年", "发表年份", "发表时间", "Year", "%D", "YR"]),
                journal: field(&["来源出版物", "期刊", "刊名", "Source Title", "%J", "JF"]),
                affiliation: field(&["机构", "作者单位", "Affiliation", "%+", "AD"]),
                sjtu: true,
                sha256: packet.material.sha256.clone(),
                fields,
            }
        };
        candidate.sha256 = packet.material.sha256.clone();
        if candidate.title.trim().is_empty()
            || (!task.record.doi.is_empty()
                && !candidate.doi.is_empty()
                && normalized_doi(&task.record.doi) != candidate.doi)
        {
            return Err(Failure::new(
                "IDENTITY_CONFLICT",
                "提交材料缺少真实题名或 DOI 与名单冲突，请重新核对。",
            ));
        }
        let ext = Path::new(&packet.material.path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        Ok(
            json!({"sa_id":task.id,"instructions":packet.instructions,"candidate":candidate,
            "channel":packet.channel,"filename":if packet.material.kind=="original" {format!("SA-WOS-{}.txt",task.id)} else {format!("SA-{}-{}.{}",packet.channel,task.id,ext)},"submission_packet":packet}),
        )
    } else {
        Ok(
            json!({"sa_id":task.id,"instructions":format!("SA补充-{}",task.id),"candidate":task.artifact.as_ref().map(|a|&a.candidate)}),
        )
    }
}
/// Recover from persisted intents, including verified uploads after a restart.
pub fn recovery_payload(store: &Store, task: &Task) -> Result<Value> {
    if let Some(saved) = store.latest_import_intent(&task.id)? {
        if saved["sa_id"] != task.id || saved["instructions"] != format!("SA补充-{}", task.id) {
            return Err(Failure::new(
                "INPUT_CHANGED",
                "原操作与当前 SA 任务不对应，未采用。",
            ));
        }
        return Ok(saved);
    }
    // Imported legacy checkpoints predate the local intent store. Only their
    // native WOS archive can provide this compatibility path; never rebuild a
    // new-format submission from today's material fields.
    if current(store, &task.id)?.is_some() {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "缺少原提交意图，不能从当前材料重新生成；请人工核对原批次。",
        ));
    }
    let artifact = task
        .artifact
        .as_ref()
        .ok_or_else(|| Failure::new("FILE_INVALID", "缺少原导入归档或意图。"))?;
    Ok(
        json!({"sa_id":task.id,"instructions":format!("SA补充-{}",task.id),"candidate":artifact.candidate}),
    )
}
/// Recovery verifies the exact saved bytes without requiring an editable stage.
pub fn read_archive(store: &Store, task: &Task, payload: &Value) -> Result<Vec<u8>> {
    let Some(value) = payload.get("submission_packet").filter(|v| !v.is_null()) else {
        return files::read_import_archive(task, payload);
    };
    let packet: Packet = serde_json::from_value(value.clone())?;
    if packet.sa_id != task.id
        || packet.input_hash != task.input_hash
        || packet.record.fingerprint() != task.record.fingerprint()
        || packet.facts != materials::facts(&store.root, task)?
        || !task.evidence.iter().any(|e| {
            e.kind == "submission_packet"
                && serde_json::from_str::<Value>(&e.text).is_ok_and(|v| {
                    v["schema"] == "submission_packet_v1" && v["packet"] == json!(packet)
                })
        })
        || payload["sa_id"] != task.id
        || payload["instructions"] != packet.instructions
        || payload
            .get("contentSha")
            .is_some_and(|v| *v != json!(packet.material.sha256))
        || payload["candidate"]["sha256"] != packet.material.sha256
        || payload["channel"] != packet.channel
    {
        return Err(fail("原操作的材料、名单或来源变化，保持结果未知。"));
    }
    let path = Path::new(&packet.material.path);
    if packet.material.recipe.len() != 64
        || !packet
            .material
            .recipe
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(fail("材料版本编号无效。"));
    }
    let owned = store
        .root
        .join("materials")
        .join("products")
        .join(&packet.material.recipe)
        .canonicalize()?;
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || !path.canonicalize()?.starts_with(owned)
        || meta.len() > 524288
    {
        return Err(Failure::new(
            "FILE_INVALID",
            "提交归档路径或大小无效；每篇最多 512 KB。",
        ));
    }
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() || hash(&bytes) != packet.material.sha256 {
        return Err(fail("原提交文件字节变化。"));
    }
    Ok(bytes)
}
/// Cross-channel duplicate protection uses every preserved write intent, including history.
pub fn assert_no_prior(store: &Store, task: &Task, payload: &Value) -> Result<()> {
    let target = &payload["candidate"];
    for other in store.tasks()? {
        for e in &other.evidence {
            if !matches!(
                e.kind.as_str(),
                "import_receipt" | "import_verified" | "upload_verified"
            ) {
                continue;
            }
            let v: Value = serde_json::from_str(&e.text)?;
            let c = &v["payload"]["candidate"];
            if c.is_null() {
                continue;
            }
            let same = (!target["doi"].as_str().unwrap_or("").is_empty()
                && c["doi"] == target["doi"])
                || (!target["wos"].as_str().unwrap_or("").is_empty() && c["wos"] == target["wos"])
                || c["sha256"] == target["sha256"]
                || (c["title"]
                    .as_str()
                    .is_some_and(|s| norm(s) == norm(target["title"].as_str().unwrap_or(""))));
            if same {
                return Err(Failure::new(
                    "DUPLICATE_WRITE",
                    "该论文已有保存的平台操作回执，请回读或关联原条目，不能新上传。",
                ));
            }
        }
        if other.id != task.id
            && matches!(
                other.stage,
                Stage::Uploaded | Stage::Imported | Stage::Pushed | Stage::Claimed | Stage::Unknown
            )
            && norm(&other.record.title) == norm(target["title"].as_str().unwrap_or(""))
        {
            return Err(Failure::new(
                "DUPLICATE_WRITE",
                "同题名任务已有平台操作，请先核对原批次。",
            ));
        }
    }
    Ok(())
}

/// Expected source labels are checked against the live list filter before any file write.
pub fn batch_source(channel: &str) -> Option<&'static str> {
    Some(match channel {
        "wos_txt" | "wos_excel" => "WOS",
        "cnki" => "CNKI",
        "cscd" => "CSCD",
        "cssci" => "CSSCI",
        "wanfang" => "万方",
        "ei" => "EI",
        "vip" => "VIP",
        "incopat" => "IncoPat",
        "general" => "手工",
        _ => return None,
    })
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, Store, Task) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        let record:Record=serde_json::from_value(json!({"row":2,"owner":"负责人","sa_id":"frozen-sa","title":"Paper","doi":"","wos":"","staff_id":"001","matches":0,"item_ids":"","mark":"待处理","reason":"","done":false,"skipped":false,"source":""})).unwrap();
        store.import(vec![record], "input".into()).unwrap();
        let task = store.task("frozen-sa").unwrap();
        (dir, store, task)
    }
    #[test]
    fn original_verified_intent_survives_restart_and_new_unsent_draft() {
        let (dir, store, mut task) = setup();
        let original = json!({"sa_id":task.id,"instructions":"SA补充-frozen-sa","channel":"cnki","candidate":{"title":"Original paper","sha256":"original"},"filename":"SA-cnki-frozen-sa.txt"});
        let attempt = store
            .begin_attempt_with_payload(&mut task, "import_upload", original.clone())
            .unwrap();
        task.running = false;
        task.stage = Stage::Uploaded;
        store
            .finish_attempt(
                &mut task,
                &attempt,
                "verified",
                json!({"uploaded":true}),
                "uploaded",
            )
            .unwrap();
        let later = store
            .begin_attempt_with_payload(
                &mut task,
                "import_submit",
                json!({"candidate":{"title":"New draft"}}),
            )
            .unwrap();
        task.running = false;
        store
            .finish_attempt(&mut task, &later, "not_sent", json!({}), "not_sent")
            .unwrap();
        drop(store);
        let reopened = Store::new(dir.path()).unwrap();
        assert_eq!(
            recovery_payload(&reopened, &reopened.task(&task.id).unwrap()).unwrap(),
            original
        );
    }
    #[test]
    fn material_cannot_be_reselected_or_reviewed_after_upload() {
        let (_dir, store, mut task) = setup();
        task.stage = Stage::Uploaded;
        task.batch = Some(json!({"uploaded":true}));
        store.save(&mut task, "uploaded").unwrap();
        assert_eq!(
            prepare(&store, &task.id, "original-export", task.revision)
                .unwrap_err()
                .code,
            "INVALID_TRANSITION"
        );
        assert_eq!(
            review_material(
                &store,
                &task.id,
                "new-packet",
                task.revision,
                "proof",
                true,
                true
            )
            .unwrap_err()
            .code,
            "INVALID_TRANSITION"
        );
        assert!(store.unresolved(&task.id).unwrap().is_empty());
    }
    #[test]
    fn recovery_adopts_the_preserved_packet_not_the_latest_material_selection() {
        let (dir, store, mut task) = setup();
        let bytes = b"original source bytes";
        let recipe = hash(b"original product");
        let folder = dir.path().join("materials/products").join(&recipe);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("original.txt");
        std::fs::write(&file, bytes).unwrap();
        let packet = Packet {
            id: "original-packet".into(),
            sa_id: task.id.clone(),
            input_hash: task.input_hash.clone(),
            record: task.record.clone(),
            facts: materials::facts(&store.root, &task).unwrap(),
            material: Product {
                sa_id: task.id.clone(),
                recipe,
                kind: "original_pending".into(),
                path: file.to_string_lossy().into(),
                sha256: hash(bytes),
                audit: folder.join("missing-audit.json").to_string_lossy().into(),
                validation: json!({"ready":false}),
                reused: false,
            },
            channel: "cnki".into(),
            channel_label: "CNKI 数据导入 (Excel/Txt)".into(),
            work_type: None,
            organisation: "上海交通大学".into(),
            instructions: "SA补充-frozen-sa".into(),
            created: now(),
        };
        for p in [packet.clone(), {
            let mut p = packet.clone();
            p.id = "later-selection".into();
            p
        }] {
            task.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "submission_packet".into(),
                source: "local".into(),
                text: json!({"schema":"submission_packet_v1","packet":p}).to_string(),
                created: now(),
            });
        }
        task.stage = Stage::Unknown;
        store.save(&mut task, "unknown").unwrap();
        let payload = json!({"sa_id":task.id,"instructions":packet.instructions,"channel":packet.channel,"candidate":{"sha256":packet.material.sha256},"contentSha":packet.material.sha256,"submission_packet":packet});
        assert_eq!(read_archive(&store, &task, &payload).unwrap(), bytes);
        std::fs::write(&file, b"changed bytes").unwrap();
        assert_eq!(
            read_archive(&store, &task, &payload).unwrap_err().code,
            "SUBMISSION_CHANGED"
        );
    }
    #[test]
    fn mismatched_original_sa_intent_is_not_rebuilt_from_current_fields() {
        let (_dir, store, mut task) = setup();
        let saved = json!({"sa_id":"other-sa","instructions":"SA补充-other-sa"});
        store
            .begin_attempt_with_payload(&mut task, "import_upload", saved)
            .unwrap();
        assert_eq!(
            recovery_payload(&store, &task).unwrap_err().code,
            "INPUT_CHANGED"
        );
    }
}
