//! Stop a single local attempt only if its last owned checkpoint still matches.
use crate::*;
use rusqlite::{params, TransactionBehavior};
use serde_json::json;

impl Store {
    pub fn record_single_wos_failure(&self, checkpoint: &Task, error: &Failure) -> Result<bool> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: String =
            tx.query_row("SELECT data FROM tasks WHERE id=?", [&checkpoint.id], |r| {
                r.get(0)
            })?;
        let current: Task = serde_json::from_str(&raw)?;
        let pending: u32 = tx.query_row(
            "SELECT count(*) FROM pending_inputs WHERE task_id=?",
            [&current.id],
            |r| r.get(0),
        )?;
        let unresolved: u32 = tx.query_row(
            "SELECT count(*) FROM attempts WHERE task_id=? AND state IN ('intent','unknown')",
            [&current.id],
            |r| r.get(0),
        )?;
        let native_pending: u32 = tx.query_row("SELECT count(*) FROM native_downloads WHERE task_id=? AND state IN ('armed','requested','completed')", [&current.id], |r| r.get(0))?;
        let apply = serde_json::to_value(&current)? == serde_json::to_value(checkpoint)?
            && pending == 0
            && unresolved == 0
            && !current.record.done
            && current.record.matches == 0
            && matches!(current.route, Route::ZeroReview | Route::Missing)
            && matches!(
                current.stage,
                Stage::Pending
                    | Stage::Searching
                    | Stage::Downloading
                    | Stage::AwaitingReview
                    | Stage::Downloaded
                    | Stage::Ready
            )
            && !current
                .last_error
                .as_ref()
                .is_some_and(|e| e.code == "INPUT_CHANGED");
        let audit = json!({"schema":"single_wos_failure_v1", "task_id":checkpoint.id,
            "checkpoint":checkpoint,"current_revision":current.revision,"error":error,
            "task_failure_applied":apply,"pending_input_count":pending,"unresolved_platform_count":unresolved,
            "pending_native_download_count":native_pending,"observed_at":now(),
            "factual_metadata":false,"absence_proof":false,"platform_verified":false});
        tx.execute(
            "INSERT INTO events(task_id,created,kind,data) VALUES(?,?,?,?)",
            params![
                checkpoint.id,
                now() as i64,
                "single_wos_failure",
                audit.to_string()
            ],
        )?;
        if apply {
            let mut next = current;
            next.running = false;
            next.last_error = Some(error.clone());
            // A late native callback still owns its receipt; keep its recovery phase.
            if native_pending == 0 && !matches!(next.stage, Stage::Downloaded | Stage::Ready) {
                next.stage = Stage::AwaitingReview;
            }
            // Failure changes no artifact, paper cache or business proof. In particular,
            // an invalid cached artifact must not prevent saving its own error.
            next.revision += 1;
            next.updated = now();
            let changed = tx.execute(
                "UPDATE tasks SET revision=?,data=? WHERE id=? AND revision=?",
                params![
                    next.revision,
                    serde_json::to_string(&next)?,
                    next.id,
                    checkpoint.revision
                ],
            )?;
            if changed != 1 {
                return Err(Failure::new(
                    "TASK_CHANGED",
                    "保存单篇下载失败前任务已变化，未覆盖任务。",
                ));
            }
        }
        tx.commit()?;
        Ok(apply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup(stage: Stage) -> (tempfile::TempDir, Store, Task) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        let record = Record {
            row: 2,
            owner: "one".into(),
            sa_id: "single".into(),
            title: "Original paper".into(),
            doi: "10.1234/test".into(),
            wos: "".into(),
            staff_id: "001".into(),
            matches: 0,
            item_ids: "".into(),
            mark: "待处理".into(),
            reason: "缺失".into(),
            skipped: false,
            done: false,
            source: "list.xlsx".into(),
        };
        store.import(vec![record], "original-input".into()).unwrap();
        let mut task = store.task("single").unwrap();
        task.stage = stage;
        task.running = true;
        store.save(&mut task, "fixture_attempt_checkpoint").unwrap();
        (dir, store, task)
    }
    #[test]
    fn failure_stops_owned_attempt_and_full_original_error_survives_restart_and_report() {
        let (dir, store, task) = setup(Stage::Searching);
        let error = Failure::new("PAGE_TIMEOUT", "original search timeout");
        assert!(store.record_single_wos_failure(&task, &error).unwrap());
        let after = store.task(&task.id).unwrap();
        assert!(!after.running);
        assert_eq!(after.stage, Stage::AwaitingReview);
        assert_eq!(after.last_error.as_ref().unwrap().code, "PAGE_TIMEOUT");
        assert!(after.artifact.is_none());
        assert!(after.evidence.is_empty());
        assert!(!store.record_single_wos_failure(&task, &error).unwrap());
        assert_eq!(store.task(&task.id).unwrap().revision, after.revision);
        let reopened = Store::new(dir.path()).unwrap();
        let (report, _) = reopened.report_snapshot().unwrap();
        let trace: serde_json::Value = serde_json::from_str(&report[0].evidence[0].text).unwrap();
        assert_eq!(trace["checkpoint"]["record"]["title"], "Original paper");
        assert_eq!(trace["checkpoint"]["input_hash"], "original-input");
        assert_eq!(trace["error"]["message"], error.message);
        assert_eq!(trace["task_failure_applied"], true);
        assert!(classification::sources(dir.path(), &report[0])
            .unwrap()
            .iter()
            .all(|e| e.kind != "execution_failure"));
        assert!(search_scopes::assert_not_found(&report[0]).is_err());
    }
    #[test]
    fn task_changes_and_unresolved_platform_intent_keep_current_business_state() {
        for stage in [
            Stage::Uploaded,
            Stage::Imported,
            Stage::Pushed,
            Stage::Claimed,
            Stage::Completed,
            Stage::Unknown,
        ] {
            let (_dir, store, checkpoint) = setup(Stage::Searching);
            let mut advanced = checkpoint.clone();
            advanced.stage = stage;
            advanced.platform_id = "new-item".into();
            store.save(&mut advanced, "fixture_advanced").unwrap();
            assert!(!store
                .record_single_wos_failure(
                    &checkpoint,
                    &Failure::new("PAGE_TIMEOUT", "late failure")
                )
                .unwrap());
            assert_eq!(
                serde_json::to_value(store.task(&checkpoint.id).unwrap()).unwrap(),
                serde_json::to_value(&advanced).unwrap()
            );
        }
        let (_dir, store, mut task) = setup(Stage::Searching);
        store.begin_attempt(&mut task, "import_upload").unwrap();
        assert!(!store
            .record_single_wos_failure(&task, &Failure::new("PAGE_TIMEOUT", "late failure"))
            .unwrap());
        assert_eq!(
            serde_json::to_value(store.task(&task.id).unwrap()).unwrap(),
            serde_json::to_value(&task).unwrap()
        );
        assert_eq!(store.unresolved(&task.id).unwrap().len(), 1);
    }
    #[test]
    fn unfinished_native_receipt_is_preserved_for_late_callback_without_reissuing_download() {
        let (_dir, store, task) = setup(Stage::Downloading);
        let receipt = store
            .prepare_native_download(
                &task,
                "https://www.webofscience.com/wos/woscc/full-record/WOS:000123456789012",
            )
            .unwrap();
        store
            .request_native_download(&receipt.id, "https://www.webofscience.com/export")
            .unwrap();
        assert!(store
            .record_single_wos_failure(
                &task,
                &Failure::new("DOWNLOAD_RESULT_UNKNOWN", "no completion yet")
            )
            .unwrap());
        let after = store.task(&task.id).unwrap();
        assert!(!after.running);
        assert_eq!(after.stage, Stage::Downloading);
        assert!(after.artifact.is_none());
        assert_eq!(
            store.native_downloads(&task.id).unwrap()[0].state,
            "requested"
        );
        assert!(store
            .prepare_native_download(&after, &receipt.record_url)
            .is_err());
        assert_eq!(
            store.recover_native_download(&task.id).unwrap_err().code,
            "DOWNLOAD_RESULT_UNKNOWN"
        );
    }
}
