//! Frozen zero-match material scope. Products have durable local file receipts.
use crate::{
    materials::{self, Product},
    queue::QueueStatus,
    templates::Template,
    *,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub id: String,
    pub record: Record,
    #[serde(default)]
    pub artifact: Option<Artifact>,
    pub input_hash: String,
    pub revision: i64,
    pub facts: Option<Value>,
    pub template: Option<Template>,
    pub error: Option<Failure>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub id: String,
    pub product: Option<Product>,
    pub error: Option<Failure>,
    pub finished: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Batch {
    pub id: String,
    pub revision: u32,
    pub owner: String,
    pub targets: Vec<Target>,
    pub outcomes: Vec<Outcome>,
    pub cursor: usize,
    pub status: QueueStatus,
    pub pause_requested: bool,
    pub created: u64,
    pub updated: u64,
}
fn bad(message: &str) -> Failure {
    Failure::new("MATERIAL_BATCH_CHANGED", message)
}
fn read(db: &Connection, id: &str) -> Result<Batch> {
    let raw: String = db.query_row("SELECT data FROM material_batches WHERE id=?", [id], |r| {
        r.get(0)
    })?;
    Ok(serde_json::from_str(&raw)?)
}
impl Store {
    pub fn material_batches(&self) -> Result<Vec<Batch>> {
        let db = self.connect()?;
        let mut query = db.prepare("SELECT data FROM material_batches ORDER BY rowid")?;
        let rows = query.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn latest_material_batch(&self) -> Result<Option<Batch>> {
        let raw: Option<String> = self
            .connect()?
            .query_row(
                "SELECT data FROM material_batches ORDER BY rowid DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn material_batch(&self, id: &str) -> Result<Batch> {
        read(&self.connect()?, id)
    }
    pub fn start_material_batch(&self, owner: &str) -> Result<Batch> {
        if owner.trim().is_empty() {
            return Err(bad("选择材料整理负责人。"));
        }
        let schemas: Vec<Template> =
            serde_json::from_value(self.setting("templates")?.unwrap_or(json!([])))?;
        let mut targets = vec![];
        for t in self.tasks()?.into_iter().filter(|t| {
            t.record.owner == owner
                && t.record.matches == 0
                && !t.record.done
                && !t.record.skipped
                && t.stage != Stage::Completed
        }) {
            let prepared = (|| -> Result<(Value, Option<Template>)> {
                let facts = materials::facts(&self.root, &t)?;
                let schema = if t.artifact.is_some() {
                    None
                } else {
                    let id = t
                        .classification
                        .as_ref()
                        .and_then(|v| v["template_id"].as_str())
                        .ok_or_else(|| {
                            Failure::new(
                                "EVIDENCE_REQUIRED",
                                "此篇还没有针对实际模板的已保存建议。",
                            )
                        })?;
                    let saved = schemas.iter().find(|s| s.id == id).ok_or_else(|| {
                        Failure::new("TEMPLATE_INVALID", "建议对应的模板未注册。")
                    })?;
                    Some(materials::actual_template(saved)?)
                };
                Ok((facts, schema))
            })();
            let (facts, template, error) = match prepared {
                Ok((f, s)) => (Some(f), s, None),
                Err(e) => (None, None, Some(e)),
            };
            targets.push(Target {
                id: t.id,
                record: t.record,
                artifact: t.artifact,
                input_hash: t.input_hash,
                revision: t.revision,
                facts,
                template,
                error,
            });
        }
        if targets.is_empty() {
            return Err(bad("原负责人没有未完成且未跳过的零匹配论文。"));
        }
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let pending:u32=tx.query_row("SELECT count(*) FROM material_batches WHERE status IN ('running','paused','blocked','interrupted')",[],|r|r.get(0))?;
        if pending > 0 {
            return Err(bad("先继续或结束原材料整理范围。"));
        }
        let q = Batch {
            id: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            owner: owner.into(),
            targets,
            outcomes: vec![],
            cursor: 0,
            status: QueueStatus::Running,
            pause_requested: false,
            created: now(),
            updated: now(),
        };
        tx.execute(
            "INSERT INTO material_batches VALUES(?,?,?,?,?)",
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
    fn change_material_batch(
        &self,
        id: &str,
        change: impl FnOnce(&mut Batch) -> Result<()>,
    ) -> Result<Batch> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        let mut q = read(&tx, id)?;
        let old = q.revision;
        change(&mut q)?;
        q.revision += 1;
        q.updated = now();
        if tx.execute(
            "UPDATE material_batches SET revision=?,status=?,data=? WHERE id=? AND revision=?",
            params![
                q.revision,
                serde_json::to_value(&q.status)?.as_str().unwrap(),
                serde_json::to_string(&q)?,
                q.id,
                old
            ],
        )? != 1
        {
            return Err(bad("材料范围已变化。"));
        }
        tx.execute(
            "INSERT INTO events(task_id,created,kind,data) VALUES('',?,'material_batch',?)",
            params![
                now() as i64,
                json!({"id":q.id,"cursor":q.cursor,"status":q.status}).to_string()
            ],
        )?;
        tx.commit()?;
        Ok(q)
    }
    pub fn material_target(&self, id: &str) -> Result<Product> {
        let q = self.material_batch(id)?;
        if q.status != QueueStatus::Running || q.pause_requested {
            return Err(bad("材料整理未运行或已请求暂停。"));
        }
        let target = q
            .targets
            .get(q.cursor)
            .ok_or_else(|| bad("材料原范围位置无效。"))?;
        if let Some(e) = &target.error {
            return Err(e.clone());
        }
        if let Some(frozen) = &target.template {
            let current: Vec<Template> =
                serde_json::from_value(self.setting("templates")?.unwrap_or(json!([])))?;
            let current = current
                .iter()
                .find(|s| s.id == frozen.id)
                .ok_or_else(|| Failure::new("TEMPLATE_CHANGED", "原范围的模板注册已变化。"))?;
            if materials::template_semantics(&json!(current))
                != materials::template_semantics(&json!(frozen))
            {
                return Err(Failure::new(
                    "TEMPLATE_CHANGED",
                    "原范围的模板要求已变化，请核对。",
                ));
            }
        }
        materials::prepare(
            self,
            &target.id,
            target
                .facts
                .as_ref()
                .ok_or_else(|| bad("原材料事实缺失。"))?,
            target.template.as_ref(),
        )
    }
    pub fn finish_material_target(&self, id: &str, result: Result<Product>) -> Result<Batch> {
        if let Ok(product) = &result {
            let q = self.material_batch(id)?;
            let target = q
                .targets
                .get(q.cursor)
                .ok_or_else(|| bad("材料原范围位置无效。"))?;
            if product.sa_id != target.id {
                return Err(bad("材料结果不属于原论文。"));
            }
            materials::verify_product(
                self,
                product,
                target
                    .facts
                    .as_ref()
                    .ok_or_else(|| bad("原材料事实缺失。"))?,
            )?;
        }
        self.change_material_batch(id, |q| {
            if q.status != QueueStatus::Running || q.cursor >= q.targets.len() {
                return Err(bad("材料原范围不在执行位置。"));
            }
            let target = &q.targets[q.cursor];
            let (product, error) = match result {
                Ok(p) => {
                    if p.sa_id != target.id {
                        return Err(bad("材料结果不属于原论文。"));
                    }
                    (Some(p), None)
                }
                Err(e) => (None, Some(e)),
            };
            let blocked = error.as_ref().is_some_and(|e| e.code == "STORAGE_ERROR");
            q.outcomes.push(Outcome {
                id: target.id.clone(),
                product,
                error,
                finished: now(),
            });
            q.cursor += 1;
            if q.cursor == q.targets.len() {
                q.status = QueueStatus::Completed;
            } else if blocked {
                q.status = QueueStatus::Blocked;
            } else if q.pause_requested {
                q.status = QueueStatus::Paused;
            }
            Ok(())
        })
    }
    pub fn request_material_pause(&self) -> Result<()> {
        if let Some(q) = self
            .latest_material_batch()?
            .filter(|q| q.status == QueueStatus::Running)
        {
            self.change_material_batch(&q.id, |q| {
                q.pause_requested = true;
                Ok(())
            })?;
        }
        Ok(())
    }
    pub fn pause_material_batch(&self, id: &str) -> Result<()> {
        self.change_material_batch(id, |q| {
            if q.status == QueueStatus::Running {
                q.pause_requested = true;
                q.status = QueueStatus::Paused;
            }
            Ok(())
        })?;
        Ok(())
    }
    pub fn resume_material_batch(&self, id: &str) -> Result<()> {
        self.change_material_batch(id, |q| {
            if !q.status.unfinished() || q.status == QueueStatus::Running {
                return Err(bad("原材料范围不能继续。"));
            }
            q.status = QueueStatus::Running;
            q.pause_requested = false;
            Ok(())
        })?;
        Ok(())
    }
    pub fn cancel_material_batch(&self, id: &str) -> Result<()> {
        self.change_material_batch(id, |q| {
            if q.status == QueueStatus::Running {
                return Err(bad("先暂停原材料范围。"));
            }
            if q.status.unfinished() {
                q.status = QueueStatus::Cancelled;
            }
            Ok(())
        })?;
        Ok(())
    }
    pub fn recover_material_batch(&self) -> Result<()> {
        if let Some(q) = self
            .latest_material_batch()?
            .filter(|q| q.status == QueueStatus::Running)
        {
            self.change_material_batch(&q.id, |q| {
                q.status = QueueStatus::Interrupted;
                q.pause_requested = true;
                Ok(())
            })?;
        }
        Ok(())
    }
}
