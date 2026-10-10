//! Source-backed author roles and complete author/institution order (PPT page 4).
use crate::*;
use serde_json::{json, Value};

pub fn payload(
    task: &mut Task,
    live: &Value,
    prepared: &Value,
    key: &str,
    index: u64,
    source: &str,
    proof: &str,
    note: &str,
) -> Result<Value> {
    build_payload(
        task,
        live,
        prepared,
        key,
        index,
        "role",
        &Value::Null,
        source,
        proof,
        note,
    )
}
pub fn order_payload(
    task: &mut Task,
    live: &Value,
    prepared: &Value,
    key: &str,
    index: u64,
    operation: &str,
    order: &Value,
    source: &str,
    proof: &str,
    note: &str,
) -> Result<Value> {
    build_payload(
        task, live, prepared, key, index, operation, order, source, proof, note,
    )
}
fn build_payload(
    task: &mut Task,
    live: &Value,
    prepared: &Value,
    key: &str,
    index: u64,
    operation: &str,
    order: &Value,
    source: &str,
    proof: &str,
    note: &str,
) -> Result<Value> {
    if matches!(task.stage, Stage::Unknown | Stage::Completed) || task.running {
        return Err(Failure::new(
            "INVALID_TRANSITION",
            "先核验上次操作，不能再次编辑。",
        ));
    }
    let reorder = operation != "role";
    if (!reorder && !["corresponding_author", "first_author"].contains(&key))
        || (reorder
            && !matches!(
                (key, operation),
                ("first_author", "author_order") | ("first_institution", "institution_order")
            ))
    {
        return Err(Failure::new(
            "PAGE_UNSUPPORTED",
            "该原因仍需实际顺序控件适配，不能改隐藏字段。",
        ));
    }
    let plan = task
        .issue_plan
        .as_ref()
        .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "先读取逐项核对清单。"))?;
    if !plan.requirements.iter().any(|r| r.key == key) || prepared["sa"] != *live {
        return Err(Failure::new(
            "TASK_CHANGED",
            "原因或 SA 字段在编辑准备后变化。",
        ));
    }
    let ids = matched_ids(&live["row"])?;
    let identity = &prepared["identity"];
    let result = &prepared["result"];
    if ids.len() != 1
        || live["row"]["saLzkId"] != task.id
        || live["row"]["gh"] != task.record.staff_id
        || live["row"]["markStatus"] != "待处理"
        || norm(live["row"]["titleValue"].as_str().unwrap_or("")) != norm(&task.record.title)
        || result["item_id"] != ids[0]
        || result["staff_id"] != task.record.staff_id
        || identity["staff_id"] != task.record.staff_id
        || identity["scholar"]["wno"] != task.record.staff_id
        || identity["scholar"]["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .is_none()
        || result["scholar"] != identity["scholar"]
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "编辑目标与 SA、题名、完整工号和学者身份不一致。",
        ));
    }
    let original_sa = issues::issue_value(&plan.baseline, key, "sa")?;
    if issues::issue_value(live, key, "sa")? != original_sa
        || !["是", "否"].contains(&original_sa.as_str())
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "SA 角色值已变化或未唯一定位。",
        ));
    }
    let rows = result["authors"]
        .as_array()
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "缺少实际作者列表。"))?;
    let selected: Vec<_> = rows.iter().filter(|a| a["index"] == index).collect();
    if selected.len() != 1 || selected[0]["eligible"] != true {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "请选择经过完整工号核对的作者行。",
        ));
    }
    let author = selected[0];
    let field = if key == "corresponding_author" {
        "correspondent"
    } else {
        "commonFirst"
    };
    if !reorder
        && !author["fields"]
            .as_array()
            .map(|fields| fields.iter().any(|f| f == field))
            .unwrap_or(false)
    {
        return Err(Failure::new(
            "PAGE_UNSUPPORTED",
            "对应角色没有实际可编辑控件。",
        ));
    }
    let snapshot = &result["snapshot"];
    let original_author = snapshot["form"]["metadata"]["author"]
        .as_array()
        .and_then(|a| a.get(index as usize))
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "作者不在完整原始表单中。"))?;
    if original_author["id"] != author["id"]
        || original_author["fullname"] != author["fullname"]
        || (!reorder && original_author[field].as_bool().is_none())
        || snapshot["form"]["id"] != ids[0]
        || !snapshot["form"]["metadata"].is_object()
        || !snapshot["fields"].is_array()
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "实际表单与作者预览不一致。",
        ));
    }
    let author_fields: Vec<_> = live["comparison"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["label"] == "作者信息")
        .collect();
    if author_fields.len() != 1 {
        return Err(Failure::new("PAGE_UNSUPPORTED", "作者信息未唯一定位。"));
    }
    for side in ["sa", "library"] {
        let text = author_fields[0][side]
            .as_str()
            .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "作者信息不是文本。"))?;
        let staff: Vec<_> = text
            .lines()
            .filter_map(|line| line.trim().strip_prefix("工号："))
            .collect();
        if staff.len() != 1 || staff[0].trim() != task.record.staff_id {
            return Err(Failure::new(
                "IDENTITY_CONFLICT",
                "角色未定位到同一完整工号。",
            ));
        }
        if side == "library" {
            let names: Vec<_> = text
                .lines()
                .filter_map(|line| line.trim().strip_prefix("署名："))
                .collect();
            if names.len() != 1
                || Some(names[0].trim()) != author["fullname"].as_str().map(str::trim)
            {
                return Err(Failure::new(
                    "IDENTITY_CONFLICT",
                    "选中作者与 SA 本库对比中的实际署名不同。",
                ));
            }
        }
    }
    if source.trim().is_empty() || proof.trim().is_empty() || note.trim().is_empty() {
        return Err(Failure::new(
            "EVIDENCE_REQUIRED",
            "编辑前须填写本项原文来源、具体依据与备注。",
        ));
    }
    let expected_form = if reorder {
        let permission = if operation == "author_order" {
            "can_reorder_authors"
        } else {
            "can_reorder_institutions"
        };
        if result[permission] != true {
            return Err(Failure::new(
                "PAGE_UNSUPPORTED",
                "完整原编辑表单没有可用的顺序修改条件。",
            ));
        }
        let wanted = crate::metadata_order::reordered_form(&snapshot["form"], operation, order)?;
        let value = if operation == "author_order" {
            let candidates: Vec<_> = wanted["metadata"]["author"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| a["id"] == author["id"])
                .collect();
            if candidates.len() != 1 {
                return Err(Failure::new("IDENTITY_CONFLICT", "原作者身份未唯一定位。"));
            }
            let a = candidates[0];
            a["order"] == 1 || a["ownFirst"] == true || a["commonFirst"] == true
        } else {
            let first = order.as_array().unwrap()[0].as_u64().unwrap();
            let rows = result["institutions"]
                .as_array()
                .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "缺少页面计算的第一单位值。"))?;
            let original = snapshot["form"]["metadata"]["authorInstitution"]
                .as_array()
                .unwrap();
            if rows.len() != original.len()
                || rows
                    .iter()
                    .zip(original)
                    .enumerate()
                    .any(|(i, (row, original))| {
                        row["index"] != i
                            || row["order"] != original["order"]
                            || row["address"] != original["address"]
                    })
            {
                return Err(Failure::new(
                    "IDENTITY_CONFLICT",
                    "单位预览与完整原编辑表单不一致。",
                ));
            }
            let row = rows
                .iter()
                .find(|r| r["index"] == first)
                .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "目标单位不在页面完整原列表。"))?;
            match row["first_institution_value"].as_str() {
                Some("是") => true,
                Some("否") => false,
                _ => {
                    return Err(Failure::new(
                        "PAGE_UNSUPPORTED",
                        "页面没有明确计算该单位的交大归属。",
                    ))
                }
            }
        };
        if value != (original_sa == "是") {
            return Err(Failure::new(
                "REVIEW_REQUIRED",
                "新顺序与原文核实的 SA 值不一致；其他角色标记保持原值，请重新核对。",
            ));
        }
        wanted
    } else {
        Value::Null
    };
    let evidence_id = uuid::Uuid::new_v4().to_string();
    let mut payload = json!({"sa_id":task.id,"item_id":ids[0],"staff_id":task.record.staff_id,"scholar":identity["scholar"],"names":prepared["names"],
        "expected_row":live["row"],"author_index":index,"author_id":author["id"],"fullname":author["fullname"],"key":key,"value":original_sa == "是",
        "expected_snapshot":snapshot,"confirmed":true,"evidence_id":evidence_id,"note":note.trim(),"source":source.trim(),"proof":proof.trim(),"previous_stage":task.stage});
    if reorder {
        payload["operation"] = operation.into();
        payload["order"] = order.clone();
        payload["expected_form"] = expected_form;
    }
    task.evidence.push(Evidence {
        id: evidence_id,
        kind: "human_review".into(),
        source: source.trim().into(),
        text: proof.trim().into(),
        created: now(),
    });
    Ok(payload)
}
pub fn assert_result(payload: &Value, result: &Value) -> Result<()> {
    for key in ["item_id", "staff_id", "author_id", "key", "value"] {
        if result[key] != payload[key] {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "角色修改回读的目标或值不一致。",
            ));
        }
    }
    let index = payload["author_index"]
        .as_u64()
        .ok_or_else(|| Failure::new("REMOTE_RESULT_UNKNOWN", "缺少本次作者位置。"))?
        as usize;
    let reorder = matches!(
        payload["operation"].as_str(),
        Some("author_order" | "institution_order")
    );
    let field = if reorder {
        ""
    } else if payload["key"] == "corresponding_author" {
        "correspondent"
    } else if payload["key"] == "first_author" {
        "commonFirst"
    } else {
        return Err(Failure::new("REMOTE_RESULT_UNKNOWN", "未知角色控件。"));
    };
    let mut wanted = payload["expected_snapshot"]["form"].clone();
    let author = wanted["metadata"]["author"]
        .as_array_mut()
        .and_then(|a| a.get_mut(index))
        .ok_or_else(|| Failure::new("REMOTE_RESULT_UNKNOWN", "原始作者快照不存在。"))?;
    if !reorder {
        author[field] = payload["value"].clone();
    }
    if reorder {
        wanted = crate::metadata_order::reordered_form(
            &wanted,
            payload["operation"].as_str().unwrap(),
            &payload["order"],
        )?;
        if wanted != payload["expected_form"] || result["operation"] != payload["operation"] {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "回读顺序计划或完整预期表单不一致。",
            ));
        }
    }
    let content = |form: &Value| {
        let mut metadata = form["metadata"].clone();
        if let Some(authors) = metadata["author"].as_array_mut() {
            for author in authors {
                if let Some(numbers) = author["institutionOrderNums"].as_array() {
                    if numbers.iter().all(Value::is_string) {
                        author["institutionOrderNums"] = numbers
                            .iter()
                            .map(|v| v.as_str().unwrap())
                            .collect::<Vec<_>>()
                            .join(",")
                            .into();
                    }
                }
            }
        }
        json!({"id":form["id"],"modelId":form["modelId"],"modelName":form["modelName"],"datasetIds":form["datasetIds"],"dataSources":form["dataSources"],"fullTexts":form["fullTexts"],"metadata":metadata})
    };
    if result["verified"] != true
        || content(&result["before"]) != content(&payload["expected_snapshot"]["form"])
        || content(&result["after"]) != content(&wanted)
    {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "角色以外的出版信息、作者、单位或附件变化，需核验实际结果。",
        ));
    }
    Ok(())
}
