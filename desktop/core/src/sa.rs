//! Keep roster facts immutable; validate the live SA-to-item transition separately.
use crate::*;
use serde_json::{json, Value};

const COMPLETE_ROW_KEYS: &[&str] = &[
    "id",
    "saLzkId",
    "itemId",
    "matchCount",
    "markStatus",
    "reason",
    "title",
    "titleValue",
    "doi",
    "doiValue",
    "wos",
    "wosValue",
    "claimStatus",
    "gh",
    "qr",
    "updateTime",
    "updateUsername",
    "remark",
];

fn complete_row(snapshot: &Value) -> Result<&serde_json::Map<String, Value>> {
    let row = snapshot["row"]
        .as_object()
        .ok_or_else(|| Failure::new("REMOTE_RESULT_UNKNOWN", "缺少完整 SA 行回读。"))?;
    if COMPLETE_ROW_KEYS.iter().any(|key| {
        !row.get(*key)
            .is_some_and(|v| v.is_string() || v.is_number() || v.is_boolean())
    }) || ["id", "saLzkId", "gh"].iter().any(|key| {
        !row.get(*key)
            .and_then(Value::as_str)
            .is_some_and(|v| !v.trim().is_empty())
    }) || !snapshot["comparison"].as_array().is_some_and(|rows| {
        !rows.is_empty()
            && rows.iter().all(|r| {
                ["label", "sa", "library"]
                    .iter()
                    .all(|key| r[*key].is_string())
            })
    }) {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "SA 行或逐项回读不完整，不能确认完成。",
        ));
    }
    Ok(row)
}

/// A manual note handoff is separate from the status-changing completion command.
pub fn not_found_note_payload(task: &Task, snapshot: &Value) -> Result<Value> {
    complete_row(snapshot)?;
    if task.route != Route::NotFound
        || task.record.done
        || task.record.skipped
        || task.running
        || task.batch.is_some()
        || task.batch_recheck.is_some()
        || !task.platform_id.is_empty()
        || !matches!(
            task.stage,
            Stage::AwaitingReview | Stage::Downloaded | Stage::Ready
        )
        || !identity(task, snapshot)?.is_empty()
    {
        return Err(Failure::new(
            "INVALID_TRANSITION",
            "仅未查询到文献的待处理、零匹配任务可以准备独立备注。",
        ));
    }
    let mut checked = task.clone();
    checked.sa_snapshot = Some(snapshot.clone());
    let review = task
        .review
        .as_ref()
        .filter(|r| r.route == Route::NotFound)
        .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "先保存有实际检索范围的未查询到结论。"))?;
    checked.validate_review(review)?;
    let note = review.note.trim();
    if note.chars().count() > 2000 {
        return Err(Failure::new(
            "INCOMPLETE_METADATA",
            "平台备注不能超过 2000 字符。",
        ));
    }
    let scope_ids: Vec<_> = search_scopes::current(&checked)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let proofs: Vec<_> = task
        .evidence
        .iter()
        .filter(|e| e.id == review.evidence_id || scope_ids.contains(&e.id))
        .collect();
    Ok(json!({"schema":"sa_not_found_note_v1", "sa_id":task.id,
        "input_hash":task.input_hash,"record":task.record,"route":task.route,
        "previous_stage":task.stage,"review":review,"proofs":proofs,
        "original_sa":snapshot,"note":note,"mark_status":"待处理",
        "execution":"manual_note_only","write_sent":false,"platform_completed":false}))
}

pub fn latest_note_plan(task: &Task) -> Result<(String, Value)> {
    let evidence = task
        .evidence
        .iter()
        .rev()
        .find(|e| e.kind == "sa_note_handoff")
        .ok_or_else(|| {
            Failure::new(
                "REVIEW_REQUIRED",
                "先准备独立备注，保存原 SA 回读与检索依据。",
            )
        })?;
    let payload: Value = serde_json::from_str(&evidence.text)
        .map_err(|_| Failure::new("REMOTE_RESULT_UNKNOWN", "原备注交接记录不完整。"))?;
    if payload["schema"] != "sa_not_found_note_v1"
        || not_found_note_payload(task, &payload["original_sa"])? != payload
    {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "名单、结论或检索依据变化，请重新核对备注交接记录。",
        ));
    }
    Ok((evidence.id.clone(), payload))
}

/// Only remark and server audit fields may change. Status must remain pending.
pub fn verify_not_found_note(task: &Task, payload: &Value, snapshot: &Value) -> Result<()> {
    if payload["schema"] != "sa_not_found_note_v1"
        || not_found_note_payload(task, &payload["original_sa"])? != *payload
    {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "原备注范围或依据已变化，未确认保存结果。",
        ));
    }
    let before = complete_row(&payload["original_sa"])?;
    let after = complete_row(snapshot)?;
    let allowed = ["remark", "updateTime", "updateUsername"];
    if after["markStatus"] != "待处理"
        || after["remark"] != payload["note"]
        || before.len() != after.len()
        || before
            .iter()
            .any(|(k, v)| !allowed.contains(&k.as_str()) && after.get(k) != Some(v))
        || snapshot["comparison"] != payload["original_sa"]["comparison"]
    {
        return Err(Failure::new("REMOTE_RESULT_UNKNOWN", "独立备注尚未准确保存，或 SA 状态、匹配及其他字段变化；未设置已处理，也不会发送状态操作。"));
    }
    Ok(())
}

pub fn note_status(task: &Task) -> Option<Value> {
    let (id, payload) = latest_note_plan(task).ok()?;
    let verified = task.sa_snapshot.as_ref().is_some_and(|snapshot| {
        verify_not_found_note(task, &payload, snapshot).is_ok()
            && task.evidence.iter().any(|e| {
                e.kind == "sa_note_verified"
                    && serde_json::from_str::<Value>(&e.text).is_ok_and(|saved| {
                        saved["schema"] == "sa_not_found_note_verified_v1"
                            && saved["write_sent"] == false
                            && saved["platform_completed"] == false
                            && saved["plan_id"] == id
                            && saved["payload"] == payload
                            && saved["sa_after"] == *snapshot
                    })
            })
    });
    Some(
        json!({"plan_id":id,"note":payload["note"],"verified":verified,
        "original_remark":payload["original_sa"]["row"]["remark"],"platform_completed":false}),
    )
}

/// Freeze the complete intent before sending the single status write.
pub fn complete_payload(task: &Task, snapshot: &Value) -> Result<Value> {
    complete_row(snapshot)?;
    let ids = identity(task, snapshot)?;
    if matches!(task.stage, Stage::Unknown | Stage::Completed) {
        return Err(Failure::new(
            "INVALID_TRANSITION",
            "先核验上次操作，不能再次设置已处理。",
        ));
    }
    let mut checked = task.clone();
    checked.sa_snapshot = Some(snapshot.clone());
    checked.assert_complete()?;
    if (task.route == Route::NonSjtu && !ids.is_empty())
        || (task.route == Route::Existing && ids.len() != 1)
        || (matches!(
            task.route,
            Route::Missing | Route::CorrectedExisting | Route::Duplicate
        ) && ids != vec![task.platform_id.clone()])
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "实时 SA 匹配与原完成分支或条目不一致。",
        ));
    }
    if ids.len() == 1 {
        issues::assert_resolved(&checked, snapshot)?;
    }
    let review = task.review.as_ref().unwrap();
    if review.route != task.route {
        return Err(Failure::new(
            "TASK_CHANGED",
            "核验结论与当前完成分支不一致。",
        ));
    }
    checked.validate_review(review)?;
    let library_before = if matches!(task.route, Route::Missing | Route::CorrectedExisting) {
        library::latest(task)?
    } else {
        Value::Null
    };
    let mut note = review.note.trim().to_owned();
    if !task.issue_reviews.is_empty() {
        note.push_str("；逐项核对：");
        note.push_str(
            &task
                .issue_reviews
                .iter()
                .map(|r| {
                    let label = task
                        .issue_plan
                        .as_ref()
                        .and_then(|p| p.requirements.iter().find(|i| i.key == r.key))
                        .map(|i| i.label.as_str())
                        .unwrap_or(&r.key);
                    format!("{}：{}", label, r.note)
                })
                .collect::<Vec<_>>()
                .join("；"),
        );
    }
    if note.is_empty() || note.chars().count() > 2000 {
        return Err(Failure::new(
            "INCOMPLETE_METADATA",
            "完成备注须非空且不超过平台 2000 字符限制。",
        ));
    }
    let proofs: Vec<_> = task
        .evidence
        .iter()
        .filter(|e| {
            e.id == review.evidence_id
                || task.issue_reviews.iter().any(|r| r.evidence_id == e.id)
                || task.merges.iter().any(|m| m.evidence_id == e.id)
        })
        .collect();
    Ok(json!({
        "schema":"sa_complete_v1", "sa_id":task.id,
        "input_hash":task.input_hash, "record_fingerprint":task.record.fingerprint(),
        "route":task.route, "previous_stage":task.stage, "platform_id":task.platform_id,
        "original_review":review, "issue_plan":task.issue_plan,
        "issue_reviews":task.issue_reviews, "merges":task.merges, "proofs":proofs,
        "library_before":library_before,
        "original_sa":snapshot, "expected":snapshot["row"],
        "expected_comparison":snapshot["comparison"], "reviewed":true, "note":note
    }))
}

/// A status/remark marker alone cannot prove that the original task completed.
/// This read-only check also handles restart recovery; it never sends a write.
pub fn verify_complete(task: &Task, payload: &Value, snapshot: &Value) -> Result<()> {
    if payload["schema"] != "sa_complete_v1" {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "旧完成检查点缺少完整原意图，须人工核对，不能自动确认或重发。",
        ));
    }
    let previous: Stage = serde_json::from_value(payload["previous_stage"].clone())
        .map_err(|_| Failure::new("REMOTE_RESULT_UNKNOWN", "缺少原完成阶段。"))?;
    if task.record.done || (task.stage != Stage::Unknown && task.stage != previous) {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "任务阶段已变化，不能把原完成检查点用于其他阶段。",
        ));
    }
    let mut original = task.clone();
    original.stage = previous;
    if complete_payload(&original, &payload["original_sa"])? != *payload {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "名单、分支、条目或完整核验依据已变化，不能沿用原完成意图。",
        ));
    }
    let row = complete_row(snapshot)?;
    let before = complete_row(&payload["original_sa"])?;
    let allowed = ["markStatus", "remark", "updateTime", "updateUsername"];
    if row["markStatus"] != "已处理"
        || row["remark"] != payload["note"]
        || before
            .iter()
            .any(|(key, value)| !allowed.contains(&key.as_str()) && row.get(key) != Some(value))
        || snapshot["comparison"] != payload["expected_comparison"]
    {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "未回读到原 SA 的准确完成结果，或其他字段/逐项比对已变化；不能重复提交。",
        ));
    }
    Ok(())
}

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
