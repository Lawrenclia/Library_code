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
    Ok(
        json!({"sa_id":task.id,"expected":snapshot["row"],"item_id":task.platform_id,
        "reviewed":true,"note":"关联已核验的平台唯一号",
        "previous_stage":task.stage,"input_hash":task.input_hash,"review_evidence_id":review.evidence_id,
        "library_evidence_id":task.evidence.iter().rev().find(|e|e.kind=="library_search").map(|e|&e.id),
        "already_linked":!ids.is_empty()}),
    )
}
pub fn verify_link(task: &Task, payload: &Value, snapshot: &Value) -> Result<()> {
    let ids = identity(task, snapshot)?;
    let review = task
        .review
        .as_ref()
        .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "缺少关联前的条目核验。"))?;
    if payload["sa_id"] != task.id
        || payload["input_hash"] != task.input_hash
        || payload["item_id"] != task.platform_id
        || payload["review_evidence_id"] != review.evidence_id
        || !review.identity_confirmed
        || ids != vec![task.platform_id.clone()]
        || ["saLzkId", "gh", "titleValue"]
            .iter()
            .any(|key| payload["expected"][key] != snapshot["row"][key])
    {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "未回读到本次关联的同一任务、工号、题名与唯一条目。",
        ));
    }
    if matches!(task.route, Route::Missing | Route::CorrectedExisting)
        && !task
            .evidence
            .iter()
            .any(|e| e.kind == "library_search" && e.id == payload["library_evidence_id"])
    {
        return Err(Failure::new(
            "EVIDENCE_REQUIRED",
            "缺少本次关联使用的实际前端检索依据。",
        ));
    }
    Ok(())
}
