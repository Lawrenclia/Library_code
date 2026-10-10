use super::*;
use base64::Engine as _;

impl Engine {
    pub(super) async fn execute_write(
        &self,
        app: &AppHandle,
        id: &str,
        action: &str,
        approved: bool,
        mut t: Task,
        extra: Value,
    ) -> Result<Value> {
        let a = t.artifact.as_ref();
        let common = json!({"sa_id":id,"instructions":format!("SA补充-{id}"),"candidate":a.map(|a|serde_json::to_value(&a.candidate).unwrap())});
        if !approved {
            return Err(Failure::new(
                "REVIEW_REQUIRED",
                "请核对操作内容后确认执行。",
            ));
        }
        let (label, payload, next) = match action {
            "save_metadata" => {
                let prepared = self
                    .store
                    .setting(&format!("metadata:{id}"))?
                    .ok_or_else(|| {
                        Failure::new("REVIEW_REQUIRED", "先按工号读取实际作者编辑控件。")
                    })?;
                self.browser.execute(app,"sa","metadata_close",json!({"sa_id":id,"item_id":prepared["result"]["item_id"],"staff_id":t.record.staff_id,"expected_row":prepared["sa"]["row"]}),40).await?;
                let live = self.read_sa(app, &mut t).await?;
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
                if identity != prepared["identity"] {
                    return Err(Failure::new(
                        "TASK_CHANGED",
                        "学者或别名在准备后变化，请重新核对。",
                    ));
                }
                let index = extra["author_index"]
                    .as_u64()
                    .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "请选择已核对的作者行。"))?;
                let operation = extra["operation"].as_str().unwrap_or("role");
                let payload = if operation == "role" {
                    metadata::payload(
                        &mut t,
                        &live,
                        &prepared,
                        extra["key"].as_str().unwrap_or(""),
                        index,
                        extra["source"].as_str().unwrap_or(""),
                        extra["proof"].as_str().unwrap_or(""),
                        extra["note"].as_str().unwrap_or(""),
                    )?
                } else {
                    metadata::order_payload(
                        &mut t,
                        &live,
                        &prepared,
                        extra["key"].as_str().unwrap_or(""),
                        index,
                        operation,
                        &extra["order"],
                        extra["source"].as_str().unwrap_or(""),
                        extra["proof"].as_str().unwrap_or(""),
                        extra["note"].as_str().unwrap_or(""),
                    )?
                };
                self.browser
                    .execute(
                        app,
                        "sa",
                        "open_metadata",
                        json!({"sa_id":id,"expected":live["row"],"automated":true}),
                        60,
                    )
                    .await?;
                let fresh = self
                    .browser
                    .execute(app, "sa", "metadata_read", payload.clone(), 60)
                    .await?;
                if fresh["snapshot"] != payload["expected_snapshot"] {
                    return Err(Failure::new(
                        "TASK_CHANGED",
                        "本库完整字段在读取后变化，请重新准备。",
                    ));
                }
                ("sa", payload, t.stage.clone())
            }
            "merge_duplicate" => {
                let prepared = self
                    .store
                    .setting(&format!("duplicate_prepared:{id}"))?
                    .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "先读取并核对候选组详情。"))?;
                let sa = self.read_sa(app, &mut t).await?;
                let p = merge::payload(&t, &sa, &prepared, &extra)?;
                ("duplicate", p, t.stage.clone())
            }
            "add_alias" => {
                if matches!(t.stage, Stage::Unknown | Stage::Completed) {
                    return Err(Failure::new(
                        "INVALID_TRANSITION",
                        "当前任务需先回读或已完成。",
                    ));
                }
                let prepared = self
                    .store
                    .setting(&format!("alias:{id}"))?
                    .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "先按工号读取学者与别名。"))?;
                let alias = extra["alias"].as_str().unwrap_or("").trim();
                let evidence_id = extra["evidence_id"].as_str().unwrap_or("");
                let d = self.read_sa(app, &mut t).await?;
                let p = alias::payload(&t, &d, &prepared, alias, evidence_id)?;
                ("scholar", p, t.stage.clone())
            }
            "import_upload" => {
                library_core::submission::check_upload(&self.store, &t)?;
                t.import_ready()?;
                if !matches!(t.stage, Stage::Ready | Stage::Downloaded) {
                    return Err(Failure::new("INVALID_TRANSITION", "当前任务不能再次上传。"));
                }
                // An earlier absence receipt is not enough to upload: repeat the
                // same front-end queries immediately before the write boundary.
                let target = library::latest(&t)?["target"].clone();
                self.search_library(app, &mut t, target).await?;
                t.import_ready()?;
                let snapshot = self.read_sa(app, &mut t).await?;
                if snapshot["row"]["matchCount"] != 0 && snapshot["row"]["matchCount"] != "0" {
                    return Err(Failure::new(
                        "TASK_CHANGED",
                        "SA 已有匹配，先核对现有条目。",
                    ));
                }
                let current_title = snapshot["row"]["titleValue"].as_str().unwrap_or("");
                if norm(current_title) != norm(&t.record.title) {
                    return Err(Failure::new(
                        "TASK_CHANGED",
                        "SA 题名已变化，请重新核对任务。",
                    ));
                }
                let scan = self
                    .browser
                    .execute(app, "import", "import_scan", common.clone(), 55)
                    .await?;
                if scan["batches"]
                    .as_array()
                    .map(|v| !v.is_empty())
                    .unwrap_or(true)
                {
                    return Err(Failure::new(
                        "REMOTE_RESULT_UNKNOWN",
                        "已有同说明批次，请先核验。",
                    ));
                }
                let artifact = t.artifact.as_ref().unwrap();
                library_core::legacy::assert_no_prior_import(&self.store, id, &artifact.candidate)?;
                self.store.assert_no_prior_upload(&artifact.candidate)?;
                for other in self.store.tasks()? {
                    if other.id != id
                        && other
                            .artifact
                            .as_ref()
                            .map(|a| a.candidate.wos == artifact.candidate.wos)
                            .unwrap_or(false)
                        && matches!(
                            other.stage,
                            Stage::Uploaded
                                | Stage::Imported
                                | Stage::Pushed
                                | Stage::Claimed
                                | Stage::Completed
                                | Stage::Unknown
                        )
                    {
                        return Err(Failure::new(
                            "DUPLICATE_WRITE",
                            "同一论文已有平台操作，请核验并关联现有条目。",
                        ));
                    }
                }
                let raw = std::fs::read(&artifact.path)?;
                if hash(&raw) != artifact.candidate.sha256 {
                    return Err(Failure::new("FILE_INVALID", "归档文件已变化。"));
                }
                let mut p = common;
                p["submission_packet"] =
                    serde_json::to_value(library_core::submission::current(&self.store, id)?)?;
                p["content"] = base64::engine::general_purpose::STANDARD.encode(raw).into();
                p["contentSha"] = artifact.candidate.sha256.clone().into();
                ("import", p, Stage::Uploaded)
            }
            "import_submit" => {
                t.import_ready()?;
                if t.stage != Stage::Uploaded {
                    return Err(Failure::new("INVALID_TRANSITION", "先等待上传成功。"));
                }
                let mut p = common;
                p["upload"] = t.batch.clone().unwrap_or(Value::Null);
                ("import", p, Stage::Unknown)
            }
            "import_push" => {
                t.import_ready()?;
                if t.stage != Stage::Imported {
                    return Err(Failure::new("INVALID_TRANSITION", "先回读导入成功的批次。"));
                }
                let mut p = common;
                p["batch"] = t.batch.clone().unwrap_or(Value::Null);
                ("import", p, Stage::Unknown)
            }
            "link" => {
                if matches!(t.route, Route::Missing | Route::CorrectedExisting) {
                    let target = library::latest(&t)?["target"].clone();
                    self.search_library(app, &mut t, target).await?;
                    library::review(
                        &t,
                        t.review.as_ref().ok_or_else(|| {
                            Failure::new("REVIEW_REQUIRED", "先保存实际条目核验。")
                        })?,
                    )?;
                }
                let d = self.read_sa(app, &mut t).await?;
                let payload = sa::link_payload(&t, &d)?;
                if payload["already_linked"] == true {
                    Self::record_link_snapshot(&mut t, &payload, &d)?;
                    self.store.save(&mut t, "sa_link_readback")?;
                    self.changed(app);
                    return Ok(
                        json!({"verified":true,"already_linked":true,"row":d["row"],"comparison":d["comparison"]}),
                    );
                }
                ("sa", payload, t.stage.clone())
            }
            "complete" => {
                if matches!(t.route, Route::Missing | Route::CorrectedExisting) {
                    let target = library::latest(&t)?["target"].clone();
                    self.search_library(app, &mut t, target).await?;
                }
                t.assert_complete()?;
                let d = self.read_sa(app, &mut t).await?;
                if matched_ids(&d["row"])?.len() == 1 {
                    issues::assert_resolved(&t, &d)?;
                }
                if t.route == Route::Duplicate
                    && matched_ids(&d["row"])? != vec![t.platform_id.clone()]
                {
                    return Err(Failure::new(
                        "REVIEW_REQUIRED",
                        "合并后 SA 尚未回读为唯一主条目，不能设置已处理。",
                    ));
                }
                let count = d["row"]["matchCount"]
                    .as_u64()
                    .or_else(|| d["row"]["matchCount"].as_str().and_then(|s| s.parse().ok()));
                if (t.route == Route::NonSjtu && count != Some(0))
                    || (t.route == Route::Existing && count != Some(1))
                {
                    return Err(Failure::new(
                        "TASK_CHANGED",
                        "实时匹配数量已变化，请重新核验业务分支。",
                    ));
                }
                if matches!(
                    t.route,
                    Route::Missing | Route::CorrectedExisting | Route::Duplicate
                ) && d["row"]["itemId"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches(',')
                    != t.platform_id
                {
                    return Err(Failure::new(
                        "REVIEW_REQUIRED",
                        "SA 尚未关联核验后的平台唯一号。",
                    ));
                }
                let mut note = t.review.as_ref().unwrap().note.clone();
                if !t.issue_reviews.is_empty() {
                    note.push_str("；逐项核对：");
                    note.push_str(
                        &t.issue_reviews
                            .iter()
                            .map(|r| {
                                format!(
                                    "{}：{}",
                                    t.issue_plan
                                        .as_ref()
                                        .and_then(|p| p
                                            .requirements
                                            .iter()
                                            .find(|i| i.key == r.key))
                                        .map(|i| i.label.as_str())
                                        .unwrap_or(&r.key),
                                    r.note
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("；"),
                    );
                }
                if note.chars().count() > 2000 {
                    return Err(Failure::new(
                        "INCOMPLETE_METADATA",
                        "逐项备注合计超过平台 2000 字符限制，请缩短结论；完整依据保留在来源报告。",
                    ));
                }
                (
                    "sa",
                    json!({"sa_id":id,"expected":d["row"],"expected_comparison":d["comparison"],"reviewed":true,"note":note}),
                    Stage::Completed,
                )
            }
            "submit_claim" => {
                if matches!(
                    t.route,
                    Route::NonSjtu | Route::NotFound | Route::ZeroReview
                ) || (t.route == Route::Missing
                    && !matches!(t.stage, Stage::Pushed | Stage::Claimed))
                {
                    return Err(Failure::new(
                        "INVALID_TRANSITION",
                        "当前分支尚不能认领，请先完成条目核对或推送核验。",
                    ));
                }
                if t.stage == Stage::Unknown {
                    return Err(Failure::new(
                        "REMOTE_RESULT_UNKNOWN",
                        "先核验上次认领结果。",
                    ));
                }
                let prepared = self
                    .store
                    .setting(&format!("claim:{id}"))?
                    .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "先读取可认领作者。"))?;
                let index = extra["author_index"]
                    .as_u64()
                    .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "请选择已核对的作者行。"))?;
                let live = self.read_sa(app, &mut t).await?;
                let p = claim::payload(&t, &live, &prepared, index)?;
                ("sa", p, Stage::Claimed)
            }
            _ => return Err(Failure::new("INVALID_ACTION", "未知业务操作。")),
        };
        let mut audit = payload.clone();
        if let Some(data) = audit.as_object_mut() {
            data.remove("content");
        }
        let attempt = self
            .store
            .begin_attempt_with_payload(&mut t, action, audit.clone())?;
        self.changed(app);
        let browser_action = match action {
            "add_alias" => "alias_add",
            "merge_duplicate" => "duplicate_merge",
            "save_metadata" => "metadata_save",
            _ => action,
        };
        let result = self
            .browser
            .execute(app, label, browser_action, payload, 70)
            .await;
        match result {
            Ok(d) => {
                if [
                    "complete",
                    "link",
                    "submit_claim",
                    "add_alias",
                    "merge_duplicate",
                    "save_metadata",
                ]
                .contains(&action)
                    && d["verified"] != true
                {
                    t.running = false;
                    t.stage = Stage::Unknown;
                    t.last_error = Some(Failure::new(
                        "REMOTE_RESULT_UNKNOWN",
                        "未收到平台回读核验结果。",
                    ));
                    self.store.finish_attempt(
                        &mut t,
                        &attempt,
                        "unknown",
                        d.clone(),
                        "result_unknown",
                    )?;
                    self.changed(app);
                    return Err(t.last_error.unwrap());
                }
                if action == "import_upload"
                    && (d["uploaded"] != true
                        || d["sha256"] != t.artifact.as_ref().unwrap().candidate.sha256)
                {
                    t.running = false;
                    t.stage = Stage::Unknown;
                    t.last_error = Some(Failure::new(
                        "REMOTE_RESULT_UNKNOWN",
                        "上传结果不一致，需要核验。",
                    ));
                    self.store.finish_attempt(
                        &mut t,
                        &attempt,
                        "unknown",
                        d.clone(),
                        "result_unknown",
                    )?;
                    self.changed(app);
                    return Err(t.last_error.unwrap());
                }
                if action == "merge_duplicate" {
                    let checked = match self.read_sa(app, &mut t).await {
                        Ok(sa) => Self::record_merge(&mut t, &audit, &d, &sa),
                        Err(error) => Err(error),
                    };
                    if let Err(error) = checked {
                        t.stage = Stage::Unknown;
                        t.running = false;
                        t.last_error = Some(error.clone());
                        self.store.finish_attempt(
                            &mut t,
                            &attempt,
                            "unknown",
                            d,
                            "result_unknown",
                        )?;
                        self.changed(app);
                        return Err(error);
                    }
                }
                if action == "save_metadata" {
                    if let Err(error) = self.record_metadata(app, &mut t, &audit, &d).await {
                        t.stage = Stage::Unknown;
                        t.running = false;
                        t.last_error = Some(error.clone());
                        self.store.finish_attempt(
                            &mut t,
                            &attempt,
                            "unknown",
                            d,
                            "result_unknown",
                        )?;
                        self.changed(app);
                        return Err(error);
                    }
                }
                if action == "link" {
                    let checked = match self.read_sa(app, &mut t).await {
                        Ok(live) => Self::record_link_snapshot(&mut t, &audit, &live),
                        Err(error) => Err(error),
                    };
                    if let Err(error) = checked {
                        t.stage = Stage::Unknown;
                        t.running = false;
                        t.last_error = Some(error.clone());
                        self.store.finish_attempt(
                            &mut t,
                            &attempt,
                            "unknown",
                            d,
                            "result_unknown",
                        )?;
                        self.changed(app);
                        return Err(error);
                    }
                }
                if action == "submit_claim" {
                    let checked = match self.read_sa(app, &mut t).await {
                        Ok(live) => Self::record_claim(&mut t, &audit, &d, &live),
                        Err(error) => Err(error),
                    };
                    if let Err(error) = checked {
                        t.stage = Stage::Unknown;
                        t.running = false;
                        t.last_error = Some(error.clone());
                        self.store.finish_attempt(
                            &mut t,
                            &attempt,
                            "unknown",
                            d,
                            "claim_unconfirmed",
                        )?;
                        self.changed(app);
                        return Err(error);
                    }
                }
                t.stage = next;
                t.running = false;
                t.last_error = None;
                if action == "import_upload" {
                    t.batch = Some(d.clone());
                }
                if matches!(action, "import_upload" | "import_submit" | "import_push") {
                    t.evidence.push(Evidence {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: "import_receipt".into(),
                        source: "机构库导入管理 · 单次执行回执".into(),
                        text: json!({"input_hash":t.input_hash,"action":action,"attempt_id":attempt,"payload":audit,"result":d}).to_string(),
                        created: now(),
                    });
                }
                if action == "complete" {
                    t.record.done = true;
                }
                if action == "add_alias" {
                    let fresh = self.read_sa(app, &mut t).await?;
                    if let Err(error) = Self::record_alias(&mut t, &audit, &d, &fresh) {
                        t.stage = Stage::Unknown;
                        t.running = false;
                        t.last_error = Some(error.clone());
                        self.store.finish_attempt(
                            &mut t,
                            &attempt,
                            "unknown",
                            json!(error),
                            "alias_unconfirmed",
                        )?;
                        self.changed(app);
                        return Err(error);
                    }
                }
                let state = if t.stage == Stage::Unknown {
                    "unknown"
                } else {
                    "verified"
                };
                self.store
                    .finish_attempt(&mut t, &attempt, state, d.clone(), "step_finished")?;
                self.changed(app);
                Ok(d)
            }
            Err(e) => {
                if e.submitted == Some(false) {
                    t.running = false;
                    t.last_error = Some(e.clone());
                    self.store.finish_attempt(
                        &mut t,
                        &attempt,
                        "not_sent",
                        json!(e),
                        "write_not_sent",
                    )?;
                    self.changed(app);
                    return Err(e);
                }
                t.running = false;
                t.stage = Stage::Unknown;
                t.last_error = Some(Failure::new(
                    "REMOTE_RESULT_UNKNOWN",
                    format!("{}；请先回读核验，不能直接重发。", e.message),
                ));
                self.store.finish_attempt(
                    &mut t,
                    &attempt,
                    "unknown",
                    json!(e),
                    "result_unknown",
                )?;
                self.changed(app);
                Err(t.last_error.unwrap())
            }
        }
    }
}
