use super::*;

impl Engine {
    pub(super) async fn record_metadata(
        &self,
        app: &AppHandle,
        task: &mut Task,
        payload: &Value,
        result: &Value,
    ) -> Result<()> {
        metadata::assert_result(payload, result)?;
        self.browser
            .execute(app, "sa", "metadata_close", payload.clone(), 40)
            .await?;
        let live = self.read_sa(app, task).await?;
        let resolution = issues::resolve(
            task,
            &live,
            payload["key"].as_str().unwrap_or(""),
            "sa_correct",
            payload["evidence_id"].as_str().unwrap_or(""),
            payload["note"].as_str().unwrap_or(""),
        )?;
        task.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "metadata_verified".into(),
            source: "机构库编辑页与 SA · 保存后回读".into(),
            text:
                json!({"payload":payload,"result":result,"sa_after":live,"resolution":resolution})
                    .to_string(),
            created: now(),
        });
        let all_resolved = issues::assert_resolved(task, &live).is_ok();
        if let Some(review) = task.review.as_mut() {
            review.issues_resolved = all_resolved;
        }
        Ok(())
    }
    pub(super) async fn handle_prepare_metadata(
        &self,
        app: &AppHandle,
        id: &str,
        _action: &str,
        mut t: Task,
        _extra: Value,
    ) -> Result<Value> {
        if matches!(t.stage, Stage::Unknown | Stage::Completed) {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "先回读上次操作或任务已完成。",
            ));
        }
        if let Some(previous) = self.store.setting(&format!("metadata:{id}"))? {
            self.browser.execute(app, "sa", "metadata_close", json!({"sa_id":id,"item_id":previous["result"]["item_id"],"staff_id":t.record.staff_id,"expected_row":previous["sa"]["row"]}), 40).await?;
        }
        let snapshot = self.read_sa(app, &mut t).await?;
        let initial = t.issue_plan.is_none();
        issues::prepare(&mut t, &snapshot)?;
        if initial {
            t.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "issue_baseline".into(),
                source: "机构库 SA 首次逐项回读".into(),
                text: snapshot.to_string(),
                created: now(),
            });
        }
        let identity = self
            .browser
            .execute(
                app,
                "scholar",
                "alias_read",
                json!({"sa_id":id,"staff_id":t.record.staff_id,"close_after_read":true}),
                60,
            )
            .await?;
        let mut names: Vec<String> = ["nameCn", "nameEn"]
            .iter()
            .filter_map(|key| identity["scholar"][key].as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string())
            .collect();
        for alias in identity["aliases"]
            .as_array()
            .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "缺少真实学者别名列表。"))?
        {
            if let Some(name) = alias["nameAlias"].as_str().filter(|s| !s.trim().is_empty()) {
                names.push(name.trim().to_string());
            }
        }
        names.sort();
        names.dedup();
        let ids = matched_ids(&snapshot["row"])?;
        self.browser
            .execute(
                app,
                "sa",
                "open_metadata",
                json!({"sa_id":id,"expected":snapshot["row"],"automated":true}),
                60,
            )
            .await?;
        let result = self.browser.execute(app,"sa","metadata_read",json!({"sa_id":id,"item_id":ids[0],"staff_id":t.record.staff_id,"scholar":identity["scholar"],"names":names,"expected_row":snapshot["row"]}),60).await?;
        let prepared = json!({"sa":snapshot,"identity":identity,"names":names,"result":result});
        self.store
            .set_setting(&format!("metadata:{id}"), prepared.clone())?;
        self.store.save(&mut t, "metadata_prepared")?;
        self.changed(app);
        return Ok(prepared);
    }
    pub(super) async fn handle_verify_metadata(
        &self,
        app: &AppHandle,
        id: &str,
        _action: &str,
        mut t: Task,
        _extra: Value,
    ) -> Result<Value> {
        let attempts = self.store.unresolved(id)?;
        if attempts.len() != 1 || attempts[0]["action"] != "save_metadata" {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "没有唯一待确认的本库字段保存操作。",
            ));
        }
        let saved: Value = serde_json::from_str(attempts[0]["data"].as_str().unwrap_or("{}"))?;
        let mut payload = saved["payload"].clone();
        let result = match self
            .browser
            .execute(app, "sa", "metadata_check", payload.clone(), 60)
            .await
        {
            Err(error) if error.code == "EDITOR_NOT_OPEN" => {
                let fresh = self.read_sa(app, &mut t).await?;
                if fresh["row"]["gh"] != payload["staff_id"]
                    || fresh["row"]["saLzkId"] != t.id
                    || matched_ids(&fresh["row"])?
                        != vec![payload["item_id"].as_str().unwrap_or("").to_string()]
                {
                    return Err(Failure::new(
                        "IDENTITY_CONFLICT",
                        "回读目标不再是原来的唯一条目与工号。",
                    ));
                }
                payload["expected_row"] = fresh["row"].clone();
                self.browser
                    .execute(
                        app,
                        "sa",
                        "open_metadata",
                        json!({"sa_id":id,"expected":fresh["row"],"automated":true}),
                        60,
                    )
                    .await?;
                self.browser
                    .execute(app, "sa", "metadata_read", payload.clone(), 60)
                    .await?;
                self.browser
                    .execute(app, "sa", "metadata_check", payload.clone(), 60)
                    .await?
            }
            result => result?,
        };
        self.record_metadata(app, &mut t, &payload, &result).await?;
        t.stage = serde_json::from_value(payload["previous_stage"].clone())?;
        t.running = false;
        t.last_error = None;
        self.store.verify_attempt(
            &mut t,
            attempts[0]["id"].as_str().unwrap_or(""),
            result.clone(),
            "metadata_verified",
        )?;
        self.changed(app);
        return Ok(result);
    }
}
