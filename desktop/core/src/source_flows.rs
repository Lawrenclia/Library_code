//! Database acquisition recipes and durable native-export checkpoints.
//! Page metadata is factual input, never a database-original upload file.
use crate::*;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{io::Write, path::Path};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub channel: String,
    #[serde(default)]
    pub search_url: String,
    #[serde(default)]
    pub input: String,
    #[serde(default)]
    pub search_button: String,
    #[serde(default)]
    pub result_rows: String,
    #[serde(default)]
    pub result_title: String,
    #[serde(default)]
    pub result_link: String,
    #[serde(default)]
    pub result_total: String,
    #[serde(default)]
    pub empty_result: String,
    #[serde(default)]
    pub detail_title: String,
    #[serde(default)]
    pub export_steps: Vec<ExportStep>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportStep {
    pub selector: String,
    pub action: String,
    pub text: String,
}
fn invalid(message: &str) -> Failure {
    Failure::new("SOURCE_FLOW_INVALID", message)
}
pub fn allowed(site: &source_downloads::Site, raw: &str) -> Result<Url> {
    let u = Url::parse(raw).map_err(|_| invalid("来源地址无效。"))?;
    if !matches!(u.scheme(), "http" | "https")
        || !u.username().is_empty()
        || u.password().is_some()
        || !site
            .download_origins
            .contains(&u.origin().ascii_serialization())
    {
        return Err(invalid("来源页不在该渠道保存的入口域名范围内。"));
    }
    Ok(u)
}
pub fn validate(flow: Flow, site: &source_downloads::Site) -> Result<Flow> {
    if flow.channel != site.channel || flow.export_steps.len() > 12 {
        return Err(invalid("流程渠道不符或导出步骤超过 12 个。"));
    }
    for s in [
        &flow.input,
        &flow.search_button,
        &flow.result_rows,
        &flow.result_title,
        &flow.result_link,
        &flow.result_total,
        &flow.empty_result,
        &flow.detail_title,
    ] {
        if s.len() > 500 || s.contains('\0') {
            return Err(invalid("页面定位条件过长或无效。"));
        }
    }
    if !flow.search_url.is_empty() {
        if flow.search_url.len() > 4000 || !flow.search_url.contains("{title}") {
            return Err(invalid("检索地址必须保留 {title} 题名占位符。"));
        }
        allowed(
            site,
            &flow
                .search_url
                .replace("{title}", "paper")
                .replace("{doi}", "doi"),
        )?;
    }
    for s in &flow.export_steps {
        if !["click", "select", "check"].contains(&s.action.as_str())
            || s.selector.trim().is_empty()
            || s.selector.len() > 500
            || s.text.trim().is_empty()
            || s.text.len() > 200
            || s.selector.contains('\0')
            || s.text.contains('\0')
        {
            return Err(invalid("导出步骤须有定位条件、操作与页面实际显示的文字。"));
        }
    }
    Ok(flow)
}
pub fn search_url(flow: &Flow, site: &source_downloads::Site, task: &Task) -> Result<Url> {
    let encode = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    allowed(
        site,
        &flow
            .search_url
            .replace("{title}", &encode(&task.record.title))
            .replace("{doi}", &encode(&task.record.doi)),
    )
}
pub fn select_result(site: &source_downloads::Site, task: &Task, data: &Value) -> Result<Url> {
    allowed(
        site,
        data["page_url"]
            .as_str()
            .ok_or_else(|| invalid("缺少检索页地址。"))?,
    )?;
    let records = data["records"]
        .as_array()
        .ok_or_else(|| invalid("缺少完整检索结果。"))?;
    if data["query"].as_str().map(norm) != Some(norm(&task.record.title))
        || data["complete"] != true
        || data["total"].as_u64() != Some(records.len() as u64)
        || records.len() > 200
    {
        return Err(Failure::new(
            "SOURCE_SELECTION_REQUIRED",
            "无法确认完整结果范围，请缩小检索或进入已核对的单篇详情。",
        ));
    }
    if records.is_empty() {
        return Err(Failure::new(
            "SOURCE_NOT_FOUND",
            "本次渠道检索没有记录；未据此判断未发表、非交大或本库缺失。",
        ));
    }
    let mut exact = vec![];
    let mut urls = std::collections::BTreeSet::new();
    for r in records {
        let title = r["title"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| invalid("结果缺少题名。"))?;
        let u = allowed(
            site,
            r["url"]
                .as_str()
                .ok_or_else(|| invalid("结果缺少详情地址。"))?,
        )?;
        if !urls.insert(u.to_string()) {
            return Err(invalid("结果重复，不能证明唯一论文。"));
        }
        if norm(title) == norm(&task.record.title) {
            exact.push(u);
        }
    }
    if exact.len() != 1 {
        return Err(Failure::new(
            "SOURCE_SELECTION_REQUIRED",
            "没有唯一精确题名匹配，不自动选择首篇或相似论文。",
        ));
    }
    Ok(exact.remove(0))
}
impl Store {
    pub fn record_source_search(&self, task: &Task, channel: &str, data: &Value) -> Result<()> {
        let mut current = self.task(&task.id)?;
        if current.input_hash != task.input_hash || json!(current.record) != json!(task.record) {
            return Err(Failure::new(
                "INPUT_CHANGED",
                "检索期间名单变化，保留原任务。",
            ));
        }
        current.evidence.push(Evidence{id:uuid::Uuid::new_v4().to_string(),kind:"source_search".into(),source:data["page_url"].as_str().unwrap_or("").into(),text:json!({"schema":"database_search_v1","channel":channel,"input_hash":task.input_hash,"record":task.record,"results":data,"time":now()}).to_string(),created:now()});
        self.save(&mut current, "source_search_recorded")?;
        Ok(())
    }
    pub fn remember_source_capture(
        &self,
        session: &source_downloads::Session,
        draft: &source_files::Draft,
        acquisition: &Value,
    ) -> Result<Value> {
        let op = json!({"schema":"source_acquisition_v1","id":uuid::Uuid::new_v4().simple().to_string(),"task_id":session.record.sa_id,"session":session,"state":"captured","draft":draft,"acquisition":acquisition,"created":now(),"error":null});
        self.connect()?.execute(
            "INSERT INTO source_acquisitions VALUES(?,?,?,?)",
            params![
                op["id"].as_str(),
                session.record.sa_id,
                "captured",
                op.to_string()
            ],
        )?;
        Ok(op)
    }
    pub fn captured_source(
        &self,
        id: &str,
        operation_id: &str,
    ) -> Result<(source_files::Draft, Value)> {
        let task = self.task(id)?;
        let op = self
            .source_acquisitions(id)?
            .into_iter()
            .find(|o| o["id"] == operation_id && o["state"] == "captured")
            .ok_or_else(|| invalid("缺少本篇已保存详情来源。"))?;
        let original: source_files::Draft = serde_json::from_value(op["draft"].clone())?;
        if original.input_hash != task.input_hash
            || original.record_fingerprint != task.record.fingerprint()
            || op["session"]["record"] != json!(task.record)
        {
            return Err(Failure::new(
                "INPUT_CHANGED",
                "详情来源属于旧名单记录，不能直接绑定当前版本。",
            ));
        }
        let mut draft = source_files::prepare(
            &self.root,
            &task,
            &original.channel,
            Path::new(&original.path),
            original.options.clone(),
        )?;
        draft.original_name = original.original_name;
        if draft.sha256 != original.sha256 {
            return Err(invalid("已保存详情来源文件字节变化。"));
        }
        Ok((draft, op["acquisition"].clone()))
    }
    pub fn source_queue(&self) -> Result<Option<Value>> {
        self.setting("source_queue")
    }
    pub fn source_queues(&self) -> Result<Vec<Value>> {
        let db = self.connect()?;
        let mut q = db.prepare("SELECT data FROM source_queue_runs ORDER BY rowid DESC")?;
        let rows = q.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn save_source_queue_progress(&self, q: &mut Value) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Value = serde_json::from_str(&tx.query_row::<String, _, _>(
            "SELECT data FROM settings WHERE key='source_queue'",
            [],
            |r| r.get(0),
        )?)?;
        if old["id"] != q["id"] {
            return Err(Failure::new("INPUT_CHANGED", "来源队列版本变化。"));
        }
        if old["pause_requested"] == true {
            q["pause_requested"] = json!(true);
        }
        tx.execute("INSERT INTO source_queue_runs VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET state=excluded.state,data=excluded.data",params![q["id"].as_str(),q["status"].as_str(),q.to_string()])?;
        tx.execute(
            "UPDATE settings SET data=? WHERE key='source_queue'",
            [q.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn create_source_queue(&self, owner: &str, channel: &str) -> Result<Value> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT data FROM settings WHERE key='source_queue'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if prior
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .is_some_and(|q| {
                ["running", "paused", "blocked", "interrupted"]
                    .contains(&q["status"].as_str().unwrap_or(""))
            })
        {
            return Err(Failure::new(
                "BUSY",
                "有未完成的来源队列，请先继续或取消原队列。",
            ));
        }
        let mut entries = vec![];
        for task in self.tasks()? {
            if task.record.owner == owner
                && !task.record.done
                && !task.record.skipped
                && task.stage != Stage::Completed
                && task.stage != Stage::Unknown
                && self.pending_input(&task.id)?.is_none()
                && self.unresolved(&task.id)?.is_empty()
            {
                let session = self.source_session(&task.id, channel)?;
                entries.push(json!({"id":task.id,"input_hash":task.input_hash,"record":task.record,"site":session.site,"flow":self.source_flow(&session.site)?}));
            }
        }
        if entries.is_empty() {
            return Err(invalid("当前负责人没有可获取来源的未处理任务。"));
        }
        let q = json!({"id":uuid::Uuid::new_v4().simple().to_string(),"owner":owner,"channel":channel,"entries":entries,"index":0,"results":[],"status":"running","pause_requested":false,"created":now()});
        tx.execute(
            "INSERT INTO source_queue_runs VALUES(?,?,?)",
            params![q["id"].as_str(), "running", q.to_string()],
        )?;
        tx.execute("INSERT INTO settings VALUES('source_queue',?) ON CONFLICT(key) DO UPDATE SET data=excluded.data",[q.to_string()])?;
        tx.commit()?;
        Ok(q)
    }
    pub fn source_queue_control(&self, action: &str) -> Result<Value> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut q: Value = serde_json::from_str(&tx.query_row::<String, _, _>(
            "SELECT data FROM settings WHERE key='source_queue'",
            [],
            |r| r.get(0),
        )?)?;
        match action {
            "pause" if q["status"] == "running" => q["pause_requested"] = json!(true),
            "resume"
                if ["paused", "blocked", "interrupted"]
                    .contains(&q["status"].as_str().unwrap_or("")) =>
            {
                q["status"] = json!("running");
                q["pause_requested"] = json!(false);
            }
            "cancel" if q["status"] != "running" => q["status"] = json!("cancelled"),
            _ => {
                return Err(Failure::new(
                    "INVALID_TRANSITION",
                    "先等待来源队列暂停，再继续或取消。",
                ))
            }
        }
        tx.execute("INSERT INTO source_queue_runs VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET state=excluded.state,data=excluded.data",params![q["id"].as_str(),q["status"].as_str(),q.to_string()])?;
        tx.execute(
            "UPDATE settings SET data=? WHERE key='source_queue'",
            [q.to_string()],
        )?;
        tx.commit()?;
        Ok(q)
    }
    pub fn source_flows(&self) -> Result<Vec<Flow>> {
        Ok(serde_json::from_value(
            self.setting("source_flows")?.unwrap_or(json!([])),
        )?)
    }
    pub fn source_flow(&self, site: &source_downloads::Site) -> Result<Flow> {
        validate(
            self.source_flows()?
                .into_iter()
                .find(|f| f.channel == site.channel)
                .unwrap_or(Flow {
                    channel: site.channel.clone(),
                    ..Default::default()
                }),
            site,
        )
    }
    pub fn save_source_flow(&self, flow: Flow) -> Result<Flow> {
        let site = self
            .source_sites()?
            .into_iter()
            .find(|s| s.channel == flow.channel)
            .ok_or_else(|| invalid("先保存本渠道入口。"))?;
        let flow = validate(flow, &site)?;
        let mut all = self.source_flows()?;
        all.retain(|f| f.channel != flow.channel);
        all.push(flow.clone());
        self.set_setting("source_flows", json!(all))?;
        Ok(flow)
    }
    pub fn source_acquisitions(&self, id: &str) -> Result<Vec<Value>> {
        let db = self.connect()?;
        let mut q =
            db.prepare("SELECT data FROM source_acquisitions WHERE task_id=? ORDER BY rowid DESC")?;
        let rows = q.query_map([id], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn arm_source_export(
        &self,
        session: &source_downloads::Session,
        flow: &Flow,
        page: &Value,
    ) -> Result<Value> {
        let current = self.source_session(&session.record.sa_id, &session.site.channel)?;
        if json!(current.site) != json!(session.site)
            || json!(current.record) != json!(session.record)
            || current.input_hash != session.input_hash
            || json!(self.source_flow(&session.site)?) != json!(flow)
        {
            return Err(invalid("任务或渠道配置变化，未导出。"));
        }
        validate_page(&session.site, &self.task(&session.record.sa_id)?, page)?;
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM source_acquisitions WHERE task_id=? AND json_extract(data,'$.session.site.channel')=? AND state IN ('export_pending','downloading','unknown'))",params![session.record.sa_id,session.site.channel],|r|r.get(0))?;
        if exists {
            return Err(Failure::new(
                "SOURCE_EXPORT_UNKNOWN",
                "本渠道有未确认原导出；请先检查原下载结果，不能重新点击导出。",
            ));
        }
        let op = json!({"schema":"source_acquisition_v1","id":uuid::Uuid::new_v4().simple().to_string(),"task_id":session.record.sa_id,"session":session,"flow":flow,"page":page,"state":"export_pending","created":now(),"receipt_id":null,"error":null});
        tx.execute(
            "INSERT INTO source_acquisitions VALUES(?,?,?,?)",
            params![
                op["id"].as_str(),
                session.record.sa_id,
                "export_pending",
                op.to_string()
            ],
        )?;
        tx.commit()?;
        Ok(op)
    }
    pub fn source_export_update(
        &self,
        id: &str,
        state: &str,
        receipt: Option<&source_downloads::Receipt>,
        error: Option<&Failure>,
    ) -> Result<Value> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut op: Value = serde_json::from_str(&tx.query_row::<String, _, _>(
            "SELECT data FROM source_acquisitions WHERE id=?",
            [id],
            |r| r.get(0),
        )?)?;
        if op["state"] == "completed" {
            return Ok(op);
        }
        if let Some(r) = receipt {
            if op["session"]["id"] != r.session.id
                || op["session"]["record"] != json!(r.session.record)
                || op["session"]["site"] != json!(r.session.site)
                || op["receipt_id"].as_str().is_some_and(|old| old != r.id)
            {
                return Err(invalid("原导出与下载回执不对应。"));
            }
            op["receipt_id"] = json!(r.id);
            op["receipt"] = json!(r);
        }
        if !["downloading", "completed", "unknown", "failed"].contains(&state) {
            return Err(invalid("导出状态无效。"));
        }
        op["state"] = json!(state);
        op["error"] = json!(error);
        op["updated"] = json!(now());
        tx.execute(
            "UPDATE source_acquisitions SET state=?,data=? WHERE id=?",
            params![state, op.to_string(), id],
        )?;
        let task_id = op["task_id"]
            .as_str()
            .ok_or_else(|| invalid("缺少原导出任务。"))?;
        let mut task: Task = serde_json::from_str(&tx.query_row::<String, _, _>(
            "SELECT data FROM tasks WHERE id=?",
            [task_id],
            |r| r.get(0),
        )?)?;
        task.evidence.push(Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "source_acquisition".into(),
            source: op["page"]["page_url"].as_str().unwrap_or("").into(),
            text: op.to_string(),
            created: now(),
        });
        Self::write_task(&tx, &task, "source_acquisition_recorded")?;
        tx.commit()?;
        Ok(op)
    }
    pub fn recover_source_exports(&self) -> Result<()> {
        if let Some(mut q) = self.source_queue()? {
            if q["status"] == "running" {
                q["status"] = json!("interrupted");
                self.save_source_queue_progress(&mut q)?;
            }
        }
        let db = self.connect()?;
        let mut q = db.prepare(
            "SELECT id FROM source_acquisitions WHERE state IN ('export_pending','downloading')",
        )?;
        let ids = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(q);
        drop(db);
        for id in ids {
            self.source_export_update(
                &id,
                "unknown",
                None,
                Some(&Failure::new(
                    "SOURCE_EXPORT_UNKNOWN",
                    "重启前导出结果未确认，先检查原下载；不自动再次导出。",
                )),
            )?;
        }
        Ok(())
    }
    pub fn recheck_source_export(
        &self,
        task_id: &str,
        id: &str,
        download_id: Option<&str>,
    ) -> Result<Value> {
        let op = self
            .source_acquisitions(task_id)?
            .into_iter()
            .find(|o| o["id"] == id)
            .ok_or_else(|| invalid("导出不属于本任务。"))?;
        let receipts = self.source_downloads(task_id)?;
        let choices: Vec<_> = receipts
            .iter()
            .filter(|r| {
                r.session.id == op["session"]["id"]
                    && r.created >= op["created"].as_u64().unwrap_or(u64::MAX)
                    && op["receipt_id"].as_str().is_none_or(|id| id == r.id)
                    && download_id.is_none_or(|id| id == r.id)
            })
            .collect();
        if choices.len() > 1 {
            return Err(Failure::new(
                "SOURCE_SELECTION_REQUIRED",
                "原会话有多份下载，请明确选择需要核对的原文件。",
            ));
        }
        let Some(receipt) = choices.first() else {
            return Ok(op);
        };
        // Only a native completion and its exact saved bytes prove completion.
        let receipt = self.source_download_for_preview(task_id, &receipt.id)?;
        self.source_export_update(id, "completed", Some(&receipt), None)
    }
    pub fn cancel_source_export(&self, task_id: &str, id: &str, note: &str) -> Result<Value> {
        if note.trim().chars().count() < 6 || note.chars().count() > 1000 {
            return Err(invalid("填写原下载核对情况，至少 6 字，最多 1000 字。"));
        }
        let op = self
            .source_acquisitions(task_id)?
            .into_iter()
            .find(|o| o["id"] == id)
            .ok_or_else(|| invalid("原导出不属于本任务。"))?;
        if !["unknown", "failed"].contains(&op["state"].as_str().unwrap_or("")) {
            return Err(invalid("原导出尚在执行或已确认完成，不能取消记录。"));
        }
        // First read the original native receipts. A completed native download
        // must be verified/adopted, not silently discarded to re-export.
        if self.source_downloads(task_id)?.iter().any(|r| {
            r.session.id == op["session"]["id"]
                && (r.state == "requested" || r.state == "completed")
        }) {
            return Err(invalid(
                "原会话仍有进行中或已完成的下载，请先核对采用；不能取消后重下。",
            ));
        }
        let mut failed = self.source_export_update(
            id,
            "failed",
            None,
            Some(&Failure::new(
                "SOURCE_EXPORT_CANCELLED",
                format!(
                    "人工核对后取消原导出记录：{}；文件与回执保留。",
                    note.trim()
                ),
            )),
        )?;
        failed["cancelled_after_review"] = json!(true);
        Ok(failed)
    }
}
pub fn validate_page(site: &source_downloads::Site, task: &Task, data: &Value) -> Result<()> {
    allowed(
        site,
        data["page_url"]
            .as_str()
            .ok_or_else(|| invalid("缺少当前详情地址。"))?,
    )?;
    let same_doi = !task.record.doi.is_empty()
        && data["fields"]["doi"].as_array().is_some_and(|a| {
            !a.is_empty()
                && a.iter().all(|d| {
                    d.as_str()
                        .is_some_and(|s| normalized_doi(s) == normalized_doi(&task.record.doi))
                })
        });
    if (data["title"].as_str().map(norm) != Some(norm(&task.record.title)) && !same_doi)
        || data["complete"] != true
        || !data["title"].as_str().is_some_and(|s| !s.trim().is_empty())
    {
        return Err(Failure::new(
            "SOURCE_IDENTITY_CONFLICT",
            "详情页题名或加载状态与本篇不符，未采用。",
        ));
    }
    let dois = data["fields"]["doi"].as_array();
    if !task.record.doi.is_empty()
        && dois.is_some_and(|a| {
            a.iter().any(|d| {
                d.as_str()
                    .is_none_or(|s| normalized_doi(s) != normalized_doi(&task.record.doi))
            })
        })
    {
        return Err(Failure::new(
            "SOURCE_IDENTITY_CONFLICT",
            "详情页 DOI 与本篇名单冲突。",
        ));
    }
    Ok(())
}
fn archive(root: &Path, folder: &str, bytes: &[u8], ext: &str) -> Result<std::path::PathBuf> {
    let dir = root.join(folder);
    std::fs::create_dir_all(&dir)?;
    if !dir.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(invalid("来源归档目录被重定向。"));
    }
    let p = dir.join(format!("{}.{}", hash(bytes), ext));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&p)
    {
        Ok(mut f) => {
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if std::fs::symlink_metadata(&p)?.file_type().is_symlink()
                || std::fs::read(&p)? != bytes
            {
                return Err(invalid("来源归档已有不同字节。"));
            }
        }
        Err(e) => return Err(e.into()),
    }
    Ok(p)
}
pub fn capture(
    root: &Path,
    site: &source_downloads::Site,
    task: &Task,
    data: &Value,
) -> Result<(source_files::Draft, Value)> {
    validate_page(site, task, data)?;
    let fields = data["fields"]
        .as_object()
        .ok_or_else(|| invalid("详情字段缺失。"))?;
    if fields.len() > 100
        || fields.values().any(|v| {
            v.as_array()
                .is_none_or(|a| a.len() > 200 || a.iter().any(|s| !s.is_string()))
        })
    {
        return Err(invalid("详情字段格式不完整。"));
    }
    let provenance = json!({"schema":"source_page_v1","mode":"database_page_metadata","site":site,"input_hash":task.input_hash,"record":task.record,"page":data,"captured":now()});
    let raw = serde_json::to_vec(&provenance)?;
    if raw.len() > 900_000 {
        return Err(invalid("详情原文超过范围，不截断采用。"));
    }
    let response = archive(root, "source-responses", &raw, "json")?;
    let mut ris = format!(
        "TY  - GEN\nTI  - {}\nUR  - {}\n",
        data["title"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        data["page_url"].as_str().unwrap()
    );
    for (key, tag) in [
        ("authors", "AU"),
        ("affiliations", "AD"),
        ("year", "PY"),
        ("publication", "T2"),
        ("publisher", "PB"),
        ("volume", "VL"),
        ("issue", "IS"),
        ("pages", "SP"),
        ("article_number", "M1"),
        ("doi", "DO"),
        ("abstract", "AB"),
        ("keywords", "KW"),
        ("issn", "SN"),
        ("isbn", "SN"),
    ] {
        if let Some(a) = fields.get(key).and_then(Value::as_array) {
            for v in a {
                let s = v.as_str().unwrap();
                if !s.trim().is_empty() {
                    ris.push_str(&format!(
                        "{tag}  - {}\n",
                        s.split_whitespace().collect::<Vec<_>>().join(" ")
                    ));
                }
            }
        }
    }
    // Preserve complete unknown fields, repeated values and original text in one
    // hash-protected record; the derived RIS is explicitly not a native export.
    ris.push_str(&format!(
        "N1  - {}\nER  - \n",
        serde_json::to_string(&provenance)?
    ));
    let path = archive(root, "downloads/sources", ris.as_bytes(), "ris")?;
    let mut draft = source_files::prepare(
        root,
        task,
        &site.channel,
        &path,
        source_files::ReadOptions {
            tagged_format: Some("ris".into()),
            export_response: true,
            ..Default::default()
        },
    )?;
    draft.original_name = format!("{}-详情来源（供AI和模板整理）.ris", site.channel);
    let missing: Vec<_> = [
        ("authors", "作者"),
        ("affiliations", "作者单位"),
        ("year", "出版年份"),
        ("publication", "来源出版物"),
        ("pages", "页码/文献号"),
        ("doi", "DOI（需核对是否存在）"),
        ("abstract", "摘要"),
        ("keywords", "关键词"),
        ("author_roles", "第一/通讯作者角色（须核对原文）"),
    ]
    .into_iter()
    .filter(|(k, _)| {
        fields.get(*k).and_then(Value::as_array).is_none_or(|a| {
            a.iter()
                .all(|s| s.as_str().is_none_or(|s| s.trim().is_empty()))
        })
    })
    .map(|(_, v)| v)
    .collect();
    Ok((
        draft,
        json!({"mode":"database_page_metadata","source_url":data["page_url"],"response_path":response,"response_sha256":hash(&raw),"saved_path":path,"missing":missing}),
    ))
}
