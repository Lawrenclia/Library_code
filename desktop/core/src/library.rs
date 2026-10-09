use crate::{norm, now, Evidence, Failure, Result, Review, Route, Stage, Task};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn target(task: &Task, title: &str) -> Result<Value> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 1000 || title.chars().any(char::is_control) {
        return Err(Failure::new("IDENTITY_CONFLICT", "填写完整、正确的题名。"));
    }
    let (doi, wos) = task
        .artifact
        .as_ref()
        .map(|a| (a.candidate.doi.as_str(), a.candidate.wos.as_str()))
        .unwrap_or((&task.record.doi, &task.record.wos));
    Ok(json!({"title":title,"doi":doi,"wos":wos}))
}

pub fn record(task: &mut Task, expected: &Value, result: &Value) -> Result<()> {
    validate(task, expected, result)?;
    let receipt = json!({"input_hash":task.input_hash,"result":result});
    task.evidence.push(Evidence {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "library_search".into(),
        source: result["source"].as_str().unwrap().into(),
        text: receipt.to_string(),
        created: now(),
    });
    Ok(())
}

fn validate(task: &Task, expected: &Value, result: &Value) -> Result<()> {
    let fail = || {
        Failure::new(
            "SEARCH_UNVERIFIED",
            "本库查询的任务、条件或分页证据不完整，请重新查询。",
        )
    };
    let source = result["source"].as_str().ok_or_else(fail)?;
    let source_url = url::Url::parse(source).map_err(|_| fail())?;
    if !(["http", "https"].contains(&source_url.scheme())
        && source_url.host_str() == Some("www.ir.lib.sjtu.edu.cn")
        && source_url.username().is_empty()
        && source_url.password().is_none()
        && source_url.port().is_none()
        && ["/advancedSearch", "/advancedSearch/"].contains(&source_url.path()))
        || result["verified"] != true
        || result["sa_id"] != task.id
        || result["target"] != *expected
        || result["institution_id"] != "1244586319225556993"
    {
        return Err(fail());
    }
    let queries = result["queries"].as_array().ok_or_else(fail)?;
    let expected_kinds: Vec<_> = ["title", "doi", "wos"]
        .into_iter()
        .filter(|k| !expected[k].as_str().unwrap_or("").is_empty())
        .collect();
    if queries.len() != expected_kinds.len() {
        return Err(fail());
    }
    let mut union = BTreeMap::new();
    for (query, kind) in queries.iter().zip(expected_kinds) {
        if query["kind"] != kind
            || query["value"] != expected[kind]
            || query["precise"] != (kind != "title")
            || query["field"].as_str().unwrap_or("").is_empty()
        {
            return Err(fail());
        }
        let total = query["total"]
            .as_u64()
            .filter(|n| *n <= 1000)
            .ok_or_else(fail)?;
        let pages = query["pages"].as_array().ok_or_else(fail)?;
        if pages.len() as u64 != total.div_ceil(10).max(1) {
            return Err(fail());
        }
        let mut ids = BTreeSet::new();
        for (i, page) in pages.iter().enumerate() {
            let rows = page["records"].as_array().ok_or_else(fail)?;
            if page["current"] != (i + 1) as u64
                || page["size"] != 10
                || page["total"] != total
                || rows.len() as u64 != total.saturating_sub(i as u64 * 10).min(10)
            {
                return Err(fail());
            }
            for row in rows {
                let id = row["id"]
                    .as_str()
                    .filter(|id| !id.is_empty() && !id.chars().any(char::is_whitespace))
                    .ok_or_else(fail)?;
                let titles = row["metadata"]["title"]
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(fail)?;
                if titles
                    .iter()
                    .any(|s| s.as_str().map(|s| s.trim().is_empty()).unwrap_or(true))
                    || !ids.insert(id.to_string())
                {
                    return Err(fail());
                }
                if let Some(previous) = union.insert(id.to_string(), row.clone()) {
                    if previous != *row {
                        return Err(fail());
                    }
                }
            }
        }
        if ids.len() as u64 != total {
            return Err(fail());
        }
    }
    let items = result["items"].as_array().ok_or_else(fail)?;
    let item_ids: BTreeSet<_> = items.iter().filter_map(|row| row["id"].as_str()).collect();
    if items.len() != union.len()
        || item_ids.len() != items.len()
        || items
            .iter()
            .any(|row| row["id"].as_str().and_then(|id| union.get(id)) != Some(row))
    {
        return Err(fail());
    }
    let checked = result["checked_at"].as_u64().ok_or_else(fail)?;
    if checked > now() + 5000 || now().saturating_sub(checked) > 24 * 60 * 60 * 1000 {
        return Err(fail());
    }
    Ok(())
}

pub fn latest(task: &Task) -> Result<Value> {
    let evidence = task
        .evidence
        .iter()
        .rev()
        .find(|e| e.kind == "library_search")
        .ok_or_else(|| {
            Failure::new(
                "SEARCH_REQUIRED",
                "先使用正确题名在内置机构库前端检索并保存实际结果。",
            )
        })?;
    let receipt: Value = serde_json::from_str(&evidence.text).map_err(|_| {
        Failure::new(
            "SEARCH_REQUIRED",
            "旧检索备注没有实际查询条件和分页证据，请重新查本库。",
        )
    })?;
    if receipt["input_hash"] != task.input_hash {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "查询对应旧名单，请重新查本库。",
        ));
    }
    let result = &receipt["result"];
    let title = result["target"]["title"].as_str().unwrap_or("");
    let expected = target(task, title)?;
    validate(task, &expected, result)?;
    if task
        .artifact
        .as_ref()
        .map(|a| norm(&a.candidate.title) != norm(title))
        .unwrap_or(false)
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "查询题名与已取得的真实文献不同，请用真实题名重新查库。",
        ));
    }
    Ok(result.clone())
}

pub fn review(task: &Task, review: &Review) -> Result<()> {
    if !matches!(review.route, Route::Missing | Route::CorrectedExisting) {
        return Ok(());
    }
    let result = latest(task)?;
    let items = result["items"].as_array().unwrap();
    let after_push =
        review.route == Route::Missing && matches!(task.stage, Stage::Pushed | Stage::Claimed);
    if review.route == Route::Missing
        && !after_push
        && (!items.is_empty() || !review.platform_id.trim().is_empty())
    {
        return Err(Failure::new(
            "REVIEW_REQUIRED",
            "本库查询存在候选条目或已有平台号，不能判为缺失；先逐条核对。",
        ));
    }
    if (review.route == Route::CorrectedExisting || after_push)
        && (!review.identity_confirmed
            || !review.affiliation_confirmed
            || !items.iter().any(|i| i["id"] == review.platform_id))
    {
        return Err(Failure::new(
            "REVIEW_REQUIRED",
            "选择查询返回的实际条目，并核实文献身份与交大归属。",
        ));
    }
    Ok(())
}

pub fn assert_absent(task: &Task) -> Result<()> {
    if !latest(task)?["items"].as_array().unwrap().is_empty() {
        return Err(Failure::new(
            "REVIEW_REQUIRED",
            "本库已有查询候选，先核对现有条目，不能上传。",
        ));
    }
    Ok(())
}
