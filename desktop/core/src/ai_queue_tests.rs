use crate::ai_queue::*;
use crate::queue::QueueStatus;
use crate::*;
use calamine::{open_workbook_auto, Reader};
use serde_json::{json, Value};
fn records() -> Vec<Record> {
    (1..=4).map(|i|serde_json::from_value(json!({"row":i+1,"owner":"AI owner","sa_id":format!("ai-{i}"),"title":format!("Paper {i}"),"doi":"","wos":"","staff_id":"000000000000001","matches":0,"item_ids":"","mark":"待处理","reason":"原始原因完整保留","skipped":false,"done":false,"source":"list.xlsx"})).unwrap()).collect()
}
fn cfg() -> Value {
    config("https://example.test/v1", "test-model")
}
fn fixture() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    store.import(records(), "roster-hash".into()).unwrap();
    (dir, store)
}
fn saved(store: &Store, q: &AiQueue, a: &Attempt) -> Task {
    let mut t = store.task(&a.task_id).unwrap();
    let sources = classification::sources(&store.root, &t).unwrap();
    let v = json!({"type":"期刊论文","channel":"general","confidence":"低","reason":"题名候选，待核实","channel_reason":"确认实际模板与收录条件","missing":["实际出版来源"],"fields":{},"evidence_ids":[sources[0].id]});
    catalog::validate_ai(&v, &sources, q.template.as_ref()).unwrap();
    let mut v = classification::bind_result(&t, v, q.template.as_ref());
    v["queue_id"] = json!(q.id);
    v["queue_attempt_id"] = json!(a.id);
    v["source_hash"] = json!(q.targets[q.cursor].source_hash);
    v["model_config"] = q.config.clone();
    t.evidence.push(classification::audit(
        &t,
        &v,
        &sources,
        "test-model",
        q.template.as_ref(),
    ));
    t.classification = Some(v);
    store.save(&mut t, "ai_classified").unwrap();
    t
}
#[test]
fn three_ordinary_ai_failures_do_not_stop_fourth_and_preserve_stage_and_input() {
    let (_dir, store) = fixture();
    let q = store.start_ai_queue("AI owner", true, cfg(), None).unwrap();
    for _ in 0..3 {
        store.begin_ai_target(&q.id).unwrap();
        let q = store
            .finish_ai_target(
                &q.id,
                "failed",
                Some(Failure::new("AI_RESULT_INVALID", "不合格结果，未采纳")),
            )
            .unwrap();
        assert_eq!(q.status, QueueStatus::Running);
    }
    let current = store.ai_queue(&q.id).unwrap();
    let a = store.begin_ai_target(&q.id).unwrap();
    let task = saved(&store, &current, &a);
    let ended = store.finish_ai_target(&q.id, "classified", None).unwrap();
    assert_eq!(ended.status, QueueStatus::Completed);
    assert_eq!(ended.cursor, 4);
    assert_eq!(task.stage, Stage::Pending);
    assert!(task.review.is_none() && task.artifact.is_none());
    assert_eq!(
        ended
            .targets
            .iter()
            .map(|t| t.record.reason.as_str())
            .collect::<Vec<_>>(),
        vec!["原始原因完整保留"; 4]
    );
    let first = store.task("ai-1").unwrap();
    assert_eq!(first.stage, Stage::Pending);
    assert_eq!(first.last_error.unwrap().code, "AI_RESULT_INVALID");
    assert_eq!(
        classification::sources(&store.root, &store.task("ai-1").unwrap())
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn pause_during_request_persists_after_success_and_resume_keeps_exact_scope() {
    let (dir, store) = fixture();
    let q = store.start_ai_queue("AI owner", true, cfg(), None).unwrap();
    let a = store.begin_ai_target(&q.id).unwrap();
    store.request_ai_pause().unwrap();
    saved(&store, &q, &a);
    let paused = store.finish_ai_target(&q.id, "classified", None).unwrap();
    assert_eq!(paused.status, QueueStatus::Paused);
    drop(store);
    let store = Store::new(dir.path()).unwrap();
    store.recover().unwrap();
    let original = store.ai_queue(&q.id).unwrap();
    assert_eq!(original.cursor, 1);
    assert_eq!(original.targets.len(), 4);
    let mut added = records()[0].clone();
    added.sa_id = "ai-added".into();
    added.row = 99;
    store.import(vec![added], "new-roster".into()).unwrap();
    store.resume_ai_queue(&q.id, &cfg(), None).unwrap();
    assert_eq!(store.ai_queue(&q.id).unwrap().targets.len(), 4);
    let mut t = store.task("ai-2").unwrap();
    t.evidence.push(Evidence {
        id: "new-source".into(),
        kind: "human_review".into(),
        source: "manual".into(),
        text: "Changed source".into(),
        created: 1,
    });
    store.save(&mut t, "new_source").unwrap();
    assert!(store.begin_ai_target(&q.id).is_err());
    store
        .finish_ai_target(
            &q.id,
            "not_executed",
            Some(Failure::new("AI_TARGET_CHANGED", "来源变化")),
        )
        .unwrap();
    assert_eq!(store.ai_queue(&q.id).unwrap().cursor, 2);
    assert!(store.task("ai-added").unwrap().classification.is_none());
}
#[test]
fn restart_between_saved_result_and_cursor_advance_recovers_without_resending() {
    let (dir, store) = fixture();
    let q = store.start_ai_queue("AI owner", true, cfg(), None).unwrap();
    let a = store.begin_ai_target(&q.id).unwrap();
    saved(&store, &q, &a);
    drop(store);
    let store = Store::new(dir.path()).unwrap();
    store.recover().unwrap();
    let restored = store.ai_queue(&q.id).unwrap();
    assert_eq!(restored.cursor, 1);
    assert_eq!(restored.outcomes[0].status, "classified");
    assert_eq!(restored.status, QueueStatus::Interrupted);
    assert!(restored.inflight.is_none());
    assert_eq!(restored.outcomes[0].attempt.as_ref().unwrap().id, a.id);
    assert!(store.begin_ai_target(&q.id).is_err());
    store.resume_ai_queue(&q.id, &cfg(), None).unwrap();
    let next = store.begin_ai_target(&q.id).unwrap();
    assert_eq!(next.task_id, "ai-2");
}
#[test]
fn unsaved_request_is_unconfirmed_and_full_original_input_survives_report() {
    let (dir, store) = fixture();
    let mut task = store.task("ai-1").unwrap();
    task.evidence.push(Evidence {
        id: "long-original-source".into(),
        kind: "manual".into(),
        source: "isolated original source".into(),
        text: "完整原文🧪".repeat(5000),
        created: 1,
    });
    store.save(&mut task, "source_added").unwrap();
    let q = store.start_ai_queue("AI owner", true, cfg(), None).unwrap();
    let a = store.begin_ai_target(&q.id).unwrap();
    drop(store);
    let store = Store::new(dir.path()).unwrap();
    store.recover().unwrap();
    let restored = store.ai_queue(&q.id).unwrap();
    assert_eq!(restored.cursor, 1);
    assert_eq!(restored.outcomes[0].status, "unconfirmed");
    assert_eq!(
        restored.outcomes[0].attempt.as_ref().unwrap().input,
        a.input
    );
    assert!(store.task("ai-1").unwrap().classification.is_none());
    let report = dir.path().join("ai-queue-report.xlsx");
    files::export_report_with_runs(
        &store.tasks().unwrap(),
        &[],
        &[],
        &store.ai_queues().unwrap(),
        &report,
    )
    .unwrap();
    let mut book = open_workbook_auto(report).unwrap();
    let sheet = book.worksheet_range("AI 批量结果").unwrap();
    assert!(
        sheet.rows().count() > 3,
        "Long input must span multiple report cells"
    );
    let raw = sheet
        .rows()
        .skip(1)
        .map(|r| r[9].to_string())
        .collect::<String>();
    let reconstructed: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(reconstructed, serde_json::to_value(&restored).unwrap());
    assert_eq!(
        sheet.rows().nth(1).unwrap()[7].to_string(),
        hash(raw.as_bytes())
    );
    store.resume_ai_queue(&q.id, &cfg(), None).unwrap();
    assert_eq!(store.begin_ai_target(&q.id).unwrap().task_id, "ai-2");
}
#[test]
fn rate_limit_blocks_remaining_and_changed_configuration_cannot_resume() {
    let (_dir, store) = fixture();
    let q = store.start_ai_queue("AI owner", true, cfg(), None).unwrap();
    store.begin_ai_target(&q.id).unwrap();
    let blocked = store
        .finish_ai_target(
            &q.id,
            "failed",
            Some(Failure::new("AI_RATE_LIMIT", "API 限流")),
        )
        .unwrap();
    assert_eq!(blocked.status, QueueStatus::Blocked);
    assert_eq!(blocked.cursor, 1);
    assert!(store.start_ai_queue("AI owner", true, cfg(), None).is_err());
    assert!(store
        .resume_ai_queue(&q.id, &config("https://other.test", "other-model"), None)
        .is_err());
    store.resume_ai_queue(&q.id, &cfg(), None).unwrap();
    assert_eq!(store.begin_ai_target(&q.id).unwrap().task_id, "ai-2");
}
#[test]
fn only_complete_matching_audit_is_reused_or_recovers_as_saved() {
    let mut rs = records();
    rs.truncate(1);
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    store.import(rs, "roster-hash".into()).unwrap();
    let q = store.start_ai_queue("AI owner", true, cfg(), None).unwrap();
    let a = store.begin_ai_target(&q.id).unwrap();
    let task = saved(&store, &q, &a);
    store.finish_ai_target(&q.id, "classified", None).unwrap();
    let new = store.start_ai_queue("AI owner", true, cfg(), None).unwrap();
    assert!(store.reuse_ai_target(&new.id).unwrap());
    store.finish_ai_target(&new.id, "reused", None).unwrap();
    assert_eq!(store.task(&task.id).unwrap().revision, task.revision);
    let new = store
        .start_ai_queue(
            "AI owner",
            true,
            config("https://example.test/v1", "other-model"),
            None,
        )
        .unwrap();
    assert!(!store.reuse_ai_target(&new.id).unwrap());
    assert!(store.finish_ai_target(&new.id, "reused", None).is_err());
    store.pause_ai_queue(&new.id).unwrap();
    store.cancel_ai_queue(&new.id).unwrap();
    let new = store.start_ai_queue("AI owner", true, cfg(), None).unwrap();
    let a = store.begin_ai_target(&new.id).unwrap();
    let mut forged = store.task(&a.task_id).unwrap();
    let v = forged.classification.as_mut().unwrap();
    v["queue_id"] = json!(new.id);
    v["queue_attempt_id"] = json!(a.id);
    store.save(&mut forged, "forged_markers").unwrap();
    store.recover_ai_queue().unwrap();
    assert_eq!(
        store.ai_queue(&new.id).unwrap().outcomes[0].status,
        "unconfirmed"
    );
}
#[test]
fn invalid_boundary_and_completion_without_result_never_advance_queue() {
    let (_dir, store) = fixture();
    let q = store.start_ai_queue("AI owner", true, cfg(), None).unwrap();
    assert!(store.finish_ai_target(&q.id, "classified", None).is_err());
    assert!(store
        .finish_ai_target(
            &q.id,
            "failed",
            Some(Failure::new("AI_REQUEST_FAILED", "未请求"))
        )
        .is_err());
    store.begin_ai_target(&q.id).unwrap();
    assert!(store.begin_ai_target(&q.id).is_err());
    assert!(store.finish_ai_target(&q.id, "classified", None).is_err());
    assert!(store.finish_ai_target(&q.id, "not_executed", None).is_err());
    assert!(store.cancel_ai_queue(&q.id).is_err());
    assert_eq!(store.ai_queue(&q.id).unwrap().cursor, 0);
}
#[test]
fn owner_zero_filter_skip_done_and_templates_are_frozen_without_credentials() {
    let (_dir, store) = fixture();
    for id in ["ai-2", "ai-3", "ai-4"] {
        let mut t = store.task(id).unwrap();
        match id {
            "ai-2" => t.record.matches = 1,
            "ai-3" => t.record.skipped = true,
            _ => t.record.done = true,
        };
        store.save(&mut t, "scope").unwrap();
    }
    let template = json!({"id":"original-template","fingerprint":"original-bytes","columns":[],"notes":"原枚举要求"});
    let q = store
        .start_ai_queue("AI owner", true, cfg(), Some(template.clone()))
        .unwrap();
    assert_eq!(q.targets.len(), 1);
    assert!(!serde_json::to_string(&q).unwrap().contains("api-key"));
    store.pause_ai_queue(&q.id).unwrap();
    let mut changed = template.clone();
    changed["notes"] = json!("changed");
    assert!(store
        .resume_ai_queue(&q.id, &cfg(), Some(&changed))
        .is_err());
    store.cancel_ai_queue(&q.id).unwrap();
    let all = store
        .start_ai_queue("AI owner", false, cfg(), None)
        .unwrap();
    assert_eq!(all.targets.len(), 2);
}
#[test]
fn changed_current_source_cannot_recover_saved_result_even_with_original_audit() {
    let (dir, store) = fixture();
    let mut t = store.task("ai-1").unwrap();
    t.evidence.push(Evidence {
        id: "original-source".into(),
        kind: "manual".into(),
        source: "isolated".into(),
        text: "Original factual source".into(),
        created: 1,
    });
    store.save(&mut t, "source_added").unwrap();
    let q = store.start_ai_queue("AI owner", true, cfg(), None).unwrap();
    let a = store.begin_ai_target(&q.id).unwrap();
    let mut saved = saved(&store, &q, &a);
    saved
        .evidence
        .iter_mut()
        .find(|e| e.id == "original-source")
        .unwrap()
        .text = "Changed source".into();
    // Keep revision and the full original AI audit to exercise source validation itself.
    store
        .connect()
        .unwrap()
        .execute(
            "UPDATE tasks SET data=? WHERE id=?",
            rusqlite::params![serde_json::to_string(&saved).unwrap(), saved.id],
        )
        .unwrap();
    drop(store);
    let store = Store::new(dir.path()).unwrap();
    store.recover().unwrap();
    let q = store.ai_queue(&q.id).unwrap();
    assert_eq!(q.outcomes[0].status, "unconfirmed");
    assert_eq!(q.status, QueueStatus::Interrupted);
    assert_eq!(store.task("ai-1").unwrap().stage, Stage::Pending);
    assert!(store.task("ai-1").unwrap().classification.is_some());
}
#[test]
fn failed_ai_request_preserves_existing_business_error_and_adds_separate_audit() {
    let (_dir, store) = fixture();
    let mut t = store.task("ai-1").unwrap();
    t.last_error = Some(Failure::new("BUSINESS_PENDING", "Original business reason"));
    store.save(&mut t, "original_business_error").unwrap();
    let q = store.start_ai_queue("AI owner", true, cfg(), None).unwrap();
    store.begin_ai_target(&q.id).unwrap();
    store
        .finish_ai_target(
            &q.id,
            "failed",
            Some(Failure::new("AI_RESULT_INVALID", "Rejected AI output")),
        )
        .unwrap();
    let t = store.task("ai-1").unwrap();
    assert_eq!(t.last_error.unwrap().code, "BUSINESS_PENDING");
    assert_eq!(t.stage, Stage::Pending);
    assert!(t
        .evidence
        .iter()
        .any(|e| e.kind == "ai_failure" && e.text.contains("Rejected AI output")));
    assert_eq!(
        store.ai_queue(&q.id).unwrap().outcomes[0]
            .error
            .as_ref()
            .unwrap()
            .code,
        "AI_RESULT_INVALID"
    );
}
