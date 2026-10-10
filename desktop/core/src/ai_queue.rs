//! Frozen AI scope and durable request boundaries. No platform writes or automatic retries.
use crate::queue::QueueStatus;
use crate::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub id: String,
    pub record: Record,
    pub input_hash: String,
    pub task_revision: i64,
    pub source_hash: String,
    pub preparation_error: Option<Failure>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub id: String,
    pub task_id: String,
    pub task_revision: i64,
    pub input: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub id: String,
    pub status: String,
    pub error: Option<Failure>,
    pub finished: u64,
    pub attempt: Option<Attempt>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiQueue {
    pub id: String,
    pub revision: u32,
    pub owner: String,
    pub zero_only: bool,
    pub config: Value,
    pub template: Option<Value>,
    pub targets: Vec<Target>,
    pub cursor: usize,
    pub outcomes: Vec<Outcome>,
    pub status: QueueStatus,
    pub pause_requested: bool,
    pub inflight: Option<Attempt>,
    pub last_error: Option<Failure>,
    pub created: u64,
    pub updated: u64,
}
pub fn config(base: &str, model: &str) -> Value {
    json!({"base":base,"model":model})
}
pub fn source_hash(sources: &[Evidence]) -> Result<String> {
    Ok(hash(&serde_json::to_vec(sources)?))
}
pub fn channel_error(error: &Failure) -> bool {
    matches!(
        error.code.as_str(),
        "AI_CONFIG_INVALID"
            | "AI_AUTH_REQUIRED"
            | "AI_RATE_LIMIT"
            | "AI_UNAVAILABLE"
            | "AI_NETWORK_UNAVAILABLE"
            | "AI_CONFIG_CHANGED"
            | "TEMPLATE_CHANGED"
            | "TEMPLATE_INVALID"
    )
}
fn bad(message: &str) -> Failure {
    Failure::new("AI_QUEUE_CHANGED", message)
}
fn task(db: &Connection, id: &str) -> Result<Task> {
    let raw: String = db.query_row("SELECT data FROM tasks WHERE id=?", [id], |r| r.get(0))?;
    Ok(serde_json::from_str(&raw)?)
}
fn read(db: &Connection, id: &str) -> Result<AiQueue> {
    let raw: Option<String> = db
        .query_row("SELECT data FROM ai_queues WHERE id=?", [id], |r| r.get(0))
        .optional()?;
    Ok(serde_json::from_str(
        &raw.ok_or_else(|| bad("原 AI 队列不存在。"))?,
    )?)
}
fn save(tx: &Transaction<'_>, q: &mut AiQueue, kind: &str) -> Result<()> {
    let old = q.revision;
    q.revision += 1;
    q.updated = now();
    let status = serde_json::to_value(&q.status)?;
    if tx.execute(
        "UPDATE ai_queues SET revision=?,status=?,data=? WHERE id=? AND revision=?",
        params![
            q.revision,
            status.as_str().unwrap(),
            serde_json::to_string(q)?,
            q.id,
            old
        ],
    )? != 1
    {
        return Err(bad("AI 队列已变化，请刷新后继续。"));
    }
    tx.execute("INSERT INTO events(task_id,created,kind,data) VALUES('',?,?,?)",params![now() as i64,kind,json!({"queue_id":q.id,"cursor":q.cursor,"status":q.status,"inflight":q.inflight,"error":q.last_error}).to_string()])?;
    Ok(())
}
fn current(q: &AiQueue) -> Result<&Target> {
    if q.status != QueueStatus::Running {
        return Err(bad("AI 队列未运行。"));
    }
    q.targets
        .get(q.cursor)
        .ok_or_else(|| bad("AI 队列位置与原范围不一致。"))
}
fn available(store: &Store, db: &Connection, target: &Target) -> Result<Task> {
    if let Some(error) = &target.preparation_error {
        return Err(error.clone());
    }
    let t = task(db, &target.id)?;
    let pending:u32=db.query_row("SELECT (SELECT count(*) FROM pending_inputs WHERE task_id=?)+(SELECT count(*) FROM attempts WHERE task_id=? AND state IN ('intent','unknown'))",params![t.id,t.id],|r|r.get(0))?;
    if pending > 0
        || t.record.done
        || t.record.skipped
        || t.running
        || t.stage == Stage::Unknown
        || t.stage == Stage::Completed
        || t.revision != target.task_revision
        || t.record.fingerprint() != target.record.fingerprint()
        || t.input_hash != target.input_hash
        || source_hash(&classification::sources(&store.root, &t)?)? != target.source_hash
    {
        return Err(Failure::new(
            "AI_TARGET_CHANGED",
            "原范围的任务、来源或待确认操作已变化；未调用 API，也未覆盖当前任务。",
        ));
    }
    Ok(t)
}
fn result_matches(
    root: &std::path::Path,
    q: &AiQueue,
    target: &Target,
    t: &Task,
    attempt: &Attempt,
) -> bool {
    if t.revision != attempt.task_revision + 1
        || !classification::sources(root, t)
            .is_ok_and(|sources| source_hash(&sources).is_ok_and(|h| h == target.source_hash))
    {
        return false;
    }
    t.classification.as_ref().is_some_and(|v| {
        v["queue_id"] == q.id
            && v["queue_attempt_id"] == attempt.id
            && v["input_hash"] == target.input_hash
            && v["record_fingerprint"] == target.record.fingerprint()
            && v["source_hash"] == target.source_hash
            && v["model_config"] == q.config
            && v["schema_version"] == 2
            && t.input_hash == target.input_hash
            && t.record.fingerprint() == target.record.fingerprint()
            && v["template_hash"]
                == q.template
                    .as_ref()
                    .map(|t| t["fingerprint"].clone())
                    .unwrap_or(Value::Null)
            && t.evidence
                .iter()
                .filter(|e| e.kind == "ai_classification")
                .filter_map(|e| serde_json::from_str::<Value>(&e.text).ok())
                .any(|a| {
                    let parsed = serde_json::from_value::<Vec<Evidence>>(a["sources"].clone());
                    a["result"] == *v
                        && a["review_required"] == true
                        && a["platform_verified"] == false
                        && a["model"] == q.config["model"]
                        && parsed.is_ok_and(|sources| {
                            source_hash(&sources).is_ok_and(|h| h == target.source_hash)
                                && catalog::validate_ai(v, &sources, q.template.as_ref()).is_ok()
                        })
                })
    })
}
impl Store {
    pub fn ai_queues(&self) -> Result<Vec<AiQueue>> {
        let db = self.connect()?;
        let mut s = db.prepare("SELECT data FROM ai_queues ORDER BY rowid")?;
        let rows = s.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn latest_ai_queue(&self) -> Result<Option<AiQueue>> {
        Ok(self.ai_queues()?.pop())
    }
    pub fn ai_queue(&self, id: &str) -> Result<AiQueue> {
        read(&self.connect()?, id)
    }
    pub fn start_ai_queue(
        &self,
        owner: &str,
        zero_only: bool,
        cfg: Value,
        template: Option<Value>,
    ) -> Result<AiQueue> {
        if owner.trim().is_empty()
            || cfg["base"].as_str().is_none_or(|s| s.is_empty())
            || cfg["model"].as_str().is_none_or(|s| s.is_empty())
            || cfg.as_object().is_none_or(|o| o.len() != 2)
        {
            return Err(Failure::new(
                "AI_CONFIG_INVALID",
                "选择负责人并保存 API 地址与模型。",
            ));
        }
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let count:u32=tx.query_row("SELECT count(*) FROM ai_queues WHERE status IN ('running','paused','blocked','interrupted')",[],|r|r.get(0))?;
        if count > 0 {
            return Err(Failure::new(
                "AI_QUEUE_PENDING",
                "已有未结束的 AI 范围，先继续或结束原范围。",
            ));
        }
        let tasks = {
            let mut s = tx.prepare("SELECT data FROM tasks ORDER BY rowid")?;
            let rows = s.query_map([], |r| r.get::<_, String>(0))?;
            rows.map(|r| Ok(serde_json::from_str::<Task>(&r?)?))
                .collect::<Result<Vec<_>>>()?
        };
        let mut targets = vec![];
        for mut t in tasks.into_iter().filter(|t| {
            t.record.owner == owner
                && !t.record.done
                && !t.record.skipped
                && t.stage != Stage::Completed
                && (!zero_only || t.record.matches == 0)
        }) {
            let prepared = (|| -> Result<String> {
                if files::ensure_metadata_evidence(&mut t)? {
                    t = Self::write_task(&tx, &t, "metadata_sources_restored")?;
                }
                source_hash(&classification::sources(&self.root, &t)?)
            })();
            targets.push(Target {
                id: t.id.clone(),
                record: t.record.clone(),
                input_hash: t.input_hash.clone(),
                task_revision: t.revision,
                source_hash: prepared.as_ref().cloned().unwrap_or_default(),
                preparation_error: prepared.err(),
            });
        }
        if targets.is_empty() {
            return Err(Failure::new(
                "AI_SCOPE_EMPTY",
                "当前负责人范围没有未完成且未跳过的论文。",
            ));
        }
        let q = AiQueue {
            id: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            owner: owner.into(),
            zero_only,
            config: cfg,
            template,
            targets,
            cursor: 0,
            outcomes: vec![],
            status: QueueStatus::Running,
            pause_requested: false,
            inflight: None,
            last_error: None,
            created: now(),
            updated: now(),
        };
        tx.execute(
            "INSERT INTO ai_queues VALUES(?,?,?,?,?)",
            params![
                q.id,
                0,
                "running",
                serde_json::to_string(&q)?,
                q.created as i64
            ],
        )?;
        tx.commit()?;
        Ok(q)
    }
    pub fn reuse_ai_target(&self, id: &str) -> Result<bool> {
        let q = self.ai_queue(id)?;
        let target = current(&q)?;
        let t = available(self, &self.connect()?, target)?;
        let Some(v) = &t.classification else {
            return Ok(false);
        };
        let template_hash = q
            .template
            .as_ref()
            .map(|t| t["fingerprint"].clone())
            .unwrap_or(Value::Null);
        if v["schema_version"] != 2
            || v["input_hash"] != target.input_hash
            || v["record_fingerprint"] != target.record.fingerprint()
            || v["source_hash"] != target.source_hash
            || v["model_config"] != q.config
            || v["template_hash"] != template_hash
        {
            return Ok(false);
        }
        let sources = classification::sources(&self.root, &t)?;
        if catalog::validate_ai(v, &sources, q.template.as_ref()).is_err() {
            return Ok(false);
        }
        Ok(t.evidence
            .iter()
            .filter(|e| e.kind == "ai_classification")
            .filter_map(|e| serde_json::from_str::<Value>(&e.text).ok())
            .any(|a| {
                a["result"] == *v && a["review_required"] == true && a["platform_verified"] == false
            }))
    }
    pub fn begin_ai_target(&self, id: &str) -> Result<Attempt> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut q = read(&tx, id)?;
        if q.pause_requested || q.inflight.is_some() {
            return Err(bad("AI 队列已请求暂停或已有请求结果待记录。"));
        }
        let t = available(self, &tx, current(&q)?)?;
        let attempt = Attempt {
            id: uuid::Uuid::new_v4().to_string(),
            task_id: t.id.clone(),
            task_revision: t.revision,
            input: classification::input(
                &t,
                &classification::sources(&self.root, &t)?,
                q.template.as_ref(),
            ),
        };
        q.inflight = Some(attempt.clone());
        save(&tx, &mut q, "ai_request_started")?;
        tx.commit()?;
        Ok(attempt)
    }
    pub fn finish_ai_target(
        &self,
        id: &str,
        status: &str,
        error: Option<Failure>,
    ) -> Result<AiQueue> {
        if !["classified", "reused", "failed", "not_executed"].contains(&status) {
            return Err(bad("AI 逐篇结果类型无效。"));
        }
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut q = read(&tx, id)?;
        let target = current(&q)?.clone();
        if (status == "failed" && (q.inflight.is_none() || error.is_none()))
            || (["reused", "not_executed"].contains(&status) && q.inflight.is_some())
            || (["classified", "reused"].contains(&status) && error.is_some())
        {
            return Err(bad("逐篇结果与原请求边界不符。"));
        }
        if status == "reused" && !self.reuse_ai_target(id)? {
            return Err(bad("原建议不符合当前模型、来源或模板，不能记为复用。"));
        }
        if status == "classified" {
            let a = q
                .inflight
                .as_ref()
                .ok_or_else(|| bad("分类缺少原请求记录。"))?;
            if !result_matches(&self.root, &q, &target, &task(&tx, &target.id)?, a) {
                return Err(bad("保存的分类没有对应当前原请求，不能记为成功。"));
            }
        }
        if let (Some(a), Some(error)) = (&q.inflight, &error) {
            let mut t = task(&tx, &target.id)?;
            if t.revision == a.task_revision {
                if t.last_error
                    .as_ref()
                    .is_none_or(|e| e.code.starts_with("AI_"))
                {
                    t.last_error = Some(error.clone());
                }
                t.evidence.push(Evidence {
                    id: uuid::Uuid::new_v4().to_string(),
                    kind: "ai_failure".into(),
                    source: "API 分类".into(),
                    text: json!({"queue_id":q.id,"attempt":a,"target":target,"error":error})
                        .to_string(),
                    created: now(),
                });
                Self::write_task(&tx, &t, "ai_failed")?;
            }
        }
        let blocked = error.as_ref().is_some_and(channel_error);
        q.last_error = error.clone();
        q.outcomes.push(Outcome {
            id: target.id,
            status: status.into(),
            error,
            finished: now(),
            attempt: q.inflight.clone(),
        });
        q.cursor += 1;
        q.inflight = None;
        if q.cursor == q.targets.len() {
            q.status = QueueStatus::Completed;
        } else if blocked {
            q.status = QueueStatus::Blocked;
        } else if q.pause_requested {
            q.status = QueueStatus::Paused;
        }
        save(&tx, &mut q, "ai_target_finished")?;
        tx.commit()?;
        Ok(q)
    }
    pub fn request_ai_pause(&self) -> Result<()> {
        if let Some(mut q) = self
            .latest_ai_queue()?
            .filter(|q| q.status == QueueStatus::Running)
        {
            let mut db = self.connect()?;
            let tx = db.transaction()?;
            q = read(&tx, &q.id)?;
            q.pause_requested = true;
            save(&tx, &mut q, "ai_pause_requested")?;
            tx.commit()?;
        }
        Ok(())
    }
    pub fn pause_ai_queue(&self, id: &str) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut q = read(&tx, id)?;
        if q.inflight.is_some() {
            return Err(bad("先等待当前 AI 请求结束。"));
        }
        if q.status == QueueStatus::Running {
            q.status = QueueStatus::Paused;
            q.pause_requested = true;
            save(&tx, &mut q, "ai_paused")?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn resume_ai_queue(&self, id: &str, cfg: &Value, template: Option<&Value>) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut q = read(&tx, id)?;
        if !q.status.unfinished() || q.status == QueueStatus::Running || q.inflight.is_some() {
            return Err(bad("原 AI 队列不能继续，请刷新状态。"));
        }
        if q.config != *cfg || q.template.as_ref() != template {
            return Err(Failure::new(
                "AI_CONFIG_CHANGED",
                "模型地址、模型或模板已变化；原范围不能混用新配置。结束原范围后再建立新范围。",
            ));
        }
        q.status = QueueStatus::Running;
        q.pause_requested = false;
        q.last_error = None;
        save(&tx, &mut q, "ai_resumed")?;
        tx.commit()?;
        Ok(())
    }
    pub fn cancel_ai_queue(&self, id: &str) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut q = read(&tx, id)?;
        if q.status == QueueStatus::Running || q.inflight.is_some() {
            return Err(bad("先暂停并等待当前请求结束，再结束原范围。"));
        }
        if q.status.unfinished() {
            q.status = QueueStatus::Cancelled;
            save(&tx, &mut q, "ai_cancelled")?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn recover_ai_queue(&self) -> Result<()> {
        let Some(mut q) = self
            .latest_ai_queue()?
            .filter(|q| q.status == QueueStatus::Running)
        else {
            return Ok(());
        };
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        q = read(&tx, &q.id)?;
        if let Some(a) = &q.inflight {
            let target = q
                .targets
                .get(q.cursor)
                .ok_or_else(|| bad("AI 原请求位置无效。"))?;
            let saved = result_matches(&self.root, &q, target, &task(&tx, &target.id)?, a);
            let error = if saved {
                None
            } else {
                Some(Failure::new(
                    "AI_REQUEST_UNKNOWN",
                    "重启前请求结果未保存，可能已消耗额度；未自动重发，可核对后单条补做。",
                ))
            };
            q.outcomes.push(Outcome {
                id: target.id.clone(),
                status: if saved { "classified" } else { "unconfirmed" }.into(),
                error: error.clone(),
                finished: now(),
                attempt: Some(a.clone()),
            });
            q.cursor += 1;
            q.inflight = None;
            q.last_error = error;
        }
        q.status = if q.cursor == q.targets.len() {
            QueueStatus::Completed
        } else {
            QueueStatus::Interrupted
        };
        q.pause_requested = true;
        save(&tx, &mut q, "ai_recovered")?;
        tx.commit()?;
        Ok(())
    }
}
