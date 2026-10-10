//! Bind a duplicate merge to its original SA, candidate fields and source proof.
use crate::*;
use serde_json::{json, Value};
fn identity(task: &Task, sa: &Value) -> Result<Vec<String>> {
    if sa["row"]["saLzkId"] != task.id
        || sa["row"]["gh"] != task.record.staff_id
        || sa["row"]["markStatus"] != "待处理"
        || sa["row"]["titleValue"].as_str().map(norm) != Some(norm(&task.record.title))
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "原合并任务、完整工号、题名或待处理状态变化。",
        ));
    }
    matched_ids(&sa["row"])
}
fn group(g: &Value) -> Result<&Vec<Value>> {
    let rows = g["items"]
        .as_array()
        .filter(|r| r.len() >= 2)
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "原候选组缺少完整条目。"))?;
    let mut ids = std::collections::BTreeSet::new();
    if g["id"].as_str().is_none_or(str::is_empty)
        || rows.iter().any(|r| {
            r["id"]
                .as_str()
                .is_none_or(|id| id.is_empty() || !ids.insert(id))
                || r["model_name"].as_str().is_none_or(str::is_empty)
                || r["metadata"]["title"].as_array().is_none_or(|titles| {
                    titles.is_empty()
                        || titles
                            .iter()
                            .any(|v| v.as_str().is_none_or(|s| s.trim().is_empty()))
                })
        })
        || !rows.iter().any(|r| r["id"] == g["primary_id"])
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "候选组身份、模型或题名缺失/不唯一。",
        ));
    }
    Ok(rows)
}
pub fn payload(task: &Task, sa: &Value, prepared: &Value, extra: &Value) -> Result<Value> {
    let ids = identity(task, sa)?;
    if task.running
        || prepared["sa"] != *sa
        || prepared["row"] != sa["row"]
        || prepared["input_hash"] != task.input_hash
        || prepared["record_fingerprint"] != task.record.fingerprint()
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "候选准备后的完整 SA 或名单变化，请重新核对。",
        ));
    }
    let source = extra["source_id"].as_str().unwrap_or("");
    let target = extra["target_id"].as_str().unwrap_or("");
    let proof = extra["evidence_id"].as_str().unwrap_or("");
    let retained = extra["retained"].as_str().unwrap_or("");
    validate_merge_review(
        task,
        source,
        target,
        &ids,
        proof,
        extra["identity_confirmed"] == true,
        retained,
    )?;
    let g = &prepared["result"]["group"];
    let rows = group(g)?;
    let master = rows
        .iter()
        .find(|r| r["id"] == target)
        .ok_or_else(|| Failure::new("IDENTITY_CONFLICT", "主条目不在原候选中。"))?;
    let original_source = rows
        .iter()
        .find(|r| r["id"] == source)
        .ok_or_else(|| Failure::new("IDENTITY_CONFLICT", "被合并条目不在原候选中。"))?;
    let threshold = prepared["result"]["title_similarity"]
        .as_f64()
        .filter(|v| v.is_finite() && (85.0..=100.0).contains(v))
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "缺少原实际检索相似度。"))?;
    if prepared["result"]["title"] != task.record.title
        || prepared["result"]["matched_ids"] != json!(ids)
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "候选对应另一题名或 SA 匹配集合。",
        ));
    }
    let evidence = task.evidence.iter().find(|e| e.id == proof).unwrap();
    Ok(
        json!({"schema":"merge_context_v1","sa_id":task.id,"title":task.record.title,"group_id":g["id"],"source_id":source,"target_id":target,
        "expected_group":g,"expected_master":master,"expected_source":original_source,"expected_threshold":threshold,"confirmed":true,"identity_confirmed":true,
        "evidence_id":proof,"original_evidence":evidence,"retained":retained,"previous_stage":task.stage,"previous_platform_id":task.platform_id,"previous_merges":task.merges,
        "expected_sa":sa["row"],"original_sa":sa,"prepared":prepared,"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint()}),
    )
}
pub fn assert_plan(task: &Task, p: &Value, sa: &Value) -> Result<()> {
    identity(task, sa)?;
    if p["schema"] != "merge_context_v1"
        || p["input_hash"] != task.input_hash
        || p["record_fingerprint"] != task.record.fingerprint()
        || p["sa_id"] != task.id
        || p["previous_platform_id"] != task.platform_id
        || p["previous_merges"] != serde_json::to_value(&task.merges)?
    {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "原合并意图、名单或合并历史不完整/已变化，不能重建原目标。",
        ));
    }
    let mut old = task.clone();
    old.stage = serde_json::from_value(p["previous_stage"].clone())?;
    old.running = false;
    let extra = json!({"source_id":p["source_id"],"target_id":p["target_id"],"evidence_id":p["evidence_id"],"retained":p["retained"],"identity_confirmed":p["identity_confirmed"]});
    if payload(&old, &p["original_sa"], &p["prepared"], &extra)? != *p {
        return Err(Failure::new(
            "TASK_CHANGED",
            "原合并来源、完整候选或选择被替换。",
        ));
    }
    let allowed = [
        "itemId",
        "matchCount",
        "claimStatus",
        "reason",
        "updateTime",
        "updateUsername",
    ];
    if p["expected_sa"].as_object().is_none_or(|row| {
        row.iter()
            .any(|(key, value)| !allowed.contains(&key.as_str()) && sa["row"][key] != *value)
    }) {
        return Err(Failure::new(
            "TASK_CHANGED",
            "SA 的其他字段变化，先核对原合并任务。",
        ));
    }
    let values = |v: &Value| {
        v["comparison"].as_array().map(|rows| {
            rows.iter()
                .map(|r| json!({"label":r["label"],"sa":r["sa"]}))
                .collect::<Vec<_>>()
        })
    };
    if values(&p["original_sa"]).is_none() || values(&p["original_sa"]) != values(sa) {
        return Err(Failure::new(
            "TASK_CHANGED",
            "SA 原比对来源变化，不能沿用原合并依据。",
        ));
    }
    Ok(())
}
pub fn assert_sa_after(task: &Task, p: &Value, sa: &Value) -> Result<()> {
    assert_plan(task, p, sa)?;
    let mut expected = matched_ids(&p["expected_sa"])?;
    expected.retain(|id| Some(id.as_str()) != p["source_id"].as_str());
    expected.sort();
    let mut actual = matched_ids(&sa["row"])?;
    actual.sort();
    if expected != actual
        || !actual
            .iter()
            .any(|id| Some(id.as_str()) == p["target_id"].as_str())
    {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "SA 条目集合尚未与原合并结果一致，不能重发。",
        ));
    }
    Ok(())
}
pub fn retained_fields(master: &Value, source: &Value, actual: &Value) -> Result<()> {
    let before = master["metadata"]
        .as_object()
        .ok_or_else(|| Failure::new("REMOTE_RESULT_UNKNOWN", "缺少主条目完整原字段。"))?;
    let fields = actual
        .as_object()
        .ok_or_else(|| Failure::new("REMOTE_RESULT_UNKNOWN", "缺少主条目完整回读字段。"))?;
    for (key, value) in before {
        if key == "title" {
            let original = value.as_array().unwrap();
            let current = fields
                .get(key)
                .and_then(Value::as_array)
                .ok_or_else(|| Failure::new("REMOTE_RESULT_UNKNOWN", "主条目题名缺失。"))?;
            let source_titles = source["metadata"]["title"].as_array();
            let known = |v: &Value| {
                original
                    .iter()
                    .chain(source_titles.into_iter().flatten())
                    .any(|old| old.as_str().map(norm) == v.as_str().map(norm))
            };
            if !original.iter().all(|old| {
                current
                    .iter()
                    .any(|v| v.as_str().map(norm) == old.as_str().map(norm))
            }) || current
                .iter()
                .any(|v| v.as_str().is_none_or(str::is_empty) || !known(v))
            {
                return Err(Failure::new(
                    "REMOTE_RESULT_UNKNOWN",
                    "主条目题名缺失或含非原来源题名。",
                ));
            }
        } else if fields.get(key) != Some(value) {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                format!("主条目原字段 {key} 丢失或改变，不能确认保留内容。"),
            ));
        }
    }
    for (key, value) in fields {
        if !before.contains_key(key) && source["metadata"].get(key) != Some(value) {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "主条目新增字段没有原候选来源。",
            ));
        }
    }
    Ok(())
}
pub fn assert_result(task: &Task, p: &Value, r: &Value, sa: &Value) -> Result<()> {
    assert_sa_after(task, p, sa)?;
    if r["verified"] != true
        || r["source_id"] != p["source_id"]
        || r["target_id"] != p["target_id"]
        || r["master_after"]["id"] != p["target_id"]
        || r["master_after"]["model_name"] != p["expected_master"]["model_name"]
        || task.merges.iter().any(|m| m.source_id == p["source_id"])
        || r["pool_after"].as_array().is_none_or(|groups| {
            groups.iter().any(|g| {
                g["items"]
                    .as_array()
                    .is_none_or(|items| items.iter().any(|i| i["id"] == p["source_id"]))
            })
        })
    {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "合并主条目、原对象或完整候选池回读不一致。",
        ));
    }
    retained_fields(
        &p["expected_master"],
        &p["expected_source"],
        &r["master_after"]["fields"],
    )?;
    Ok(())
}
