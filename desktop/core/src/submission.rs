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
fn product(audit: &Value, evidence: &Evidence) -> Option<Product> {
    if audit["schema"] != "template_material_v1" {
        return None;
    }
    Some(Product {
        sa_id: audit["sa_id"].as_str()?.into(),
        recipe: audit["recipe"].as_str()?.into(),
        kind: if audit["plan"]["kind"] == "original" {
            "original"
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
    if packet.material.kind == "draft" || packet.material.validation["ready"] != true {
        return Err(Failure::new(
            "INCOMPLETE_METADATA",
            "当前材料仍是草稿，不能上传。",
        ));
    }
    let expected_channel = if packet.material.kind == "original" {
        "wos_txt"
    } else {
        "general"
    };
    if packet.channel != expected_channel || packet.material.sa_id != task.id {
        return Err(fail("文件与原提交渠道或论文不对应。"));
    }
    if packet.channel == "wos_txt" {
        let original = task
            .artifact
            .as_ref()
            .ok_or_else(|| fail("原始导出已缺失。"))?;
        if !original.identity_confirmed || original.candidate.sha256 != packet.material.sha256 {
            return Err(fail("所选原始导出与当前核验归档不一致。"));
        }
    } else {
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
fn gates(store: &Store, task: &Task, packet: &Packet) -> Vec<Failure> {
    let mut issues = vec![];
    if let Err(e) = eligible(store, task) {
        issues.push(e);
    }
    if let Err(e) = validate(store, task, packet) {
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
    if packet.channel != "wos_txt" {
        issues.push(Failure::new(
            "CHANNEL_UNSUPPORTED",
            "本地模板材料已准备；通用 Excel 上传页面驱动尚未接通，不能自动提交。",
        ));
    } else if let Err(e) = task.import_ready() {
        issues.push(e);
    }
    if !matches!(task.stage, Stage::Ready | Stage::Downloaded) {
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
pub fn options(store: &Store, id: &str) -> Result<Value> {
    let task = store.task(id)?;
    let facts = materials::facts(&store.root, &task)?;
    let mut choices = vec![];
    if let Some(original) = &task.artifact {
        choices.push(json!({"recipe":"original-export","kind":"original","path":original.path,"sha256":original.candidate.sha256,"usable":original.identity_confirmed,"issue":if original.identity_confirmed {Value::Null} else {json!(Failure::new("IDENTITY_CONFLICT","原始导出身份待核验。"))}}));
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
        choices.push(json!({"recipe":p.recipe,"kind":p.kind,"path":p.path,"sha256":p.sha256,"usable":error.is_none(),"issue":error,"validation":p.validation}));
    }
    let selected = current(store, id)?;
    Ok(
        json!({"sa_id":id,"task_revision":task.revision,"choices":choices,"prepared":selected.as_ref().map(|p|prepared(store,&task,p))}),
    )
}
pub fn prepare(store: &Store, id: &str, recipe: &str, expected_revision: i64) -> Result<Value> {
    let mut task = store.task(id)?;
    eligible(store, &task)?;
    if task.revision != expected_revision {
        return Err(fail("论文资料已更新，请刷新可用材料后选择。"));
    }
    let facts = materials::facts(&store.root, &task)?;
    let material = if recipe == "original-export" {
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
    let channel = if material.kind == "original" {
        "wos_txt"
    } else {
        "general"
    };
    let packet = Packet {
        id: uuid::Uuid::new_v4().to_string(),
        sa_id: id.into(),
        input_hash: task.input_hash.clone(),
        record: task.record.clone(),
        facts,
        material,
        channel: channel.into(),
        channel_label: if channel == "wos_txt" {
            "WOS 数据导入 (Txt)"
        } else {
            "数据导入"
        }
        .into(),
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
/// Existing WOS driver may use a selected material only after full source recheck.
pub fn check_upload(store: &Store, task: &Task) -> Result<()> {
    if let Some(packet) = current(store, &task.id)? {
        eligible(store, task)?;
        validate(store, task, &packet)?;
        if packet.channel != "wos_txt" {
            return Err(Failure::new(
                "CHANNEL_UNSUPPORTED",
                "所选 Excel 材料的上传驱动尚未接通。",
            ));
        }
        if !Path::new(&packet.material.path).is_file() {
            return Err(fail("所选提交文件已缺失。"));
        }
    }
    Ok(())
}
