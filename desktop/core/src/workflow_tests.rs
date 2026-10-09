use crate::*;
use serde_json::json;

fn record() -> Record {
    Record {
        row: 2,
        owner: "测试".into(),
        sa_id: "sa1".into(),
        title: "Paper".into(),
        doi: "10.1234/test".into(),
        wos: "".into(),
        staff_id: "001".into(),
        matches: 2,
        item_ids: ",a,b".into(),
        mark: "待处理".into(),
        reason: "重复".into(),
        skipped: false,
        done: false,
        source: "".into(),
    }
}
fn attempt_state(store: &Store, id: &str) -> String {
    rusqlite::Connection::open(store.root.join("workspace.sqlite3"))
        .unwrap()
        .query_row("SELECT state FROM attempts WHERE id=?", [id], |r| r.get(0))
        .unwrap()
}
#[test]
fn verified_write_and_task_stage_survive_restart_together() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    store.import(vec![record()], "input".into()).unwrap();
    let mut task = store.task("sa1").unwrap();
    let attempt = store
        .begin_attempt_with_payload(&mut task, "import_push", json!({"batch_id":"batch1"}))
        .unwrap();
    task.stage = Stage::Pushed;
    task.batch = Some(json!({"id":"batch1","status":2}));
    task.running = false;
    store
        .finish_attempt(
            &mut task,
            &attempt,
            "verified",
            json!({"verified":true}),
            "step_finished",
        )
        .unwrap();
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    let restored = reopened.task("sa1").unwrap();
    assert_eq!(restored.stage, Stage::Pushed);
    assert!(!restored.running);
    assert_eq!(restored.batch.unwrap()["id"], "batch1");
    assert!(reopened.unresolved("sa1").unwrap().is_empty());
    assert_eq!(attempt_state(&reopened, &attempt), "verified");
}
#[test]
fn stale_finish_rolls_back_receipt_and_keeps_write_unknown_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    store.import(vec![record()], "input".into()).unwrap();
    let mut task = store.task("sa1").unwrap();
    let attempt = store
        .begin_attempt_with_payload(
            &mut task,
            "merge_duplicate",
            json!({"source_id":"b","target_id":"a"}),
        )
        .unwrap();
    let mut concurrent = store.task("sa1").unwrap();
    store.save(&mut concurrent, "other_event").unwrap();
    task.running = false;
    task.platform_id = "a".into();
    assert_eq!(
        store
            .finish_attempt(
                &mut task,
                &attempt,
                "verified",
                json!({"verified":true}),
                "step_finished"
            )
            .unwrap_err()
            .code,
        "TASK_CHANGED"
    );
    assert_eq!(attempt_state(&store, &attempt), "intent");
    store.recover().unwrap();
    let mut restored = store.task("sa1").unwrap();
    assert_eq!(restored.stage, Stage::Unknown);
    assert!(store
        .begin_attempt(&mut restored, "merge_duplicate")
        .is_err());
    assert!(restored.platform_id.is_empty());
}
#[test]
fn readback_is_atomic_scoped_and_keeps_original_payload() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    let mut second = record();
    second.sa_id = "sa2".into();
    store
        .import(vec![record(), second], "input".into())
        .unwrap();
    let mut task = store.task("sa1").unwrap();
    let attempt = store
        .begin_attempt_with_payload(
            &mut task,
            "merge_duplicate",
            json!({"source_id":"b","target_id":"a"}),
        )
        .unwrap();
    store.recover().unwrap();
    let mut other = store.task("sa2").unwrap();
    assert!(store
        .verify_attempt(&mut other, &attempt, json!({"verified":true}), "recovered")
        .is_err());
    assert_eq!(attempt_state(&store, &attempt), "unknown");
    let mut restored = store.task("sa1").unwrap();
    restored.stage = Stage::AwaitingReview;
    restored.platform_id = "a".into();
    restored.running = false;
    restored.last_error = None;
    store
        .verify_attempt(
            &mut restored,
            &attempt,
            json!({"verified":true}),
            "recovered",
        )
        .unwrap();
    store.recover().unwrap();
    assert_eq!(store.task("sa1").unwrap().stage, Stage::AwaitingReview);
    let db = rusqlite::Connection::open(dir.path().join("workspace.sqlite3")).unwrap();
    let raw: String = db
        .query_row("SELECT data FROM attempts WHERE id=?", [attempt], |r| {
            r.get(0)
        })
        .unwrap();
    let audit: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(audit["payload"]["source_id"], "b");
    assert_eq!(audit["verification"]["verified"], true);
}
#[test]
fn imported_batch_cannot_resolve_unknown_push() {
    let imported = json!({"verified":true,"batch":{"status":1}});
    assert_eq!(
        verified_import_stage(&imported, false).unwrap(),
        Stage::Imported
    );
    assert_eq!(
        verified_import_stage(&imported, true).unwrap_err().code,
        "REMOTE_RESULT_UNKNOWN"
    );
    assert_eq!(
        verified_import_stage(&json!({"verified":true,"batch":{"status":"2"}}), true).unwrap(),
        Stage::Pushed
    );
}
#[test]
fn upload_recovery_requires_original_task_file_and_success_receipt() {
    let payload = json!({"sa_id":"sa1","instructions":"SA补充-sa1","contentSha":"a".repeat(64),"candidate":{"sha256":"a".repeat(64)}});
    let result = json!({"verified":true,"uploaded":true,"sa_id":"sa1","instructions":"SA补充-sa1","sha256":"a".repeat(64),
        "filename":"SA-WOS-sa1.txt","size":14,"dataset_label":"上海交通大学","dataset_id":"institution1",
        "server_name":"original-object.txt","response":{"success":true,"data":{"name":"original-object.txt"}}});
    validate_upload_readback(&result, &payload, 14).unwrap();
    for (key, value) in [
        ("verified", json!(false)),
        ("uploaded", json!(false)),
        ("sa_id", json!("other")),
        ("instructions", json!("SA补充-other")),
        ("sha256", json!("b".repeat(64))),
        ("filename", json!("other.txt")),
        ("size", json!(15)),
        ("dataset_label", json!("其他机构")),
        ("dataset_id", json!("")),
        ("server_name", json!("other.txt")),
        (
            "response",
            json!({"success":false,"code":500,"data":{"name":"original-object.txt"}}),
        ),
    ] {
        let mut changed = result.clone();
        changed[key] = value;
        assert_eq!(
            validate_upload_readback(&changed, &payload, 14)
                .unwrap_err()
                .code,
            "REMOTE_RESULT_UNKNOWN",
            "{key}"
        );
    }
    let mut changed = payload.clone();
    changed["contentSha"] = json!("b".repeat(64));
    assert!(validate_upload_readback(&result, &changed, 14).is_err());
    assert!(validate_upload_readback(&result, &payload, 0).is_err());
    assert!(validate_upload_readback(&result, &payload, 524289).is_err());
    let mut rejected = result.clone();
    rejected["response"]["success"] = json!(false);
    rejected["response"]["code"] = json!(200);
    assert!(
        validate_upload_readback(&rejected, &payload, 14).is_err(),
        "HTTP 成功不能覆盖业务失败"
    );
}
#[test]
fn duplicate_completion_requires_actual_merge_history() {
    let mut task = Task::new(record(), "input".into());
    task.platform_id = "a".into();
    task.review = Some(Review {
        route: Route::Duplicate,
        evidence_id: "proof".into(),
        library_checked: true,
        platform_id: "a".into(),
        affiliation_confirmed: true,
        identity_confirmed: true,
        issues_resolved: true,
        note: "已核对".into(),
    });
    assert!(task.assert_complete().is_err());
    task.evidence.push(Evidence {
        id: "merged".into(),
        kind: "duplicate_merge_verified".into(),
        source: "平台回读".into(),
        text: "{}".into(),
        created: 1,
    });
    task.merges.push(MergeRecord {
        group_id: "g".into(),
        source_id: "b".into(),
        target_id: "a".into(),
        evidence_id: "merged".into(),
        created: 1,
    });
    task.assert_complete().unwrap();
    task.platform_id = "other".into();
    assert!(task.assert_complete().is_err());
}
#[test]
fn changing_master_tracks_transitive_merges_and_rejects_disconnected_history() {
    let mut task = Task::new(record(), "input".into());
    task.platform_id = "c".into();
    for (source, target) in [("b", "a"), ("a", "c")] {
        let e = source.to_string();
        task.evidence.push(Evidence {
            id: e.clone(),
            kind: "duplicate_merge_verified".into(),
            source: "平台".into(),
            text: "{}".into(),
            created: 1,
        });
        task.merges.push(MergeRecord {
            group_id: "g".into(),
            source_id: source.into(),
            target_id: target.into(),
            evidence_id: e,
            created: 1,
        });
    }
    assert!(task.has_verified_merges());
    task.merges[0].target_id = "unrelated".into();
    assert!(!task.has_verified_merges());
}
#[test]
fn merge_requires_current_sa_members_and_paper_source() {
    let mut task = Task::new(record(), "input".into());
    let ids = vec!["a".into(), "b".into()];
    task.evidence.push(Evidence {
        id: "proof".into(),
        kind: "human_review".into(),
        source: "真实论文".into(),
        text: "已核对为同篇，保留准确字段".into(),
        created: 1,
    });
    validate_merge_review(
        &task,
        "b",
        "a",
        &ids,
        "proof",
        true,
        "保留主条目作者和出版信息",
    )
    .unwrap();
    assert!(validate_merge_review(&task, "outside", "a", &ids, "proof", true, "说明").is_err());
    assert!(validate_merge_review(&task, "b", "a", &ids, "proof", false, "说明").is_err());
    task.evidence[0].kind = "alias_verified".into();
    assert!(validate_merge_review(&task, "b", "a", &ids, "proof", true, "说明").is_err());
}
#[test]
fn live_matched_ids_keep_text_precision_and_validate_count() {
    assert_eq!(
        matched_ids(&json!({"matchCount":"2","itemId":",1234567890123456789,00001"})).unwrap(),
        vec!["1234567890123456789", "00001"]
    );
    assert!(matched_ids(&json!({"matchCount":2,"itemId":"a,a"})).is_err());
    assert!(matched_ids(&json!({"matchCount":2,"itemId":"a"})).is_err());
}
