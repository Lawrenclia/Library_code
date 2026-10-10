//! Frozen download scope and durable progress. This queue never submits imports.
use crate::{now, Failure, Record, Result, Route, Stage, Store, Task};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueStatus {
    Running,
    Paused,
    Blocked,
    Interrupted,
    Completed,
    Cancelled,
}
impl QueueStatus {
    pub fn unfinished(&self) -> bool {
        !matches!(self, Self::Completed | Self::Cancelled)
    }
    fn key(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Blocked => "blocked",
            Self::Interrupted => "interrupted",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueTarget {
    pub id: String,
    pub fingerprint: String,
    pub skipped: bool,
    #[serde(default)]
    pub record: Option<Record>,
    #[serde(default)]
    pub input_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueOutcome {
    pub id: String,
    pub status: String,
    pub error: Option<Failure>,
    #[serde(default)]
    pub scope_error: Option<Failure>,
    pub finished: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadQueue {
    pub id: String,
    pub revision: u32,
    pub owner: String,
    pub retry_skipped: bool,
    pub targets: Vec<QueueTarget>,
    pub cursor: usize,
    pub outcomes: Vec<QueueOutcome>,
    pub status: QueueStatus,
    pub pause_requested: bool,
    pub last_error: Option<Failure>,
    pub created: u64,
    pub updated: u64,
}
impl QueueTarget {
    pub fn matches(&self, record: &Record) -> bool {
        self.id == record.sa_id
            && self.fingerprint == record.fingerprint()
            && self.skipped == record.skipped
    }
    /// Resume the original roster snapshot, including its accepted input version.
    /// Old targets without a recorded version remain audit history, never inferred scopes.
    pub fn matches_task(&self, task: &Task) -> bool {
        !self.input_hash.is_empty()
            && self.input_hash == task.input_hash
            && self.matches(&task.record)
    }
}
fn read_queue(db: &Connection, id: &str) -> Result<DownloadQueue> {
    let raw: Option<String> = db
        .query_row("SELECT data FROM download_queues WHERE id=?", [id], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(serde_json::from_str(&raw.ok_or_else(|| {
        Failure::new("QUEUE_MISSING", "原下载队列不存在，请刷新工作台。")
    })?)?)
}
fn pending_queue(db: &Connection) -> Result<Option<DownloadQueue>> {
    let raw: Option<String> = db.query_row(
        "SELECT data FROM download_queues WHERE status IN ('running','paused','blocked','interrupted')",
        [], |r| r.get(0),
    ).optional()?;
    raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}
fn save_queue(tx: &Transaction<'_>, queue: &mut DownloadQueue, kind: &str) -> Result<()> {
    let old = queue.revision;
    queue.revision += 1;
    queue.updated = now();
    if tx.execute(
        "UPDATE download_queues SET revision=?,status=?,data=? WHERE id=? AND revision=?",
        params![
            queue.revision,
            queue.status.key(),
            serde_json::to_string(queue)?,
            queue.id,
            old
        ],
    )? != 1
    {
        return Err(Failure::new(
            "QUEUE_CHANGED",
            "下载队列已变化，请刷新后继续。",
        ));
    }
    tx.execute(
        "INSERT INTO events(task_id,created,kind,data) VALUES('',?,?,?)",
        params![
            now() as i64,
            kind,
            json!({"queue_id":queue.id,"revision":queue.revision,"status":queue.status,
        "cursor":queue.cursor,"total":queue.targets.len(),"pause_requested":queue.pause_requested,
        "error":queue.last_error})
            .to_string()
        ],
    )?;
    Ok(())
}
fn current_target(queue: &DownloadQueue, id: &str) -> Result<()> {
    if queue.status != QueueStatus::Running
        || queue
            .targets
            .get(queue.cursor)
            .is_none_or(|target| target.id != id)
    {
        return Err(Failure::new(
            "QUEUE_CHANGED",
            "队列当前位置已变化，未覆盖原进度。",
        ));
    }
    Ok(())
}
fn guarded_task(tx: &Connection, queue: &DownloadQueue) -> Result<Task> {
    let target = queue
        .targets
        .get(queue.cursor)
        .ok_or_else(|| Failure::new("QUEUE_CHANGED", "队列原范围位置变化。"))?;
    let task: Task = serde_json::from_str(&tx.query_row::<String, _, _>(
        "SELECT data FROM tasks WHERE id=?",
        [&target.id],
        |r| r.get(0),
    )?)?;
    let fenced: u32 = tx.query_row(
        "SELECT (SELECT count(*) FROM pending_inputs WHERE task_id=?1)+(SELECT count(*) FROM attempts WHERE task_id=?1 AND state IN ('intent','unknown'))",
        [&target.id], |r| r.get(0),
    )?;
    if !target.matches_task(&task)
        || fenced > 0
        || task.record.done
        || task.record.matches != 0
        || task.record.owner != queue.owner
        || !matches!(task.route, Route::ZeroReview | Route::Missing)
        || !matches!(
            task.stage,
            Stage::Pending
                | Stage::Searching
                | Stage::Downloading
                | Stage::Downloaded
                | Stage::Ready
                | Stage::AwaitingReview
        )
        || task
            .last_error
            .as_ref()
            .is_some_and(|e| e.code == "INPUT_CHANGED")
    {
        return Err(Failure::new(
            "QUEUE_TARGET_CHANGED",
            "执行期间任务名单、业务分支或平台阶段变化；仅保留原队列结果，不覆盖当前任务。",
        ));
    }
    Ok(task)
}
fn save_failure(
    tx: &Transaction<'_>,
    queue: &DownloadQueue,
    proposed: &Task,
    error: &Failure,
) -> Result<Task> {
    current_target(queue, &proposed.id)?;
    let current = guarded_task(tx, queue)?;
    if proposed.revision != current.revision {
        return Err(Failure::new(
            "TASK_CHANGED",
            "失败回写前任务修订已变化，未覆盖任务或推进队列。",
        ));
    }
    // Failure may add search audit and show an error. It cannot modify roster,
    // artifact, classification, review, platform IDs, issue decisions or history.
    let mut preserved = proposed.clone();
    preserved.stage = current.stage.clone();
    preserved.running = current.running;
    preserved.last_error = current.last_error.clone();
    preserved.evidence = current.evidence.clone();
    let expected_stage = if matches!(current.stage, Stage::Downloaded | Stage::Ready) {
        current.stage.clone()
    } else {
        Stage::AwaitingReview
    };
    if serde_json::to_value(&preserved)? != serde_json::to_value(&current)?
        || proposed.running
        || proposed.stage != expected_stage
        || serde_json::to_value(&proposed.last_error)? != serde_json::to_value(Some(error))?
        || proposed.evidence.len() < current.evidence.len()
        || serde_json::to_value(&proposed.evidence[..current.evidence.len()])?
            != serde_json::to_value(&current.evidence)?
        || proposed.evidence[current.evidence.len()..]
            .iter()
            .any(|e| e.kind != "search_result")
    {
        return Err(Failure::new(
            "QUEUE_TARGET_CHANGED",
            "下载失败载荷包含其他任务状态变化，未覆盖当前任务。",
        ));
    }
    Store::write_task(tx, proposed, "search_failed")
}
fn advance(queue: &mut DownloadQueue, channel_blocked: bool) {
    queue.cursor += 1;
    queue.status = if queue.cursor == queue.targets.len() {
        QueueStatus::Completed
    } else if channel_blocked {
        QueueStatus::Blocked
    } else if queue.pause_requested {
        QueueStatus::Paused
    } else {
        QueueStatus::Running
    };
}
fn blocks_download_channel(error: &Failure) -> bool {
    error.channel() || matches!(error.code.as_str(), "REMOTE_RESULT_UNKNOWN" | "BUSY")
}
impl Store {
    /// A consistent read-only view for the task/source/queue Excel report.
    pub fn report_snapshot(&self) -> Result<(Vec<Task>, Vec<DownloadQueue>)> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut tasks: Vec<Task> = {
            let mut stmt = tx.prepare("SELECT data FROM tasks ORDER BY rowid")?;
            let raw = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let mut tasks = Vec::new();
            for row in raw {
                tasks.push(serde_json::from_str(&row?)?);
            }
            tasks
        };
        // Include failed/unconfirmed receipts too; an absent artifact must not
        // hide the original request, scope or owned path in the source report.
        for task in &mut tasks {
            let mut stmt =
                tx.prepare("SELECT data FROM native_downloads WHERE task_id=? ORDER BY rowid")?;
            let rows = stmt.query_map([&task.id], |r| r.get::<_, String>(0))?;
            for raw in rows {
                let raw = raw?;
                let receipt: crate::download::DownloadReceipt = serde_json::from_str(&raw)?;
                task.evidence.push(crate::Evidence {
                    id: format!("{}:receipt", receipt.id),
                    kind: "native_download_receipt".into(),
                    source: receipt.record_url,
                    text: raw,
                    created: receipt.finished.unwrap_or(receipt.created),
                });
            }
        }
        let queues = {
            let mut stmt = tx.prepare("SELECT data FROM download_queues ORDER BY rowid")?;
            let raw = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let mut queues = Vec::new();
            for row in raw {
                queues.push(serde_json::from_str(&row?)?);
            }
            queues
        };
        tx.commit()?;
        Ok((tasks, queues))
    }
    pub fn latest_download_queue(&self) -> Result<Option<DownloadQueue>> {
        let raw: Option<String> = self
            .connect()?
            .query_row(
                "SELECT data FROM download_queues ORDER BY rowid DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn download_queue(&self, id: &str) -> Result<DownloadQueue> {
        read_queue(&self.connect()?, id)
    }
    pub fn start_download_queue(&self, owner: &str, retry_skipped: bool) -> Result<DownloadQueue> {
        if owner.trim().is_empty() {
            return Err(Failure::new("INPUT_INVALID", "请选择负责人。"));
        }
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        if pending_queue(&tx)?.is_some() {
            return Err(Failure::new(
                "QUEUE_PENDING",
                "已有未结束的下载队列，请继续原队列，或先结束它再建立新范围。",
            ));
        }
        // Preserve roster insertion order, exact owner and skip scope. New input
        // rows imported later cannot silently join this run.
        let targets = {
            let mut stmt = tx.prepare("SELECT data FROM tasks ORDER BY rowid")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let mut targets = Vec::new();
            for row in rows {
                let t: Task = serde_json::from_str(&row?)?;
                if t.record.owner == owner
                    && t.record.matches == 0
                    && !t.record.done
                    && t.record.skipped == retry_skipped
                    && matches!(t.route, Route::ZeroReview | Route::Missing)
                    && matches!(
                        t.stage,
                        Stage::Pending
                            | Stage::Searching
                            | Stage::Downloading
                            | Stage::AwaitingReview
                    )
                {
                    targets.push(QueueTarget {
                        id: t.id,
                        fingerprint: t.record.fingerprint(),
                        skipped: t.record.skipped,
                        record: Some(t.record),
                        input_hash: t.input_hash,
                    });
                }
            }
            targets
        };
        if targets.is_empty() {
            return Err(Failure::new(
                "QUEUE_EMPTY",
                "当前负责人在此范围内没有待下载论文。",
            ));
        }
        let mut queue = DownloadQueue {
            id: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            owner: owner.into(),
            retry_skipped,
            targets,
            cursor: 0,
            outcomes: Vec::new(),
            status: QueueStatus::Running,
            pause_requested: false,
            last_error: None,
            created: now(),
            updated: now(),
        };
        tx.execute(
            "INSERT INTO download_queues VALUES(?,?,?,?,?)",
            params![
                queue.id,
                0,
                queue.status.key(),
                serde_json::to_string(&queue)?,
                queue.created as i64
            ],
        )?;
        save_queue(&tx, &mut queue, "queue_started")?;
        tx.commit()?;
        Ok(queue)
    }
    pub fn resume_download_queue(&self, id: &str) -> Result<DownloadQueue> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut queue = read_queue(&tx, id)?;
        if !matches!(
            queue.status,
            QueueStatus::Paused | QueueStatus::Blocked | QueueStatus::Interrupted
        ) {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "此下载队列不能继续，请刷新当前状态。",
            ));
        }
        queue.status = QueueStatus::Running;
        queue.pause_requested = false;
        queue.last_error = None;
        save_queue(&tx, &mut queue, "queue_resumed")?;
        tx.commit()?;
        Ok(queue)
    }
    pub fn request_download_pause(&self) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        if let Some(mut queue) = pending_queue(&tx)? {
            if queue.status == QueueStatus::Running && !queue.pause_requested {
                queue.pause_requested = true;
                save_queue(&tx, &mut queue, "queue_pause_requested")?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn pause_download_queue(&self, id: &str) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut queue = read_queue(&tx, id)?;
        if queue.status == QueueStatus::Running {
            queue.status = QueueStatus::Paused;
            queue.pause_requested = true;
            save_queue(&tx, &mut queue, "queue_paused")?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn cancel_download_queue(&self, id: &str) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut queue = read_queue(&tx, id)?;
        if !queue.status.unfinished() || queue.status == QueueStatus::Running {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "请先暂停正在运行的队列，已结束的队列不能再次取消。",
            ));
        }
        queue.status = QueueStatus::Cancelled;
        save_queue(&tx, &mut queue, "queue_cancelled")?;
        tx.commit()?;
        Ok(())
    }
    pub fn block_download_queue(
        &self,
        id: &str,
        error: Failure,
        task: Option<&mut Task>,
    ) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut queue = read_queue(&tx, id)?;
        if queue.status != QueueStatus::Running {
            return Err(Failure::new("QUEUE_CHANGED", "队列已停止，未改写原进度。"));
        }
        let saved = if let Some(t) = task.as_deref() {
            current_target(&queue, &t.id)?;
            Some(save_failure(&tx, &queue, t, &error)?)
        } else {
            None
        };
        queue.status = QueueStatus::Blocked;
        queue.last_error = Some(error);
        save_queue(&tx, &mut queue, "queue_blocked")?;
        tx.commit()?;
        if let (Some(t), Some(next)) = (task, saved) {
            *t = next;
        }
        Ok(())
    }
    pub fn finish_download_target(
        &self,
        id: &str,
        target_id: &str,
        status: &str,
        error: Option<Failure>,
        task: Option<&mut Task>,
    ) -> Result<()> {
        if !matches!(status, "downloaded" | "review" | "not_executed") {
            return Err(Failure::new("INPUT_INVALID", "未知队列结果。"));
        }
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut queue = read_queue(&tx, id)?;
        current_target(&queue, target_id)?;
        if status == "downloaded" {
            if error.is_some() || task.is_some() {
                return Err(Failure::new(
                    "FILE_INVALID",
                    "成功下载结果不能同时携带失败或替换任务载荷。",
                ));
            }
            let current = guarded_task(&tx, &queue)?;
            let pending: u32 = tx.query_row(
                "SELECT count(*) FROM native_downloads WHERE task_id=? AND state IN ('armed','requested','completed')",
                [target_id], |r| r.get(0),
            )?;
            if current.running
                || !matches!(
                    current.stage,
                    Stage::Downloaded | Stage::Ready | Stage::AwaitingReview
                )
                || current.artifact.is_none()
                || pending > 0
            {
                return Err(Failure::new(
                    "DOWNLOAD_RESULT_UNKNOWN",
                    "没有已归档并核验的原文件，或原下载仍待确认；未记成功或推进队列。",
                ));
            }
            crate::files::read_metadata_candidate(&current)?;
            if crate::files::verified_metadata_ids(&current)?.is_empty() {
                return Err(Failure::new(
                    "EVIDENCE_REQUIRED",
                    "原始文件缺少完整来源记录，未记下载成功。",
                ));
            }
        }
        if status == "review" && task.is_none() {
            guarded_task(&tx, &queue)?;
            if error.is_none() {
                return Err(Failure::new(
                    "INPUT_INVALID",
                    "待核验结果缺少实际检索错误。",
                ));
            }
        }
        let saved = if let Some(t) = task.as_deref() {
            if t.id != target_id {
                return Err(Failure::new("QUEUE_CHANGED", "队列与任务不一致。"));
            }
            if status != "review" {
                return Err(Failure::new(
                    "INPUT_INVALID",
                    "未执行结果不能回写失败任务。",
                ));
            }
            Some(save_failure(
                &tx,
                &queue,
                t,
                error
                    .as_ref()
                    .ok_or_else(|| Failure::new("INPUT_INVALID", "失败回写缺少实际错误。"))?,
            )?)
        } else {
            None
        };
        queue.outcomes.push(QueueOutcome {
            id: target_id.into(),
            status: status.into(),
            error,
            scope_error: None,
            finished: now(),
        });
        queue.last_error = None;
        advance(&mut queue, false);
        save_queue(&tx, &mut queue, "queue_target_finished")?;
        tx.commit()?;
        if let (Some(t), Some(next)) = (task, saved) {
            *t = next;
        }
        Ok(())
    }
    /// The live callback passes the original queue/target and actual error.
    /// Current business state is reread in this single transaction.
    pub fn record_download_failure(
        &self,
        id: &str,
        target_id: &str,
        error: Failure,
    ) -> Result<DownloadQueue> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut queue = read_queue(&tx, id)?;
        current_target(&queue, target_id)?;
        let target = queue
            .targets
            .get(queue.cursor)
            .cloned()
            .ok_or_else(|| Failure::new("QUEUE_CHANGED", "下载失败不对应原范围位置。"))?;
        let blocked = blocks_download_channel(&error);
        match guarded_task(&tx, &queue) {
            Ok(mut task) => {
                if !matches!(task.stage, Stage::Downloaded | Stage::Ready) {
                    task.stage = Stage::AwaitingReview;
                }
                task.running = false;
                task.last_error = Some(error.clone());
                task.evidence.push(crate::Evidence {
                    id: uuid::Uuid::new_v4().to_string(), kind: "search_result".into(), source: "WOS".into(),
                    text: json!({"schema":"download_failure_v1","queue_id":queue.id,"target":target,
                        "error":error,"current_revision":task.revision,"input_hash":task.input_hash}).to_string(),
                    created: now(),
                });
                save_failure(&tx, &queue, &task, &error)?;
                queue.last_error = if blocked { Some(error.clone()) } else { None };
                if blocked {
                    queue.status = QueueStatus::Blocked;
                } else {
                    queue.outcomes.push(QueueOutcome {
                        id: target.id,
                        status: "review".into(),
                        error: Some(error.clone()),
                        scope_error: None,
                        finished: now(),
                    });
                    advance(&mut queue, false);
                }
            }
            Err(scope) if scope.code == "QUEUE_TARGET_CHANGED" => {
                // The search may actually have been sent. This is not labelled
                // 'not executed', and both original error and scope conflict survive.
                queue.outcomes.push(QueueOutcome {
                    id: target.id,
                    status: "scope_changed".into(),
                    error: Some(error.clone()),
                    scope_error: Some(scope),
                    finished: now(),
                });
                queue.last_error = if blocked { Some(error.clone()) } else { None };
                advance(&mut queue, blocked);
            }
            Err(other) => return Err(other),
        }
        save_queue(&tx, &mut queue, "queue_failure_recorded")?;
        tx.commit()?;
        Ok(queue)
    }
    pub(crate) fn recover_download_queue(&self) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        if let Some(mut queue) = pending_queue(&tx)? {
            if queue.status == QueueStatus::Running {
                queue.status = if queue.pause_requested {
                    QueueStatus::Paused
                } else {
                    QueueStatus::Interrupted
                };
                queue.last_error = Some(Failure::new(
                    "PAUSED",
                    "应用退出前的下载队列已保留。请确认工作页后继续原范围。",
                ));
                save_queue(&tx, &mut queue, "queue_recovered")?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}
