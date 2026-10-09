use crate::browser::Browser;
use base64::Engine as _;
use library_core::{files, *};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Emitter};

pub struct Engine {
    _instance_lock: std::fs::File,
    pub store: Store,
    pub browser: Browser,
    pub active: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
}
impl Engine {
    fn record_alias(task: &mut Task, payload: &Value, result: &Value, live: &Value) -> Result<()> {
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
    pub fn new(root: std::path::PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        let root = root.canonicalize()?;
        #[cfg(windows)]
        let root = std::path::PathBuf::from(root.to_string_lossy().trim_start_matches("\\\\?\\"));
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("workspace.lock"))?;
        lock.try_lock()
            .map_err(|_| Failure::new("BUSY", "工作台已在其他窗口运行，请回到现有窗口。"))?;
        let store = Store::new(root)?;
        store.recover()?;
        Ok(Self {
            _instance_lock: lock,
            store,
            browser: Browser::default(),
            active: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn acquire(&self) -> Result<Lease> {
        if self
            .active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(Failure::new("BUSY", "已有任务运行，先暂停或等待完成。"));
        }
        self.pause.store(false, Ordering::SeqCst);
        Ok(Lease(self.active.clone()))
    }
    pub fn changed(&self, app: &AppHandle) {
        let _ = app.emit_to("main", "workspace-changed", json!({"time":now()}));
    }
    pub fn snapshot(&self, app: &AppHandle) -> Result<Value> {
        let download_queue = self.store.latest_download_queue()?;
        let queue_paused = download_queue
            .as_ref()
            .is_some_and(|q| q.status.unfinished() && q.status != queue::QueueStatus::Running);
        let mut tasks = serde_json::to_value(self.store.tasks()?)?;
        for task in tasks.as_array_mut().unwrap() {
            let attempts = self.store.unresolved(task["id"].as_str().unwrap_or(""))?;
            task["pending_action"] = if attempts.len() == 1 {
                attempts[0]["action"].clone()
            } else {
                Value::Null
            };
            task["pending_input"] =
                serde_json::to_value(self.store.pending_input(task["id"].as_str().unwrap())?)?;
        }
        Ok(
            json!({"tasks":tasks,"root":self.store.root.to_string_lossy(),"running":self.active.load(Ordering::SeqCst),"paused":self.pause.load(Ordering::SeqCst)||queue_paused,"download_queue":download_queue,"browsers":self.browser.states(app),"policy":PUSH_POLICY}),
        )
    }
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
        let raw = std::fs::read(path)?;
        let c = files::parse_wos(&raw)?;
        let mut t = self.store.task(id)?;
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
        if let Some(a) = t.artifact.as_ref() {
            let raw = std::fs::read(&a.path)?;
            if hash(&raw) != a.candidate.sha256 {
                return Err(Failure::new("FILE_INVALID", "本地存档发生变化。"));
            }
            return Ok(());
        }
        if self.store.recover_native_download(id)? {
            self.changed(app);
            return Ok(());
        }
        if let Some(a) = self.store.reusable_artifact(&t.record)? {
            let raw = std::fs::read(&a.path)?;
            if hash(&raw) != a.candidate.sha256 {
                return Err(Failure::new(
                    "FILE_INVALID",
                    "共享来源归档已变化，不能复用。",
                ));
            }
            self.attach(id, std::path::Path::new(&a.path), a.source, a.record_url)?;
            self.changed(app);
            return Ok(());
        }
        t.stage = Stage::Searching;
        t.running = true;
        t.last_error = None;
        self.store.save(&mut t, "search_started")?;
        self.changed(app);
        let payload = json!({"sa_id":t.id,"title":t.record.title,"doi":normalized_doi(&t.record.doi),"wos":normalized_wos(&t.record.wos)});
        let record = self.browser.search(app, payload.clone()).await?;
        if self.pause.load(Ordering::SeqCst) {
            t.running = false;
            t.stage = Stage::Pending;
            self.store.save(&mut t, "paused")?;
            return Ok(());
        }
        let url = record["record_url"]
            .as_str()
            .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "检索没有返回来源链接。"))?;
        t.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "database_search".into(),
            source: url.into(),
            text: format!("WOS 检索：{}", t.record.title),
            created: now(),
        });
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
    async fn execute_download_queue(&self, app: &AppHandle, id: &str) -> Result<()> {
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
    async fn download_queue_loop(&self, app: &AppHandle, queue_id: &str) -> Result<()> {
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
            let available = target.matches(&t.record)
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
                    "QUEUE_TARGET_CHANGED", "原队列的任务输入、跳过标记、业务阶段或待确认操作已变化；未检索或覆盖当前任务，请核对。")), None)?;
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
                    self.store
                        .finish_download_target(queue_id, id, "downloaded", None, None)?;
                }
                Err(error) => {
                    let mut t = self.store.task(id)?;
                    t.running = false;
                    t.last_error = Some(error.clone());
                    t.stage = Stage::AwaitingReview;
                    t.evidence.push(Evidence {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: "search_result".into(),
                        source: "WOS".into(),
                        text: json!({"error":error,"queue_id":queue_id,
                            "input_fingerprint":target.fingerprint,"target":t.record})
                        .to_string(),
                        created: now(),
                    });
                    if error.channel()
                        || matches!(error.code.as_str(), "REMOTE_RESULT_UNKNOWN" | "BUSY")
                    {
                        self.store
                            .block_download_queue(queue_id, error, Some(&mut t))?;
                        self.pause.store(true, Ordering::SeqCst);
                        break;
                    }
                    // Task failure and cursor advance commit together. A restart
                    // never re-runs already recorded zero/ambiguous outcomes.
                    self.store.finish_download_target(
                        queue_id,
                        id,
                        "review",
                        Some(error),
                        Some(&mut t),
                    )?;
                }
            }
            self.changed(app);
        }
        Ok(())
    }
    pub fn review(&self, id: &str, mut r: Review, source: String, proof: String) -> Result<Task> {
        let mut t = self.store.task(id)?;
        if self.store.pending_input(id)?.is_some()
            || t.last_error
                .as_ref()
                .map(|e| e.code == "INPUT_CHANGED")
                .unwrap_or(false)
        {
            return Err(Failure::new(
                "INPUT_CHANGED",
                "名单关键字段已变化，先核对任务版本，不能用旧名单保存核验。",
            ));
        }
        if matches!(t.stage, Stage::Unknown | Stage::Completed) {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "当前状态需先回读或已经完成。",
            ));
        }
        if source.trim().is_empty() || proof.trim().is_empty() {
            return Err(Failure::new(
                "EVIDENCE_REQUIRED",
                "填写核验来源和具体依据。",
            ));
        }
        let e = Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "human_review".into(),
            source,
            text: proof,
            created: now(),
        };
        r.evidence_id = e.id.clone();
        t.evidence.push(e);
        if matches!(r.route, Route::Missing | Route::CorrectedExisting) {
            library::review(&t, &r)?;
            r.library_checked = true;
        }
        if t.needs_issue_review() {
            r.issues_resolved = t
                .sa_snapshot
                .as_ref()
                .map(|s| issues::assert_resolved(&t, s).is_ok())
                .unwrap_or(false);
        }
        t.validate_review(&r)?;
        if let Some(a) = t.artifact.as_mut() {
            if r.identity_confirmed {
                files::verify_identity(&t.record, &a.candidate)?;
                a.identity_confirmed = true;
            }
        }
        t.route = r.route.clone();
        t.platform_id = r.platform_id.clone();
        t.review = Some(r);
        t.last_error = None;
        if !matches!(
            t.stage,
            Stage::Uploaded | Stage::Imported | Stage::Pushed | Stage::Claimed
        ) {
            t.stage = if t.route == Route::Missing && t.import_ready().is_ok() {
                Stage::Ready
            } else {
                Stage::AwaitingReview
            };
        }
        self.store.save(&mut t, "human_review")?;
        Ok(t)
    }
    pub async fn prepare_input_version(&self, app: &AppHandle, id: &str) -> Result<Value> {
        let proposal = self
            .store
            .pending_input(id)?
            .ok_or_else(|| Failure::new("INPUT_UNCHANGED", "没有待核验的新名单版本。"))?;
        let mut task = self.store.task(id)?;
        if !self.store.unresolved(id)?.is_empty() || task.stage == Stage::Unknown {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "先回读旧版本的待确认操作，不丢弃原执行记录。",
            ));
        }
        let snapshot = self.read_sa(app, &mut task).await?;
        library_core::versions::validate_live(&proposal, &snapshot)?;
        let result = json!({"proposal_id":proposal.id,"previous_fingerprint":task.record.fingerprint(),"snapshot":snapshot});
        self.store
            .set_setting(&format!("input-version:{id}"), result.clone())?;
        self.changed(app);
        Ok(result)
    }
    pub async fn accept_input_version(
        &self,
        app: &AppHandle,
        id: &str,
        proposal_id: &str,
        source: &str,
        proof: &str,
    ) -> Result<Task> {
        let proposal = self
            .store
            .pending_input(id)?
            .ok_or_else(|| Failure::new("INPUT_UNCHANGED", "没有待核验的新名单版本。"))?;
        let prepared = self
            .store
            .setting(&format!("input-version:{id}"))?
            .ok_or_else(|| {
                Failure::new("REVIEW_REQUIRED", "先读取并核对新名单与实时 SA 的差异。")
            })?;
        let mut task = self.store.task(id)?;
        if proposal.id != proposal_id
            || prepared["proposal_id"] != proposal_id
            || prepared["previous_fingerprint"] != task.record.fingerprint()
        {
            return Err(Failure::new(
                "INPUT_CHANGED",
                "名单版本在准备后变化，请重新读取差异。",
            ));
        }
        if !self.store.unresolved(id)?.is_empty() || task.stage == Stage::Unknown {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "旧版本仍有待确认操作，不能切换。",
            ));
        }
        let snapshot = self.read_sa(app, &mut task).await?;
        if snapshot != prepared["snapshot"] {
            return Err(Failure::new(
                "TASK_CHANGED",
                "平台字段在核对后变化，请重新读取新版本。",
            ));
        }
        let next = self
            .store
            .accept_input(&task, &proposal, &snapshot, source, proof)?;
        self.changed(app);
        Ok(next)
    }
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
    pub async fn step(
        &self,
        app: &AppHandle,
        id: &str,
        action: &str,
        approved: bool,
        extra: Value,
    ) -> Result<Value> {
        let mut t = self.store.task(id)?;
        if (self.store.pending_input(id)?.is_some()
            || t.last_error
                .as_ref()
                .map(|e| e.code == "INPUT_CHANGED")
                .unwrap_or(false))
            && (is_write(action) || action.starts_with("prepare_") || action == "review_issue")
        {
            return Err(Failure::new(
                "INPUT_CHANGED",
                "名单关键字段已变化，先处理任务版本，不能按旧名单写入。",
            ));
        }
        if is_write(action) && (t.stage == Stage::Unknown || !self.store.unresolved(id)?.is_empty())
        {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "本条有未确认结果，先核验原操作；不会重新准备或发送写入。",
            ));
        }
        if action == "library_search" {
            if t.stage == Stage::Completed {
                return Err(Failure::new("INVALID_TRANSITION", "任务已经完成。"));
            }
            let target = library::target(&t, extra["title"].as_str().unwrap_or(""))?;
            let result = self.search_library(app, &mut t, target).await?;
            self.changed(app);
            return Ok(result);
        }
        if ["prepare_issues", "review_issue"].contains(&action) {
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
        if action == "prepare_metadata" {
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
        if action == "verify_metadata" {
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
        if ["scan_duplicates", "prepare_duplicate"].contains(&action) {
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
                json!({"row":sa["row"],"result":result}),
            )?;
            self.changed(app);
            return Ok(result);
        }
        if action == "verify_duplicate" {
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
            let expected_ids = matched_ids(&p["expected_sa"])?;
            let source = p["source_id"].as_str().unwrap_or("");
            let target = p["target_id"].as_str().unwrap_or("");
            let mut expected_after: Vec<_> = expected_ids
                .iter()
                .filter(|i| i.as_str() != source)
                .cloned()
                .collect();
            expected_after.sort();
            let mut actual = ids.clone();
            actual.sort();
            if actual != expected_after
                || !ids.iter().any(|i| i == target)
                || sa["row"]["gh"] != p["expected_sa"]["gh"]
                || sa["row"]["titleValue"] != p["expected_sa"]["titleValue"]
            {
                return Err(Failure::new(
                    "REMOTE_RESULT_UNKNOWN",
                    "SA 条目集合尚未与本次合并结果一致，请在平台核对，不重复合并。",
                ));
            }
            let result=self.browser.execute(app,"duplicate","duplicate_check",json!({"sa_id":id,"title":p["title"],"source_id":p["source_id"],"target_id":p["target_id"],"expected_master":p["expected_master"],"expected_threshold":p["expected_threshold"],"sa_verified":true,"sa_ids":ids,"expected_sa_ids":expected_ids}),75).await?;
            Self::record_merge(&mut t, p, &result)?;
            t.stage = serde_json::from_value(p["previous_stage"].clone())?;
            t.running = false;
            t.last_error = None;
            self.store.verify_attempt(
                &mut t,
                attempts[0]["id"].as_str().unwrap_or(""),
                result.clone(),
                "duplicate_verified",
            )?;
            self.changed(app);
            return Ok(result);
        }
        if action == "prepare_alias" {
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
        if action == "verify_alias" {
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
        if action == "read_sa" {
            let d = self.read_sa(app, &mut t).await?;
            self.changed(app);
            return Ok(d);
        }
        if action == "search_wos" {
            self.download_one(app, id).await?;
            return Ok(json!({}));
        }
        if action == "verify_legacy_sa" {
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
        if action == "verify_sa" {
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
            let next = match action {
                "complete"
                    if d["row"]["markStatus"] == "已处理"
                        && d["row"]["remark"] == payload["note"] =>
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
                    Self::record_link_snapshot(&mut t, payload, &d)?;
                    serde_json::from_value(payload["previous_stage"].clone())?
                }
                "submit_claim" => {
                    let mut verify = payload.clone();
                    verify["expected"] = d["row"].clone();
                    let result = self
                        .browser
                        .execute(app, "sa", "verify_claim", verify, 65)
                        .await?;
                    if result["verified"] != true {
                        return Err(Failure::new(
                            "REMOTE_RESULT_UNKNOWN",
                            "尚未回读到认领成功。",
                        ));
                    }
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
                d.clone(),
                "sa_operation_verified",
            )?;
            self.changed(app);
            return Ok(d);
        }
        if action == "open_metadata" || action == "open_claim" || action == "prepare_claim" {
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
                self.store
                    .set_setting(&format!("claim:{id}"), result.clone())?;
            }
            self.changed(app);
            return Ok(result);
        }
        let a = t.artifact.as_ref();
        let common = json!({"sa_id":id,"instructions":format!("SA补充-{id}"),"candidate":a.map(|a|serde_json::to_value(&a.candidate).unwrap())});
        if action == "verify_import" {
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
                    let saved: Value =
                        serde_json::from_str(attempt["data"].as_str().unwrap_or("{}"))?;
                    saved["payload"].clone()
                } else {
                    let upload = checkpoint.as_ref().ok_or_else(|| {
                        Failure::new("REMOTE_RESULT_UNKNOWN", "缺少原上传回执，不能重新上传。")
                    })?;
                    if upload["uploaded"] != true
                        || upload["filename"] != format!("SA-WOS-{id}.txt")
                    {
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
                if result["uploaded"] == true {
                    validate_upload_readback(&result, &cmd, raw.len())?;
                    if checkpoint.as_ref().is_some_and(|previous| {
                        previous["uploaded"] == true
                            && previous["dataset_id"] != result["dataset_id"]
                    }) {
                        return Err(Failure::new(
                            "REMOTE_RESULT_UNKNOWN",
                            "所属机构与原上传回执不一致，不能重新上传。",
                        ));
                    }
                    t.stage = Stage::Uploaded;
                    t.batch = Some(result.clone());
                } else {
                    t.stage = verified_import_stage(&result, false)?;
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
            cmd["batch_id"] = original
                .get("batch")
                .or(t.batch.as_ref())
                .and_then(|v| v["id"].as_str())
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
                .execute(app, "import", "import_check", cmd, 65)
                .await?;
            if d["verified"] != true {
                return Err(Failure::new(
                    "REMOTE_RESULT_UNKNOWN",
                    "尚未核验到导入/推送成功。",
                ));
            }
            t.stage = verified_import_stage(&d, require_push)?;
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
                if sa["row"] != prepared["row"] {
                    return Err(Failure::new(
                        "TASK_CHANGED",
                        "SA 在读取详情后已变化，请重新准备合并。",
                    ));
                }
                let ids = matched_ids(&sa["row"])?;
                let source = extra["source_id"].as_str().unwrap_or("");
                let target = extra["target_id"].as_str().unwrap_or("");
                let evidence_id = extra["evidence_id"].as_str().unwrap_or("");
                let retained = extra["retained"].as_str().unwrap_or("");
                validate_merge_review(
                    &t,
                    source,
                    target,
                    &ids,
                    evidence_id,
                    extra["identity_confirmed"] == true,
                    retained,
                )?;
                let g = &prepared["result"]["group"];
                let rows = g["items"]
                    .as_array()
                    .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "候选详情格式未知。"))?;
                let target_row = rows
                    .iter()
                    .find(|r| r["id"] == target)
                    .ok_or_else(|| Failure::new("IDENTITY_CONFLICT", "主条目不在已核对候选中。"))?;
                let source_row = rows.iter().find(|r| r["id"] == source).ok_or_else(|| {
                    Failure::new("IDENTITY_CONFLICT", "被合并条目不在已核对候选中。")
                })?;
                let mut master = target_row.clone();
                let titles = rows_title_union(target_row, source_row)?;
                master["metadata"]["title"] = titles;
                let p = json!({"sa_id":id,"title":t.record.title,"group_id":g["id"],"source_id":source,"target_id":target,"expected_group":g,"expected_master":master,"expected_threshold":prepared["result"]["title_similarity"],"confirmed":true,"evidence_id":evidence_id,"retained":retained,"previous_stage":t.stage,"expected_sa":sa["row"]});
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
                if !prepared["prepared"]["authors"]
                    .as_array()
                    .map(|rows| {
                        rows.iter().any(|a| {
                            a["index"] == index && a["eligible"] == true && a["scholarId"] == ""
                        })
                    })
                    .unwrap_or(false)
                {
                    return Err(Failure::new("REVIEW_REQUIRED", "所选作者不可认领。"));
                }
                let p = json!({"sa_id":id,"confirmed":true,"expected":prepared["row"],"sa_text":prepared["prepared"]["sa_text"],"staff_id":prepared["prepared"]["staff_id"],"roster_staff_id":t.record.staff_id,"author_index":index,"prepared":prepared["prepared"]});
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
                    if let Err(error) = Self::record_merge(&mut t, &audit, &d) {
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
    fn record_merge(task: &mut Task, payload: &Value, result: &Value) -> Result<()> {
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
            text: json!({"payload":payload,"result":result}).to_string(),
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
    fn record_issue_plan(task: &mut Task, snapshot: &Value) -> Result<Value> {
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
    fn record_link_snapshot(task: &mut Task, payload: &Value, snapshot: &Value) -> Result<()> {
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
    async fn search_library(
        &self,
        app: &AppHandle,
        t: &mut Task,
        mut target: Value,
    ) -> Result<Value> {
        let expected = target.clone();
        target["sa_id"] = t.id.clone().into();
        let result = self
            .browser
            .execute(app, "library", "library_search", target, 150)
            .await?;
        #[cfg(feature = "smoke-test")]
        let result = {
            let mut result = result;
            if crate::browser::fixture_origin()
                .as_ref()
                .map(|origin| {
                    result["source"]
                        .as_str()
                        .map(|s| {
                            s.starts_with(&format!(
                                "{}/advancedSearch",
                                origin.origin().ascii_serialization()
                            ))
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(false)
            {
                result["fixture_source"] = result["source"].clone();
                result["source"] = "http://www.ir.lib.sjtu.edu.cn/advancedSearch".into();
            }
            result
        };
        library::record(t, &expected, &result)?;
        self.store.save(t, "library_search")?;
        Ok(result)
    }
    async fn record_metadata(
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
}
fn rows_title_union(target: &Value, source: &Value) -> Result<Value> {
    let mut titles = std::collections::BTreeSet::new();
    for row in [target, source] {
        for title in row["metadata"]["title"]
            .as_array()
            .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "候选题名格式未知。"))?
        {
            titles.insert(
                title
                    .as_str()
                    .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "候选题名不是文本。"))?
                    .to_string(),
            );
        }
    }
    Ok(json!(titles))
}
pub struct Lease(Arc<AtomicBool>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
