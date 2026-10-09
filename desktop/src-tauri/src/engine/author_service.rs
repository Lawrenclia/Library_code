use super::*;

impl Engine {
    pub(super) fn record_claim(
        task: &mut Task,
        payload: &Value,
        result: &Value,
        live: &Value,
    ) -> Result<()> {
        claim::assert_result(task, payload, result, live)?;
        task.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "claim_verified".into(),
            source: "机构库 SA、学者与完整原作者认领回读".into(),
            text: json!({"payload":payload,"result":result,"sa_after":live}).to_string(),
            created: now(),
        });
        Ok(())
    }
    pub(super) fn record_alias(
        task: &mut Task,
        payload: &Value,
        result: &Value,
        live: &Value,
    ) -> Result<()> {
        alias::assert_result(task, payload, result, live)?;
        task.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "alias_verified".into(),
            source: "机构库学者管理 · 完整别名与 SA 回读".into(),
            text: json!({"payload":payload,"result":result,"sa_after":live}).to_string(),
            created: now(),
        });
        Ok(())
    }
    pub(super) async fn handle_prepare_alias(
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
                "先核验上次别名操作，或当前任务已完成。",
            ));
        }
        if files::ensure_metadata_evidence(&mut t)? {
            self.store.save(&mut t, "metadata_sources_restored")?;
        }
        let d = self.read_sa(app, &mut t).await?;
        let staff = d["row"]["gh"].as_str().unwrap_or("");
        if staff.is_empty() || staff != t.record.staff_id {
            return Err(Failure::new(
                "IDENTITY_CONFLICT",
                "SA 工号与名单工号不一致，不能定位别名对象。",
            ));
        }
        let mut result = self
            .browser
            .execute(
                app,
                "scholar",
                "alias_read",
                json!({"sa_id":id,"staff_id":staff}),
                60,
            )
            .await?;
        result["sa"] = d;
        result["input_hash"] = t.input_hash.clone().into();
        result["record_fingerprint"] = t.record.fingerprint().into();
        self.store
            .set_setting(&format!("alias:{id}"), result.clone())?;
        self.changed(app);
        return Ok(result);
    }
    pub(super) async fn handle_verify_alias(
        &self,
        app: &AppHandle,
        id: &str,
        _action: &str,
        mut t: Task,
        _extra: Value,
    ) -> Result<Value> {
        let attempts = self.store.unresolved(id)?;
        if attempts.len() != 1 || attempts[0]["action"] != "add_alias" {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "没有唯一待确认的别名保存操作。",
            ));
        }
        let saved: Value = serde_json::from_str(attempts[0]["data"].as_str().unwrap_or("{}"))?;
        let p = &saved["payload"];
        let before = self.read_sa(app, &mut t).await?;
        alias::assert_plan(&t, p, &before)?;
        let result = self.browser.execute(app, "scholar", "alias_check", json!({"sa_id":id,"staff_id":p["staff_id"],"alias":p["alias"],"expected_scholar":p["expected_scholar"],"expected_aliases":p["expected_aliases"]}), 60).await?;
        let after = self.read_sa(app, &mut t).await?;
        Self::record_alias(&mut t, p, &result, &after)?;
        t.stage = serde_json::from_value(p["previous_stage"].clone())?;
        t.running = false;
        t.last_error = None;
        self.store.verify_attempt(
            &mut t,
            attempts[0]["id"].as_str().unwrap_or(""),
            result.clone(),
            "alias_verified",
        )?;
        self.changed(app);
        return Ok(result);
    }
    pub(super) async fn handle_prepare_claim(
        &self,
        app: &AppHandle,
        id: &str,
        action: &str,
        mut t: Task,
        _extra: Value,
    ) -> Result<Value> {
        if action == "prepare_claim" && matches!(t.stage, Stage::Unknown | Stage::Completed) {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "先核验上次认领，或任务已完成。",
            ));
        }
        let d = self.read_sa(app, &mut t).await?;
        let mut payload = json!({"sa_id":id,"expected":d["row"]});
        if action == "prepare_claim" {
            let sa_text = d["comparison"]
                .as_array()
                .and_then(|rows| rows.iter().find(|r| r["label"] == "认领状态"))
                .and_then(|r| r["sa"].as_str())
                .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "缺少 SA 认领状态。"))?;
            let pattern = regex::Regex::new(r"[（(]([0-9]{1,40})[）)]").unwrap();
            let captures: Vec<_> = pattern.captures_iter(sa_text).collect();
            if captures.len() != 1 {
                return Err(Failure::new(
                    "REVIEW_REQUIRED",
                    "SA 认领编号不唯一，请人工核对。",
                ));
            }
            payload["sa_text"] = sa_text.into();
            payload["staff_id"] = captures[0][1].into();
            payload["roster_staff_id"] = t.record.staff_id.clone().into();
        }
        let mut result = self.browser.execute(app, "sa", action, payload, 60).await?;
        if action == "prepare_claim" {
            // A name match is a candidate, not confirmation of the author row.
            result["suggested_index"] = serde_json::Value::Null;
            result["input_hash"] = t.input_hash.clone().into();
            result["record_fingerprint"] = t.record.fingerprint().into();
            self.store
                .set_setting(&format!("claim:{id}"), result.clone())?;
        }
        self.changed(app);
        return Ok(result);
    }
}
