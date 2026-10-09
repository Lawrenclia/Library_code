//! Bind a claim to the original author identity, complete metadata and SA task.
use crate::*;
use serde_json::{json, Value};
fn identity(task: &Task, live: &Value) -> Result<()> {
    let row = &live["row"];
    if row["saLzkId"] != task.id
        || row["gh"] != task.record.staff_id
        || row["markStatus"] != "待处理"
        || row["titleValue"].as_str().map(norm) != Some(norm(&task.record.title))
        || matched_ids(row)?.len() != 1
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "SA 任务、完整工号、题名、匹配条目或待处理状态变化。",
        ));
    }
    Ok(())
}
fn source(live: &Value) -> Result<&str> {
    let rows: Vec<_> = live["comparison"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["label"] == "认领状态")
        .collect();
    if rows.len() != 1 {
        return Err(Failure::new("PAGE_UNSUPPORTED", "认领来源字段不唯一。"));
    }
    rows[0]["sa"]
        .as_str()
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "缺少真实 SA 认领来源。"))
}
fn source_staff(live: &Value, staff: &str) -> Result<()> {
    let text = source(live)?;
    let pattern = regex::Regex::new(r"[（(]([0-9]{1,40})[）)]").unwrap();
    let captures: Vec<_> = pattern.captures_iter(text).collect();
    if captures.len() != 1
        || &captures[0][1] != staff
        || text.chars().filter(|c| "（()）".contains(*c)).count() != 2
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "SA 认领来源的完整工号缺失、歧义或不同。",
        ));
    }
    Ok(())
}
fn authors(value: &Value) -> Result<&Vec<Value>> {
    let rows = value
        .as_array()
        .filter(|r| !r.is_empty())
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "缺少完整原作者列表。"))?;
    let mut ids = std::collections::BTreeSet::new();
    let mut orders = std::collections::BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        let id = row["id"].as_str().filter(|s| !s.is_empty());
        let order = row["order"].as_u64().filter(|n| *n > 0).or_else(|| {
            row["order"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|n| *n > 0)
        });
        if id.is_none()
            || order.is_none()
            || !ids.insert(id.unwrap())
            || !orders.insert(order.unwrap())
            || row["index"] != index
            || row["fullname"].as_str().is_none_or(|s| s.trim().is_empty())
            || !row["scholarId"].is_string()
            || !row["eligible"].is_boolean()
            || row["relations"].as_array().is_none_or(|relations| {
                relations.iter().any(|r| {
                    r["scholarId"].as_str().is_none_or(str::is_empty)
                        || r["status"].as_u64().is_none_or(|n| n > 10)
                })
            })
        {
            return Err(Failure::new(
                "IDENTITY_CONFLICT",
                "作者身份、顺序或认领关系缺失/不唯一。",
            ));
        }
    }
    Ok(rows)
}
pub fn payload(task: &Task, live: &Value, prepared: &Value, index: u64) -> Result<Value> {
    identity(task, live)?;
    source_staff(live, &task.record.staff_id)?;
    if task.running
        || matches!(task.stage, Stage::Unknown | Stage::Completed)
        || matches!(
            task.route,
            Route::NonSjtu | Route::NotFound | Route::ZeroReview
        )
        || (task.route == Route::Missing && !matches!(task.stage, Stage::Pushed | Stage::Claimed))
    {
        return Err(Failure::new(
            "INVALID_TRANSITION",
            "当前分支或阶段不能认领。",
        ));
    }
    if prepared["row"] != live["row"]
        || prepared["input_hash"] != task.input_hash
        || prepared["record_fingerprint"] != task.record.fingerprint()
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "认领准备后输入或 SA 已变化，请重新核对。",
        ));
    }
    let p = &prepared["prepared"];
    let rows = authors(&p["authors"])?;
    let target = rows
        .get(index as usize)
        .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "请选择原作者列表中的准确行。"))?;
    if p["staff_id"] != task.record.staff_id
        || p["person"]["wno"] != task.record.staff_id
        || p["person"]["id"].as_str().is_none_or(str::is_empty)
        || p["person"]["name"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
        || !p["person"]["names"].is_array()
        || p["item_id"] != matched_ids(&live["row"])?[0]
        || p["sa_text"] != source(live)?
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "原学者、完整工号、条目或 SA 来源不一致。",
        ));
    }
    let own = target["relations"].as_array().unwrap();
    if target["eligible"] != true
        || target["scholarId"] != ""
        || rows.iter().any(|r| {
            r["scholarId"] == p["person"]["id"]
                || r["relations"].as_array().unwrap().iter().any(|rel| {
                    rel["scholarId"] == p["person"]["id"] && rel["status"].as_u64().unwrap() >= 6
                })
        })
        || own.iter().any(|r| {
            r["status"].as_u64().unwrap() >= 6
                || (r["scholarId"] == p["person"]["id"]
                    && (1..=5).contains(&r["status"].as_u64().unwrap()))
        })
    {
        return Err(Failure::new(
            "REVIEW_REQUIRED",
            "原作者不可认领、已有认领或存在否认关系，不能覆盖。",
        ));
    }
    let metadata = p["metadata"]["author"]
        .as_array()
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "缺少完整原元数据。"))?;
    if metadata.len() != rows.len()
        || rows.iter().zip(metadata).any(|(r, m)| {
            ["id", "order", "fullname"]
                .iter()
                .any(|key| r[*key] != m[*key])
                || m.get("data").is_some()
        })
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "作者预览与完整原表单不同或含未保存选择。",
        ));
    }
    Ok(
        json!({"schema":"original_author_claim_v1","sa_id":task.id,"expected":live["row"],"original_sa":live,
        "input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),"confirmed":true,
        "sa_text":p["sa_text"],"staff_id":task.record.staff_id,"roster_staff_id":task.record.staff_id,"author_index":index,"prepared":p,"previous_stage":task.stage}),
    )
}
pub fn assert_plan(task: &Task, p: &Value, live: &Value) -> Result<()> {
    identity(task, live)?;
    if p["schema"] != "original_author_claim_v1"
        || p["sa_id"] != task.id
        || p["input_hash"] != task.input_hash
        || p["record_fingerprint"] != task.record.fingerprint()
        || p["staff_id"] != task.record.staff_id
        || p["sa_text"] != source(live)?
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "原认领意图、输入身份或 SA 来源变化，不能替换原作者恢复。",
        ));
    }
    let mut old = task.clone();
    old.stage = serde_json::from_value(p["previous_stage"].clone())?;
    old.running = false;
    let prepared = json!({"row":p["expected"],"prepared":p["prepared"],"input_hash":p["input_hash"],"record_fingerprint":p["record_fingerprint"]});
    let rebuilt = payload(
        &old,
        &p["original_sa"],
        &prepared,
        p["author_index"]
            .as_u64()
            .ok_or_else(|| Failure::new("IDENTITY_CONFLICT", "原作者行缺失。"))?,
    )?;
    if rebuilt != *p {
        return Err(Failure::new("TASK_CHANGED", "原认领意图不完整或已被替换。"));
    }
    let allowed = ["claimStatus", "updateTime", "updateUsername", "reason"];
    if p["expected"].as_object().is_none_or(|row| {
        row.iter()
            .any(|(key, value)| !allowed.contains(&key.as_str()) && live["row"][key] != *value)
    }) {
        return Err(Failure::new(
            "TASK_CHANGED",
            "SA 的其他字段变化，先核对原认领任务。",
        ));
    }
    Ok(())
}
pub fn assert_result(task: &Task, p: &Value, r: &Value, live: &Value) -> Result<()> {
    assert_plan(task, p, live)?;
    let prepared = &p["prepared"];
    let before = authors(&prepared["authors"])?;
    let after = authors(&r["authors"])?;
    let index = p["author_index"].as_u64().unwrap() as usize;
    let intended = &before[index];
    let person = &prepared["person"];
    if r["verified"] != true
        || r["claimed"] != true
        || r["item_id"] != prepared["item_id"]
        || r["staff_id"] != task.record.staff_id
        || r["person"] != *person
        || r["scholar_id"] != person["id"]
        || r["author_id"] != intended["id"]
        || r["author"] != intended["fullname"]
        || r["order"] != intended["order"]
        || after.len() != before.len()
        || after
            .iter()
            .filter(|a| a["scholarId"] == person["id"])
            .count()
            != 1
    {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "认领回读未唯一对应原作者 ID、学者、完整工号与条目。",
        ));
    }
    for (i, (old, current)) in before.iter().zip(after).enumerate() {
        let mut normalized = current.clone();
        if i == index {
            if current["scholarId"] != person["id"] {
                return Err(Failure::new(
                    "REMOTE_RESULT_UNKNOWN",
                    "原目标作者尚未认领。",
                ));
            }
            let other = |rows: &Value| {
                rows.as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| r["scholarId"] != person["id"])
                    .cloned()
                    .collect::<Vec<_>>()
            };
            if other(&old["relations"]) != other(&current["relations"])
                || current["relations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["scholarId"] == person["id"] && r["status"].as_u64().unwrap() < 6)
            {
                return Err(Failure::new(
                    "REMOTE_RESULT_UNKNOWN",
                    "目标作者的其他认领关系变化。",
                ));
            }
            normalized["scholarId"] = old["scholarId"].clone();
            normalized["relations"] = old["relations"].clone();
        }
        if normalized != *old {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "原作者身份、顺序或其他作者数据变化。",
            ));
        }
    }
    let mut actual = r["metadata"].clone();
    let current = actual["author"]
        .as_array_mut()
        .and_then(|a| a.get_mut(index))
        .ok_or_else(|| Failure::new("REMOTE_RESULT_UNKNOWN", "完整元数据缺少原目标作者。"))?;
    if current["scholarId"] != person["id"] {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "完整表单未回读到原学者关系。",
        ));
    }
    if let Some(value) = prepared["metadata"]["author"][index].get("scholarId") {
        current["scholarId"] = value.clone();
    } else {
        current
            .as_object_mut()
            .ok_or_else(|| Failure::new("REMOTE_RESULT_UNKNOWN", "原作者格式变化。"))?
            .remove("scholarId");
    }
    if actual != prepared["metadata"] {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "出版、角色、单位等其他完整元数据变化。",
        ));
    }
    Ok(())
}
