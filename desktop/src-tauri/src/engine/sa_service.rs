use super::*;

impl Engine {
    pub async fn read_sa(&self, app: &AppHandle, t: &mut Task) -> Result<Value> {
        let result = self
            .browser
            .execute(app, "sa", "search", json!({"sa_id":t.id}), 60)
            .await?;
        if result["row"]["saLzkId"].as_str() != Some(&t.id) {
            return Err(Failure::new("IDENTITY_CONFLICT", "SA 回读目标不一致。"));
        }
        t.sa_snapshot = Some(result.clone());
        self.store.save(t, "sa_read")?;
        Ok(result)
    }
    pub(super) fn record_issue_plan(task: &mut Task, snapshot: &Value) -> Result<Value> {
        let initial = task.issue_plan.is_none();
        let result = issues::prepare(task, snapshot)?;
        if initial {
            task.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "issue_baseline".into(),
                source: "http://admin.ir.lib.sjtu.edu.cn/#/dataCompare/list · SA 首次逐项回读"
                    .into(),
                text: snapshot.to_string(),
                created: now(),
            });
        }
        let resolved = issues::assert_resolved(task, snapshot).is_ok();
        if let Some(review) = task.review.as_mut() {
            review.issues_resolved = resolved;
        }
        Ok(result)
    }
    pub(super) fn record_link_snapshot(
        task: &mut Task,
        payload: &Value,
        snapshot: &Value,
    ) -> Result<()> {
        sa::verify_link(task, payload, snapshot)?;
        let plan = Self::record_issue_plan(task, snapshot)?;
        task.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "sa_link_verified".into(),
            source: "机构库 SA · 关联后完整字段回读".into(),
            text: json!({"payload":payload,"sa_after":snapshot,"issues":plan}).to_string(),
            created: now(),
        });
        Ok(())
    }
    pub(super) async fn handle_issues(
        &self,
        app: &AppHandle,
        id: &str,
        action: &str,
        mut t: Task,
        extra: Value,
    ) -> Result<Value> {
        if matches!(t.stage, Stage::Unknown | Stage::Completed) {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "先回读上次操作或当前任务已经完成。",
            ));
        }
        if let Some(previous) = self.store.setting(&format!("metadata:{id}"))? {
            self.browser.execute(app,"sa","metadata_close",json!({"sa_id":id,"item_id":previous["result"]["item_id"],"staff_id":t.record.staff_id,"expected_row":previous["sa"]["row"]}),40).await?;
        }
        let snapshot = self.read_sa(app, &mut t).await?;
        let result = if action == "prepare_issues" {
            Self::record_issue_plan(&mut t, &snapshot)?
        } else {
            if extra["expected"] != snapshot {
                return Err(Failure::new(
                    "TASK_CHANGED",
                    "核对字段在页面读取后变化，请先刷新逐项清单。",
                ));
            }
            let source = extra["source"].as_str().unwrap_or("").trim();
            let proof = extra["proof"].as_str().unwrap_or("").trim();
            if source.is_empty() || proof.is_empty() {
                return Err(Failure::new(
                    "EVIDENCE_REQUIRED",
                    "每项分别填写实际核对来源与依据。",
                ));
            }
            let evidence = Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "human_review".into(),
                source: source.into(),
                text: proof.into(),
                created: now(),
            };
            let evidence_id = evidence.id.clone();
            t.evidence.push(evidence);
            let resolution = issues::resolve(
                &mut t,
                &snapshot,
                extra["key"].as_str().unwrap_or(""),
                extra["outcome"].as_str().unwrap_or(""),
                &evidence_id,
                extra["note"].as_str().unwrap_or(""),
            )?;
            t.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "issue_verified".into(),
                source: "机构库 SA 逐项核对 · 实时回读".into(),
                text: serde_json::to_string(&resolution)?,
                created: now(),
            });
            if let Some(review) = t.review.as_mut() {
                review.issues_resolved = false;
            }
            let ready = issues::assert_resolved(&t, &snapshot).is_ok();
            if let Some(review) = t.review.as_mut() {
                review.issues_resolved = ready;
            }
            json!({"resolution":resolution,"all_resolved":ready})
        };
        self.store.save(&mut t, action)?;
        self.changed(app);
        return Ok(result);
    }
    pub(super) async fn handle_read_sa(
        &self,
        app: &AppHandle,
        _id: &str,
        _action: &str,
        mut t: Task,
        _extra: Value,
    ) -> Result<Value> {
        let d = self.read_sa(app, &mut t).await?;
        self.changed(app);
        return Ok(d);
    }
    pub(super) async fn handle_verify_legacy_sa(
        &self,
        app: &AppHandle,
        id: &str,
        _action: &str,
        mut t: Task,
        _extra: Value,
    ) -> Result<Value> {
        let attempts = self.store.unresolved(id)?;
        if attempts.len() != 1 || attempts[0]["action"] != "legacy_sa" {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "没有唯一待确认的旧版 SA 检查点。",
            ));
        }
        let saved: Value = serde_json::from_str(attempts[0]["data"].as_str().unwrap_or("{}"))?;
        let snapshot = self.read_sa(app, &mut t).await?;
        library_core::legacy::verify_sa_checkpoint(&t, &saved["payload"], &snapshot)?;
        t.stage = Stage::Completed;
        t.running = false;
        t.record.done = true;
        t.last_error = None;
        t.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "legacy_sa_verified".into(),
            source: "旧版检查点 · 实时 SA 只读回读".into(),
            text: json!({"payload":saved["payload"],"sa_after":snapshot}).to_string(),
            created: now(),
        });
        self.store.verify_attempt(
            &mut t,
            attempts[0]["id"].as_str().unwrap(),
            snapshot.clone(),
            "legacy_sa_verified",
        )?;
        self.changed(app);
        return Ok(snapshot);
    }
    pub(super) async fn handle_verify_legacy_claim(
        &self,
        app: &AppHandle,
        id: &str,
        _action: &str,
        mut task: Task,
        _extra: Value,
    ) -> Result<Value> {
        let attempts = self.store.unresolved(id)?;
        if attempts.len() != 1
            || !["legacy_claim", "legacy_sa"]
                .contains(&attempts[0]["action"].as_str().unwrap_or(""))
        {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "没有唯一待确认的旧认领检查点。",
            ));
        }
        let saved: Value = serde_json::from_str(attempts[0]["data"].as_str().unwrap_or("{}"))?;
        let hydrated = legacy::hydrate_claim_checkpoint(&self.store, &saved["payload"])?;
        let payload = &hydrated;
        let before = self.read_sa(app, &mut task).await?;
        let command = legacy::claim_readback(&task, payload, &before)?;
        let result = self
            .browser
            .execute(app, "sa", "verify_claim", command, 65)
            .await?;
        let after = self.read_sa(app, &mut task).await?;
        legacy::verify_claim_checkpoint(&task, payload, &before, &result, &after)?;
        // Claim confirmation does not prove that SA reasons or closure are complete.
        task.stage = Stage::Claimed;
        task.running = false;
        task.last_error = None;
        task.platform_id = result["item_id"].as_str().unwrap().into();
        task.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "legacy_claim_verified".into(),
            source: "旧版原认领回执 · SA、完整作者与学者只读回读".into(),
            text: json!({"payload":payload,"before":before,"result":result,"after":after,"sa_completed":false}).to_string(),
            created: now(),
        });
        self.store.verify_attempt(
            &mut task,
            attempts[0]["id"].as_str().unwrap(),
            json!({"claim":result,"sa":after}),
            "legacy_claim_verified",
        )?;
        self.changed(app);
        Ok(json!({"claim":result,"sa":after,"sa_completed":false}))
    }
    pub(super) async fn handle_verify_sa(
        &self,
        app: &AppHandle,
        id: &str,
        _action: &str,
        mut t: Task,
        _extra: Value,
    ) -> Result<Value> {
        let attempts = self.store.unresolved(id)?;
        if attempts.len() != 1 {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "没有唯一待确认的 SA 操作。",
            ));
        }
        let action = attempts[0]["action"].as_str().unwrap_or("");
        let saved: Value = serde_json::from_str(attempts[0]["data"].as_str().unwrap_or("{}"))?;
        let payload = &saved["payload"];
        let d = self.read_sa(app, &mut t).await?;
        let mut verification = d.clone();
        let next = match action {
            "complete"
                if d["row"]["markStatus"] == "已处理" && d["row"]["remark"] == payload["note"] =>
            {
                Stage::Completed
            }
            "link"
                if d["row"]["itemId"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches(',')
                    == payload["item_id"].as_str().unwrap_or("__missing__") =>
            {
                sa::verify_link_plan(&t, payload, &d)?;
                if matches!(t.route, Route::Missing | Route::CorrectedExisting) {
                    let original: Evidence =
                        serde_json::from_value(payload["library_evidence"].clone())?;
                    let receipt: Value = serde_json::from_str(&original.text)?;
                    self.search_library(app, &mut t, receipt["result"]["target"].clone())
                        .await?;
                }
                let after = self.read_sa(app, &mut t).await?;
                Self::record_link_snapshot(&mut t, payload, &after)?;
                verification = json!({"payload":payload,"sa_after":after,"library_after":if matches!(t.route,Route::Missing|Route::CorrectedExisting){library::latest(&t)?}else{Value::Null}});
                serde_json::from_value(payload["previous_stage"].clone())?
            }
            "submit_claim" => {
                claim::assert_plan(&t, payload, &d)?;
                let mut verify = payload.clone();
                verify["expected"] = d["row"].clone();
                let result = self
                    .browser
                    .execute(app, "sa", "verify_claim", verify, 65)
                    .await?;
                let after = self.read_sa(app, &mut t).await?;
                Self::record_claim(&mut t, payload, &result, &after)?;
                verification = json!({"payload":payload,"result":result,"sa_after":after});
                Stage::Claimed
            }
            _ => {
                return Err(Failure::new(
                    "REMOTE_RESULT_UNKNOWN",
                    "尚未回读到上次操作的准确结果，请在平台核对，不能直接重发。",
                ))
            }
        };
        t.stage = next;
        t.running = false;
        t.last_error = None;
        if t.stage == Stage::Completed {
            t.record.done = true;
        }
        self.store.verify_attempt(
            &mut t,
            attempts[0]["id"].as_str().unwrap_or(""),
            verification,
            "sa_operation_verified",
        )?;
        self.changed(app);
        return Ok(d);
    }
}
