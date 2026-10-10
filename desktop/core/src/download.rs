//! Durable correlation of native downloads, never a scan of Downloads.
use crate::*;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadReceipt {
    pub id: String,
    pub task_id: String,
    pub record: Record,
    pub input_hash: String,
    pub record_url: String,
    pub path: String,
    pub state: String,
    pub event_url: Option<String>,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
    pub created: u64,
    pub finished: Option<u64>,
}
fn read_owned(store: &Store, r: &DownloadReceipt) -> Result<Vec<u8>> {
    let path = Path::new(&r.path);
    let folder = store.root.join("downloads");
    if path != folder.join(format!("native-{}.txt", r.id))
        || std::fs::symlink_metadata(&folder)?.file_type().is_symlink()
        || std::fs::symlink_metadata(path)?.file_type().is_symlink()
        || !path.is_file()
    {
        return Err(Failure::new("FILE_INVALID", "原生回执文件路径已变化。"));
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let mut raw = Vec::new();
    (&file).take(512 * 1024 + 1).read_to_end(&mut raw)?;
    if raw.is_empty() || raw.len() > 512 * 1024 {
        return Err(Failure::new("FILE_INVALID", "下载文件为空或过大。"));
    }
    file.sync_all()?;
    Ok(raw)
}
impl Store {
    pub fn native_downloads(&self, task_id: &str) -> Result<Vec<DownloadReceipt>> {
        let db = self.connect()?;
        let mut q =
            db.prepare("SELECT data FROM native_downloads WHERE task_id=? ORDER BY rowid")?;
        let rows = q.query_map([task_id], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn prepare_native_download(
        &self,
        task: &Task,
        record_url: &str,
    ) -> Result<DownloadReceipt> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Task = serde_json::from_str(&tx.query_row::<String, _, _>(
            "SELECT data FROM tasks WHERE id=?",
            [&task.id],
            |r| r.get(0),
        )?)?;
        if current.revision != task.revision
            || current.stage != Stage::Downloading
            || !current.running
        {
            return Err(Failure::new("TASK_CHANGED", "下载任务已变化。"));
        }
        let pending: u32 = tx.query_row("SELECT count(*) FROM native_downloads WHERE task_id=? AND state IN ('armed','requested','completed')", [&task.id], |r| r.get(0))?;
        if pending > 0 {
            return Err(Failure::new(
                "DOWNLOAD_RESULT_UNKNOWN",
                "已有未核验的下载回执，请先恢复或手动核验原文件。",
            ));
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let path = self.root.join("downloads").join(format!("native-{id}.txt"));
        std::fs::create_dir_all(path.parent().unwrap())?;
        let r = DownloadReceipt {
            id,
            task_id: task.id.clone(),
            record: task.record.clone(),
            input_hash: task.input_hash.clone(),
            record_url: record_url.into(),
            path: path.to_string_lossy().into(),
            state: "armed".into(),
            event_url: None,
            sha256: None,
            bytes: None,
            created: now(),
            finished: None,
        };
        tx.execute(
            "INSERT INTO native_downloads VALUES(?,?,?,?)",
            params![r.id, r.task_id, r.state, serde_json::to_string(&r)?],
        )?;
        tx.commit()?;
        Ok(r)
    }
    pub fn request_native_download(&self, id: &str, event_url: &str) -> Result<DownloadReceipt> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut r: DownloadReceipt = serde_json::from_str(&tx.query_row::<String, _, _>(
            "SELECT data FROM native_downloads WHERE id=? AND state='armed'",
            [id],
            |r| r.get(0),
        )?)?;
        r.state = "requested".into();
        r.event_url = Some(event_url.into());
        tx.execute(
            "UPDATE native_downloads SET state=?,data=? WHERE id=?",
            params![r.state, serde_json::to_string(&r)?, id],
        )?;
        tx.commit()?;
        Ok(r)
    }
    /// Correlate after timeout too; a dropped oneshot is not a lost file receipt.
    pub fn finish_native_download(
        &self,
        event_url: &str,
        path: &Path,
        success: bool,
    ) -> Result<Option<DownloadReceipt>> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: Option<String> = tx.query_row("SELECT data FROM native_downloads WHERE state='requested' AND json_extract(data,'$.path')=? AND json_extract(data,'$.event_url')=?", params![path.to_string_lossy(), event_url], |r| r.get(0)).optional()?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        let mut r: DownloadReceipt = serde_json::from_str(&raw)?;
        if success {
            let raw = read_owned(self, &r)?;
            r.sha256 = Some(hash(&raw));
            r.bytes = Some(raw.len() as u64);
            r.state = "completed".into();
        } else {
            r.state = "failed".into();
        }
        r.finished = Some(now());
        tx.execute(
            "UPDATE native_downloads SET state=?,data=? WHERE id=?",
            params![r.state, serde_json::to_string(&r)?, r.id],
        )?;
        tx.commit()?;
        Ok(Some(r))
    }
    pub fn abandon_unrequested_download(&self, id: &str) -> Result<()> {
        self.connect()?.execute("UPDATE native_downloads SET state='abandoned',data=json_set(data,'$.state','abandoned') WHERE id=? AND state='armed'", [id])?;
        Ok(())
    }
    /// The task, paper, evidence and consumption of the receipt commit together.
    pub fn recover_native_download(&self, task_id: &str) -> Result<bool> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: Option<String> = tx.query_row("SELECT data FROM native_downloads WHERE task_id=? AND state IN ('requested','completed') ORDER BY rowid DESC LIMIT 1", [task_id], |r| r.get(0)).optional()?;
        let Some(raw) = raw else {
            return Ok(false);
        };
        let mut r: DownloadReceipt = serde_json::from_str(&raw)?;
        if r.state == "requested" {
            return Err(Failure::new(
                "DOWNLOAD_RESULT_UNKNOWN",
                format!(
                    "下载结束尚未确认，不自动重下。请核对并手动导入原始 TXT：{}",
                    r.path
                ),
            ));
        }
        let mut task: Task = serde_json::from_str(&tx.query_row::<String, _, _>(
            "SELECT data FROM tasks WHERE id=?",
            [task_id],
            |r| r.get(0),
        )?)?;
        let changes: u32 = tx.query_row("SELECT (SELECT count(*) FROM pending_inputs WHERE task_id=?)+(SELECT count(*) FROM attempts WHERE task_id=? AND state IN ('intent','unknown'))", params![task_id, task_id], |r| r.get(0))?;
        if changes > 0
            || task.record.fingerprint() != r.record.fingerprint()
            || task.input_hash != r.input_hash
            || task.record.skipped != r.record.skipped
            || task.record.done
            || task.record.matches != 0
            || !matches!(task.route, Route::ZeroReview | Route::Missing)
            || !matches!(
                task.stage,
                Stage::Downloading | Stage::AwaitingReview | Stage::Downloaded | Stage::Ready
            )
        {
            return Err(Failure::new(
                "TASK_CHANGED",
                "保留下载原文件和回执；任务身份、名单版本或执行阶段已变化，不自动覆盖。",
            ));
        }
        let raw = read_owned(self, &r)?;
        if r.sha256.as_deref() != Some(hash(&raw).as_str()) || r.bytes != Some(raw.len() as u64) {
            return Err(Failure::new(
                "FILE_INVALID",
                "下载原文件与完成回执哈希不一致。",
            ));
        }
        let candidate = files::parse_wos(&raw)?;
        let source = url::Url::parse(&r.record_url).map_err(Failure::storage)?;
        let source_id = source
            .path()
            .replace("%3A", ":")
            .replace("%3a", ":")
            .trim_end_matches('/')
            .trim_end_matches("(overlay:export/ext)")
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_owned();
        if normalized_wos(&source_id) != normalized_wos(&candidate.wos) {
            return Err(Failure::new(
                "IDENTITY_CONFLICT",
                "原始 TXT 与下载来源 WOS 记录不一致。",
            ));
        }
        let strong = files::verify_identity(&task.record, &candidate)?;
        if let Some(a) = &task.artifact {
            if a.candidate.sha256 != candidate.sha256 {
                return Err(Failure::new(
                    "IDENTITY_CONFLICT",
                    "已有归档与下载回执不是同一文件。",
                ));
            }
        } else {
            let archive = files::archive(&self.root, &raw)?;
            task.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "metadata".into(),
                source: r.record_url.clone(),
                text: serde_json::to_string_pretty(&candidate.fields)?,
                created: now(),
            });
            task.evidence.push(Evidence {
                id: r.id.clone(),
                kind: "native_download".into(),
                source: r.record_url.clone(),
                text: serde_json::to_string(&r)?,
                created: now(),
            });
            task.artifact = Some(Artifact {
                path: archive.to_string_lossy().into(),
                source: "WOS".into(),
                record_url: r.record_url.clone(),
                downloaded: r.finished.unwrap(),
                candidate,
                identity_confirmed: strong,
            });
            task.classification = None;
            task.stage = Stage::Downloaded;
            task.running = false;
            task.last_error = None;
            Self::write_task(&tx, &task, "native_download_recovered")?;
        }
        r.state = "adopted".into();
        tx.execute(
            "UPDATE native_downloads SET state=?,data=? WHERE id=?",
            params![r.state, serde_json::to_string(&r)?, r.id],
        )?;
        tx.commit()?;
        Ok(true)
    }
    pub(crate) fn recover_native_downloads(&self) -> Result<()> {
        self.connect()?.execute("UPDATE native_downloads SET state='abandoned',data=json_set(data,'$.state','abandoned') WHERE state='armed'", [])?;
        for mut task in self.tasks()? {
            if let Err(error) = self.recover_native_download(&task.id) {
                if task.artifact.is_none()
                    && matches!(task.stage, Stage::Downloading | Stage::AwaitingReview)
                    && self.pending_input(&task.id)?.is_none()
                    && self.unresolved(&task.id)?.is_empty()
                {
                    task.running = false;
                    task.last_error = Some(error);
                    task.stage = Stage::AwaitingReview;
                    self.save(&mut task, "native_download_needs_review")?;
                }
            }
        }
        Ok(())
    }
}
