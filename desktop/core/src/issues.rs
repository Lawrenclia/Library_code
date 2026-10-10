//! PPT page 4: independent, source-backed decisions against fresh SA fields.
use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueRequirement {
    pub key: String,
    pub label: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuePlan {
    pub baseline: Value,
    pub requirements: Vec<IssueRequirement>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueReview {
    pub key: String,
    pub outcome: String,
    pub evidence_id: String,
    pub note: String,
    pub snapshot: Value,
    pub created: u64,
}
fn field(snapshot: &Value, label: &str, side: &str) -> Result<String> {
    let found: Vec<_> = snapshot["comparison"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["label"] == label)
        .collect();
    if found.len() != 1 {
        return Err(Failure::new(
            "PAGE_UNSUPPORTED",
            format!("缺少唯一的核对字段：{label}"),
        ));
    }
    found[0][side]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "核对字段不是文本。"))
}
fn role(snapshot: &Value, label: &str, side: &str) -> Result<String> {
    let raw = field(snapshot, "作者信息", side)?;
    let values: Vec<_> = raw
        .lines()
        .filter_map(|line| line.trim().strip_prefix(&format!("{label}：")))
        .collect();
    if values.len() == 1 && ["是", "否"].contains(&values[0].trim()) {
        Ok(values[0].trim().into())
    } else {
        Err(Failure::new(
            "REVIEW_REQUIRED",
            format!("{label}未唯一定位到该工号作者，不能按相似姓名判断。"),
        ))
    }
}
pub fn issue_value(snapshot: &Value, key: &str, side: &str) -> Result<String> {
    match key {
        "corresponding_author" => role(snapshot, "是否通讯作者", side),
        "first_author" => role(snapshot, "是否第一作者", side),
        "first_institution" => field(snapshot, "交大是否第一单位", side),
        "author_claim" => Ok(format!(
            "{}\n{}",
            field(snapshot, "认领状态", side)?,
            field(snapshot, "作者信息", side)?
        )),
        "identifiers" => Ok(format!(
            "DOI: {}\nWOS: {}",
            field(snapshot, "DOI", side)?,
            field(snapshot, "WOS记录号", side)?
        )),
        _ => Ok(snapshot["comparison"].to_string()),
    }
}
fn anchor(snapshot: &Value) -> Value {
    let row = &snapshot["row"];
    json!([
        row["saLzkId"],
        row["itemId"].as_str().unwrap_or("").trim_start_matches(','),
        row["gh"],
        row["titleValue"]
    ])
}
fn context(task: &Task, snapshot: &Value) -> Result<()> {
    let row = &snapshot["row"];
    let ids = matched_ids(row)?;
    if row["saLzkId"] != task.id
        || ids.len() != 1
        || (!task.platform_id.is_empty() && ids[0] != task.platform_id)
        || row["gh"].as_str().filter(|s| !s.is_empty()) != Some(task.record.staff_id.as_str())
        || norm(row["titleValue"].as_str().unwrap_or("")) != norm(&task.record.title)
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "逐项核对需要唯一条目及一致的 SA、题名和完整工号。",
        ));
    }
    Ok(())
}
fn requirements(snapshot: &Value) -> Result<Vec<IssueRequirement>> {
    let mut reason = snapshot["row"]["reason"]
        .as_str()
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "待处理原因不是文本。"))?
        .to_owned();
    let definitions = [
        (
            "corresponding_author",
            "通讯作者标记不一致",
            vec!["通讯作者标记不一致", "通讯作者不一致"],
        ),
        (
            "first_author",
            "第一作者标记不一致",
            vec!["第一作者标记不一致", "第一作者不一致"],
        ),
        (
            "first_institution",
            "交大是否第一单位不一致",
            vec![
                "交大是否第一单位不一致",
                "交大第一单位不一致",
                "第一单位标记不一致",
            ],
        ),
        (
            "author_claim",
            "作者不一致 / 认领核对",
            vec!["作者不一致", "未认领"],
        ),
        (
            "identifiers",
            "DOI 和 WOS ID 都不一致",
            vec!["DOI和WOSID都不一致", "DOI 和 WOS ID 都不一致"],
        ),
    ];
    let mut found = Vec::new();
    for (key, label, aliases) in definitions {
        let mut required = false;
        for alias in aliases {
            if reason.contains(alias) {
                required = true;
                reason = reason.replace(alias, "");
            }
        }
        // Even a missing reason flag cannot hide two different visible roles.
        if ["corresponding_author", "first_author", "first_institution"].contains(&key) {
            if let (Ok(sa), Ok(lib)) = (
                issue_value(snapshot, key, "sa"),
                issue_value(snapshot, key, "library"),
            ) {
                required |= !sa.trim().is_empty() && !lib.trim().is_empty() && sa != lib;
            }
        }
        if key == "author_claim" {
            required |= field(snapshot, "认领状态", "library")
                .map(|s| s == "未认领")
                .unwrap_or(false);
        }
        if key == "identifiers" {
            if let (Ok(sd), Ok(ld), Ok(sw), Ok(lw)) = (
                field(snapshot, "DOI", "sa"),
                field(snapshot, "DOI", "library"),
                field(snapshot, "WOS记录号", "sa"),
                field(snapshot, "WOS记录号", "library"),
            ) {
                required |= !sd.is_empty()
                    && !ld.is_empty()
                    && !sw.is_empty()
                    && !lw.is_empty()
                    && normalized_doi(&sd) != normalized_doi(&ld)
                    && normalized_wos(&sw) != normalized_wos(&lw);
            }
        }
        if required {
            found.push(IssueRequirement {
                key: key.into(),
                label: label.into(),
            });
        }
    }
    let unknown: String = reason
        .chars()
        .filter(|c| !c.is_whitespace() && !"，,；;、。|/[]【】\"'".contains(*c))
        .collect();
    if !unknown.is_empty() {
        found.push(IssueRequirement {
            key: format!("other:{}", hash(unknown.as_bytes())),
            label: format!("其他待处理原因：{reason}"),
        });
    }
    Ok(found)
}
pub fn prepare(task: &mut Task, snapshot: &Value) -> Result<Value> {
    context(task, snapshot)?;
    let required = requirements(snapshot)?;
    if let Some(plan) = task.issue_plan.as_mut() {
        if anchor(&plan.baseline) != anchor(snapshot) {
            return Err(Failure::new(
                "TASK_CHANGED",
                "核对对象已变化，不能沿用旧原因结论。",
            ));
        }
        for issue in required {
            if !plan.requirements.iter().any(|i| i.key == issue.key) {
                plan.requirements.push(issue);
            }
        }
    } else {
        task.issue_plan = Some(IssuePlan {
            baseline: snapshot.clone(),
            requirements: required,
        });
    }
    let plan = task.issue_plan.as_ref().unwrap();
    let items:Vec<_>=plan.requirements.iter().map(|i|json!({"key":i.key,"label":i.label,
        "sa":issue_value(&plan.baseline,&i.key,"sa").ok(),"library_before":issue_value(&plan.baseline,&i.key,"library").ok(),
        "library":issue_value(snapshot,&i.key,"library").ok()})).collect();
    Ok(json!({"requirements":items,"baseline":plan.baseline,"live":snapshot}))
}
pub fn resolve(
    task: &mut Task,
    snapshot: &Value,
    key: &str,
    outcome: &str,
    evidence_id: &str,
    note: &str,
) -> Result<IssueReview> {
    context(task, snapshot)?;
    let plan = task
        .issue_plan
        .as_ref()
        .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "先读取逐项核对清单。"))?;
    if anchor(&plan.baseline) != anchor(snapshot) || !plan.requirements.iter().any(|i| i.key == key)
    {
        return Err(Failure::new("TASK_CHANGED", "核对对象或待处理原因已变化。"));
    }
    if note.trim().is_empty()
        || !task
            .evidence
            .iter()
            .any(|e| e.id == evidence_id && matches!(e.kind.as_str(), "metadata" | "human_review"))
    {
        return Err(Failure::new(
            "EVIDENCE_REQUIRED",
            "每个原因分别提供真实原文/来源依据与结论。",
        ));
    }
    let value = issue_value(snapshot, key, "library")?;
    if ["corresponding_author", "first_author"].contains(&key) {
        for side in ["sa", "library"] {
            let info = field(snapshot, "作者信息", side)?;
            let expected = format!("工号：{}", task.record.staff_id);
            if info.lines().filter(|l| l.trim() == expected).count() != 1 {
                return Err(Failure::new(
                    "IDENTITY_CONFLICT",
                    "作者角色尚未唯一对应完整工号，不能按相似姓名确认。",
                ));
            }
        }
    }
    match outcome {
        "sa_correct"
            if ["corresponding_author", "first_author", "first_institution"].contains(&key) =>
        {
            let expected = issue_value(&plan.baseline, key, "sa")?;
            if issue_value(snapshot, key, "sa")?.trim() != expected.trim() {
                return Err(Failure::new(
                    "TASK_CHANGED",
                    "SA 本项值在取证后变化，请重新核对原文和实时字段。",
                ));
            }
            if !["是", "否"].contains(&expected.trim()) || value.trim() != expected.trim() {
                return Err(Failure::new(
                    "REVIEW_REQUIRED",
                    "本库字段尚未回读为来源确认的 SA 值；先编辑正确字段，再回读。",
                ));
            }
        }
        "library_correct"
            if ["corresponding_author", "first_author", "first_institution"].contains(&key) =>
        {
            if !["是", "否"].contains(&value.trim()) {
                return Err(Failure::new(
                    "REVIEW_REQUIRED",
                    "本库角色字段未明确，不能保存正确结论。",
                ));
            }
        }
        "claimed" if key == "author_claim" => {
            if field(snapshot, "认领状态", "library")? != "已认领" {
                return Err(Failure::new("REVIEW_REQUIRED", "未回读到该工号已认领。"));
            }
            let info = field(snapshot, "作者信息", "library")?;
            let expected = format!("工号：{}", task.record.staff_id);
            if info.lines().filter(|l| l.trim() == expected).count() != 1 {
                return Err(Failure::new(
                    "IDENTITY_CONFLICT",
                    "认领作者尚未唯一对应完整工号。",
                ));
            }
        }
        "same_paper" if key == "identifiers" => {
            if issue_value(snapshot, key, "sa")? != issue_value(&plan.baseline, key, "sa")?
                || value != issue_value(&plan.baseline, key, "library")?
            {
                return Err(Failure::new(
                    "TASK_CHANGED",
                    "标识符在取证后变化，请重新核对对应文献。",
                ));
            }
        }
        "other_confirmed" if key.starts_with("other:") => {}
        _ => {
            return Err(Failure::new(
                "REVIEW_REQUIRED",
                "该结论不适用于这个待处理原因。",
            ))
        }
    }
    let result = IssueReview {
        key: key.into(),
        outcome: outcome.into(),
        evidence_id: evidence_id.into(),
        note: note.trim().into(),
        snapshot: snapshot.clone(),
        created: now(),
    };
    task.issue_reviews.retain(|r| r.key != key);
    task.issue_reviews.push(result.clone());
    Ok(result)
}
pub fn assert_resolved(task: &Task, snapshot: &Value) -> Result<()> {
    context(task, snapshot)?;
    let plan = task
        .issue_plan
        .as_ref()
        .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "先读取并逐项解决实时待处理原因。"))?;
    if anchor(&plan.baseline) != anchor(snapshot) {
        return Err(Failure::new("TASK_CHANGED", "逐项核对后目标条目发生变化。"));
    }
    let mut required = plan.requirements.clone();
    for issue in requirements(snapshot)? {
        if !required.iter().any(|r| r.key == issue.key) {
            required.push(issue);
        }
    }
    for issue in required {
        let r = task
            .issue_reviews
            .iter()
            .find(|r| r.key == issue.key)
            .ok_or_else(|| Failure::new("REVIEW_REQUIRED", format!("尚未解决：{}", issue.label)))?;
        if issue_value(snapshot, &issue.key, "sa")? != issue_value(&r.snapshot, &issue.key, "sa")?
            || issue_value(snapshot, &issue.key, "library")?
                != issue_value(&r.snapshot, &issue.key, "library")?
        {
            return Err(Failure::new(
                "TASK_CHANGED",
                format!("{} 在核对后改变，请重新取证。", issue.label),
            ));
        }
        let mut check = task.clone();
        resolve(
            &mut check,
            snapshot,
            &r.key,
            &r.outcome,
            &r.evidence_id,
            &r.note,
        )?;
    }
    Ok(())
}
