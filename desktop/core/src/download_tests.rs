use crate::{download::DownloadReceipt, *};

const SOURCE: &str = "https://webofscience.clarivate.cn/wos/woscc/full-record/WOS:000123456789012";
const EVENT: &str = "blob:https://webofscience.clarivate.cn/native-file";
const TXT: &[u8] = b"TI\tAU\tAF\tSO\tPY\tC1\tUT\tDI\r\nPaper\tTest, A\tAlice Test\tJournal\t2026\tShanghai Jiao Tong Univ\tWOS:000123456789012\t10.1234/test\r\n";
fn setup() -> (tempfile::TempDir, Store, DownloadReceipt) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    store
        .import(
            vec![Record {
                row: 2,
                owner: "one".into(),
                sa_id: "sa1".into(),
                title: "Paper".into(),
                doi: "10.1234/test".into(),
                wos: "".into(),
                staff_id: "001".into(),
                matches: 0,
                item_ids: "".into(),
                mark: "待处理".into(),
                reason: "缺失".into(),
                skipped: false,
                done: false,
                source: "原始名单来源".into(),
            }],
            "input".into(),
        )
        .unwrap();
    let mut task = store.task("sa1").unwrap();
    task.stage = Stage::Downloading;
    task.running = true;
    store.save(&mut task, "export_started").unwrap();
    let receipt = store.prepare_native_download(&task, SOURCE).unwrap();
    (dir, store, receipt)
}
fn finished(store: &Store, receipt: &DownloadReceipt, raw: &[u8]) {
    store.request_native_download(&receipt.id, EVENT).unwrap();
    std::fs::write(&receipt.path, raw).unwrap();
    store
        .finish_native_download(EVENT, std::path::Path::new(&receipt.path), true)
        .unwrap()
        .unwrap();
}
#[test]
fn native_completed_before_task_save_is_adopted_once_at_restart() {
    let (dir, store, receipt) = setup();
    let mut before = store.task("sa1").unwrap();
    before.classification = Some(serde_json::json!({"old_roster_only_suggestion":true}));
    store
        .save(&mut before, "fixture_previous_classification")
        .unwrap();
    let q = store.start_download_queue("one", false).unwrap();
    finished(&store, &receipt, TXT);
    assert!(store.task("sa1").unwrap().artifact.is_none());
    assert_eq!(store.native_downloads("sa1").unwrap()[0].state, "completed");
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    let task = reopened.task("sa1").unwrap();
    let a = task.artifact.as_ref().unwrap();
    assert_eq!(task.stage, Stage::Downloaded);
    assert!(!task.running);
    assert!(task.classification.is_none());
    assert_eq!(std::fs::read(&a.path).unwrap(), TXT);
    assert_eq!(a.candidate.sha256, hash(TXT));
    assert_eq!(a.record_url, SOURCE);
    assert!(a.identity_confirmed);
    let e = task
        .evidence
        .iter()
        .find(|e| e.kind == "native_download")
        .unwrap();
    let persisted: DownloadReceipt = serde_json::from_str(&e.text).unwrap();
    assert_eq!(persisted.record.source, "原始名单来源");
    assert_eq!(persisted.input_hash, "input");
    assert_eq!(persisted.sha256.unwrap(), hash(TXT));
    assert_eq!(
        reopened.native_downloads("sa1").unwrap()[0].state,
        "adopted"
    );
    assert_eq!(reopened.download_queue(&q.id).unwrap().cursor, 0);
    assert_eq!(
        reopened.download_queue(&q.id).unwrap().status,
        queue::QueueStatus::Interrupted
    );
    reopened.recover().unwrap();
    assert_eq!(reopened.task("sa1").unwrap().revision, task.revision);
    assert_eq!(
        std::fs::read_dir(dir.path().join("downloads"))
            .unwrap()
            .count(),
        1
    );
    assert!(reopened.unresolved("sa1").unwrap().is_empty());
}
#[test]
fn only_exact_native_url_and_destination_can_finish_a_receipt() {
    let (_dir, store, receipt) = setup();
    store.request_native_download(&receipt.id, EVENT).unwrap();
    std::fs::write(&receipt.path, TXT).unwrap();
    let other = store.root.join("downloads/manual.txt");
    std::fs::write(&other, TXT).unwrap();
    assert!(store
        .finish_native_download(EVENT, &other, true)
        .unwrap()
        .is_none());
    assert!(store
        .finish_native_download(
            "blob:https://webofscience.clarivate.cn/other",
            std::path::Path::new(&receipt.path),
            true
        )
        .unwrap()
        .is_none());
    assert_eq!(store.native_downloads("sa1").unwrap()[0].state, "requested");
    assert_eq!(
        store.recover_native_download("sa1").unwrap_err().code,
        "DOWNLOAD_RESULT_UNKNOWN"
    );
    store
        .finish_native_download(EVENT, std::path::Path::new(&receipt.path), true)
        .unwrap();
    assert!(store.recover_native_download("sa1").unwrap());
    assert!(store
        .finish_native_download(EVENT, std::path::Path::new(&receipt.path), true)
        .unwrap()
        .is_none());
}
#[test]
fn valid_txt_without_native_completion_never_becomes_a_download_or_retries() {
    let (dir, store, receipt) = setup();
    store.request_native_download(&receipt.id, EVENT).unwrap();
    std::fs::write(&receipt.path, TXT).unwrap();
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    let task = reopened.task("sa1").unwrap();
    assert!(task.artifact.is_none());
    assert_eq!(task.stage, Stage::AwaitingReview);
    assert_eq!(
        task.last_error.as_ref().unwrap().code,
        "DOWNLOAD_RESULT_UNKNOWN"
    );
    assert!(task
        .last_error
        .as_ref()
        .unwrap()
        .message
        .contains(&receipt.path));
    let mut retry = task;
    retry.stage = Stage::Downloading;
    retry.running = true;
    reopened.save(&mut retry, "retry").unwrap();
    assert_eq!(
        reopened
            .prepare_native_download(&retry, SOURCE)
            .unwrap_err()
            .code,
        "DOWNLOAD_RESULT_UNKNOWN"
    );
    // A genuine late Finished still resolves this exact request.
    reopened
        .finish_native_download(EVENT, std::path::Path::new(&receipt.path), true)
        .unwrap();
    reopened.recover().unwrap();
    assert!(reopened.task("sa1").unwrap().artifact.is_some());
}
#[test]
fn changed_completed_file_or_missing_original_cannot_be_replaced_by_a_newer_file() {
    for missing in [false, true] {
        let (dir, store, receipt) = setup();
        finished(&store, &receipt, TXT);
        if missing {
            std::fs::remove_file(&receipt.path).unwrap();
        } else {
            std::fs::write(&receipt.path, [TXT, b"changed"].concat()).unwrap();
        }
        std::fs::write(dir.path().join("downloads/newest.txt"), TXT).unwrap();
        let reopened = Store::new(dir.path()).unwrap();
        reopened.recover().unwrap();
        assert!(reopened.task("sa1").unwrap().artifact.is_none());
        assert_eq!(
            reopened.native_downloads("sa1").unwrap()[0].state,
            "completed"
        );
        assert!(reopened.recover_native_download("sa1").is_err());
    }
}
#[test]
fn complete_receipt_still_requires_single_full_record_and_matching_source() {
    for raw in [
        std::str::from_utf8(TXT)
            .unwrap()
            .replace("WOS:000123456789012", "WOS:000123456789013")
            .into_bytes(),
        [
            TXT,
            &TXT[TXT.iter().position(|b| *b == b'\n').unwrap() + 1..],
        ]
        .concat(),
        b"TI\tUT\r\nPaper\tWOS:000123456789012\r\n".to_vec(),
    ] {
        let (_dir, store, receipt) = setup();
        finished(&store, &receipt, &raw);
        assert!(store.recover_native_download("sa1").is_err());
        assert!(store.task("sa1").unwrap().artifact.is_none());
        assert_eq!(store.native_downloads("sa1").unwrap()[0].state, "completed");
    }
}
#[test]
fn changed_input_skip_or_platform_intent_cannot_be_overwritten_by_completion() {
    for kind in ["input", "skip", "write"] {
        let (_dir, store, receipt) = setup();
        finished(&store, &receipt, TXT);
        let mut task = store.task("sa1").unwrap();
        match kind {
            "input" => {
                let mut r = task.record.clone();
                r.title = "Different paper".into();
                store.import(vec![r], "new-input".into()).unwrap();
            }
            "skip" => {
                task.record.skipped = true;
                store.save(&mut task, "skip").unwrap();
            }
            _ => {
                store.begin_attempt(&mut task, "import_upload").unwrap();
            }
        }
        let before = serde_json::to_value(store.task("sa1").unwrap()).unwrap();
        assert_eq!(
            store.recover_native_download("sa1").unwrap_err().code,
            "TASK_CHANGED"
        );
        assert_eq!(
            serde_json::to_value(store.task("sa1").unwrap()).unwrap(),
            before
        );
        assert_eq!(store.native_downloads("sa1").unwrap()[0].state, "completed");
    }
}
#[test]
fn receipt_consumption_failure_rolls_back_task_paper_and_evidence() {
    let (_dir, store, receipt) = setup();
    finished(&store, &receipt, TXT);
    store.connect().unwrap().execute_batch("CREATE TRIGGER reject_adoption BEFORE UPDATE ON native_downloads WHEN NEW.state='adopted' BEGIN SELECT RAISE(ABORT,'test receipt commit failed'); END;").unwrap();
    let before = serde_json::to_value(store.task("sa1").unwrap()).unwrap();
    assert!(store.recover_native_download("sa1").is_err());
    assert_eq!(
        serde_json::to_value(store.task("sa1").unwrap()).unwrap(),
        before
    );
    assert_eq!(store.native_downloads("sa1").unwrap()[0].state, "completed");
    let papers: u32 = store
        .connect()
        .unwrap()
        .query_row("SELECT count(*) FROM papers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(papers, 0);
    store
        .connect()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_adoption")
        .unwrap();
    assert!(store.recover_native_download("sa1").unwrap());
}
#[test]
fn unrequested_and_known_failed_downloads_allow_safe_retry_after_restart() {
    for requested in [false, true] {
        let (dir, store, receipt) = setup();
        if requested {
            store.request_native_download(&receipt.id, EVENT).unwrap();
            store
                .finish_native_download(EVENT, std::path::Path::new(&receipt.path), false)
                .unwrap();
        }
        let reopened = Store::new(dir.path()).unwrap();
        reopened.recover().unwrap();
        assert_eq!(
            reopened.native_downloads("sa1").unwrap()[0].state,
            if requested { "failed" } else { "abandoned" }
        );
        let mut task = reopened.task("sa1").unwrap();
        task.running = true;
        reopened.save(&mut task, "retry").unwrap();
        assert_ne!(
            reopened.prepare_native_download(&task, SOURCE).unwrap().id,
            receipt.id
        );
    }
}
#[test]
fn source_report_retains_unconfirmed_receipt_without_claiming_a_file_is_ready() {
    let (_dir, store, receipt) = setup();
    store.request_native_download(&receipt.id, EVENT).unwrap();
    let (tasks, _) = store.report_snapshot().unwrap();
    assert!(tasks[0].artifact.is_none());
    assert_eq!(tasks[0].stage, Stage::Downloading);
    let evidence = tasks[0]
        .evidence
        .iter()
        .find(|e| e.kind == "native_download_receipt")
        .unwrap();
    let saved: DownloadReceipt = serde_json::from_str(&evidence.text).unwrap();
    assert_eq!(saved.id, receipt.id);
    assert_eq!(saved.state, "requested");
    assert_eq!(saved.record.source, receipt.record.source);
    assert_eq!(saved.path, receipt.path);
    assert!(saved.sha256.is_none());
    assert!(saved.finished.is_none());
    assert_eq!(evidence.source, SOURCE);
    assert!(
        store.task("sa1").unwrap().evidence.is_empty(),
        "Report views must not mutate task evidence"
    );
}
#[test]
fn explicit_new_input_version_preserves_old_receipt_but_can_download_its_own_paper() {
    for complete in [false, true] {
        let (_dir, store, receipt) = setup();
        if complete {
            finished(&store, &receipt, TXT);
        } else {
            store.request_native_download(&receipt.id, EVENT).unwrap();
        }
        let mut incoming = store.task("sa1").unwrap().record;
        incoming.title = "Different paper".into();
        incoming.doi = "10.1234/new".into();
        store
            .import(vec![incoming.clone()], "new-input".into())
            .unwrap();
        let proposal = store.pending_input("sa1").unwrap().unwrap();
        let live = serde_json::json!({"row":{"saLzkId":incoming.sa_id,"titleValue":incoming.title,"gh":incoming.staff_id,"doiValue":incoming.doi,"wosValue":incoming.wos,"matchCount":incoming.matches,"itemId":incoming.item_ids,"markStatus":incoming.mark,"reason":incoming.reason}});
        assert_eq!(
            store.recover_native_download("sa1").unwrap_err().code,
            if complete {
                "TASK_CHANGED"
            } else {
                "DOWNLOAD_RESULT_UNKNOWN"
            }
        );
        let task = store.task("sa1").unwrap();
        let mut next = store
            .accept_input(
                &task,
                &proposal,
                &live,
                "明确核对新名单",
                "新题名、标识符与完整工号已回读",
            )
            .unwrap();
        assert!(next.artifact.is_none());
        assert!(!store.recover_native_download("sa1").unwrap());
        assert_eq!(
            store.native_downloads("sa1").unwrap()[0].state,
            "superseded"
        );
        let raw: String = store
            .connect()
            .unwrap()
            .query_row(
                "SELECT data FROM native_downloads WHERE id=?",
                [&receipt.id],
                |r| r.get(0),
            )
            .unwrap();
        let audit: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            audit["superseded_from"],
            if complete { "completed" } else { "requested" }
        );
        assert_eq!(audit["record"]["title"], "Paper");
        assert_eq!(audit["input_hash"], "input");
        next.stage = Stage::Downloading;
        next.running = true;
        store.save(&mut next, "new_export").unwrap();
        let new = store.prepare_native_download(&next, SOURCE).unwrap();
        assert_eq!(new.record.title, "Different paper");
        assert_eq!(new.input_hash, "new-input");
        assert_ne!(new.id, receipt.id);
        assert!(
            store
                .finish_native_download(EVENT, std::path::Path::new(&receipt.path), true)
                .unwrap()
                .is_none(),
            "Old late callback cannot attach to new input"
        );
    }
}
