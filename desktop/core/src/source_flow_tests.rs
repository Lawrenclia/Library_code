//! Regression sources only; do not execute without user authorization.
use crate::{source_flows::*, *};
use serde_json::json;
fn fixture() -> (tempfile::TempDir, Store, source_downloads::Session, Task) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    let record=serde_json::from_value(json!({"row":2,"owner":"one","sa_id":"s1","title":"Paper","doi":"10.1234/example","wos":"","staff_id":"001","matches":0,"item_ids":"","mark":"","reason":"","skipped":false,"done":false,"source":"list.xlsx"})).unwrap();
    store.import(vec![record], "input".into()).unwrap();
    store
        .save_source_site(source_downloads::Site {
            channel: "ei".into(),
            entry_url: "https://example.org/search".into(),
            download_origins: vec![],
        })
        .unwrap();
    let session = store.source_session("s1", "ei").unwrap();
    let task = store.task("s1").unwrap();
    (dir, store, session, task)
}
fn page() -> serde_json::Value {
    json!({"page_url":"https://example.org/record/one","title":"Paper","complete":true,"fields":{"authors":["One","Two"],"doi":["10.1234/example"],"unknown_field":["first","second"]},"visible_text":"full source text\nsecond line","jsonld":["{\"unknown\":42}"],"metas":[{"name":"citation_custom","value":"retained"}]})
}
#[test]
fn selection_rejects_incomplete_ambiguous_and_foreign_results() {
    let (_dir, _store, session, task) = fixture();
    let mut data = json!({"page_url":"https://example.org/search","query":"Paper","complete":true,"total":1,"records":[{"title":"Paper","url":"https://example.org/record/one"}]});
    assert!(select_result(&session.site, &task, &data).is_ok());
    data["total"] = json!(20);
    assert!(select_result(&session.site, &task, &data).is_err());
    data["total"] = json!(1);
    data["records"][0]["url"] = json!("https://example.org.evil.invalid/record/one");
    assert!(select_result(&session.site, &task, &data).is_err());
    data["records"] = json!([]);
    data["total"] = json!(0);
    assert_eq!(
        select_result(&session.site, &task, &data).unwrap_err().code,
        "SOURCE_NOT_FOUND"
    );
}
#[test]
fn detail_capture_preserves_complete_provenance_and_is_never_original_upload() {
    let (_dir, store, session, task) = fixture();
    let data = page();
    let (draft, info) = capture(&store.root, &session.site, &task, &data).unwrap();
    assert!(draft.options.export_response);
    assert_eq!(draft.format, "ris");
    let raw = std::fs::read_to_string(&draft.path).unwrap();
    let parsed = ris::parse(&raw).unwrap();
    assert_eq!(parsed.len(), 1);
    let provenance: serde_json::Value =
        serde_json::from_str(&parsed[0].fields.iter().find(|(k, _)| k == "N1").unwrap().1).unwrap();
    assert_eq!(provenance["page"], data);
    assert!(info["missing"].as_array().unwrap().len() > 0);
    let op = store
        .remember_source_capture(&session, &draft, &info)
        .unwrap();
    let (fresh, _) = store
        .captured_source("s1", op["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(fresh.sha256, draft.sha256);
    std::fs::write(&draft.path, "changed").unwrap();
    assert!(store
        .captured_source("s1", op["id"].as_str().unwrap())
        .is_err());
}
#[test]
fn export_unknown_survives_restart_and_prevents_another_issue() {
    let (_dir, store, session, _task) = fixture();
    let flow = store.source_flow(&session.site).unwrap();
    let op = store.arm_source_export(&session, &flow, &page()).unwrap();
    store.recover_source_exports().unwrap();
    assert_eq!(
        store.source_acquisitions("s1").unwrap()[0]["state"],
        "unknown"
    );
    assert!(store.arm_source_export(&session, &flow, &page()).is_err());
    assert_eq!(
        store
            .recheck_source_export("s1", op["id"].as_str().unwrap(), None)
            .unwrap()["state"],
        "unknown"
    );
    assert!(store
        .cancel_source_export("s1", op["id"].as_str().unwrap(), "x")
        .is_err());
    store
        .cancel_source_export(
            "s1",
            op["id"].as_str().unwrap(),
            "窗口已关闭，核对后确认下载未完成",
        )
        .unwrap();
    assert!(store.arm_source_export(&session, &flow, &page()).is_ok());
}
#[test]
fn queue_keeps_pause_request_and_scope_on_progress_and_restart() {
    let (_dir, store, _session, _task) = fixture();
    let mut queue = store.create_source_queue("one", "ei").unwrap();
    store.source_queue_control("pause").unwrap();
    store.save_source_queue_progress(&mut queue).unwrap();
    assert_eq!(queue["pause_requested"], true);
    assert_eq!(queue["entries"][0]["input_hash"], "input");
    store.recover_source_exports().unwrap();
    assert_eq!(
        store.source_queue().unwrap().unwrap()["status"],
        "interrupted"
    );
    assert!(store.create_source_queue("one", "ei").is_err());
    store.source_queue_control("resume").unwrap();
    assert!(store.source_queue_control("resume").is_err());
}
