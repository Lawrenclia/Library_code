//! User-operated source browsers. Download receipts are not verified metadata.
use crate::*;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Site {
    pub channel: String,
    pub entry_url: String,
    pub download_origins: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub site: Site,
    pub record: Record,
    pub input_hash: String,
    pub created: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
    pub session: Session,
    pub page_url: String,
    pub event_url: String,
    pub original_name: String,
    pub format: String,
    pub path: String,
    pub state: String,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
    pub error: Option<Failure>,
    pub created: u64,
    pub finished: Option<u64>,
}
fn fail(message: &str) -> Failure {
    Failure::new("SOURCE_DOWNLOAD_INVALID", message)
}
fn web_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).map_err(|_| fail("请输入完整数据库网页地址。"))?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || matches!(
            url.host_str(),
            Some("localhost" | "127.0.0.1" | "tauri.localhost" | "[::1]")
        )
    {
        return Err(fail("数据库入口须为不含账号密码的外部 HTTP/HTTPS 网页。"));
    }
    Ok(url)
}
pub fn validate_site(mut site: Site) -> Result<Site> {
    source_files::channel_formats(&site.channel)?;
    let entry = web_url(&site.entry_url)?;
    let mut origins = vec![entry.origin().ascii_serialization()];
    for raw in &site.download_origins {
        let u = web_url(raw)?;
        if u.path() != "/" || u.query().is_some() || u.fragment().is_some() {
            return Err(fail(
                "下载来源只填写协议、域名和端口，不包含页面路径或查询参数。",
            ));
        }
        let origin = u.origin().ascii_serialization();
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }
    if origins.len() > 12 {
        return Err(fail("下载来源地址含入口最多配置 12 个。"));
    }
    site.entry_url = entry.to_string();
    site.download_origins = origins;
    Ok(site)
}
fn unchanged(task: &Task, session: &Session) -> Result<()> {
    if task.input_hash != session.input_hash
        || json!(task.record) != json!(session.record)
        || task.record.done
        || task.record.skipped
        || matches!(task.stage, Stage::Unknown | Stage::Completed)
    {
        return Err(fail(
            "来源窗口对应的名单已变化或任务不可新增来源，请重新选择当前任务打开。",
        ));
    }
    Ok(())
}
impl Store {
    pub fn source_sites(&self) -> Result<Vec<Site>> {
        Ok(serde_json::from_value(
            self.setting("source_sites")?.unwrap_or(json!([])),
        )?)
    }
    pub fn save_source_site(&self, site: Site) -> Result<Site> {
        let site = validate_site(site)?;
        let mut sites = self.source_sites()?;
        sites.retain(|s| s.channel != site.channel);
        sites.push(site.clone());
        self.set_setting("source_sites", json!(sites))?;
        Ok(site)
    }
    pub fn source_session(&self, id: &str, channel: &str) -> Result<Session> {
        let task = self.task(id)?;
        let site = self
            .source_sites()?
            .into_iter()
            .find(|s| s.channel == channel)
            .ok_or_else(|| fail("先配置该渠道实际数据库入口。"))?;
        let session = Session {
            id: uuid::Uuid::new_v4().simple().to_string(),
            site: validate_site(site)?,
            record: task.record.clone(),
            input_hash: task.input_hash.clone(),
            created: now(),
        };
        unchanged(&task, &session)?;
        if self.pending_input(id)?.is_some() || !self.unresolved(id)?.is_empty() {
            return Err(fail("先处理原名单版本或待确认操作。"));
        }
        Ok(session)
    }
    pub fn source_downloads(&self, id: &str) -> Result<Vec<Receipt>> {
        let db = self.connect()?;
        let mut q =
            db.prepare("SELECT data FROM source_downloads WHERE task_id=? ORDER BY rowid DESC")?;
        let rows = q.query_map([id], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn request_source_download(
        &self,
        session: &Session,
        page: &Url,
        event: &Url,
        name: &Path,
    ) -> Result<Receipt> {
        unchanged(&self.task(&session.record.sa_id)?, session)?;
        if self.pending_input(&session.record.sa_id)?.is_some()
            || !self.unresolved(&session.record.sa_id)?.is_empty()
        {
            return Err(fail("先处理待确认版本或平台操作，再下载新来源。"));
        }
        let site = validate_site(session.site.clone())?;
        let configured = self
            .source_sites()?
            .into_iter()
            .find(|s| s.channel == site.channel);
        if !configured.is_some_and(|current| json!(current) == json!(site)) {
            return Err(fail(
                "数据库入口或下载来源配置已变化，请按已保存配置重新打开本篇窗口。",
            ));
        }
        let page = web_url(page.as_str())?;
        let event_origin = if event.scheme() == "blob" {
            Url::parse(&event.as_str()[5..])
                .map_err(|_| fail("下载来源无法确认。"))?
                .origin()
                .ascii_serialization()
        } else {
            web_url(event.as_str())?.origin().ascii_serialization()
        };
        if !site
            .download_origins
            .contains(&page.origin().ascii_serialization())
            || !site.download_origins.contains(&event_origin)
        {
            return Err(fail("页面或下载域名没有登记，请配置实际下载来源后继续。"));
        }
        let format = name
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !source_files::channel_formats(&site.channel)?.contains(&format) {
            return Err(fail("原始文件格式不符合所选渠道。"));
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let folder = self.root.join("downloads").join("sources");
        fs::create_dir_all(&folder)?;
        if !folder
            .canonicalize()?
            .starts_with(self.root.canonicalize()?)
        {
            return Err(fail("来源下载目录已被重定向。"));
        }
        let r = Receipt {
            id: id.clone(),
            session: session.clone(),
            page_url: page.to_string(),
            event_url: event.to_string(),
            original_name: name
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            format: format.clone(),
            path: folder
                .join(format!("{id}.{format}"))
                .to_string_lossy()
                .into(),
            state: "requested".into(),
            sha256: None,
            bytes: None,
            error: None,
            created: now(),
            finished: None,
        };
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task: Task = serde_json::from_str(&tx.query_row::<String, _, _>(
            "SELECT data FROM tasks WHERE id=?",
            [&session.record.sa_id],
            |row| row.get(0),
        )?)?;
        unchanged(&task, session)?;
        let blocked: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pending_inputs WHERE task_id=?) OR EXISTS(SELECT 1 FROM attempts WHERE task_id=? AND state IN ('intent','unknown'))",
            params![session.record.sa_id,session.record.sa_id],|row|row.get(0))?;
        if blocked {
            return Err(fail("名单版本或平台操作已变化，请先核对再下载。"));
        }
        tx.execute(
            "INSERT INTO source_downloads VALUES(?,?,?,?)",
            params![
                r.id,
                session.record.sa_id,
                r.state,
                serde_json::to_string(&r)?
            ],
        )?;
        task.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "source_download".into(),
            source: r.page_url.clone(),
            text: serde_json::to_string(&r)?,
            created: now(),
        });
        Self::write_task(&tx, &task, "source_download_requested")?;
        tx.commit()?;
        Ok(r)
    }
    pub fn finish_source_download(
        &self,
        session_id: &str,
        event: &Url,
        path: Option<&Path>,
        success: bool,
    ) -> Result<Option<Receipt>> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut q = tx.prepare("SELECT data FROM source_downloads WHERE state='requested' AND json_extract(data,'$.session.id')=? AND json_extract(data,'$.event_url')=?")?;
        let rows = q.query_map(params![session_id, event.as_str()], |r| {
            r.get::<_, String>(0)
        })?;
        let mut candidates = Vec::new();
        for raw in rows {
            let r: Receipt = serde_json::from_str(&raw?)?;
            if path.is_none_or(|p| p == Path::new(&r.path)) {
                candidates.push(r);
            }
        }
        drop(q);
        if candidates.is_empty() {
            return Ok(None);
        }
        if candidates.len() != 1 {
            return Err(fail("下载结束事件无法唯一对应文件，保留待确认。"));
        }
        let mut receipt = candidates.remove(0);
        let result = if success && path.is_some() {
            self.source_download_bytes(&receipt, false)
        } else {
            Err(fail("浏览器未确认文件下载完成。"))
        };
        match result {
            Ok(bytes) => {
                receipt.state = "completed".into();
                receipt.sha256 = Some(hash(&bytes));
                receipt.bytes = Some(bytes.len() as u64);
            }
            Err(error) => {
                receipt.state = "failed".into();
                receipt.error = Some(error);
            }
        }
        receipt.finished = Some(now());
        tx.execute(
            "UPDATE source_downloads SET state=?,data=? WHERE id=?",
            params![receipt.state, serde_json::to_string(&receipt)?, receipt.id],
        )?;
        let raw: Option<String> = tx
            .query_row(
                "SELECT data FROM tasks WHERE id=?",
                [&receipt.session.record.sa_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(raw) = raw {
            let mut task: Task = serde_json::from_str(&raw)?;
            task.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "source_download".into(),
                source: receipt.page_url.clone(),
                text: serde_json::to_string(&receipt)?,
                created: now(),
            });
            Self::write_task(&tx, &task, "source_download_recorded")?;
        }
        tx.commit()?;
        Ok(Some(receipt))
    }
    fn source_download_bytes(&self, receipt: &Receipt, completed: bool) -> Result<Vec<u8>> {
        if receipt.id.len() != 32
            || !receipt.id.bytes().all(|b| b.is_ascii_hexdigit())
            || !source_files::channel_formats(&receipt.session.site.channel)?
                .contains(&receipt.format)
        {
            return Err(fail("来源下载回执编号或格式无效。"));
        }
        let folder = self.root.join("downloads").join("sources");
        let path = Path::new(&receipt.path);
        if path != folder.join(format!("{}.{}", receipt.id, receipt.format))
            || !folder
                .canonicalize()?
                .starts_with(self.root.canonicalize()?)
        {
            return Err(fail("来源下载路径不属于原回执。"));
        }
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_file()
            || meta.file_type().is_symlink()
            || meta.len() == 0
            || meta.len() > source_files::MAX_BYTES as u64
        {
            return Err(fail("原始文件为空、过大或路径被改变。"));
        }
        let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
        let mut bytes = Vec::new();
        (&file)
            .take(source_files::MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        file.sync_all()?;
        if bytes.is_empty()
            || bytes.len() > source_files::MAX_BYTES
            || file.metadata()?.len() != bytes.len() as u64
            || (completed
                && (receipt.sha256.as_deref() != Some(hash(&bytes).as_str())
                    || receipt.bytes != Some(bytes.len() as u64)))
        {
            return Err(fail("原始下载内容与已保存回执不同。"));
        }
        Ok(bytes)
    }
    pub fn source_download_for_preview(&self, id: &str, download_id: &str) -> Result<Receipt> {
        let receipt = self
            .source_downloads(id)?
            .into_iter()
            .find(|r| r.id == download_id)
            .ok_or_else(|| fail("下载记录不属于本条任务。"))?;
        if receipt.state != "completed" {
            return Err(fail("本次原生下载未确认完成，不会通过扫描文件夹推断成功。"));
        }
        unchanged(&self.task(id)?, &receipt.session)?;
        self.source_download_bytes(&receipt, true)?;
        Ok(receipt)
    }
    pub fn interrupt_source_downloads(&self) -> Result<()> {
        self.interrupt_source_downloads_inner(
            None,
            "重启前没有下载结束回执，不能扫描目录推断成功。请先核对原文件。",
        )
    }
    pub fn interrupt_source_download_ids(&self, ids: &[String]) -> Result<()> {
        self.interrupt_source_downloads_inner(
            Some(ids),
            "来源窗口已关闭，尚无文件下载完成回执；保留原请求与文件，不自动重下或推断成功。",
        )
    }
    fn interrupt_source_downloads_inner(
        &self,
        ids: Option<&[String]>,
        message: &str,
    ) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut q = tx.prepare("SELECT data FROM source_downloads WHERE state='requested'")?;
        let raws = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(q);
        for raw in raws {
            let mut receipt: Receipt = serde_json::from_str(&raw)?;
            if ids.is_some_and(|ids| !ids.contains(&receipt.id)) {
                continue;
            }
            receipt.state = "interrupted".into();
            receipt.error = Some(fail(message));
            tx.execute(
                "UPDATE source_downloads SET state=?,data=? WHERE id=?",
                params![receipt.state, serde_json::to_string(&receipt)?, receipt.id],
            )?;
            let raw: Option<String> = tx
                .query_row(
                    "SELECT data FROM tasks WHERE id=?",
                    [&receipt.session.record.sa_id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(raw) = raw {
                let mut task: Task = serde_json::from_str(&raw)?;
                task.evidence.push(Evidence {
                    id: uuid::Uuid::new_v4().to_string(),
                    kind: "source_download".into(),
                    source: receipt.page_url.clone(),
                    text: serde_json::to_string(&receipt)?,
                    created: now(),
                });
                Self::write_task(&tx, &task, "source_download_interrupted")?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}
