use crate::{model::*, now};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Store {
    pub root: PathBuf,
}
fn validate_roster_records(records: &[Record]) -> Result<()> {
    let mut ids = std::collections::BTreeMap::new();
    for record in records {
        if record.sa_id.trim().is_empty() {
            return Err(Failure::new(
                "INPUT_INVALID",
                format!("原表第 {} 行缺少 SA ID，未导入名单。", record.row),
            ));
        }
        if let Some(previous_row) = ids.insert(&record.sa_id, record.row) {
            return Err(Failure::new(
                "INPUT_INVALID",
                format!(
                    "重复 SA ID：{}，原表第 {}、{} 行；整批未导入。",
                    record.sa_id, previous_row, record.row
                ),
            ));
        }
    }
    Ok(())
}
impl Store {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let s = Self {
            root: root.as_ref().to_path_buf(),
        };
        std::fs::create_dir_all(&s.root)?;
        let db = s.connect()?;
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY,revision INTEGER NOT NULL,data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY,task_id TEXT,created INTEGER,kind TEXT,data TEXT); CREATE TABLE IF NOT EXISTS attempts(id TEXT PRIMARY KEY,task_id TEXT,action TEXT,state TEXT,data TEXT,created INTEGER); CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,data TEXT NOT NULL);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS papers(id TEXT PRIMARY KEY,updated INTEGER NOT NULL,data TEXT NOT NULL);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS legacy_migrations(fingerprint TEXT PRIMARY KEY,data TEXT NOT NULL,created INTEGER NOT NULL);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS pending_inputs(task_id TEXT PRIMARY KEY,data TEXT NOT NULL);CREATE TABLE IF NOT EXISTS task_versions(id TEXT PRIMARY KEY,task_id TEXT NOT NULL,data TEXT NOT NULL,created INTEGER NOT NULL);CREATE TABLE IF NOT EXISTS input_history(id TEXT PRIMARY KEY,task_id TEXT NOT NULL,data TEXT NOT NULL,created INTEGER NOT NULL);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS download_queues(id TEXT PRIMARY KEY,revision INTEGER NOT NULL,status TEXT NOT NULL,data TEXT NOT NULL,created INTEGER NOT NULL); CREATE UNIQUE INDEX IF NOT EXISTS one_pending_download_queue ON download_queues((1)) WHERE status IN ('running','paused','blocked','interrupted');")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS ai_queues(id TEXT PRIMARY KEY,revision INTEGER NOT NULL,status TEXT NOT NULL,data TEXT NOT NULL,created INTEGER NOT NULL); CREATE UNIQUE INDEX IF NOT EXISTS one_pending_ai_queue ON ai_queues((1)) WHERE status IN ('running','paused','blocked','interrupted');")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS material_batches(id TEXT PRIMARY KEY,revision INTEGER NOT NULL,status TEXT NOT NULL,data TEXT NOT NULL,created INTEGER NOT NULL); CREATE UNIQUE INDEX IF NOT EXISTS one_pending_material_batch ON material_batches((1)) WHERE status IN ('running','paused','blocked','interrupted');")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS native_downloads(id TEXT PRIMARY KEY,task_id TEXT NOT NULL,state TEXT NOT NULL,data TEXT NOT NULL); CREATE UNIQUE INDEX IF NOT EXISTS one_pending_native_download ON native_downloads(task_id) WHERE state IN ('armed','requested','completed');")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS source_downloads(id TEXT PRIMARY KEY,task_id TEXT NOT NULL,state TEXT NOT NULL,data TEXT NOT NULL);")?;
        Ok(s)
    }
    pub(crate) fn connect(&self) -> Result<Connection> {
        let db = Connection::open(self.root.join("workspace.sqlite3"))?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA synchronous=FULL;")?;
        Ok(db)
    }
    pub fn tasks(&self) -> Result<Vec<Task>> {
        let db = self.connect()?;
        let mut q = db.prepare("SELECT data FROM tasks ORDER BY rowid")?;
        let rows = q.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn task(&self, id: &str) -> Result<Task> {
        let db = self.connect()?;
        let data: Option<String> = db
            .query_row("SELECT data FROM tasks WHERE id=?", [id], |r| r.get(0))
            .optional()?;
        serde_json::from_str(&data.ok_or_else(|| Failure::new("TASK_MISSING", "任务不存在。"))?)
            .map_err(Into::into)
    }
    pub fn save(&self, task: &mut Task, kind: &str) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let next = Self::write_task(&tx, task, kind)?;
        tx.commit()?;
        *task = next;
        Ok(())
    }
    pub(crate) fn write_task(tx: &Transaction<'_>, task: &Task, kind: &str) -> Result<Task> {
        let rev = task.revision;
        let mut next = task.clone();
        next.revision += 1;
        next.updated = now();
        if let Some(artifact) = next.artifact.as_ref().filter(|a| a.identity_confirmed) {
            let paper_id = crate::hash(normalized_wos(&artifact.candidate.wos).as_bytes());
            let existing: Option<String> = tx
                .query_row("SELECT data FROM papers WHERE id=?", [&paper_id], |r| {
                    r.get(0)
                })
                .optional()?;
            if let Some(raw) = existing {
                let paper: Paper = serde_json::from_str(&raw)?;
                let old_doi = normalized_doi(&paper.artifact.candidate.doi);
                let new_doi = normalized_doi(&artifact.candidate.doi);
                if !old_doi.is_empty() && !new_doi.is_empty() && old_doi != new_doi {
                    return Err(Failure::new(
                        "IDENTITY_CONFLICT",
                        "同一 WOS 论文的归档 DOI 不一致，请核验来源版本。",
                    ));
                }
            }
            let paper = Paper {
                id: paper_id.clone(),
                artifact: artifact.clone(),
                updated: now(),
            };
            tx.execute("INSERT INTO papers VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET updated=excluded.updated,data=excluded.data",params![paper.id,paper.updated as i64,serde_json::to_string(&paper)?])?;
            next.paper_id = paper_id;
        }
        let n = tx.execute(
            "UPDATE tasks SET revision=?,data=? WHERE id=? AND revision=?",
            params![next.revision, serde_json::to_string(&next)?, next.id, rev],
        )?;
        if n != 1 {
            return Err(Failure::new(
                "TASK_CHANGED",
                "任务已在其他操作中变化，请刷新后重试。",
            ));
        }
        tx.execute(
            "INSERT INTO events(task_id,created,kind,data) VALUES(?,?,?,?)",
            params![
                next.id,
                now() as i64,
                kind,
                serde_json::to_string(
                    &json!({"stage":next.stage,"revision":next.revision,"error":next.last_error})
                )?
            ],
        )?;
        Ok(next)
    }
    pub fn import(&self, records: Vec<Record>, hash: String) -> Result<usize> {
        self.import_from(records, hash, "")
    }
    pub fn import_from(
        &self,
        records: Vec<Record>,
        hash: String,
        source_file: &str,
    ) -> Result<usize> {
        // Every entry path (including legacy migration) must reject one ambiguous
        // SA input before touching any persisted task, proposal or history.
        validate_roster_records(&records)?;
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let count = records.len();
        for record in records {
            let existing: Option<String> = tx
                .query_row("SELECT data FROM tasks WHERE id=?", [&record.sa_id], |r| {
                    r.get(0)
                })
                .optional()?;
            if let Some(raw) = existing {
                let mut old: Task = serde_json::from_str(&raw)?;
                let pending: u32 = tx.query_row(
                    "SELECT count(*) FROM pending_inputs WHERE task_id=?",
                    [&old.id],
                    |r| r.get(0),
                )?;
                if old.record.fingerprint() != record.fingerprint()
                    || old.record.mark != record.mark
                    || old.record.done != record.done
                    || (old.batch_recheck.is_some()
                        && (old.input_hash != hash || json!(old.record) != json!(record)))
                    || pending > 0
                {
                    let pending_data: Option<String> = tx
                        .query_row(
                            "SELECT data FROM pending_inputs WHERE task_id=?",
                            [&old.id],
                            |r| r.get(0),
                        )
                        .optional()?;
                    let prior = pending_data
                        .map(|r| serde_json::from_str::<crate::versions::Proposal>(&r))
                        .transpose()?;
                    let previous_version_id = prior
                        .as_ref()
                        .and_then(|p| p.previous_version_id.clone())
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                    tx.execute(
                        "INSERT OR IGNORE INTO task_versions VALUES(?,?,?,?)",
                        params![
                            previous_version_id,
                            old.id,
                            serde_json::to_string(&old)?,
                            now() as i64
                        ],
                    )?;
                    let proposal = crate::versions::Proposal {
                        id: crate::hash(
                            format!("{}:{}:{}", old.id, record.fingerprint(), hash).as_bytes(),
                        ),
                        record: record.clone(),
                        input_hash: hash.clone(),
                        source_file: source_file.into(),
                        previous_fingerprint: old.record.fingerprint(),
                        previous_stage: Some(
                            prior
                                .as_ref()
                                .and_then(|p| p.previous_stage.clone())
                                .unwrap_or_else(|| old.stage.clone()),
                        ),
                        previous_version_id: Some(previous_version_id),
                        created: now(),
                    };
                    let data = serde_json::to_string(&proposal)?;
                    tx.execute("INSERT INTO pending_inputs VALUES(?,?) ON CONFLICT(task_id) DO UPDATE SET data=excluded.data",params![old.id,data])?;
                    tx.execute(
                        "INSERT OR IGNORE INTO input_history VALUES(?,?,?,?)",
                        params![proposal.id, old.id, data, now() as i64],
                    )?;
                    old.last_error = Some(Failure::new(
                        "INPUT_CHANGED",
                        "名单关键字段已变化，保留历史任务和写入记录；请核验后建立新的任务版本。",
                    ));
                    old.running = false;
                    if old.stage != Stage::Completed && old.stage != Stage::Unknown {
                        old.stage = Stage::AwaitingReview;
                    }
                } else {
                    old.record.row = record.row;
                    old.record.source = record.source;
                    old.record.skipped = record.skipped;
                    old.input_hash = hash.clone();
                }
                old.revision += 1;
                old.updated = now();
                tx.execute(
                    "UPDATE tasks SET revision=?,data=? WHERE id=?",
                    params![old.revision, serde_json::to_string(&old)?, old.id],
                )?;
            } else {
                let t = Task::new(record, hash.clone());
                tx.execute(
                    "INSERT INTO tasks VALUES(?,?,?)",
                    params![t.id, 0, serde_json::to_string(&t)?],
                )?;
            }
        }
        tx.commit()?;
        Ok(count)
    }
    pub fn pending_input(&self, id: &str) -> Result<Option<crate::versions::Proposal>> {
        let raw: Option<String> = self
            .connect()?
            .query_row(
                "SELECT data FROM pending_inputs WHERE task_id=?",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn accept_input(
        &self,
        task: &Task,
        proposal: &crate::versions::Proposal,
        snapshot: &Value,
        source: &str,
        proof: &str,
    ) -> Result<Task> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let raw: String = tx.query_row(
            "SELECT data FROM pending_inputs WHERE task_id=?",
            [&task.id],
            |r| r.get(0),
        )?;
        let current: crate::versions::Proposal = serde_json::from_str(&raw)?;
        if current.id != proposal.id || current.input_hash != proposal.input_hash {
            return Err(Failure::new(
                "INPUT_CHANGED",
                "待核验的新名单版本已变化，请重新读取差异。",
            ));
        }
        let unresolved: u32 = tx.query_row(
            "SELECT count(*) FROM attempts WHERE task_id=? AND state IN ('intent','unknown')",
            [&task.id],
            |r| r.get(0),
        )?;
        if unresolved > 0 {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "先回读旧版本的待确认操作，不丢弃旧检查点。",
            ));
        }
        let mut next = crate::versions::build_next(task, &current, snapshot, source, proof)?;
        if let Some(id) = &current.previous_version_id {
            let raw: String = tx.query_row(
                "SELECT data FROM task_versions WHERE id=? AND task_id=?",
                params![id, task.id],
                |r| r.get(0),
            )?;
            let mut original: Task = serde_json::from_str(&raw)?;
            original
                .evidence
                .retain(|e| e.kind != "input_version_history");
            next.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "input_version_history".into(),
                source: "名单变更前的原始任务快照".into(),
                text: serde_json::to_string(&original)?,
                created: now(),
            });
        }
        tx.execute(
            "INSERT INTO task_versions VALUES(?,?,?,?)",
            params![
                uuid::Uuid::new_v4().to_string(),
                task.id,
                serde_json::to_string(task)?,
                now() as i64
            ],
        )?;
        let next = Self::write_task(&tx, &next, "input_version_accepted")?;
        // Explicitly accepted input starts a new local version. Keep old
        // download receipts with their original state and scope for audit;
        // they cannot attach to, or permanently block, the new version.
        tx.execute("UPDATE native_downloads SET data=json_set(data,'$.superseded_from',state,'$.state','superseded'),state='superseded' WHERE task_id=? AND state IN ('armed','requested','completed')", [&task.id])?;
        tx.execute("DELETE FROM pending_inputs WHERE task_id=?", [&task.id])?;
        for prefix in [
            "claim:",
            "alias:",
            "metadata:",
            "duplicate:",
            "input-version:",
        ] {
            tx.execute(
                "DELETE FROM settings WHERE key=?",
                [format!("{prefix}{}", task.id)],
            )?;
        }
        tx.commit()?;
        Ok(next)
    }
    pub fn assert_no_prior_upload(&self, candidate: &Candidate) -> Result<()> {
        let db = self.connect()?;
        let mut q=db.prepare("SELECT data FROM attempts WHERE action IN ('import_upload','import_submit','import_push') AND state IN ('intent','unknown','verified')")?;
        let same = |c: &Value| -> bool {
            let wos = normalized_wos(c["wos"].as_str().unwrap_or(""));
            let doi = normalized_doi(c["doi"].as_str().unwrap_or(""));
            (!wos.is_empty() && wos == normalized_wos(&candidate.wos))
                || (!doi.is_empty() && doi == normalized_doi(&candidate.doi))
        };
        for row in q.query_map([], |r| r.get::<_, String>(0))? {
            let audit: Value = serde_json::from_str(&row?)?;
            if same(&audit["payload"]["candidate"]) {
                return Err(Failure::new("DUPLICATE_WRITE","本篇已有上传/导入/推送记录，即使更换名单版本也不能再次上传；回读既有批次或查本库。"));
            }
        }
        let mut q = db.prepare("SELECT data FROM task_versions")?;
        for row in q.query_map([], |r| r.get::<_, String>(0))? {
            let old: Task = serde_json::from_str(&row?)?;
            if matches!(
                old.stage,
                Stage::Uploaded
                    | Stage::Imported
                    | Stage::Pushed
                    | Stage::Claimed
                    | Stage::Completed
                    | Stage::Unknown
            ) && old.batch.is_some()
                && old
                    .artifact
                    .as_ref()
                    .is_some_and(|a| same(&serde_json::to_value(&a.candidate).unwrap()))
            {
                return Err(Failure::new(
                    "DUPLICATE_WRITE",
                    "旧任务版本已有该论文的批次，不能创建新批次。",
                ));
            }
        }
        Ok(())
    }
    pub fn legacy_snapshots(&self) -> Result<Vec<Value>> {
        let db = self.connect()?;
        let mut query = db.prepare("SELECT data FROM legacy_migrations ORDER BY created")?;
        let rows = query.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn legacy_snapshot(&self, fingerprint: &str) -> Result<Option<Value>> {
        let db = self.connect()?;
        let raw: Option<String> = db
            .query_row(
                "SELECT data FROM legacy_migrations WHERE fingerprint=?",
                [fingerprint],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|v| serde_json::from_str(&v).map_err(Into::into))
            .transpose()
    }
    pub fn apply_legacy(&self, plan: &crate::legacy::Plan) -> Result<Value> {
        validate_roster_records(&plan.records)?;
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        if tx.query_row(
            "SELECT count(*) FROM legacy_migrations WHERE fingerprint=?",
            [&plan.preview.fingerprint],
            |r| r.get::<_, u32>(0),
        )? > 0
        {
            return Ok(
                json!({"already_imported":true,"count":0,"fingerprint":plan.preview.fingerprint}),
            );
        }
        let conflicts = Self::legacy_conflicts_on(&tx, plan)?;
        if let Some(first) = conflicts.first() {
            return Err(Failure::new(
                "MIGRATION_CONFLICT",
                format!(
                    "发现 {} 项迁移冲突；SA {}：{}。尚未复制文件或迁移任务。",
                    conflicts.len(),
                    first.sa_id,
                    first.message
                ),
            ));
        }
        let material_archives = crate::legacy_materials::archive(
            &self.root,
            std::path::Path::new(&plan.preview.root),
            &plan.materials,
        )?;
        for record in &plan.records {
            let old: Option<String> = tx
                .query_row("SELECT data FROM tasks WHERE id=?", [&record.sa_id], |r| {
                    r.get(0)
                })
                .optional()?;
            let mut task = if let Some(raw) = old {
                let old: Task = serde_json::from_str(&raw)?;
                if serde_json::to_value(&old.record)? != serde_json::to_value(record)?
                    || old.running
                    || old.stage == Stage::Unknown
                {
                    return Err(Failure::new(
                        "MIGRATION_CONFLICT",
                        format!(
                            "SA {} 在工作台已有不同版本或待确认操作；未迁移任何任务。",
                            record.sa_id
                        ),
                    ));
                }
                old
            } else {
                let task = Task::new(
                    record.clone(),
                    plan.snapshot["roster_hash"].as_str().unwrap().into(),
                );
                tx.execute(
                    "INSERT INTO tasks VALUES(?,?,?)",
                    params![task.id, 0, serde_json::to_string(&task)?],
                )?;
                task
            };
            let rows = crate::legacy::rows_for(&plan.snapshot, &record.sa_id);
            let material_evidence = crate::legacy_materials::evidence(
                &plan.materials,
                &task,
                &plan.preview.fingerprint,
                &material_archives,
            )?;
            let has_materials = !material_evidence.is_empty();
            task.evidence.extend(material_evidence);
            if rows.is_empty() {
                if has_materials {
                    Self::write_task(&tx, &task, "legacy_materials_migrated")?;
                }
                continue;
            }
            // Keep the whole original journal, including historical input keys and events.
            task.evidence.push(Evidence{id:uuid::Uuid::new_v4().to_string(),kind:"legacy_history".into(),source:plan.preview.root.clone(),text:json!({"migration":plan.preview.fingerprint,"rows":rows,"events":plan.snapshot["sources"].as_array().unwrap().iter().flat_map(|s|s["snapshot"]["events"].as_array().unwrap()).filter(|e|rows.iter().any(|r|r["record_key"]==e["record_key"])).collect::<Vec<_>>()}).to_string(),created:now()});
            let changed = rows
                .iter()
                .any(|r| r["record_key"] != plan.keys[&record.sa_id]);
            let writes: Vec<_> = rows
                .iter()
                .filter(|r| crate::legacy::write_history(r))
                .collect();
            for source in plan.snapshot["sources"].as_array().unwrap() {
                for row in source["snapshot"]["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| r["sa_id"] == record.sa_id && r["data"].is_object())
                {
                    let key = format!("{}:{}", source["path"].as_str().unwrap(), record.sa_id);
                    let mut artifact = plan.artifacts.get(&key).unwrap().clone();
                    let strong = crate::files::verify_identity(record, &artifact.candidate)?;
                    artifact.identity_confirmed =
                        strong && row["data"]["identity_confirmed"] == true && !changed;
                    if task
                        .artifact
                        .as_ref()
                        .is_some_and(|a| a.candidate.sha256 != artifact.candidate.sha256)
                    {
                        return Err(Failure::new(
                            "MIGRATION_CONFLICT",
                            "同一 SA 有不同的归档版本，请先核验来源；未迁移任何任务。",
                        ));
                    }
                    let raw = std::fs::read(&artifact.path)?;
                    if crate::hash(&raw) != artifact.candidate.sha256 {
                        return Err(Failure::new("FILE_CHANGED", "旧归档在迁移期间变化。"));
                    }
                    let folder = self.root.join("artifacts");
                    std::fs::create_dir_all(&folder)?;
                    let target = folder.join(format!("{}.txt", artifact.candidate.sha256));
                    if target.exists() {
                        if std::fs::symlink_metadata(&target)?.file_type().is_symlink()
                            || !target
                                .canonicalize()?
                                .starts_with(self.root.canonicalize()?)
                        {
                            return Err(Failure::new(
                                "MIGRATION_INVALID",
                                "工作台归档不是专用目录内的实际文件。",
                            ));
                        }
                        if std::fs::read(&target)? != raw {
                            return Err(Failure::new("FILE_CHANGED", "工作台同名归档已变化。"));
                        }
                    } else {
                        use std::io::Write;
                        let mut file = std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&target)?;
                        file.write_all(&raw)?;
                        file.sync_all()?;
                    }
                    artifact.path = target.to_string_lossy().into();
                    task.evidence.push(Evidence {
                        id: uuid::Uuid::new_v4().to_string(),
                        kind: "metadata".into(),
                        source: format!(
                            "{} · {}",
                            plan.preview.root,
                            source["path"].as_str().unwrap()
                        ),
                        text: serde_json::to_string(&artifact.candidate.fields)?,
                        created: now(),
                    });
                    task.artifact = Some(artifact);
                    if row["data"]["batch"].is_object() {
                        task.batch = Some(row["data"]["batch"].clone());
                    } else if row["data"]["upload"].is_object() {
                        task.batch = Some(row["data"]["upload"].clone());
                    }
                }
            }
            if !writes.is_empty() && task.stage != Stage::Completed {
                // Old write receipts are not a fresh remote readback. Never resend.
                let remote: Vec<_> = writes.iter().filter(|r| r["data"].is_object()).collect();
                if remote.len() > 1 {
                    return Err(Failure::new(
                        "MIGRATION_CONFLICT",
                        "同一 SA 有多个旧导入写入来源，需先核对历史批次。",
                    ));
                }
                let action = remote
                    .first()
                    .map(|r| match r["data"]["phase"].as_str().unwrap() {
                        "upload_intent" => "import_upload",
                        "push_intent" | "pushed" => "import_push",
                        _ => "import_submit",
                    })
                    .unwrap_or_else(|| {
                        if writes.iter().all(|r| {
                            ["认领待提交", "认领已核验"]
                                .contains(&r["state"].as_str().unwrap_or(""))
                        }) {
                            "legacy_claim"
                        } else {
                            "legacy_sa"
                        }
                    });
                let pending:u32=tx.query_row("SELECT count(*) FROM attempts WHERE task_id=? AND state IN ('intent','unknown')",[&task.id],|r|r.get(0))?;
                if pending > 0 {
                    return Err(Failure::new(
                        "MIGRATION_CONFLICT",
                        "已有待确认的工作台操作；未迁移任何任务。",
                    ));
                }
                let checkpoint = if action == "legacy_claim" {
                    crate::legacy::claim_checkpoint(&plan.snapshot, &task.id)?
                } else {
                    None
                };
                let payload = json!({"sa_id":task.id,"legacy_migration":plan.preview.fingerprint,"legacy_rows":writes,"input_matches":!changed,"input_hash":task.input_hash,"instructions":format!("SA补充-{}",task.id),"candidate":task.artifact.as_ref().map(|a|&a.candidate),"batch":task.batch,"claim_checkpoint":checkpoint});
                tx.execute(
                    "INSERT INTO attempts VALUES(?,?,?,?,?,?)",
                    params![
                        uuid::Uuid::new_v4().to_string(),
                        task.id,
                        action,
                        "unknown",
                        json!({"payload":payload}).to_string(),
                        now() as i64
                    ],
                )?;
                task.stage = Stage::Unknown;
                task.last_error = Some(Failure::new(
                    "REMOTE_RESULT_UNKNOWN",
                    "旧版已有平台操作；先回读历史批次或 SA，不重新提交。",
                ));
            } else if matches!(task.stage, Stage::Pending | Stage::AwaitingReview)
                && task.artifact.is_some()
            {
                task.stage = Stage::Downloaded;
            }
            if changed {
                task.last_error = Some(Failure::new(
                    "INPUT_CHANGED",
                    "旧日志与名单任务版本不一致；完整历史已保留，不能按新输入重发旧操作。",
                ));
                if task.stage != Stage::Unknown && task.stage != Stage::Completed {
                    task.stage = Stage::AwaitingReview;
                }
            }
            Self::write_task(&tx, &task, "legacy_migrated")?;
        }
        if crate::legacy::inspect(std::path::Path::new(&plan.preview.root))?
            .preview
            .fingerprint
            != plan.preview.fingerprint
        {
            return Err(Failure::new(
                "INPUT_CHANGED",
                "迁移期间旧数据变化；任务与历史记录未提交，请关闭旧助手后重新预览。",
            ));
        }
        let mut snapshot = plan.snapshot.clone();
        snapshot["material_archives"] = serde_json::to_value(&material_archives)?;
        tx.execute(
            "INSERT INTO legacy_migrations VALUES(?,?,?)",
            params![plan.preview.fingerprint, snapshot.to_string(), now() as i64],
        )?;
        tx.commit()?;
        Ok(
            json!({"already_imported":false,"count":plan.records.len(),"fingerprint":plan.preview.fingerprint,"orphans":plan.preview.orphan_count}),
        )
    }
    pub fn legacy_conflicts(
        &self,
        plan: &crate::legacy::Plan,
    ) -> Result<Vec<crate::legacy::Conflict>> {
        let db = self.connect()?;
        if db.query_row(
            "SELECT count(*) FROM legacy_migrations WHERE fingerprint=?",
            [&plan.preview.fingerprint],
            |r| r.get::<_, u32>(0),
        )? > 0
        {
            return Ok(Vec::new());
        }
        Self::legacy_conflicts_on(&db, plan)
    }
    fn legacy_conflicts_on(
        db: &Connection,
        plan: &crate::legacy::Plan,
    ) -> Result<Vec<crate::legacy::Conflict>> {
        let mut conflicts = crate::legacy::source_conflicts(&plan.snapshot, &plan.artifacts);
        for record in &plan.records {
            let raw: Option<String> = db
                .query_row("SELECT data FROM tasks WHERE id=?", [&record.sa_id], |r| {
                    r.get(0)
                })
                .optional()?;
            let Some(raw) = raw else {
                continue;
            };
            let current: Task = serde_json::from_str(&raw)?;
            let pending: u32 = db.query_row(
                "SELECT count(*) FROM pending_inputs WHERE task_id=?",
                [&record.sa_id],
                |r| r.get(0),
            )?;
            let unknown: u32 = db.query_row(
                "SELECT count(*) FROM attempts WHERE task_id=? AND state IN ('intent','unknown')",
                [&record.sa_id],
                |r| r.get(0),
            )?;
            let rows = crate::legacy::rows_for(&plan.snapshot, &record.sa_id);
            let writing = rows.iter().any(|r| crate::legacy::write_history(r));
            let reason = if json!(current.record) != json!(record) {
                Some("工作台已有不同的完整名单字段（含原行号、状态与跳过标记）。")
            } else if current.running
                || current.stage == Stage::Unknown
                || pending > 0
                || unknown > 0
                || current.batch_recheck.is_some()
            {
                Some("工作台正在执行、切换名单或回读未确认的检查点。")
            } else if writing
                && (current.batch.is_some()
                    || matches!(
                        current.stage,
                        Stage::Uploaded | Stage::Imported | Stage::Pushed | Stage::Claimed
                    ))
            {
                Some("工作台已有平台执行结果，不能将另一份旧写入检查点替换为当前操作。")
            } else if current.artifact.as_ref().is_some_and(|a| {
                plan.snapshot["sources"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|s| {
                        plan.artifacts
                            .get(&format!(
                                "{}:{}",
                                s["path"].as_str().unwrap_or(""),
                                record.sa_id
                            ))
                            .is_some_and(|old| old.candidate.sha256 != a.candidate.sha256)
                    })
            }) {
                Some("工作台已有不同版本的实际论文归档。")
            } else {
                None
            };
            if let Some(reason) = reason {
                conflicts.push(crate::legacy::Conflict {
                    sa_id: record.sa_id.clone(),
                    kind: "workspace_task".into(),
                    source: "当前工作台".into(),
                    message: reason.into(),
                });
            }
        }
        Ok(conflicts)
    }
    pub fn reusable_artifact(&self, record: &Record) -> Result<Option<Artifact>> {
        crate::wos_reuse::select(&self.connect()?, record).map(|v| v.map(|v| v.artifact))
    }
    pub fn begin_attempt(&self, task: &mut Task, action: &str) -> Result<String> {
        self.begin_attempt_with_payload(task, action, json!({}))
    }
    pub fn begin_attempt_with_payload(
        &self,
        task: &mut Task,
        action: &str,
        payload: Value,
    ) -> Result<String> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let unresolved: u32 = tx.query_row(
            "SELECT count(*) FROM attempts WHERE task_id=? AND state IN ('intent','unknown')",
            [&task.id],
            |r| r.get(0),
        )?;
        if unresolved > 0 {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "有尚未核实的操作，先回读平台结果。",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut next = task.clone();
        next.running = true;
        next.revision += 1;
        next.updated = now();
        let changed = tx.execute(
            "UPDATE tasks SET revision=?,data=? WHERE id=? AND revision=?",
            params![
                next.revision,
                serde_json::to_string(&next)?,
                next.id,
                task.revision
            ],
        )?;
        if changed != 1 {
            return Err(Failure::new("TASK_CHANGED", "任务已变化，未发出操作。"));
        }
        tx.execute(
            "INSERT INTO attempts VALUES(?,?,?,?,?,?)",
            params![
                id,
                task.id,
                action,
                "intent",
                json!({"payload":payload}).to_string(),
                now() as i64
            ],
        )?;
        tx.commit()?;
        *task = next;
        Ok(id)
    }
    pub fn finish_attempt(
        &self,
        task: &mut Task,
        attempt: &str,
        state: &str,
        response: Value,
        kind: &str,
    ) -> Result<()> {
        if !["verified", "unknown", "not_sent"].contains(&state) {
            return Err(Failure::new("INVALID_TRANSITION", "执行结果状态无效。"));
        }
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let n=tx.execute("UPDATE attempts SET state=?,data=json_set(data,'$.response',json(?)) WHERE id=? AND task_id=? AND state='intent'",params![state,response.to_string(),attempt,task.id])?;
        if n != 1 {
            return Err(Failure::new(
                "TASK_CHANGED",
                "执行尝试已经变化，未覆盖原结果。",
            ));
        }
        let next = Self::write_task(&tx, task, kind)?;
        tx.commit()?;
        *task = next;
        Ok(())
    }
    pub fn verify_attempt(
        &self,
        task: &mut Task,
        attempt: &str,
        verification: Value,
        kind: &str,
    ) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let n=tx.execute("UPDATE attempts SET state='verified',data=json_set(data,'$.verification',json(?)) WHERE id=? AND task_id=? AND state IN ('intent','unknown')",params![verification.to_string(),attempt,task.id])?;
        if n != 1 {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "没有本条待确认的执行尝试。",
            ));
        }
        let next = Self::write_task(&tx, task, kind)?;
        tx.commit()?;
        *task = next;
        Ok(())
    }
    pub fn attempt_payload(&self, id: &str, payload: Value) -> Result<()> {
        self.connect()?.execute(
            "UPDATE attempts SET data=? WHERE id=? AND state='intent'",
            params![json!({"payload":payload}).to_string(), id],
        )?;
        Ok(())
    }
    pub fn end_attempt(&self, id: &str, state: &str, data: Value) -> Result<()> {
        self.connect()?.execute("UPDATE attempts SET state=?,data=json_set(data,'$.response',json(?)) WHERE id=? AND state='intent'",params![state,data.to_string(),id])?;
        Ok(())
    }
    pub fn unresolved(&self, id: &str) -> Result<Vec<Value>> {
        let db = self.connect()?;
        let mut q = db.prepare(
            "SELECT id,action,data FROM attempts WHERE task_id=? AND state IN ('intent','unknown')",
        )?;
        let rows=q.query_map([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"action":r.get::<_,String>(1)?,"data":r.get::<_,String>(2)?})))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }
    pub fn resolve_actions(&self, id: &str, actions: &[&str], data: Value) -> Result<()> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        for action in actions {
            tx.execute("UPDATE attempts SET state='verified',data=json_set(data,'$.verification',json(?)) WHERE task_id=? AND action=? AND state IN ('intent','unknown')",params![data.to_string(),id,action])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn recover(&self) -> Result<()> {
        self.interrupt_source_downloads()?;
        self.connect()?.execute(
            "UPDATE attempts SET state='unknown' WHERE state='intent'",
            [],
        )?;
        for mut t in self.tasks()? {
            let unknown: u32 = self.connect()?.query_row(
                "SELECT count(*) FROM attempts WHERE task_id=? AND state='unknown'",
                [&t.id],
                |r| r.get(0),
            )?;
            if unknown > 0 || t.running {
                t.running = false;
                t.last_error = Some(Failure::new(
                    if unknown > 0 {
                        "REMOTE_RESULT_UNKNOWN"
                    } else {
                        "PAUSED"
                    },
                    if unknown > 0 {
                        "重启前有操作结果待确认，请先回读平台。"
                    } else {
                        "任务已恢复，可继续执行。"
                    },
                ));
                if unknown > 0 {
                    t.stage = Stage::Unknown;
                }
                self.save(&mut t, "recovered")?;
            }
        }
        self.recover_native_downloads()?;
        self.recover_download_queue()?;
        self.recover_ai_queue()?;
        self.recover_material_batch()?;
        Ok(())
    }
    pub fn events(&self, id: &str) -> Result<Vec<Value>> {
        let db = self.connect()?;
        let mut q = db.prepare(
            "SELECT created,kind,data FROM events WHERE task_id=? ORDER BY seq DESC LIMIT 100",
        )?;
        let rows=q.query_map([id],|r|Ok(json!({"created":r.get::<_,i64>(0)?,"kind":r.get::<_,String>(1)?,"data":r.get::<_,String>(2)?})))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }
    pub fn setting(&self, key: &str) -> Result<Option<Value>> {
        let raw: Option<String> = self
            .connect()?
            .query_row("SELECT data FROM settings WHERE key=?", [key], |r| r.get(0))
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn set_setting(&self, key: &str, value: Value) -> Result<()> {
        self.connect()?.execute(
            "INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET data=excluded.data",
            params![key, value.to_string()],
        )?;
        Ok(())
    }
}
