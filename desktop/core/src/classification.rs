//! Classification inputs and complete provider responses; suggestions never advance a task.
use crate::*;
use serde_json::{json, Value};
use std::path::Path;

pub fn sources(root: &Path, task: &Task) -> Result<Vec<Evidence>> {
    let mut sources = source_files::verified_evidence(root, task)?;
    let input=json!({"schema":"roster_input_v1","input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),"record":{"title":task.record.title,"doi":task.record.doi,"wos":task.record.wos}}).to_string();
    sources.push(Evidence {
        id: format!("roster:{}", hash(input.as_bytes())),
        kind: "roster_input".into(),
        source: format!("名单输入 / SA {} / 第 {} 行", task.id, task.record.row),
        text: input,
        created: 0,
    });
    Ok(sources)
}
pub fn parse_response(raw: &[u8], sources: &[Evidence], template: Option<&Value>) -> Result<Value> {
    let invalid = |message: &str| Failure::new("AI_RESULT_INVALID", message);
    if raw.len() > 1024 * 1024 {
        return Err(invalid("AI 返回过大，未采纳。"));
    }
    let response: Value = serde_json::from_slice(raw).map_err(|_| invalid("AI 返回不是 JSON。"))?;
    let choices = response["choices"]
        .as_array()
        .filter(|c| c.len() == 1)
        .ok_or_else(|| invalid("AI 必须返回唯一完整分类结果。"))?;
    let choice = &choices[0];
    if choice["finish_reason"] != "stop"
        || choice["message"]["refusal"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        || choice["message"]["tool_calls"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
    {
        return Err(invalid(
            "AI 输出未正常完成或未返回分类，未采纳；请检查模型设置或输出长度。",
        ));
    }
    let content = choice["message"]["content"]
        .as_str()
        .ok_or_else(|| invalid("AI 没有返回分类内容。"))?
        .trim();
    let content = if let Some(body) = content
        .strip_prefix("```json")
        .or_else(|| content.strip_prefix("```"))
    {
        body.strip_suffix("```")
            .ok_or_else(|| invalid("AI JSON 围栏未结束。"))?
            .trim()
    } else {
        content
    };
    let result: Value =
        serde_json::from_str(content).map_err(|_| invalid("分类内容不是结构化 JSON。"))?;
    catalog::validate_ai(&result, sources, template)?;
    Ok(result)
}

pub fn audit(
    task: &Task,
    result: &Value,
    sources: &[Evidence],
    model: &str,
    template: Option<&Value>,
) -> Evidence {
    Evidence{id:uuid::Uuid::new_v4().to_string(),kind:"ai_classification".into(),source:format!("API 模型：{model}"),created:now(),text:json!({"schema":"ai_classification_v2","sa_id":task.id,"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),"result":result,"sources":sources,"template":template,"model":model,"review_required":true,"platform_verified":false}).to_string()}
}

pub fn bind_result(task: &Task, mut result: Value, template: Option<&Value>) -> Value {
    result["schema_version"] = json!(2);
    result["review_required"] = json!(true);
    result["platform_verified"] = json!(false);
    result["input_hash"] = json!(task.input_hash);
    result["record_fingerprint"] = json!(task.record.fingerprint());
    result["template_id"] = template.and_then(|t| t["id"].as_str()).unwrap_or("").into();
    result["template_hash"] = template
        .map(|t| t["fingerprint"].clone())
        .unwrap_or(Value::Null);
    result
}
pub fn validate_saved(
    task: &Task,
    result: &Value,
    sources: &[Evidence],
    template: &Value,
) -> Result<()> {
    if result["schema_version"] != 2
        || result["input_hash"] != task.input_hash
        || result["record_fingerprint"] != task.record.fingerprint()
        || result["template_id"] != template["id"]
        || result["template_hash"] != template["fingerprint"]
    {
        return Err(Failure::new(
            "AI_RESULT_INVALID",
            "分类建议的名单或模板版本不同/未记录，请重新调用 AI。",
        ));
    }
    catalog::validate_ai(result, sources, Some(template))
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{open_workbook_auto, Reader};
    fn task() -> Task {
        Task::new(serde_json::from_value(json!({"row":2,"owner":"private-owner","sa_id":"sa-ai","title":"Paper","doi":"10.1234/test","wos":"","staff_id":"private-staff","matches":0,"item_ids":"","mark":"","reason":"","skipped":false,"done":false,"source":""})).unwrap(),"input".into())
    }
    fn result(source: &str) -> Value {
        json!({"type":"期刊论文","channel":"general","confidence":"低","reason":"题名线索，仍需核实类型","channel_reason":"候选模板入口，仍需确认实际模板和来源","evidence_ids":[source],"missing":["出版来源与署名"],"fields":{}})
    }
    fn envelope(v: &Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"choices":[{"index":0,"finish_reason":"stop","message":{"content":v.to_string()}}]})).unwrap()
    }
    #[test]
    fn roster_only_uses_current_identity_and_allows_unknown_without_claiming_database() {
        let dir = tempfile::tempdir().unwrap();
        let t = task();
        let source = sources(dir.path(), &t).unwrap();
        assert_eq!(source.len(), 1);
        assert!(
            !source[0].text.contains("private-owner") && !source[0].text.contains("private-staff")
        );
        let mut v = result(&source[0].id);
        v["type"] = Value::Null;
        v["channel"] = Value::Null;
        assert!(parse_response(&envelope(&v), &source, None).is_ok());
        let mut t2 = t.clone();
        t2.input_hash = "different".into();
        assert_ne!(source[0].id, sources(dir.path(), &t2).unwrap()[0].id);
        assert!(parse_response(&envelope(&v), &sources(dir.path(), &t2).unwrap(), None).is_err());
    }
    #[test]
    fn roster_cannot_claim_high_confidence_or_invent_author_fields() {
        let dir = tempfile::tempdir().unwrap();
        let source = sources(dir.path(), &task()).unwrap();
        let schema = json!({"columns":["Title","Author"]});
        let mut v = result(&source[0].id);
        v["confidence"] = json!("高");
        assert!(parse_response(&envelope(&v), &source, None).is_err());
        v["confidence"] = json!("低");
        v["fields"] = json!({"Author":{"value":"Invented Author","evidence_ids":[source[0].id]}});
        assert!(parse_response(&envelope(&v), &source, Some(&schema)).is_err());
        v["fields"] = json!({"Title":{"value":"Paper","evidence_ids":[source[0].id]}});
        assert!(parse_response(&envelope(&v), &source, Some(&schema)).is_ok());
        v["fields"] = json!({"Author":{"value":"Paper","evidence_ids":[source[0].id]}});
        assert!(parse_response(&envelope(&v), &source, Some(&schema)).is_err());
    }
    #[test]
    fn malformed_confidence_types_channels_missing_and_citations_refuse_results() {
        let dir = tempfile::tempdir().unwrap();
        let source = sources(dir.path(), &task()).unwrap();
        for (key, bad) in [
            ("confidence", json!(0.9)),
            ("type", json!("made-up")),
            ("channel", json!("made-up")),
            ("missing", json!([])),
            ("reason", json!("")),
            ("channel_reason", json!(null)),
            ("evidence_ids", json!([source[0].id, source[0].id])),
        ] {
            let mut v = result(&source[0].id);
            v[key] = bad;
            assert!(
                parse_response(&envelope(&v), &source, None).is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn valid_json_with_truncated_or_filtered_completion_never_becomes_result() {
        let dir = tempfile::tempdir().unwrap();
        let source = sources(dir.path(), &task()).unwrap();
        let v = result(&source[0].id);
        for finish in ["length", "content_filter", "tool_calls", ""] {
            let mut e: Value = serde_json::from_slice(&envelope(&v)).unwrap();
            e["choices"][0]["finish_reason"] = json!(finish);
            assert!(parse_response(&serde_json::to_vec(&e).unwrap(), &source, None).is_err());
        }
        let mut e: Value = serde_json::from_slice(&envelope(&v)).unwrap();
        let choice = e["choices"][0].clone();
        e["choices"].as_array_mut().unwrap().push(choice);
        assert!(parse_response(&serde_json::to_vec(&e).unwrap(), &source, None).is_err());
        let mut e: Value = serde_json::from_slice(&envelope(&v)).unwrap();
        e["choices"][0]["message"]["refusal"] = json!("denied");
        assert!(parse_response(&serde_json::to_vec(&e).unwrap(), &source, None).is_err());
    }
    #[test]
    fn old_versions_and_changed_templates_cannot_fill_saved_classification() {
        let dir = tempfile::tempdir().unwrap();
        let t = task();
        let source = sources(dir.path(), &t).unwrap();
        let schema = json!({"id":"template","fingerprint":"old-template","columns":[]});
        let v = bind_result(&t, result(&source[0].id), Some(&schema));
        assert!(validate_saved(&t, &v, &source, &schema).is_ok());
        let mut changed = schema.clone();
        changed["fingerprint"] = json!("new-template");
        assert!(validate_saved(&t, &v, &source, &changed).is_err());
        assert!(validate_saved(&t, &result(&source[0].id), &source, &schema).is_err());
        let mut changed = t.clone();
        changed.record.doi = "10.1234/other".into();
        assert!(validate_saved(&changed, &v, &source, &schema).is_err());
    }
    #[test]
    fn complete_long_values_audit_survive_restart_and_excel_without_advancing_stage() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::new(dir.path()).unwrap();
        let mut t = task();
        store
            .import(vec![t.record.clone()], t.input_hash.clone())
            .unwrap();
        t = store.task(&t.id).unwrap();
        let long = "原始摘要🙂；字段引用完整。".repeat(5000);
        t.evidence.push(Evidence {
            id: "actual-source".into(),
            kind: "human_review".into(),
            source: "https://example.test/paper".into(),
            text: long.clone(),
            created: 1,
        });
        let source = sources(dir.path(), &t).unwrap();
        let schema = json!({"id":"template","fingerprint":"template-hash","columns":["Abstract"]});
        let mut v = result("actual-source");
        v["confidence"] = json!("中");
        v["fields"] = json!({"Abstract":{"value":long,"evidence_ids":["actual-source"]}});
        let v = bind_result(
            &t,
            parse_response(&envelope(&v), &source, Some(&schema)).unwrap(),
            Some(&schema),
        );
        t.evidence
            .push(audit(&t, &v, &source, "test-model", Some(&schema)));
        t.classification = Some(v.clone());
        store.save(&mut t, "ai_classified").unwrap();
        drop(store);
        let store = crate::store::Store::new(dir.path()).unwrap();
        let t = store.task("sa-ai").unwrap();
        assert_eq!(t.stage, Stage::Pending);
        assert!(t.review.is_none() && t.artifact.is_none());
        let next = sources(dir.path(), &t).unwrap();
        assert_eq!(next.len(), 2);
        assert!(!next.iter().any(|e| e.kind == "ai_classification"));
        let raw: Value = serde_json::from_str(
            &t.evidence
                .iter()
                .find(|e| e.kind == "ai_classification")
                .unwrap()
                .text,
        )
        .unwrap();
        assert_eq!(raw["result"], v);
        assert_eq!(raw["platform_verified"], false);
        let report = dir.path().join("ai-report.xlsx");
        files::export_report(&[t], &report).unwrap();
        let mut book = open_workbook_auto(report).unwrap();
        let fields = book.worksheet_range("AI 字段来源").unwrap();
        let value = fields
            .rows()
            .skip(1)
            .map(|r| r[3].to_string())
            .collect::<String>();
        assert_eq!(value, long);
        assert_eq!(
            fields.rows().nth(1).unwrap()[6].to_string(),
            hash(long.as_bytes())
        );
        assert!(fields.height() > 2);
        let summary = book.worksheet_range("任务与来源").unwrap();
        assert_eq!(summary.rows().nth(1).unwrap()[17].to_string(), "中");
        assert_eq!(summary.rows().nth(1).unwrap()[20].to_string(), "建议待复核");
    }
}
