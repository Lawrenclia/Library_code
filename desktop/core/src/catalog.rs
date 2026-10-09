use crate::*;
use serde_json::{json, Value};
pub fn channels() -> Value {
    json!([
        {"id":"general","label":"数据导入","formats":["xlsx"],"automated":false},
        {"id":"wos_excel","label":"WOS 数据导入 (Excel)","formats":["xlsx"],"automated":false},
        {"id":"wos_txt","label":"WOS 数据导入 (Txt)","formats":["txt"],"automated":true},
        {"id":"cscd","label":"CSCD 数据导入 (Txt)","formats":["txt"],"automated":false},
        {"id":"cssci","label":"CSSCI 数据导入 (Txt)","formats":["txt"],"automated":false},
        {"id":"cnki","label":"CNKI 数据导入 (Excel/Txt)","formats":["xlsx","txt"],"automated":false},
        {"id":"wanfang","label":"万方数据导入","formats":["txt","xlsx"],"automated":false},
        {"id":"ei","label":"EI 数据导入 (Csv/Excel)","formats":["csv","xlsx"],"automated":false},
        {"id":"vip","label":"VIP 数据导入 (Excel)","formats":["xlsx"],"automated":false},
        {"id":"incopat","label":"IncoPat 数据导入 (Excel)","formats":["xlsx"],"automated":false},
        {"id":"other","label":"其他来源 / 平台模板","formats":["xlsx","txt","csv"],"automated":false}
    ])
}
pub fn validate_ai(value: &Value, evidence: &[Evidence], template: Option<&Value>) -> Result<()> {
    let valid_ids = |ids: &Value| {
        ids.as_array()
            .map(|ids| {
                !ids.is_empty()
                    && ids
                        .iter()
                        .all(|id| evidence.iter().any(|e| Some(e.id.as_str()) == id.as_str()))
            })
            .unwrap_or(false)
    };
    if !valid_ids(&value["evidence_ids"]) {
        return Err(Failure::new("AI_RESULT_INVALID", "AI 缺少有效来源引用。"));
    }
    if !channels()
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == value["channel"])
    {
        return Err(Failure::new("AI_RESULT_INVALID", "AI 推荐了未知导入渠道。"));
    }
    if !value["missing"]
        .as_array()
        .map(|a| a.iter().all(Value::is_string))
        .unwrap_or(false)
    {
        return Err(Failure::new("AI_RESULT_INVALID", "缺项必须为文本数组。"));
    }
    let fields = value["fields"]
        .as_object()
        .ok_or_else(|| Failure::new("AI_RESULT_INVALID", "字段不是对象。"))?;
    for (name, field) in fields {
        if !template
            .and_then(|t| t["columns"].as_array())
            .map(|c| c.iter().any(|v| v == name))
            .unwrap_or(false)
            || !field["value"].is_string()
            || !valid_ids(&field["evidence_ids"])
        {
            return Err(Failure::new(
                "AI_RESULT_INVALID",
                format!("字段 {name} 没有模板约束或具体来源。"),
            ));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsupported_field_citation() {
        let evidence = vec![Evidence {
            id: "source".into(),
            kind: "test".into(),
            source: "url".into(),
            text: "paper".into(),
            created: 0,
        }];
        let template = json!({"columns":["题名"]});
        let mut value = json!({"channel":"general","evidence_ids":["source"],"missing":[],"fields":{"题名":{"value":"paper","evidence_ids":["invented"]}}});
        assert!(validate_ai(&value, &evidence, Some(&template)).is_err());
        value["fields"]["题名"]["evidence_ids"] = json!(["source"]);
        assert!(validate_ai(&value, &evidence, Some(&template)).is_ok());
    }
}
