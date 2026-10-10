//! Restore bibliographic files without transferring another SA's business decisions.
use crate::*;
use rusqlite::{Connection, TransactionBehavior};
use serde_json::{json, Value};

pub(crate) struct Shared {
    pub artifact: Artifact,
    origins: Vec<Value>,
}

fn checked(record: &Record, artifact: &Artifact) -> Result<()> {
    let mut probe = Task::new(record.clone(), String::new());
    probe.artifact = Some(artifact.clone());
    // This reparses all actual fields and compares them with the persisted
    // candidate, not merely a self-consistent cached hash or identity checkbox.
    files::read_metadata_candidate(&probe)?;
    Ok(())
}

pub(crate) fn select(db: &Connection, record: &Record) -> Result<Option<Shared>> {
    let mut candidates = Vec::new();
    let mut statement = db.prepare("SELECT data FROM tasks ORDER BY rowid")?;
    for raw in statement.query_map([], |r| r.get::<_, String>(0))? {
        let task: Task = serde_json::from_str(&raw?)?;
        if let Some(artifact) = task.artifact {
            let factual: Vec<_> = task
                .evidence
                .into_iter()
                .filter(|e| matches!(e.kind.as_str(), "metadata" | "native_download"))
                .collect();
            candidates.push((
                artifact,
                json!({"kind":"sa_task","task_id":task.id,
                "input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),
                "original_record":task.record,"source_evidence":factual}),
            ));
        }
    }
    let mut statement = db.prepare("SELECT data FROM papers ORDER BY updated DESC")?;
    for raw in statement.query_map([], |r| r.get::<_, String>(0))? {
        let paper: Paper = serde_json::from_str(&raw?)?;
        candidates.push((
            paper.artifact,
            json!({"kind":"paper_archive","paper_id":paper.id,
            "saved_at":paper.updated,"sa_identity_available":false}),
        ));
    }
    let mut found = std::collections::BTreeMap::<String, Shared>::new();
    for (artifact, mut origin) in candidates {
        if !artifact.identity_confirmed
            || !files::verify_identity(record, &artifact.candidate).unwrap_or(false)
        {
            continue;
        }
        checked(record, &artifact)?;
        // The first matching path is not enough when two export versions exist.
        // Verify every matching origin before deduplicating equal file bytes.
        origin["artifact"] = json!(artifact);
        let entry = found
            .entry(artifact.candidate.sha256.clone())
            .or_insert_with(|| Shared {
                artifact,
                origins: Vec::new(),
            });
        entry.origins.push(origin);
    }
    if found.len() > 1 {
        return Err(Failure::new(
            "AMBIGUOUS_RESULT",
            "已有多个不同版本的核验文件，请选择实际来源版本，不能自动复用。",
        ));
    }
    Ok(found.into_values().next())
}

impl Store {
    /// Existing file restoration, selection, origin audit and target update use
    /// one SQLite snapshot. No search, export or remote operation is performed.
    pub fn restore_wos_artifact(&self, id: &str, revision: i64) -> Result<bool> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut task: Task = serde_json::from_str(&tx.query_row::<String, _, _>(
            "SELECT data FROM tasks WHERE id=?",
            [id],
            |r| r.get(0),
        )?)?;
        let fenced: u32 = tx.query_row(
            "SELECT (SELECT count(*) FROM pending_inputs WHERE task_id=?1)+(SELECT count(*) FROM attempts WHERE task_id=?1 AND state IN ('intent','unknown'))+(SELECT count(*) FROM native_downloads WHERE task_id=?1 AND state IN ('armed','requested','completed'))",
            [id], |r| r.get(0),
        )?;
        if task.revision != revision
            || fenced > 0
            || task.record.done
            || task.record.matches != 0
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
                "TASK_CHANGED",
                "原任务、名单、业务分支或待确认操作已变化，未复用文件或覆盖状态。",
            ));
        }
        let reused = task.artifact.is_none();
        if reused {
            let Some(shared) = select(&tx, &task.record)? else {
                return Ok(false);
            };
            let audit = json!({"schema":"wos_artifact_reuse_v1","target_id":task.id,
                "input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),
                "artifact":shared.artifact,"origins":shared.origins,"reused_at":now(),
                "new_download":false,"sa_state_transferred":false,"platform_verified":false})
            .to_string();
            if audit.len() > crate::source_files::MAX_BYTES {
                return Err(Failure::new(
                    "EVIDENCE_REQUIRED",
                    "同篇来源审计超过 16 MB，请明确选择单个原始来源；未自动复用。",
                ));
            }
            task.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "artifact_reuse".into(),
                source: shared.artifact.record_url.clone(),
                text: audit,
                created: now(),
            });
            // Preserve original download time, source and all actual file fields.
            task.artifact = Some(shared.artifact);
            task.classification = None;
        }
        checked(&task.record, task.artifact.as_ref().unwrap())?;
        let restored_evidence = files::ensure_metadata_evidence(&mut task)?;
        let interrupted = task.running
            || matches!(
                task.stage,
                Stage::Pending | Stage::Searching | Stage::Downloading
            );
        if reused || interrupted {
            task.running = false;
            task.stage = Stage::Downloaded;
            task.last_error = None;
        }
        if reused || interrupted || restored_evidence {
            // Read again immediately before commit; a changed archive cannot
            // acquire a successful reuse audit, task stage or paper record.
            checked(&task.record, task.artifact.as_ref().unwrap())?;
            Self::write_task(
                &tx,
                &task,
                if reused {
                    "artifact_reused"
                } else {
                    "artifact_restored"
                },
            )?;
        }
        tx.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TXT: &[u8] = b"TI\tAU\tAF\tSO\tPY\tC1\tUT\tDI\r\nPaper\tTest, A\tAlice Test\tJournal\t2026\tShanghai Jiao Tong Univ\tWOS:000123456789012\t10.1234/test\r\n";
    fn setup() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        let records = ["origin", "target"].map(|id| Record {
            row: if id == "origin" { 2 } else { 3 },
            owner: id.into(),
            sa_id: id.into(),
            title: "Paper".into(),
            doi: "10.1234/test".into(),
            wos: String::new(),
            staff_id: if id == "origin" { "001" } else { "002" }.into(),
            matches: 0,
            item_ids: String::new(),
            mark: "待处理".into(),
            reason: "missing".into(),
            skipped: false,
            done: false,
            source: "original.xlsx".into(),
        });
        store
            .import(records.to_vec(), "roster-version".into())
            .unwrap();
        let mut origin = store.task("origin").unwrap();
        let path = files::archive(dir.path(), TXT).unwrap();
        origin.artifact = Some(Artifact {
            path: path.to_string_lossy().into(),
            source: "WOS".into(),
            record_url:
                "https://webofscience.clarivate.cn/wos/woscc/full-record/WOS:000123456789012".into(),
            downloaded: 1234,
            candidate: files::parse_wos(TXT).unwrap(),
            identity_confirmed: true,
        });
        files::ensure_metadata_evidence(&mut origin).unwrap();
        origin.stage = Stage::Completed;
        origin.record.done = true;
        origin.platform_id = "origin-only-platform-item".into();
        store
            .save(&mut origin, "fixture_original_download_and_business_state")
            .unwrap();
        (dir, store)
    }
    #[test]
    fn frozen_origins_survive_restart_and_reuse_never_copies_sa_state_or_download_time() {
        let (dir, store) = setup();
        let target = store.task("target").unwrap();
        let original = store.task("origin").unwrap();
        assert!(store
            .restore_wos_artifact(&target.id, target.revision)
            .unwrap());
        let reused = store.task("target").unwrap();
        assert_eq!(reused.stage, Stage::Downloaded);
        assert!(!reused.record.done);
        assert_eq!(reused.record.staff_id, "002");
        assert!(reused.platform_id.is_empty());
        assert!(reused.review.is_none());
        assert!(reused.classification.is_none());
        assert!(store.unresolved("target").unwrap().is_empty());
        assert!(store.native_downloads("target").unwrap().is_empty());
        assert_eq!(reused.artifact.as_ref().unwrap().downloaded, 1234);
        assert_eq!(store.task("origin").unwrap().revision, original.revision);
        let audit: Value = serde_json::from_str(
            &reused
                .evidence
                .iter()
                .find(|e| e.kind == "artifact_reuse")
                .unwrap()
                .text,
        )
        .unwrap();
        assert_eq!(audit["new_download"], false);
        assert_eq!(audit["sa_state_transferred"], false);
        assert_eq!(audit["origins"][0]["original_record"]["staff_id"], "001");
        assert_eq!(audit["artifact"]["downloaded"], 1234);
        assert!(classification::sources(dir.path(), &reused)
            .unwrap()
            .iter()
            .all(|e| e.kind != "artifact_reuse"));
        let reopened = Store::new(dir.path()).unwrap();
        reopened.recover().unwrap();
        let before = reopened.task("target").unwrap();
        assert!(reopened
            .restore_wos_artifact("target", before.revision)
            .unwrap());
        let after = reopened.task("target").unwrap();
        assert_eq!(before.revision, after.revision);
        assert_eq!(
            after
                .evidence
                .iter()
                .filter(|e| e.kind == "artifact_reuse")
                .count(),
            1
        );
    }
    #[test]
    fn forged_cached_candidate_is_not_reused_even_when_its_file_hash_is_correct() {
        let (_dir, store) = setup();
        let mut origin = store.task("origin").unwrap();
        origin.artifact.as_mut().unwrap().candidate.authors = "Forged author".into();
        store.save(&mut origin, "fixture_corrupted_cache").unwrap();
        let target = store.task("target").unwrap();
        assert_eq!(
            store
                .restore_wos_artifact("target", target.revision)
                .unwrap_err()
                .code,
            "FILE_INVALID"
        );
        assert_eq!(store.task("target").unwrap().revision, target.revision);
        assert!(store.task("target").unwrap().artifact.is_none());
        assert!(store.reusable_artifact(&target.record).is_err());
    }
    #[test]
    fn original_revision_remote_attempt_and_unfinished_download_fence_shared_file_adoption() {
        let (_dir, store) = setup();
        let target = store.task("target").unwrap();
        assert!(store
            .restore_wos_artifact("target", target.revision + 1)
            .is_err());
        let mut changed = target.clone();
        store.begin_attempt(&mut changed, "import_upload").unwrap();
        assert!(store
            .restore_wos_artifact("target", changed.revision)
            .is_err());
        assert!(store.task("target").unwrap().artifact.is_none());
        let (_dir2, fresh) = setup();
        let mut target = fresh.task("target").unwrap();
        target.stage = Stage::Downloading;
        target.running = true;
        fresh.save(&mut target, "fixture_export_armed").unwrap();
        fresh
            .prepare_native_download(
                &target,
                "https://webofscience.clarivate.cn/wos/woscc/full-record/WOS:000123456789012",
            )
            .unwrap();
        assert!(fresh
            .restore_wos_artifact("target", target.revision)
            .is_err());
        assert_eq!(fresh.task("target").unwrap().revision, target.revision);
        assert!(fresh.task("target").unwrap().artifact.is_none());
    }
}
