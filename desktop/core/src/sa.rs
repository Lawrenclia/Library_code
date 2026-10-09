//! Keep roster facts immutable; validate the live SA-to-item transition separately.
use crate::*;
use serde_json::{json, Value};

fn identity(task: &Task, snapshot: &Value) -> Result<Vec<String>> {
    let row = &snapshot["row"];
    if row["saLzkId"] != task.id
        || row["gh"] != task.record.staff_id
        || norm(row["titleValue"].as_str().unwrap_or("")) != norm(&task.record.title)
        || row["markStatus"] != "待处理"
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "实时 SA 的任务、题名、完整工号或待处理状态已变化。",
        ));
    }
    matched_ids(row)
}
pub fn link_payload(task: &Task, snapshot: &Value) -> Result<Value> {
    let ids = identity(task, snapshot)?;
    if matches!(task.stage, Stage::Unknown | Stage::Completed)
        || task.platform_id.is_empty()
        || !task.platform_id.chars().all(|c| c.is_ascii_digit())
    {
        return Err(Failure::new(
            "INVALID_TRANSITION",
            "先核验实际平台唯一号或上次结果。",
        ));
    }
    let review = task
        .review
        .as_ref()
        .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "先保存条目身份与分支核验。"))?;
    if !review.identity_confirmed
        || review.platform_id != task.platform_id
        || !task
            .evidence
            .iter()
            .any(|e| e.id == review.evidence_id && e.kind == "human_review")
    {
        return Err(Failure::new(
            "EVIDENCE_REQUIRED",
            "条目身份与明确选择的唯一号缺少核验依据。",
        ));
    }
    match task.route {
        Route::Missing if matches!(task.stage, Stage::Pushed | Stage::Claimed) => {
            library::review(task, review)?
        }
        Route::CorrectedExisting => library::review(task, review)?,
        Route::Existing if ids == vec![task.platform_id.clone()] => {}
        Route::Duplicate if task.has_verified_merges() && ids == vec![task.platform_id.clone()] => {
        }
        _ => {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "该分支尚不能关联；先推送、合并或核对现有条目。",
            ))
        }
    }
    if !ids.is_empty() && ids != vec![task.platform_id.clone()] {
        return Err(Failure::new(
            "TASK_CHANGED",
            "SA 已关联其他或多个条目，不能覆盖；先核对最新匹配。",
        ));
    }
    let review_evidence = task
        .evidence
        .iter()
        .find(|e| e.id == review.evidence_id)
        .unwrap();
    let library_evidence = if matches!(task.route, Route::Missing | Route::CorrectedExisting) {
        Some(
            task.evidence
                .iter()
                .rev()
                .find(|e| e.kind == "library_search")
                .unwrap(),
        )
    } else {
        None
    };
    let selected = if library_evidence.is_some() {
        library::latest(task)?["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == task.platform_id)
            .cloned()
            .ok_or_else(|| Failure::new("IDENTITY_CONFLICT", "原前端查询缺少明确选择的条目。"))?
    } else {
        Value::Null
    };
    Ok(
        json!({"schema":"sa_link_v2","sa_id":task.id,"expected":snapshot["row"],"original_sa":snapshot,"item_id":task.platform_id,
        "reviewed":true,"note":"关联已核验的平台唯一号",
        "previous_stage":task.stage,"route":task.route,"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),
        "original_review":review,"review_evidence":review_evidence,"review_evidence_id":review.evidence_id,
        "library_evidence":library_evidence,"library_evidence_id":library_evidence.map(|e|&e.id),"selected_item":selected,
        "already_linked":!ids.is_empty()}),
    )
}
pub fn verify_link_plan(task: &Task, payload: &Value, snapshot: &Value) -> Result<()> {
    identity(task, snapshot)?;
    let review = task
        .review
        .as_ref()
        .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "缺少关联前的条目核验。"))?;
    let previous: Stage = serde_json::from_value(payload["previous_stage"].clone())
        .map_err(|_| Failure::new("REMOTE_RESULT_UNKNOWN", "缺少原关联阶段。"))?;
    let old_ids = identity(task, &payload["original_sa"])?;
    if payload["schema"] != "sa_link_v2"
        || payload["sa_id"] != task.id
        || payload["input_hash"] != task.input_hash
        || payload["record_fingerprint"] != task.record.fingerprint()
        || payload["item_id"] != task.platform_id
        || payload["review_evidence_id"] != review.evidence_id
        || payload["original_review"] != serde_json::to_value(review)?
        || payload["route"] != serde_json::to_value(&task.route)?
        || payload["expected"] != payload["original_sa"]["row"]
        || payload["reviewed"] != true
        || payload["note"] != "关联已核验的平台唯一号"
        || payload["already_linked"] != !old_ids.is_empty()
        || (!old_ids.is_empty() && old_ids != vec![task.platform_id.clone()])
        || matches!(previous, Stage::Unknown | Stage::Completed)
        || (task.route == Route::Missing && !matches!(previous, Stage::Pushed | Stage::Claimed))
        || !matches!(
            task.route,
            Route::Missing | Route::CorrectedExisting | Route::Existing | Route::Duplicate
        )
        || !review.identity_confirmed
        || !task.evidence.iter().any(|e| {
            e.id == review.evidence_id
                && e.kind == "human_review"
                && serde_json::to_value(e).ok().as_ref() == Some(&payload["review_evidence"])
        })
    {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "原关联意图、名单、完整核验依据或阶段变化，不能替换原目标恢复。",
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
    let expected = payload["expected"]
        .as_object()
        .ok_or_else(|| Failure::new("REMOTE_RESULT_UNKNOWN", "缺少完整原 SA 行。"))?;
    if expected
        .iter()
        .any(|(key, value)| !allowed.contains(&key.as_str()) && snapshot["row"][key] != *value)
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "SA 的其他字段变化，先核对原关联任务。",
        ));
    }
    let sa_values = |s: &Value| {
        s["comparison"].as_array().map(|rows| {
            rows.iter()
                .map(|r| json!({"label":r["label"],"sa":r["sa"]}))
                .collect::<Vec<_>>()
        })
    };
    if sa_values(&payload["original_sa"]).is_none()
        || sa_values(&payload["original_sa"]) != sa_values(snapshot)
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "SA 原始比对值变化，不能沿用原关联依据。",
        ));
    }
    if matches!(task.route, Route::Missing | Route::CorrectedExisting) {
        let evidence = task
            .evidence
            .iter()
            .find(|e| e.kind == "library_search" && e.id == payload["library_evidence_id"])
            .ok_or_else(|| Failure::new("EVIDENCE_REQUIRED", "缺少原关联前端查询。"))?;
        let receipt: Value = serde_json::from_str(&evidence.text)?;
        if serde_json::to_value(evidence)? != payload["library_evidence"]
            || receipt["input_hash"] != task.input_hash
            || receipt["result"]["sa_id"] != task.id
            || receipt["result"]["verified"] != true
            || !receipt["result"]["items"].as_array().is_some_and(|rows| {
                rows.iter()
                    .any(|r| r["id"] == task.platform_id && *r == payload["selected_item"])
            })
        {
            return Err(Failure::new(
                "EVIDENCE_REQUIRED",
                "原前端来源或完整选择条目被替换。",
            ));
        }
    } else if payload["library_evidence"] != Value::Null || payload["selected_item"] != Value::Null
    {
        return Err(Failure::new(
            "EVIDENCE_REQUIRED",
            "该关联分支含非原始的条目检索依据。",
        ));
    }
    Ok(())
}
pub fn verify_link(task: &Task, payload: &Value, snapshot: &Value) -> Result<()> {
    verify_link_plan(task, payload, snapshot)?;
    if matched_ids(&snapshot["row"])? != vec![task.platform_id.clone()] {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "尚未回读到原关联的唯一条目。",
        ));
    }
    if matches!(task.route, Route::Missing | Route::CorrectedExisting) {
        let fresh = library::latest(task)?;
        let old: Value = serde_json::from_value::<Evidence>(payload["library_evidence"].clone())
            .and_then(|e| serde_json::from_str(&e.text))?;
        if fresh["target"] != old["result"]["target"]
            || !fresh["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| *r == payload["selected_item"])
        {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "原选择条目缺失或完整元数据变化，不能确认关联。",
            ));
        }
    }
    Ok(())
}
