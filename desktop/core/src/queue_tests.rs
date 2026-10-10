use crate::{queue::*, *};

fn record(id: &str, owner: &str, skipped: bool) -> Record {
    Record {
        row: 2,
        owner: owner.into(),
        sa_id: id.into(),
        title: "Paper".into(),
        doi: "10.1234/test".into(),
        wos: "".into(),
        staff_id: "001".into(),
        matches: 0,
        item_ids: "".into(),
        mark: "待处理".into(),
        reason: "缺失".into(),
        skipped,
        done: false,
        source: "".into(),
    }
}
fn setup() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    store
        .import(
            vec![
                record("a", "one", false),
                record("b", "two", false),
                record("c", "one", true),
                record("d", "one", false),
            ],
            "input".into(),
        )
        .unwrap();
    (dir, store)
}
// Cursor tests must also supply an actual archived/parsed source before a
// successful outcome; a string label alone is no longer a download receipt.
fn saved_download(store: &Store, id: &str) {
    let raw = b"TI\tAU\tAF\tSO\tPY\tC1\tUT\tDI\r\nPaper\tTest, A\tAlice Test\tJournal\t2026\tShanghai Jiao Tong Univ\tWOS:000123456789012\t10.1234/test\r\n";
    let mut task = store.task(id).unwrap();
    task.artifact = Some(Artifact {
        path: files::archive(&store.root, raw)
            .unwrap()
            .to_string_lossy()
            .into(),
        source: "WOS".into(),
        record_url: "https://webofscience.clarivate.cn/wos/woscc/full-record/WOS:000123456789012"
            .into(),
        downloaded: 1,
        candidate: files::parse_wos(raw).unwrap(),
        identity_confirmed: true,
    });
    files::ensure_metadata_evidence(&mut task).unwrap();
    task.stage = Stage::Downloaded;
    task.running = false;
    store.save(&mut task, "fixture_actual_download").unwrap();
}
#[test]
fn scope_order_and_history_are_frozen_until_explicitly_cancelled() {
    let (_dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    assert_eq!(
        q.targets.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        ["a", "d"]
    );
    store
        .import(vec![record("e", "one", false)], "new".into())
        .unwrap();
    assert_eq!(store.download_queue(&q.id).unwrap().targets.len(), 2);
    assert_eq!(
        store.start_download_queue("two", false).unwrap_err().code,
        "QUEUE_PENDING"
    );
    assert_eq!(
        store.start_download_queue("one", true).unwrap_err().code,
        "QUEUE_PENDING"
    );
    assert!(store.cancel_download_queue(&q.id).is_err());
    store.request_download_pause().unwrap();
    store.pause_download_queue(&q.id).unwrap();
    store.cancel_download_queue(&q.id).unwrap();
    assert_eq!(
        store.download_queue(&q.id).unwrap().status,
        QueueStatus::Cancelled
    );
    let retry = store.start_download_queue("one", true).unwrap();
    assert_eq!(retry.targets[0].id, "c");
    assert_eq!(retry.targets.len(), 1);
    assert_eq!(store.download_queue(&q.id).unwrap().targets.len(), 2);
}
#[test]
fn ordinary_failures_commit_with_cursor_and_do_not_stop_after_three() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    store
        .import(
            (0..4)
                .map(|i| record(&i.to_string(), "one", false))
                .collect(),
            "input".into(),
        )
        .unwrap();
    let q = store.start_download_queue("one", false).unwrap();
    for i in 0..3 {
        let id = i.to_string();
        let mut t = store.task(&id).unwrap();
        t.last_error = Some(Failure::new("NO_RESULT", "实际检索零条"));
        t.stage = Stage::AwaitingReview;
        store
            .finish_download_target(&q.id, &id, "review", t.last_error.clone(), Some(&mut t))
            .unwrap();
        assert_eq!(
            store.download_queue(&q.id).unwrap().status,
            QueueStatus::Running
        );
    }
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    let resumed = reopened.resume_download_queue(&q.id).unwrap();
    assert_eq!(resumed.cursor, 3);
    assert_eq!(resumed.outcomes.len(), 3);
    assert!(resumed
        .outcomes
        .iter()
        .all(|o| o.error.as_ref().unwrap().code == "NO_RESULT"));
    saved_download(&reopened, "3");
    reopened
        .finish_download_target(&q.id, "3", "downloaded", None, None)
        .unwrap();
    let complete = reopened.download_queue(&q.id).unwrap();
    assert_eq!(complete.status, QueueStatus::Completed);
    assert_eq!(complete.cursor, 4);
    assert!(reopened.resume_download_queue(&q.id).is_err());
    assert!(reopened
        .finish_download_target(&q.id, "3", "downloaded", None, None)
        .is_err());
}
#[test]
fn channel_failure_retains_current_target_after_restart() {
    let (dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    saved_download(&store, "a");
    store
        .finish_download_target(&q.id, "a", "downloaded", None, None)
        .unwrap();
    let mut t = store.task("d").unwrap();
    let error = Failure::new("AUTH_REQUIRED", "机构访问失效");
    t.last_error = Some(error.clone());
    t.stage = Stage::AwaitingReview;
    t.running = false;
    store
        .block_download_queue(&q.id, error, Some(&mut t))
        .unwrap();
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    let blocked = reopened.download_queue(&q.id).unwrap();
    assert_eq!(blocked.status, QueueStatus::Blocked);
    assert_eq!(blocked.cursor, 1);
    assert_eq!(blocked.last_error.unwrap().code, "AUTH_REQUIRED");
    assert_eq!(
        reopened.task("d").unwrap().last_error.unwrap().code,
        "AUTH_REQUIRED"
    );
    let resumed = reopened.resume_download_queue(&q.id).unwrap();
    assert_eq!(resumed.targets[resumed.cursor].id, "d");
    assert_eq!(resumed.outcomes.len(), 1);
}
#[test]
fn requested_pause_survives_crash_and_resume_keeps_original_scope() {
    let (dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    store.request_download_pause().unwrap();
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    let paused = reopened.download_queue(&q.id).unwrap();
    assert_eq!(paused.status, QueueStatus::Paused);
    assert!(paused.pause_requested);
    assert_eq!(paused.cursor, 0);
    let resumed = reopened.resume_download_queue(&q.id).unwrap();
    assert_eq!(resumed.status, QueueStatus::Running);
    assert!(!resumed.pause_requested);
    assert_eq!(resumed.owner, "one");
    assert!(!resumed.retry_skipped);
}
#[test]
fn pause_during_current_result_stops_before_next_target() {
    let (_dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    store.request_download_pause().unwrap();
    saved_download(&store, "a");
    store
        .finish_download_target(&q.id, "a", "downloaded", None, None)
        .unwrap();
    let paused = store.download_queue(&q.id).unwrap();
    assert_eq!(paused.status, QueueStatus::Paused);
    assert_eq!(paused.cursor, 1);
    assert!(store
        .finish_download_target(&q.id, "d", "downloaded", None, None)
        .is_err());
    store.resume_download_queue(&q.id).unwrap();
    store.request_download_pause().unwrap();
    saved_download(&store, "d");
    store
        .finish_download_target(&q.id, "d", "downloaded", None, None)
        .unwrap();
    assert_eq!(
        store.download_queue(&q.id).unwrap().status,
        QueueStatus::Completed
    );
}
#[test]
fn stale_task_revision_rolls_back_queue_cursor_and_failure_together() {
    let (_dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    let mut stale = store.task("a").unwrap();
    let mut current = stale.clone();
    current.record.source = "新来源".into();
    store.save(&mut current, "update").unwrap();
    stale.stage = Stage::AwaitingReview;
    stale.last_error = Some(Failure::new("NO_RESULT", "旧查询"));
    assert_eq!(
        store
            .finish_download_target(
                &q.id,
                "a",
                "review",
                stale.last_error.clone(),
                Some(&mut stale)
            )
            .unwrap_err()
            .code,
        "TASK_CHANGED"
    );
    let unchanged = store.download_queue(&q.id).unwrap();
    assert_eq!(unchanged.cursor, 0);
    assert!(unchanged.outcomes.is_empty());
    assert_eq!(store.task("a").unwrap().record.source, "新来源");
    assert!(store.task("a").unwrap().last_error.is_none());
    assert!(store
        .block_download_queue(
            &q.id,
            Failure::new("AUTH_REQUIRED", "失效"),
            Some(&mut stale)
        )
        .is_err());
    assert_eq!(
        store.download_queue(&q.id).unwrap().status,
        QueueStatus::Running
    );
}
#[test]
fn wrong_cursor_or_task_cannot_overwrite_completed_result() {
    let (_dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    assert!(store
        .finish_download_target(&q.id, "d", "downloaded", None, None)
        .is_err());
    let mut t = store.task("d").unwrap();
    assert!(store
        .finish_download_target(&q.id, "a", "review", None, Some(&mut t))
        .is_err());
    store
        .finish_download_target(
            &q.id,
            "a",
            "review",
            Some(Failure::new("AMBIGUOUS_RESULT", "多条")),
            None,
        )
        .unwrap();
    assert!(store
        .finish_download_target(&q.id, "a", "downloaded", None, None)
        .is_err());
    let after = store.download_queue(&q.id).unwrap();
    assert_eq!(after.cursor, 1);
    assert_eq!(after.outcomes[0].status, "review");
    assert_eq!(
        after.outcomes[0].error.as_ref().unwrap().code,
        "AMBIGUOUS_RESULT"
    );
}
#[test]
fn frozen_target_detects_staff_title_owner_and_skip_changes_but_not_row_move() {
    let (_dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    let target = &q.targets[0];
    let original = store.task("a").unwrap().record;
    let mut moved = original.clone();
    moved.row = 28;
    assert!(target.matches(&moved));
    for changed in [
        Record {
            staff_id: "002".into(),
            ..original.clone()
        },
        Record {
            title: "Different".into(),
            ..original.clone()
        },
        Record {
            owner: "two".into(),
            ..original.clone()
        },
        Record {
            skipped: true,
            ..original.clone()
        },
    ] {
        assert!(!target.matches(&changed));
    }
}
#[test]
fn resumed_scope_rejects_same_fields_in_new_roster_and_preserves_remaining_original_targets() {
    let (dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    store.pause_download_queue(&q.id).unwrap();
    let original = store.task("a").unwrap();
    let mut moved = original.clone();
    moved.record.row = 28;
    assert!(q.targets[0].matches_task(&moved));
    store
        .import(vec![original.record.clone()], "new-roster-hash".into())
        .unwrap();
    let store = Store::new(dir.path()).unwrap();
    store.recover().unwrap();
    store.resume_download_queue(&q.id).unwrap();
    let mut changed = store.task("a").unwrap();
    assert!(q.targets[0].matches(&changed.record));
    assert!(!q.targets[0].matches_task(&changed));
    let before = serde_json::to_value(&changed).unwrap();
    assert_eq!(
        store
            .finish_download_target(
                &q.id,
                "a",
                "review",
                Some(Failure::new("NO_RESULT", "old search")),
                Some(&mut changed)
            )
            .unwrap_err()
            .code,
        "QUEUE_TARGET_CHANGED"
    );
    assert_eq!(
        store
            .block_download_queue(
                &q.id,
                Failure::new("AUTH_REQUIRED", "old session"),
                Some(&mut changed)
            )
            .unwrap_err()
            .code,
        "QUEUE_TARGET_CHANGED"
    );
    assert_eq!(
        serde_json::to_value(store.task("a").unwrap()).unwrap(),
        before
    );
    assert_eq!(store.download_queue(&q.id).unwrap().cursor, 0);
    store
        .finish_download_target(
            &q.id,
            "a",
            "not_executed",
            Some(Failure::new("QUEUE_TARGET_CHANGED", "roster changed")),
            None,
        )
        .unwrap();
    let resumed = store.download_queue(&q.id).unwrap();
    assert_eq!(resumed.status, QueueStatus::Running);
    assert_eq!(resumed.cursor, 1);
    assert_eq!(resumed.targets[0].input_hash, "input");
    assert!(resumed.targets[1].matches_task(&store.task("d").unwrap()));
    assert_eq!(resumed.outcomes[0].status, "not_executed");
    let mut legacy = resumed.targets[1].clone();
    legacy.input_hash.clear();
    assert!(!legacy.matches_task(&store.task("d").unwrap()));
}
#[test]
fn report_retains_cancelled_scope_original_fields_and_long_failure_without_truncation() {
    use calamine::{open_workbook_auto, Reader};
    let (_dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    let long = "原检索错误完整内容😀".repeat(6000);
    store
        .finish_download_target(
            &q.id,
            "a",
            "review",
            Some(Failure::new("NO_RESULT", &long)),
            None,
        )
        .unwrap();
    store.pause_download_queue(&q.id).unwrap();
    store.cancel_download_queue(&q.id).unwrap();
    store.start_download_queue("one", true).unwrap();
    let mut changed = store.task("a").unwrap().record;
    changed.title = "New input title".into();
    store.import(vec![changed], "new-version".into()).unwrap();
    let (tasks, queues) = store.report_snapshot().unwrap();
    assert_eq!(queues.len(), 2);
    assert_eq!(queues[0].targets[0].record.as_ref().unwrap().title, "Paper");
    let path = store.root.join("queue-report.xlsx");
    files::export_report_with_queues(&tasks, &queues, &path).unwrap();
    let mut book = open_workbook_auto(&path).unwrap();
    let sheet = book.worksheet_range("下载队列结果").unwrap();
    let rows = sheet
        .rows()
        .skip(1)
        .filter(|r| r[0].to_string() == q.id && r[5].to_string() == "a")
        .collect::<Vec<_>>();
    assert!(rows.len() > 1);
    let complete = rows.iter().map(|r| r[14].to_string()).collect::<String>();
    let raw: serde_json::Value = serde_json::from_str(&complete).unwrap();
    assert_eq!(raw["outcome"]["error"]["message"], long);
    assert_eq!(raw["target"]["record"]["title"], "Paper");
    assert_eq!(raw["target"]["input_hash"], "input");
    assert!(
        rows.iter()
            .all(|r| r[16].to_string() == hash(complete.as_bytes())
                && r[3].to_string() == "cancelled")
    );
    assert!(sheet
        .rows()
        .any(|r| r[5].to_string() == "d" && r[9].to_string() == "not_executed"));
    assert!(sheet
        .rows()
        .any(|r| r[0].to_string() == queues[1].id && r[2].to_string() == "跳过论文"));
}
#[test]
fn late_download_failure_preserves_advanced_platform_stages_and_full_original_error() {
    for stage in [
        Stage::Uploaded,
        Stage::Imported,
        Stage::Pushed,
        Stage::Claimed,
        Stage::Completed,
        Stage::Unknown,
    ] {
        let (dir, store) = setup();
        let q = store.start_download_queue("one", false).unwrap();
        let mut advanced = store.task("a").unwrap();
        advanced.stage = stage.clone();
        advanced.record.done = stage == Stage::Completed;
        advanced.platform_id = "existing-platform-item".into();
        store
            .save(&mut advanced, "fixture_platform_advanced")
            .unwrap();
        let before = serde_json::to_value(&advanced).unwrap();
        let actual = Failure::new("PAGE_TIMEOUT", "original search response arrived late");
        let outcome = store
            .record_download_failure(&q.id, "a", actual.clone())
            .unwrap();
        assert_eq!(outcome.cursor, 1);
        assert_eq!(outcome.status, QueueStatus::Running);
        assert_eq!(outcome.outcomes[0].status, "scope_changed");
        assert_eq!(
            outcome.outcomes[0].error.as_ref().unwrap().message,
            actual.message
        );
        assert_eq!(
            outcome.outcomes[0].scope_error.as_ref().unwrap().code,
            "QUEUE_TARGET_CHANGED"
        );
        assert_eq!(
            serde_json::to_value(store.task("a").unwrap()).unwrap(),
            before
        );
        let reopened = Store::new(dir.path()).unwrap();
        reopened.recover().unwrap();
        assert_eq!(
            serde_json::to_value(reopened.task("a").unwrap()).unwrap(),
            before
        );
        let saved = reopened.download_queue(&q.id).unwrap();
        assert_eq!(saved.targets[0].input_hash, "input");
        assert_eq!(
            saved.outcomes[0].error.as_ref().unwrap().code,
            "PAGE_TIMEOUT"
        );
    }
}
#[test]
fn raw_failure_update_cannot_replace_metadata_review_or_platform_identity() {
    let (_dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    let original = store.task("a").unwrap();
    let error = Failure::new("NO_RESULT", "actual zero results");
    let mut forged = original.clone();
    forged.stage = Stage::AwaitingReview;
    forged.last_error = Some(error.clone());
    forged.platform_id = "invented-new-id".into();
    assert_eq!(
        store
            .finish_download_target(&q.id, "a", "review", Some(error.clone()), Some(&mut forged))
            .unwrap_err()
            .code,
        "QUEUE_TARGET_CHANGED"
    );
    assert!(store
        .block_download_queue(&q.id, error, Some(&mut forged))
        .is_err());
    assert_eq!(store.download_queue(&q.id).unwrap().cursor, 0);
    assert!(store.download_queue(&q.id).unwrap().outcomes.is_empty());
    assert_eq!(
        serde_json::to_value(store.task("a").unwrap()).unwrap(),
        serde_json::to_value(original).unwrap()
    );
}
#[test]
fn original_failure_and_roster_conflict_survive_restart_without_overwriting_task_or_wrong_cursor() {
    let (dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    let mut incoming = store.task("a").unwrap().record;
    incoming.title = "Changed roster title".into();
    store.import(vec![incoming], "new-input".into()).unwrap();
    let before = serde_json::to_value(store.task("a").unwrap()).unwrap();
    let saved = store
        .record_download_failure(
            &q.id,
            "a",
            Failure::new("AUTH_REQUIRED", "real original access failure"),
        )
        .unwrap();
    assert_eq!(saved.status, QueueStatus::Blocked);
    assert_eq!(saved.cursor, 1);
    assert_eq!(saved.targets[saved.cursor].id, "d");
    assert_eq!(saved.outcomes[0].status, "scope_changed");
    assert_eq!(
        serde_json::to_value(store.task("a").unwrap()).unwrap(),
        before
    );
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    reopened.resume_download_queue(&q.id).unwrap();
    assert_eq!(
        reopened
            .record_download_failure(
                &q.id,
                "a",
                Failure::new("NO_RESULT", "late duplicate callback")
            )
            .unwrap_err()
            .code,
        "QUEUE_CHANGED"
    );
    assert!(reopened.task("d").unwrap().last_error.is_none());
    assert_eq!(reopened.download_queue(&q.id).unwrap().cursor, 1);
    let (tasks, queues) = reopened.report_snapshot().unwrap();
    let path = dir.path().join("failure-scope-report.xlsx");
    files::export_report_with_queues(&tasks, &queues, &path).unwrap();
    use calamine::{open_workbook_auto, Reader};
    let mut book = open_workbook_auto(path).unwrap();
    let sheet = book.worksheet_range("下载队列结果").unwrap();
    let full = sheet
        .rows()
        .skip(1)
        .filter(|r| r[0].to_string() == q.id && r[5].to_string() == "a")
        .map(|r| r[14].to_string())
        .collect::<String>();
    let audit: serde_json::Value = serde_json::from_str(&full).unwrap();
    assert_eq!(audit["outcome"]["error"]["code"], "AUTH_REQUIRED");
    assert_eq!(
        audit["outcome"]["scope_error"]["code"],
        "QUEUE_TARGET_CHANGED"
    );
    assert_eq!(audit["target"]["record"]["title"], "Paper");
}
#[test]
fn completion_requires_actual_archive_full_fields_and_no_unconfirmed_native_receipt() {
    let (_dir, store) = setup();
    let q = store.start_download_queue("one", false).unwrap();
    assert_eq!(
        store
            .finish_download_target(&q.id, "a", "downloaded", None, None)
            .unwrap_err()
            .code,
        "DOWNLOAD_RESULT_UNKNOWN"
    );
    saved_download(&store, "a");
    let mut original = store.task("a").unwrap();
    let before = original.revision;
    let mut forged = original.clone();
    forged.artifact.as_mut().unwrap().candidate.authors = "forged cached authors".into();
    store
        .save(&mut forged, "fixture_changed_cached_fields")
        .unwrap();
    assert_eq!(
        store
            .finish_download_target(&q.id, "a", "downloaded", None, None)
            .unwrap_err()
            .code,
        "FILE_INVALID"
    );
    original.revision = forged.revision;
    original.stage = Stage::Downloading;
    original.running = true;
    store.save(&mut original, "fixture_native_pending").unwrap();
    let receipt = store
        .prepare_native_download(&original, &original.artifact.as_ref().unwrap().record_url)
        .unwrap();
    original.stage = Stage::Downloaded;
    original.running = false;
    store
        .save(&mut original, "fixture_existing_file_and_pending_receipt")
        .unwrap();
    assert_eq!(
        store
            .finish_download_target(&q.id, "a", "downloaded", None, None)
            .unwrap_err()
            .code,
        "DOWNLOAD_RESULT_UNKNOWN"
    );
    store.abandon_unrequested_download(&receipt.id).unwrap();
    store
        .finish_download_target(&q.id, "a", "downloaded", None, None)
        .unwrap();
    assert_eq!(store.download_queue(&q.id).unwrap().cursor, 1);
    assert_eq!(store.task("a").unwrap().revision, original.revision);
    assert!(original.revision > before);
}
#[test]
fn atomic_failure_entry_keeps_ordinary_failures_running_and_channel_failure_at_original_target() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    store
        .import(
            (0..4)
                .map(|i| record(&i.to_string(), "one", false))
                .collect(),
            "input".into(),
        )
        .unwrap();
    let q = store.start_download_queue("one", false).unwrap();
    for i in 0..3 {
        let id = i.to_string();
        let updated = store
            .record_download_failure(&q.id, &id, Failure::new("NO_RESULT", "actual zero result"))
            .unwrap();
        assert_eq!(updated.status, QueueStatus::Running);
        assert_eq!(updated.cursor, i + 1);
        let task = store.task(&id).unwrap();
        assert_eq!(task.stage, Stage::AwaitingReview);
        let audit: serde_json::Value =
            serde_json::from_str(&task.evidence.last().unwrap().text).unwrap();
        assert_eq!(audit["target"]["input_hash"], "input");
        assert_eq!(audit["target"]["id"], id);
    }
    let blocked = store
        .record_download_failure(
            &q.id,
            "3",
            Failure::new("AUTH_REQUIRED", "channel unavailable"),
        )
        .unwrap();
    assert_eq!(blocked.status, QueueStatus::Blocked);
    assert_eq!(blocked.cursor, 3);
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    assert_eq!(
        reopened.download_queue(&q.id).unwrap().status,
        QueueStatus::Blocked
    );
    reopened.resume_download_queue(&q.id).unwrap();
    saved_download(&reopened, "3");
    reopened
        .finish_download_target(&q.id, "3", "downloaded", None, None)
        .unwrap();
    assert_eq!(
        reopened.download_queue(&q.id).unwrap().status,
        QueueStatus::Completed
    );
}
