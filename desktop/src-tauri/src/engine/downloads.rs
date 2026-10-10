use super::*;

impl Engine {
    pub fn attach(
        &self,
        id: &str,
        path: &std::path::Path,
        source: String,
        url: String,
    ) -> Result<Task> {
        if !path.is_file()
            || path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| !s.eq_ignore_ascii_case("txt"))
                .unwrap_or(true)
            || std::fs::symlink_metadata(path)?.file_type().is_symlink()
        {
            return Err(Failure::new(
                "FILE_INVALID",
                "请选择真实单篇 WOS TXT 文件。",
            ));
        }
        if std::fs::metadata(path)?.len() > 524288 {
            return Err(Failure::new(
                "FILE_INVALID",
                "单篇 WOS 文件超过 512 KB，未读取或绑定。",
            ));
        }
        let raw = std::fs::read(path)?;
        let c = files::parse_wos(&raw)?;
        let mut t = self.store.task(id)?;
        if self.store.pending_input(id)?.is_some() || !self.store.unresolved(id)?.is_empty() {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "先核对名单版本和未确认的平台操作，再接收原始文件。",
            ));
        }
        if matches!(
            t.stage,
            Stage::Uploaded
                | Stage::Imported
                | Stage::Pushed
                | Stage::Claimed
                | Stage::Completed
                | Stage::Unknown
        ) {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "任务已有平台操作，不替换导出文件。",
            ));
        }
        let strong = files::verify_identity(&t.record, &c)?;
        let archive = files::archive(&self.store.root, &raw)?;
        t.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "metadata".into(),
            source: url.clone(),
            // Keep every exported field available to classification, templates,
            // author identity checks and the source report.
            text: serde_json::to_string_pretty(&c.fields)?,
            created: now(),
        });
        t.artifact = Some(Artifact {
            path: archive.to_string_lossy().into(),
            source,
            record_url: url,
            downloaded: now(),
            candidate: c,
            identity_confirmed: strong,
        });
        t.classification = None;
        t.running = false;
        t.last_error = None;
        t.stage = Stage::Downloaded;
        self.store.save(&mut t, "artifact_verified")?;
        Ok(t)
    }
    pub async fn download_one(&self, app: &AppHandle, id: &str) -> Result<()> {
        let mut t = self.store.task(id)?;
        if t.record.done || t.record.matches != 0 {
            return Ok(());
        }
        if matches!(
            t.stage,
            Stage::Unknown
                | Stage::Uploaded
                | Stage::Imported
                | Stage::Pushed
                | Stage::Claimed
                | Stage::Completed
        ) {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "已有平台操作的任务不能重新检索覆盖状态。",
            ));
        }
        if self.store.pending_input(id)?.is_some()
            || t.last_error
                .as_ref()
                .map(|e| e.code == "INPUT_CHANGED")
                .unwrap_or(false)
        {
            return Err(Failure::new("INPUT_CHANGED", "任务输入已变化，请先核对。"));
        }
        // An actual native completion receipt owns recovery before any shared
        // file shortcut. Its original scope/hash cannot be bypassed by reuse.
        if self.store.recover_native_download(id)? {
            self.changed(app);
            return Ok(());
        }
        if self.store.restore_wos_artifact(id, t.revision)? {
            self.changed(app);
            return Ok(());
        }
        t.stage = Stage::Searching;
        t.running = true;
        t.last_error = None;
        self.store.save(&mut t, "search_started")?;
        self.changed(app);
        let payload = json!({"sa_id":t.id,"title":t.record.title,"doi":normalized_doi(&t.record.doi),"wos":normalized_wos(&t.record.wos)});
        let journal = library_core::wos_search::begin(&self.store, &t)?;
        let searched = self.browser.search(app, payload.clone(), &journal).await;
        journal.finish(&searched)?;
        let record = searched?;
        if self.pause.load(Ordering::SeqCst) {
            t.running = false;
            t.stage = Stage::Pending;
            self.store.save(&mut t, "paused")?;
            return Ok(());
        }
        let url = record["record_url"]
            .as_str()
            .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "检索没有返回来源链接。"))?;
        t.stage = Stage::Downloading;
        self.store.save(&mut t, "export_started")?;
        self.changed(app);
        let mut payload = payload;
        payload["expected_record_url"] = url.into();
        self.browser.download(app, &self.store, &t, payload).await?;
        self.changed(app);
        Ok(())
    }
    pub async fn queue(&self, app: &AppHandle, owner: &str, retry_skipped: bool) -> Result<()> {
        let queue = self.store.start_download_queue(owner, retry_skipped)?;
        self.execute_download_queue(app, &queue.id).await
    }
    pub async fn resume_queue(&self, app: &AppHandle, id: &str) -> Result<()> {
        self.store.resume_download_queue(id)?;
        self.execute_download_queue(app, id).await
    }
    pub(super) async fn execute_download_queue(&self, app: &AppHandle, id: &str) -> Result<()> {
        self.changed(app);
        let result = self.download_queue_loop(app, id).await;
        if let Err(error) = &result {
            self.pause.store(true, Ordering::SeqCst);
            // If storage itself failed this may also fail. The persisted running
            // queue is still recovered as interrupted by the next app startup.
            let _ = self.store.block_download_queue(id, error.clone(), None);
        }
        self.changed(app);
        result
    }
    pub(super) async fn download_queue_loop(&self, app: &AppHandle, queue_id: &str) -> Result<()> {
        loop {
            let queue = self.store.download_queue(queue_id)?;
            if queue.status != queue::QueueStatus::Running {
                break;
            }
            if self.pause.load(Ordering::SeqCst) || queue.pause_requested {
                self.store.pause_download_queue(queue_id)?;
                break;
            }
            let target = queue
                .targets
                .get(queue.cursor)
                .ok_or_else(|| Failure::new("QUEUE_CHANGED", "队列位置与原范围不一致。"))?;
            let id = &target.id;
            let t = self.store.task(id)?;
            let available = target.matches_task(&t)
                && !t.record.done
                && t.record.matches == 0
                && t.record.owner == queue.owner
                && matches!(t.route, Route::ZeroReview | Route::Missing)
                && (matches!(
                    t.stage,
                    Stage::Pending | Stage::Searching | Stage::Downloading | Stage::AwaitingReview
                ) || (matches!(t.stage, Stage::Downloaded | Stage::Ready)
                    && t.artifact.is_some()))
                && self.store.pending_input(id)?.is_none()
                && self.store.unresolved(id)?.is_empty();
            if !available {
                self.store.finish_download_target(queue_id, id, "not_executed", Some(Failure::new(
                    "QUEUE_TARGET_CHANGED", "原队列的名单版本、任务字段、跳过标记、业务阶段或待确认操作已变化，或原名单版本未记录；未检索或覆盖当前任务，请核对。")), None)?;
                self.changed(app);
                continue;
            }
            match self.download_one(app, id).await {
                Ok(()) => {
                    // Pause during search preserves this target for continuation.
                    // A completed download is checkpointed before moving on.
                    if self.pause.load(Ordering::SeqCst) && self.store.task(id)?.artifact.is_none()
                    {
                        self.store.pause_download_queue(queue_id)?;
                        break;
                    }
                    if let Err(error) =
                        self.store
                            .finish_download_target(queue_id, id, "downloaded", None, None)
                    {
                        // A file may change after the native callback. An ordinary
                        // validation failure still belongs to this one target;
                        // scope changes must preserve the newer business state.
                        let saved = self.store.record_download_failure(queue_id, id, error)?;
                        if saved.status == queue::QueueStatus::Blocked {
                            self.pause.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                }
                Err(error) => {
                    let saved = self.store.record_download_failure(queue_id, id, error)?;
                    if saved.status == queue::QueueStatus::Blocked {
                        self.pause.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }
            self.changed(app);
        }
        Ok(())
    }
    pub(super) async fn handle_search_wos(
        &self,
        app: &AppHandle,
        id: &str,
        _action: &str,
        _t: Task,
        _extra: Value,
    ) -> Result<Value> {
        self.download_one(app, id).await?;
        return Ok(json!({}));
    }
}
