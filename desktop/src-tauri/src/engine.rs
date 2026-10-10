use crate::browser::Browser;
use library_core::{files, *};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    Arc,
};
use tauri::{AppHandle, Emitter};

pub struct Engine {
    _instance_lock: std::fs::File,
    pub store: Store,
    pub browser: Browser,
    pub active: Arc<AtomicBool>,
    active_kind: Arc<AtomicU8>,
    pub pause: Arc<AtomicBool>,
}
mod author_service;
mod downloads;
mod duplicate_service;
mod input_service;
mod library_service;
mod metadata_service;
mod review_service;
mod sa_service;
mod submission_service;
mod writes;

impl Engine {
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
            active_kind: Arc::new(AtomicU8::new(0)),
            pause: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn acquire(&self) -> Result<Lease> {
        self.acquire_kind(1)
    }
    pub fn acquire_ai(&self) -> Result<Lease> {
        self.acquire_kind(3)
    }
    pub fn acquire_download(&self) -> Result<Lease> {
        self.acquire_kind(2)
    }
    pub fn acquire_materials(&self) -> Result<Lease> {
        self.acquire_kind(4)
    }
    pub fn acquire_source_download(&self) -> Result<Lease> {
        self.acquire_kind(5)
    }
    fn acquire_kind(&self, kind: u8) -> Result<Lease> {
        if self
            .active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(Failure::new("BUSY", "已有任务运行，先暂停或等待完成。"));
        }
        self.pause.store(false, Ordering::SeqCst);
        self.active_kind.store(kind, Ordering::SeqCst);
        Ok(Lease(self.active.clone(), self.active_kind.clone()))
    }
    pub fn changed(&self, app: &AppHandle) {
        let _ = app.emit_to("main", "workspace-changed", json!({"time":now()}));
    }
    pub fn snapshot(&self, app: &AppHandle) -> Result<Value> {
        let download_queue = self.store.latest_download_queue()?;
        let ai_queue = self.store.latest_ai_queue()?;
        let material_batch = self.store.latest_material_batch()?;
        let running_service = match self.active_kind.load(Ordering::SeqCst) {
            1 => Some("general"),
            2 => Some("download"),
            3 => Some("ai"),
            4 => Some("materials"),
            5 => Some("source_download"),
            _ => None,
        };
        let queue_paused = download_queue
            .as_ref()
            .is_some_and(|q| q.status.unfinished() && q.status != queue::QueueStatus::Running);
        let stored_tasks = self.store.tasks()?;
        let mut tasks = serde_json::to_value(&stored_tasks)?;
        for (task, stored) in tasks.as_array_mut().unwrap().iter_mut().zip(&stored_tasks) {
            task["sa_note"] = sa::note_status(stored).unwrap_or(Value::Null);
            task["wos_searches"] = serde_json::to_value(
                self.store
                    .wos_search_traces(task["id"].as_str().unwrap_or(""))?,
            )?;
            let attempts = self.store.unresolved(task["id"].as_str().unwrap_or(""))?;
            task["pending_action"] = if attempts.len() == 1 {
                attempts[0]["action"].clone()
            } else {
                Value::Null
            };
            task["pending_input"] =
                serde_json::to_value(self.store.pending_input(task["id"].as_str().unwrap())?)?;
            task["legacy_claim_recovery"] = (attempts.len() == 1
                && ["legacy_claim", "legacy_sa"]
                    .contains(&attempts[0]["action"].as_str().unwrap_or(""))
                && serde_json::from_str::<Value>(attempts[0]["data"].as_str().unwrap_or("{}"))
                    .is_ok_and(|p| legacy::is_claim_checkpoint(&p["payload"])))
            .into();
        }
        Ok(
            json!({"tasks":tasks,"root":self.store.root.to_string_lossy(),"running":self.active.load(Ordering::SeqCst),"running_service":running_service,"paused":self.pause.load(Ordering::SeqCst)||queue_paused||material_batch.as_ref().is_some_and(|q|q.status.unfinished()&&q.status!=queue::QueueStatus::Running)||ai_queue.as_ref().is_some_and(|q|q.status.unfinished()&&q.status!=queue::QueueStatus::Running),"download_queue":download_queue,"ai_queue":ai_queue,"material_batch":material_batch,"browsers":self.browser.states(app),"policy":PUSH_POLICY,"framework":framework::manifest(),"legacy_materials":library_core::legacy_materials::summary(&self.store.legacy_snapshots()?)}),
        )
    }
    pub async fn step(
        &self,
        app: &AppHandle,
        id: &str,
        action: &str,
        approved: bool,
        extra: Value,
    ) -> Result<Value> {
        let operation = library_core::workflow::StepAction::parse(action)?;
        let t = self.store.task(id)?;
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
        use library_core::workflow::StepAction;
        match operation {
            StepAction::LibrarySearch => {
                self.handle_library_search(app, id, action, t, extra).await
            }
            StepAction::PrepareIssues | StepAction::ReviewIssue => {
                self.handle_issues(app, id, action, t, extra).await
            }
            StepAction::PrepareMetadata => {
                self.handle_prepare_metadata(app, id, action, t, extra)
                    .await
            }
            StepAction::VerifyMetadata => {
                self.handle_verify_metadata(app, id, action, t, extra).await
            }
            StepAction::ScanDuplicates | StepAction::PrepareDuplicate => {
                self.handle_prepare_duplicate(app, id, action, t, extra)
                    .await
            }
            StepAction::VerifyDuplicate => {
                self.handle_verify_duplicate(app, id, action, t, extra)
                    .await
            }
            StepAction::PrepareAlias => self.handle_prepare_alias(app, id, action, t, extra).await,
            StepAction::VerifyAlias => self.handle_verify_alias(app, id, action, t, extra).await,
            StepAction::ReadSa => self.handle_read_sa(app, id, action, t, extra).await,
            StepAction::PrepareNote | StepAction::VerifyNote => {
                self.handle_note(app, id, action, t, extra).await
            }
            StepAction::SearchWos => self.handle_search_wos(app, id, action, t, extra).await,
            StepAction::VerifyLegacySa => {
                self.handle_verify_legacy_sa(app, id, action, t, extra)
                    .await
            }
            StepAction::VerifyLegacyClaim => {
                self.handle_verify_legacy_claim(app, id, action, t, extra)
                    .await
            }
            StepAction::VerifySa => self.handle_verify_sa(app, id, action, t, extra).await,
            StepAction::OpenMetadata | StepAction::OpenClaim | StepAction::PrepareClaim => {
                self.handle_prepare_claim(app, id, action, t, extra).await
            }
            StepAction::VerifyImport => self.handle_verify_import(app, id, action, t, extra).await,
            StepAction::SaveMetadata
            | StepAction::MergeDuplicate
            | StepAction::AddAlias
            | StepAction::ImportUpload
            | StepAction::ImportSubmit
            | StepAction::ImportPush
            | StepAction::Link
            | StepAction::Complete
            | StepAction::SubmitClaim => {
                self.execute_write(app, id, action, approved, t, extra)
                    .await
            }
        }
    }
}
pub struct Lease(Arc<AtomicBool>, Arc<AtomicU8>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.1.store(0, Ordering::SeqCst);
        self.0.store(false, Ordering::SeqCst);
    }
}
