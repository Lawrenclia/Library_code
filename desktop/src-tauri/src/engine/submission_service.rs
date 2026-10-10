use super::*;
use base64::Engine as _;

impl Engine {
    pub(super) async fn handle_verify_import(
        &self,
        app: &AppHandle,
        id: &str,
        _action: &str,
        mut t: Task,
        _extra: Value,
    ) -> Result<Value> {
        let a = t.artifact.as_ref();
        let common = json!({"sa_id":id,"instructions":format!("SA补充-{id}"),"candidate":a.map(|a|serde_json::to_value(&a.candidate).unwrap())});

        let attempts = self.store.unresolved(id)?;
        if attempts.iter().any(|a| {
            serde_json::from_str::<Value>(a["data"].as_str().unwrap_or("{}"))
                .ok()
                .is_some_and(|v| v["payload"]["input_matches"] == false)
        }) {
            return Err(Failure::new(
                "INPUT_CHANGED",
                "旧版导入日志与名单版本不一致，先核对历史输入和批次。",
            ));
        }
        if attempts.len() > 1
            || attempts
                .iter()
                .any(|a| !a["action"].as_str().unwrap_or("").starts_with("import_"))
        {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "待确认的操作不是本条导入/推送，请使用对应的回读步骤。",
            ));
        }
        let original_payload = attempts
            .first()
            .map(|attempt| {
                serde_json::from_str::<Value>(attempt["data"].as_str().unwrap_or("{}"))
                    .map(|saved| saved["payload"].clone())
            })
            .transpose()?;
        let original = original_payload.as_ref().unwrap_or(&common);
        let original_raw = files::read_import_archive(&t, original)?;
        if attempts
            .first()
            .is_some_and(|a| a["action"] == "import_upload")
            || (attempts.is_empty() && t.stage == Stage::Uploaded)
        {
            let checkpoint = t.batch.clone();
            let mut cmd = if let Some(attempt) = attempts.first() {
                let saved: Value = serde_json::from_str(attempt["data"].as_str().unwrap_or("{}"))?;
                saved["payload"].clone()
            } else {
                let upload = checkpoint.as_ref().ok_or_else(|| {
                    Failure::new("REMOTE_RESULT_UNKNOWN", "缺少原上传回执，不能重新上传。")
                })?;
                if upload["uploaded"] != true || upload["filename"] != format!("SA-WOS-{id}.txt") {
                    return Err(Failure::new(
                        "REMOTE_RESULT_UNKNOWN",
                        "原上传回执身份不一致，不能重新上传。",
                    ));
                }
                let mut cmd = common.clone();
                cmd["contentSha"] = upload["sha256"].clone();
                cmd
            };
            let raw = original_raw;
            if cmd["contentSha"] != hash(&raw) {
                return Err(Failure::new(
                    "FILE_INVALID",
                    "原上传归档与执行载荷不一致，保持结果未知。",
                ));
            }
            cmd["expect_upload"] = true.into();
            cmd["content"] = base64::engine::general_purpose::STANDARD
                .encode(&raw)
                .into();
            let result = self
                .browser
                .execute(app, "import", "import_check", cmd.clone(), 65)
                .await?;
            files::read_import_archive(&t, original)?;
            if result["uploaded"] == true {
                validate_upload_readback(&result, &cmd, raw.len())?;
                if checkpoint.as_ref().is_some_and(|previous| {
                    previous["uploaded"] == true && previous["dataset_id"] != result["dataset_id"]
                }) {
                    return Err(Failure::new(
                        "REMOTE_RESULT_UNKNOWN",
                        "所属机构与原上传回执不一致，不能重新上传。",
                    ));
                }
                t.stage = Stage::Uploaded;
                t.batch = Some(result.clone());
            } else {
                t.stage = validate_import_readback(&result, &cmd, false)?;
                t.batch = Some(result["batch"].clone());
            }
            t.running = false;
            t.last_error = None;
            t.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "upload_verified".into(),
                source: "机构库导入管理 · 原上传只读核验".into(),
                text: json!({"input_hash":t.input_hash,"attempt":attempts.first(),"previous_upload":checkpoint,"result":result})
                    .to_string(),
                created: now(),
            });
            if let Some(attempt) = attempts.first() {
                self.store.verify_attempt(
                    &mut t,
                    attempt["id"].as_str().unwrap_or(""),
                    result.clone(),
                    "upload_verified",
                )?;
            } else {
                self.store.save(&mut t, "upload_readback")?;
            }
            self.changed(app);
            return Ok(result);
        }
        let mut cmd = original.clone();
        // Upload receipts describe a temporary object, not an imported batch.
        cmd["batch"] = original
            .get("batch")
            .filter(|v| !v.is_null())
            .or(t.batch.as_ref().filter(|v| v["uploaded"] != true))
            .cloned()
            .unwrap_or(Value::Null);
        cmd["batch_id"] = original["batch_id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| cmd["batch"]["id"].as_str())
            .unwrap_or("")
            .into();
        cmd["expect_pushed"] = (attempts
            .first()
            .map(|a| a["action"] == "import_push")
            .unwrap_or(t.stage == Stage::Pushed))
        .into();
        let require_push = cmd["expect_pushed"] == true;
        let d = self
            .browser
            .execute(app, "import", "import_check", cmd.clone(), 65)
            .await?;
        if d["verified"] != true {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "尚未核验到导入/推送成功。",
            ));
        }
        files::read_import_archive(&t, original)?;
        t.stage = validate_import_readback(&d, &cmd, require_push)?;
        t.batch = Some(d["batch"].clone());
        t.running = false;
        t.last_error = None;
        t.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "import_verified".into(),
            source: "机构库导入管理 · 批次与完整文献回读".into(),
            text: json!({"input_hash":t.input_hash,"attempt":attempts.first(),"result":d})
                .to_string(),
            created: now(),
        });
        if let Some(attempt) = attempts.first() {
            self.store.verify_attempt(
                &mut t,
                attempt["id"].as_str().unwrap_or(""),
                d.clone(),
                "platform_verified",
            )?;
        } else {
            self.store.save(&mut t, "platform_verified")?;
        }
        self.changed(app);
        return Ok(d);
    }
}
