//! Source-backed alias saves and recovery of the complete original alias list.
use crate::*;
use serde_json::{json, Value};

fn context(task: &Task, row: &Value) -> Result<()> {
    if row["saLzkId"] != task.id
        || row["gh"] != task.record.staff_id
        || row["titleValue"].as_str().map(norm) != Some(norm(&task.record.title))
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "别名操作的 SA、完整工号或论文题名不一致。",
        ));
    }
    matched_ids(row)?;
    Ok(())
}
fn scholar(value: &Value, staff: &str) -> Result<()> {
    if value["id"].as_str().is_none_or(str::is_empty)
        || value["wno"] != staff
        || ["nameCn", "nameEn"].iter().any(|k| !value[*k].is_string())
        || ["nameCn", "nameEn"]
            .iter()
            .all(|k| value[*k].as_str().unwrap_or("").trim().is_empty())
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "缺少准确文本学者身份与完整工号。",
        ));
    }
    Ok(())
}
fn aliases(value: &Value) -> Result<&Vec<Value>> {
    let rows = value
        .as_array()
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "缺少完整别名列表。"))?;
    let mut ids = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    for row in rows {
        let id = row["id"].as_str().filter(|s| !s.is_empty());
        let name = row["nameAlias"].as_str().filter(|s| !s.trim().is_empty());
        if id.is_none()
            || name.is_none()
            || !ids.insert(id.unwrap())
            || !names.insert(norm(name.unwrap()))
            || ["defaultNameCn", "defaultNameEn"]
                .iter()
                .any(|k| ![json!(0), json!(1)].contains(&row[*k]))
        {
            return Err(Failure::new(
                "IDENTITY_CONFLICT",
                "别名身份、名称或默认姓名标记缺失/不唯一。",
            ));
        }
    }
    Ok(rows)
}
pub fn payload(
    task: &Task,
    live: &Value,
    prepared: &Value,
    alias: &str,
    evidence_id: &str,
) -> Result<Value> {
    if task.running || matches!(task.stage, Stage::Unknown | Stage::Completed) {
        return Err(Failure::new(
            "INVALID_TRANSITION",
            "先核验上次操作，不能重新保存别名。",
        ));
    }
    context(task, &live["row"])?;
    if prepared["sa"] != *live
        || prepared["input_hash"] != task.input_hash
        || prepared["record_fingerprint"] != task.record.fingerprint()
        || prepared["staff_id"] != task.record.staff_id
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "别名准备后名单、SA 或来源任务已变化，请重新读取。",
        ));
    }
    scholar(&prepared["scholar"], &task.record.staff_id)?;
    aliases(&prepared["aliases"])?;
    validate_alias_source(task, alias, evidence_id)?;
    let source = task.evidence.iter().find(|e| e.id == evidence_id).unwrap();
    Ok(
        json!({"schema":"source_backed_alias_v1","sa_id":task.id,"staff_id":task.record.staff_id,
        "input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),"expected_sa":live["row"],
        "expected_scholar":prepared["scholar"],"expected_aliases":prepared["aliases"],"alias":alias.trim(),
        "evidence_id":evidence_id,"source_evidence":source,"source_sha256":hash(source.text.as_bytes()),
        "confirmed":true,"previous_stage":task.stage}),
    )
}
pub fn assert_plan(task: &Task, payload: &Value, live: &Value) -> Result<()> {
    if payload["schema"] != "source_backed_alias_v1"
        || payload["sa_id"] != task.id
        || payload["staff_id"] != task.record.staff_id
        || payload["input_hash"] != task.input_hash
        || payload["record_fingerprint"] != task.record.fingerprint()
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "原别名意图缺少完整来源或输入身份，不能沿用新任务恢复。",
        ));
    }
    context(task, &live["row"])?;
    if payload["expected_sa"] != live["row"] {
        return Err(Failure::new(
            "TASK_CHANGED",
            "SA 在别名保存后变化，先核对原任务。",
        ));
    }
    scholar(&payload["expected_scholar"], &task.record.staff_id)?;
    aliases(&payload["expected_aliases"])?;
    let name = payload["alias"].as_str().unwrap_or("");
    let id = payload["evidence_id"].as_str().unwrap_or("");
    validate_alias_source(task, name, id)?;
    let source = task.evidence.iter().find(|e| e.id == id).unwrap();
    if serde_json::to_value(source)? != payload["source_evidence"]
        || payload["source_sha256"] != hash(source.text.as_bytes())
    {
        return Err(Failure::new(
            "EVIDENCE_REQUIRED",
            "原始署名依据发生变化，不能替换原保存来源。",
        ));
    }
    let stage: Stage = serde_json::from_value(payload["previous_stage"].clone())?;
    if matches!(stage, Stage::Unknown | Stage::Completed) || payload["confirmed"] != true {
        return Err(Failure::new(
            "INVALID_TRANSITION",
            "原别名意图阶段或确认无效。",
        ));
    }
    Ok(())
}
pub fn assert_result(task: &Task, payload: &Value, result: &Value, live: &Value) -> Result<()> {
    assert_plan(task, payload, live)?;
    if result["verified"] != true || result["scholar"] != payload["expected_scholar"] {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "回读学者身份与原完整身份不一致。",
        ));
    }
    let before = aliases(&payload["expected_aliases"])?;
    let after = aliases(&result["aliases"])?;
    let name = norm(payload["alias"].as_str().unwrap());
    let target = after
        .iter()
        .find(|r| norm(r["nameAlias"].as_str().unwrap()) == name)
        .ok_or_else(|| {
            Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "尚未回读到原目标别名，不能重复新增。",
            )
        })?;
    let existing = before
        .iter()
        .any(|r| norm(r["nameAlias"].as_str().unwrap()) == name);
    if result["alias"] != *target
        || !before.iter().all(|r| after.contains(r))
        || after.len() != before.len() + usize::from(!existing)
        || (!existing && (target["defaultNameCn"] != 0 || target["defaultNameEn"] != 0))
    {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "原别名或默认姓名发生变化，不能认定为原保存结果。",
        ));
    }
    Ok(())
}
