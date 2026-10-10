use super::*;

impl Engine {
    pub(super) fn record_merge(
        task: &mut Task,
        payload: &Value,
        result: &Value,
        sa: &Value,
    ) -> Result<()> {
        merge::assert_result(task, payload, result, sa)?;
        let source = payload["source_id"].as_str().unwrap_or("");
        let target = payload["target_id"].as_str().unwrap_or("");
        if result["verified"] != true
            || result["source_id"] != source
            || result["target_id"] != target
            || result["master_after"]["id"] != target
            || source.is_empty()
            || target.is_empty()
            || task.merges.iter().any(|m| m.source_id == source)
        {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "合并回读目标或记录不一致，请先核验，不能重发。",
            ));
        }
        let evidence_id = uuid::Uuid::new_v4().to_string();
        task.evidence.push(Evidence {
            id: evidence_id.clone(),
            kind: "duplicate_merge_verified".into(),
            source: "机构库重复数据管理 · 回读".into(),
            text: json!({"payload":payload,"result":result,"sa_after":sa}).to_string(),
            created: now(),
        });
        task.merges.push(MergeRecord {
            group_id: payload["group_id"].as_str().unwrap_or("").into(),
            source_id: source.into(),
            target_id: target.into(),
            evidence_id,
            created: now(),
        });
        task.platform_id = target.into();
        Ok(())
    }
    pub(super) async fn handle_prepare_duplicate(
        &self,
        app: &AppHandle,
        id: &str,
        action: &str,
        mut t: Task,
        extra: Value,
    ) -> Result<Value> {
        if t.route != Route::Duplicate || matches!(t.stage, Stage::Unknown | Stage::Completed) {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "当前任务不能准备新的合并。",
            ));
        }
        let sa = self.read_sa(app, &mut t).await?;
        let ids = matched_ids(&sa["row"])?;
        if ids.len() < 2
            || norm(sa["row"]["titleValue"].as_str().unwrap_or("")) != norm(&t.record.title)
        {
            return Err(Failure::new(
                "TASK_CHANGED",
                "实时 SA 已非多条匹配或题名已变化，请重新核验。",
            ));
        }
        let mut result = if action == "scan_duplicates" {
            self.browser
                .execute(
                    app,
                    "duplicate",
                    "duplicate_scan",
                    json!({"sa_id":id,"title":t.record.title}),
                    75,
                )
                .await?
        } else {
            let scan = self
                .store
                .setting(&format!("duplicate_scan:{id}"))?
                .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "先按题名读取重复候选组。"))?;
            if scan["row"] != sa["row"] {
                return Err(Failure::new(
                    "TASK_CHANGED",
                    "SA 在读取候选后发生变化，请重新检索。",
                ));
            }
            let group_id = extra["group_id"].as_str().unwrap_or("");
            let choices = scan["result"]["groups"]
                .as_array()
                .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "候选组格式未知。"))?;
            let candidates: Vec<_> = choices.iter().filter(|g| g["id"] == group_id).collect();
            if candidates.len() != 1 {
                return Err(Failure::new(
                    "REVIEW_REQUIRED",
                    "请选择已读取的唯一候选组。",
                ));
            }
            let group = candidates[0];
            if ids
                .iter()
                .filter(|id| {
                    group["items"]
                        .as_array()
                        .map(|rows| rows.iter().any(|r| r["id"] == id.as_str()))
                        .unwrap_or(false)
                })
                .count()
                < 2
            {
                return Err(Failure::new(
                    "IDENTITY_CONFLICT",
                    "选定候选组至少需要包含两个当前 SA 匹配条目。",
                ));
            }
            self.browser.execute(app,"duplicate","duplicate_read",json!({"sa_id":id,"title":t.record.title,"group_id":group_id,"expected_group":group,"expected_threshold":scan["result"]["title_similarity"]}),75).await?
        };
        result["matched_ids"] = json!(ids);
        self.store.set_setting(
            &format!(
                "{}:{id}",
                if action == "scan_duplicates" {
                    "duplicate_scan"
                } else {
                    "duplicate_prepared"
                }
            ),
            json!({"row":sa["row"],"sa":sa,"result":result,"input_hash":t.input_hash,"record_fingerprint":t.record.fingerprint()}),
        )?;
        self.changed(app);
        return Ok(result);
    }
    pub(super) async fn handle_verify_duplicate(
        &self,
        app: &AppHandle,
        id: &str,
        _action: &str,
        mut t: Task,
        _extra: Value,
    ) -> Result<Value> {
        let attempts = self.store.unresolved(id)?;
        if attempts.len() != 1 || attempts[0]["action"] != "merge_duplicate" {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "没有唯一待确认的合并操作。",
            ));
        }
        let saved: Value = serde_json::from_str(attempts[0]["data"].as_str().unwrap_or("{}"))?;
        let p = &saved["payload"];
        let sa = self.read_sa(app, &mut t).await?;
        let ids = matched_ids(&sa["row"])?;
        merge::assert_sa_after(&t, p, &sa)?;
        let expected_ids = matched_ids(&p["expected_sa"])?;
        let result=self.browser.execute(app,"duplicate","duplicate_check",json!({"sa_id":id,"title":p["title"],"source_id":p["source_id"],"target_id":p["target_id"],"expected_master":p["expected_master"],"expected_source":p["expected_source"],"expected_threshold":p["expected_threshold"],"sa_verified":true,"sa_ids":ids,"expected_sa_ids":expected_ids}),75).await?;
        let after = self.read_sa(app, &mut t).await?;
        Self::record_merge(&mut t, p, &result, &after)?;
        t.stage = serde_json::from_value(p["previous_stage"].clone())?;
        t.running = false;
        t.last_error = None;
        self.store.verify_attempt(
            &mut t,
            attempts[0]["id"].as_str().unwrap_or(""),
            json!({"payload":p,"result":result,"sa_after":after}),
            "duplicate_verified",
        )?;
        self.changed(app);
        return Ok(result);
    }
}
