//! Frozen download scope and durable progress. This queue never submits imports.
use crate::{now, Failure, Record, Result, Route, Stage, Store, Task};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
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
            Some(Self::write_task(&tx, t, "search_failed")?)
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
        let saved = if let Some(t) = task.as_deref() {
            if t.id != target_id {
                return Err(Failure::new("QUEUE_CHANGED", "队列与任务不一致。"));
            }
            Some(Self::write_task(&tx, t, "search_failed")?)
        } else {
            None
        };
        queue.outcomes.push(QueueOutcome {
            id: target_id.into(),
            status: status.into(),
            error,
            finished: now(),
        });
        queue.cursor += 1;
        queue.last_error = None;
        queue.status = if queue.cursor == queue.targets.len() {
            QueueStatus::Completed
        } else if queue.pause_requested {
            QueueStatus::Paused
        } else {
            QueueStatus::Running
        };
        save_queue(&tx, &mut queue, "queue_target_finished")?;
        tx.commit()?;
        if let (Some(t), Some(next)) = (task, saved) {
            *t = next;
        }
        Ok(())
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
