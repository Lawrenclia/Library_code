//! Explicit human observations of database searches; never paper metadata or absence proof.
use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub channel: String,
    pub source_url: String,
    pub scope: String,
    pub query: String,
    pub field: String,
    pub observed_at: u64,
    pub outcome: String,
    pub total: Option<u64>,
    pub explanation: String,
}

fn validated(mut value: Observation) -> Result<Observation> {
    let invalid = |message: &str| Failure::new("SEARCH_SCOPE_INVALID", message);
    for text in [
        &mut value.channel,
        &mut value.source_url,
        &mut value.scope,
        &mut value.query,
        &mut value.field,
        &mut value.outcome,
        &mut value.explanation,
    ] {
        *text = text.trim().to_owned();
    }
    if value.channel != "library"
        && !catalog::channels()
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == value.channel)
    {
        return Err(invalid("请选择已登记的来源渠道。"));
    }
    let url =
        url::Url::parse(&value.source_url).map_err(|_| invalid("填写实际结果页的完整网址。"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid("实际结果页须为不含账号密码的 HTTP/HTTPS 网址。"));
    }
    if value.scope.is_empty()
        || value.scope.chars().count() > 1000
        || value.query.is_empty()
        || value.query.chars().count() > 2000
        || value.explanation.is_empty()
        || value.explanation.chars().count() > 4000
        || value.source_url.len() > 8000
        || !["title", "doi", "wos", "keywords"].contains(&value.field.as_str())
        || value.observed_at == 0
        || value.observed_at > now().saturating_add(60_000)
    {
        return Err(invalid(
            "填写实际检索范围、查询字段、查询词、时间及结果说明；不能记录未来的查询。",
        ));
    }
    match value.outcome.as_str() {
        "zero_results" if value.total == Some(0) => {}
        "unresolved_candidates" if value.total.is_some_and(|n| n > 0) => {}
        "blocked" if value.total.is_none() => {}
        _ => {
            return Err(invalid(
                "零结果须为 0 条，候选待核对须有实际条数，访问受阻不能填写结果条数。",
            ))
        }
    }
    Ok(value)
}

pub fn current(task: &Task) -> Vec<(String, Observation)> {
    task.evidence
        .iter()
        .filter(|e| e.kind == "search_scope")
        .filter_map(|e| {
            let receipt: Value = serde_json::from_str(&e.text).ok()?;
            if receipt["schema"] != "source_search_scope_v1"
                || receipt["reported_by"] != "human"
                || receipt["task_id"] != task.id
                || receipt["input_hash"] != task.input_hash
                || receipt["record_fingerprint"] != task.record.fingerprint()
            {
                return None;
            }
            let observation: Observation =
                serde_json::from_value(receipt["observation"].clone()).ok()?;
            let observation = validated(observation).ok()?;
            if e.source != observation.source_url {
                return None;
            }
            Some((e.id.clone(), observation))
        })
        .collect()
}

pub fn assert_not_found(task: &Task) -> Result<()> {
    if task.artifact.as_ref().is_some_and(|a| a.identity_confirmed) {
        return Err(Failure::new(
            "REVIEW_REQUIRED",
            "已有身份确认的来源文献，不能改判为未查询到；先核对原来源和本库分支。",
        ));
    }
    if let Some(snapshot) = &task.sa_snapshot {
        if snapshot["row"]["saLzkId"] != task.id
            || snapshot["row"]["gh"] != task.record.staff_id
            || norm(snapshot["row"]["titleValue"].as_str().unwrap_or(""))
                != norm(&task.record.title)
            || snapshot["row"]["markStatus"] != "待处理"
            || !matched_ids(&snapshot["row"])?.is_empty()
        {
            return Err(Failure::new(
                "REVIEW_REQUIRED",
                "已读取的 SA 目标或状态变化，或存在匹配条目；先核对实时匹配，不能改判为未查询到。",
            ));
        }
    }
    let observations = current(task);
    // Keep every historical record. Only the latest observation for the same
    // exact query and scope participates in this local conclusion.
    let mut latest: std::collections::BTreeMap<_, (String, Observation)> =
        std::collections::BTreeMap::new();
    for (id, value) in &observations {
        let key = (
            &value.channel,
            &value.source_url,
            &value.scope,
            &value.field,
            &value.query,
        );
        let replace = latest
            .get(&key)
            .map_or(true, |(_, old)| old.observed_at <= value.observed_at);
        if replace {
            latest.insert(key, (id.clone(), value.clone()));
        }
    }
    if !latest.values().any(|(_, o)| o.outcome == "zero_results")
        || latest
            .values()
            .any(|(_, o)| o.outcome == "unresolved_candidates")
    {
        return Err(Failure::new(
            "EVIDENCE_REQUIRED",
            "先记录实际零结果的检索范围；候选待核对不能判为未查询到，访问受阻也不能作为零结果。",
        ));
    }
    Ok(())
}

impl Store {
    pub fn record_search_scope(
        &self,
        id: &str,
        revision: i64,
        input_hash: &str,
        observation: Observation,
    ) -> Result<Evidence> {
        let mut task = self.task(id)?;
        if task.revision != revision
            || task.input_hash != input_hash
            || self.pending_input(id)?.is_some()
            || !self.unresolved(id)?.is_empty()
            || task.running
            || task.record.done
            || task.record.skipped
            || matches!(task.stage, Stage::Unknown | Stage::Completed)
        {
            return Err(Failure::new(
                "TASK_CHANGED",
                "任务、名单或执行状态已变化，请刷新后记录实际检索。",
            ));
        }
        let observation = validated(observation)?;
        let receipt = json!({"schema":"source_search_scope_v1","reported_by":"human",
            "task_id":task.id,"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),
            "observation":observation});
        if let Some(existing) = task.evidence.iter().find(|e| {
            e.kind == "search_scope"
                && serde_json::from_str::<Value>(&e.text).ok().as_ref() == Some(&receipt)
        }) {
            return Ok(existing.clone());
        }
        let evidence = Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "search_scope".into(),
            source: observation.source_url,
            text: receipt.to_string(),
            created: now(),
        };
        task.evidence.push(evidence.clone());
        // A new unresolved candidate invalidates a prior local NotFound review.
        // Remote completion never follows this local observation.
        if task.route == Route::NotFound && assert_not_found(&task).is_err() {
            task.review = None;
            task.route = Route::ZeroReview;
            task.stage = Stage::AwaitingReview;
            task.last_error = Some(Failure::new("EVIDENCE_REQUIRED", "最新实际检索尚有待核对候选或没有明确零结果；原本地未查询到结论已撤销，请重新核验。"));
        }
        self.save(&mut task, "search_scope_recorded")?;
        Ok(evidence)
    }
}

pub(crate) fn export_report(book: &mut rust_xlsxwriter::Workbook, tasks: &[Task]) -> Result<()> {
    if !tasks
        .iter()
        .any(|t| t.evidence.iter().any(|e| e.kind == "search_scope"))
    {
        return Ok(());
    }
    let sheet = book.add_worksheet();
    sheet.set_name("实际检索范围").map_err(Failure::storage)?;
    for (col, label) in [
        "SA ID",
        "当前原表行号",
        "当前负责人",
        "来源记录 ID",
        "来源渠道",
        "实际结果页",
        "检索范围",
        "查询字段",
        "查询词",
        "实际查询时间（毫秒）",
        "结果类别",
        "结果条数",
        "结果说明",
        "对应当前任务且格式有效",
        "记录方式",
        "片段序号",
        "完整原记录",
    ]
    .iter()
    .enumerate()
    {
        sheet
            .write_string(0, col as u16, *label)
            .map_err(Failure::storage)?;
    }
    let mut row = 1;
    for task in tasks {
        let valid = current(task);
        for e in task.evidence.iter().filter(|e| e.kind == "search_scope") {
            let receipt: Value = serde_json::from_str(&e.text).unwrap_or(Value::Null);
            let o = &receipt["observation"];
            let text = |key: &str| o[key].as_str().unwrap_or("").to_owned();
            let outcome = match o["outcome"].as_str() {
                Some("zero_results") => "明确零结果",
                Some("unresolved_candidates") => "候选待核对",
                Some("blocked") => "访问或检索受阻",
                _ => "原记录类别不完整",
            };
            let values = vec![
                task.id.clone(),
                task.record.row.to_string(),
                task.record.owner.clone(),
                e.id.clone(),
                text("channel"),
                text("source_url"),
                text("scope"),
                text("field"),
                text("query"),
                o["observed_at"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                outcome.into(),
                o["total"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                text("explanation"),
                if valid.iter().any(|(id, _)| id == &e.id) {
                    "是"
                } else {
                    "否：历史版本或数据不完整"
                }
                .into(),
                "人工记录，未现场验证；不能证明论文不存在".into(),
                String::new(),
                e.text.clone(),
            ];
            let chunks: Vec<Vec<String>> = values
                .iter()
                .map(|v| {
                    let chars: Vec<_> = v.chars().collect();
                    if chars.is_empty() {
                        vec![String::new()]
                    } else {
                        chars.chunks(15000).map(|c| c.iter().collect()).collect()
                    }
                })
                .collect();
            for part in 0..chunks.iter().map(Vec::len).max().unwrap_or(1) {
                for (col, cells) in chunks.iter().enumerate() {
                    let cell = if col == 15 {
                        (part + 1).to_string()
                    } else if col <= 3 {
                        values[col].clone()
                    } else {
                        cells.get(part).cloned().unwrap_or_default()
                    };
                    sheet
                        .write_string(row, col as u16, cell)
                        .map_err(Failure::storage)?;
                }
                row += 1;
            }
        }
    }
    sheet.set_freeze_panes(1, 0).map_err(Failure::storage)?;
    for col in [5, 6, 8, 12, 16] {
        sheet.set_column_width(col, 55.).map_err(Failure::storage)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> (tempfile::TempDir, Store, Task) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        let record: Record =
            serde_json::from_value(json!({"row":2,"owner":"测试","sa_id":"scope-1",
            "title":"Paper","doi":"","wos":"","staff_id":"001","matches":0,"item_ids":"",
            "mark":"待处理","reason":"","skipped":false,"done":false,"source":"list.xlsx"}))
            .unwrap();
        store.import(vec![record], "input".into()).unwrap();
        let task = store.task("scope-1").unwrap();
        (dir, store, task)
    }

    fn observation(outcome: &str) -> Observation {
        Observation {
            channel: "wos_txt".into(),
            source_url: "https://webofscience.clarivate.cn/wos/woscc/basic-search".into(),
            scope: "核心合集；全部年份".into(),
            query: "Paper".into(),
            field: "title".into(),
            observed_at: now(),
            outcome: outcome.into(),
            total: match outcome {
                "zero_results" => Some(0),
                "unresolved_candidates" => Some(2),
                _ => None,
            },
            explanation: "人工记录页面实际显示的查询结果".into(),
        }
    }

    fn save(store: &Store, value: Observation) -> Evidence {
        let task = store.task("scope-1").unwrap();
        store
            .record_search_scope(&task.id, task.revision, &task.input_hash, value)
            .unwrap()
    }

    #[test]
    fn search_scope_survives_restart_and_exact_duplicate_is_not_another_event() {
        let (dir, store, t) = workspace();
        let value = observation("zero_results");
        let first = save(&store, value.clone());
        let revision = store.task(&t.id).unwrap().revision;
        assert_eq!(save(&store, value).id, first.id);
        assert_eq!(store.task(&t.id).unwrap().revision, revision);
        let reopened = Store::new(dir.path()).unwrap();
        reopened.recover().unwrap();
        let restored = reopened.task(&t.id).unwrap();
        assert_not_found(&restored).unwrap();
        assert_eq!(restored.stage, t.stage);
        assert!(!restored.record.done);
        assert!(reopened.unresolved(&t.id).unwrap().is_empty());
        let mut changed = restored.clone();
        changed.input_hash = "new-input".into();
        assert!(assert_not_found(&changed).is_err());
        changed = restored;
        changed.record.staff_id = "002".into();
        assert!(assert_not_found(&changed).is_err());
        let mut live_changed = reopened.task(&t.id).unwrap();
        live_changed.sa_snapshot = Some(
            json!({"row":{"saLzkId":t.id,"gh":"001","titleValue":"Paper",
            "markStatus":"待处理","matchCount":1,"itemId":"123"}}),
        );
        assert!(assert_not_found(&live_changed).is_err());
    }

    #[test]
    fn blocked_or_ambiguous_search_is_never_zero_results_and_new_candidate_revokes_review() {
        let (_dir, store, _) = workspace();
        save(&store, observation("blocked"));
        assert!(assert_not_found(&store.task("scope-1").unwrap()).is_err());
        save(&store, observation("zero_results"));
        let mut task = store.task("scope-1").unwrap();
        assert_not_found(&task).unwrap();
        task.evidence.push(Evidence {
            id: "conclusion".into(),
            kind: "search_conclusion".into(),
            source: "人工记录".into(),
            text: "实际范围未查询到该文献".into(),
            created: now(),
        });
        let review = Review {
            route: Route::NotFound,
            evidence_id: "conclusion".into(),
            library_checked: false,
            platform_id: "".into(),
            affiliation_confirmed: false,
            identity_confirmed: false,
            issues_resolved: false,
            note: "未查询到该文献".into(),
        };
        task.validate_review(&review).unwrap();
        task.route = Route::NotFound;
        task.review = Some(review);
        task.stage = Stage::AwaitingReview;
        store.save(&mut task, "local_not_found_review").unwrap();
        assert!(task.assert_complete().is_err());
        save(&store, observation("unresolved_candidates"));
        let after = store.task("scope-1").unwrap();
        assert!(after.review.is_none());
        assert_eq!(after.route, Route::ZeroReview);
        assert_eq!(after.stage, Stage::AwaitingReview);
        assert!(!after.record.done);
        assert!(after.evidence.iter().any(|e| e.id == "conclusion"));
        assert!(assert_not_found(&after).is_err());
    }

    #[test]
    fn malformed_scope_and_stale_or_pending_task_do_not_write_observation() {
        let (_dir, store, task) = workspace();
        for mutation in 0..4 {
            let mut value = observation("zero_results");
            match mutation {
                0 => value.total = None,
                1 => value.source_url = "file:///C:/data.txt".into(),
                2 => value.observed_at = now() + 120_000,
                _ => value.scope.clear(),
            }
            assert!(store
                .record_search_scope(&task.id, task.revision, &task.input_hash, value)
                .is_err());
        }
        save(&store, observation("zero_results"));
        assert!(store
            .record_search_scope(
                &task.id,
                task.revision,
                &task.input_hash,
                observation("zero_results")
            )
            .is_err());
        let mut current = store.task(&task.id).unwrap();
        let attempt = store
            .begin_attempt_with_payload(&mut current, "complete", json!({"note":"original"}))
            .unwrap();
        assert!(store
            .record_search_scope(
                &current.id,
                current.revision,
                &current.input_hash,
                observation("zero_results")
            )
            .is_err());
        assert_eq!(store.unresolved(&task.id).unwrap()[0]["id"], attempt);
        assert_eq!(store.task(&task.id).unwrap().evidence.len(), 1);
    }
}
