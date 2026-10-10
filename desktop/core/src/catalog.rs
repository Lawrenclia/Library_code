use crate::*;
use serde_json::{json, Value};
pub fn types() -> Vec<&'static str> {
    "期刊论文 会议论文 科技论文 著作章节 基金 学位论文 研究报告 著作 项目 专利 科技报告 报纸 图书 科技成果 信息服务系统 国家级规划教材 科研获奖 精品课程 学习讨论集 演讲报告 教学成果奖 内部工作文件 共享资料 会议录 岗位知识 科研装置 产品 软件著作权 软件 会议 课件 标准 期刊 影音 图像 数据集 获奖成果 其他".split_whitespace().collect()
}
pub fn channels() -> Value {
    let mut channels = json!([
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
    ]);
    for channel in channels.as_array_mut().unwrap() {
        let native = channel["id"] == "wos_txt";
        let template = channel["id"] == "general" || channel["id"] == "other";
        channel["capabilities"] = json!({
            "search":if native {"implemented"} else {"unimplemented"},
            "local_source":"implemented",
            "export":if native {"implemented"} else {"unimplemented"},
            "parse":if native {"implemented"} else {"unimplemented"},
            "submit":if native {"implemented"} else {"unimplemented"},
            "template":if template {"implemented"} else {"registered_only"}
        });
        channel["live_verified"] = false.into();
    }
    channels
}
pub fn validate_ai(value: &Value, evidence: &[Evidence], template: Option<&Value>) -> Result<()> {
    let error = |message: &str| Failure::new("AI_RESULT_INVALID", message);
    if !value.is_object()
        || !value
            .get("type")
            .is_some_and(|t| t.is_null() || t.as_str().is_some_and(|s| types().contains(&s)))
    {
        return Err(error("成果类型必须是已登记类型或 null（待判定）。"));
    }
    if !value["confidence"]
        .as_str()
        .is_some_and(|s| ["高", "中", "低"].contains(&s))
    {
        return Err(error("分类缺少高/中/低置信度，请重新调用 AI。"));
    }
    for key in ["reason", "channel_reason"] {
        if !value[key]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= 600)
        {
            return Err(error("分类理由和渠道建议条件不能为空或超过 600 字。"));
        }
    }
    let valid_ids = |ids: &Value| {
        ids.as_array()
            .map(|ids| {
                !ids.is_empty()
                    && ids
                        .iter()
                        .map(Value::as_str)
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == ids.len()
                    && ids
                        .iter()
                        .all(|id| evidence.iter().any(|e| Some(e.id.as_str()) == id.as_str()))
            })
            .unwrap_or(false)
    };
    if !valid_ids(&value["evidence_ids"]) {
        return Err(Failure::new("AI_RESULT_INVALID", "AI 缺少有效来源引用。"));
    }
    if !value.get("channel").is_some_and(|v| v.is_null())
        && !channels()
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == value["channel"])
    {
        return Err(Failure::new("AI_RESULT_INVALID", "AI 推荐了未知导入渠道。"));
    }
    let roster_only = value["evidence_ids"].as_array().unwrap().iter().all(|id| {
        evidence
            .iter()
            .any(|e| e.kind == "roster_input" && Some(e.id.as_str()) == id.as_str())
    });
    if (value["type"].is_null() || value["channel"].is_null() || roster_only)
        && value["confidence"] != "低"
    {
        return Err(error("仅名单依据或类型/渠道待判定时，置信度必须为低。"));
    }
    if !value["missing"]
        .as_array()
        .map(|a| {
            a.iter()
                .all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
        })
        .unwrap_or(false)
    {
        return Err(Failure::new("AI_RESULT_INVALID", "缺项必须为文本数组。"));
    }
    if (roster_only || value["type"].is_null() || value["channel"].is_null())
        && value["missing"].as_array().unwrap().is_empty()
    {
        return Err(error("依据不足时必须列出仍缺少的来源或信息。"));
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
        let ids = field["evidence_ids"].as_array().unwrap();
        let only_roster = ids.iter().all(|id| {
            evidence
                .iter()
                .any(|e| e.kind == "roster_input" && Some(e.id.as_str()) == id.as_str())
        });
        if only_roster {
            let heading = template
                .and_then(|t| {
                    t["columns"].as_array().and_then(|c| {
                        c.iter()
                            .position(|v| v == name)
                            .map(|i| t["headers"][i].as_str().unwrap_or(name))
                    })
                })
                .unwrap_or(name);
            let heading = heading
                .split(['（', '(', '\n'])
                .next()
                .unwrap_or(heading)
                .trim()
                .trim_matches('*')
                .trim()
                .to_lowercase();
            let roster_key = match heading.as_str() {
                "title" | "题名" | "标题" | "文献题名" | "论文题名" => Some("title"),
                "doi" => Some("doi"),
                "wos_id" | "wos id" | "wosid" | "wos编号" | "入藏号" | "accession number" => {
                    Some("wos")
                }
                _ => None,
            };
            let supported = evidence
                .iter()
                .filter(|e| ids.iter().any(|id| Some(e.id.as_str()) == id.as_str()))
                .any(|e| {
                    serde_json::from_str::<Value>(&e.text)
                        .ok()
                        .is_some_and(|v| {
                            roster_key.is_some_and(|key| {
                                v["record"][key].as_str().is_some_and(|s| {
                                    !s.is_empty() && Some(s) == field["value"].as_str()
                                })
                            })
                        })
                });
            if !supported {
                return Err(error(
                    "仅名单来源只能填写原题名、DOI 或 WOS ID，不能补写其他信息。",
                ));
            }
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
        let mut value = json!({"type":"期刊论文","confidence":"中","reason":"来源支持","channel_reason":"建议，仍需确认实际模板","channel":"general","evidence_ids":["source"],"missing":[],"fields":{"题名":{"value":"paper","evidence_ids":["invented"]}}});
        assert!(validate_ai(&value, &evidence, Some(&template)).is_err());
        value["fields"]["题名"]["evidence_ids"] = json!(["source"]);
        assert!(validate_ai(&value, &evidence, Some(&template)).is_ok());
    }
}
