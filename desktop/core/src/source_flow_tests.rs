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

#[test]
fn corrected_detail_title_uses_real_doi_and_conflicting_doi_cannot_be_adopted() {
    let (_dir, store, session, task) = fixture();
    let mut data = page();
    data["title"] = json!("Corrected title");
    let (draft, _) = capture(&store.root, &session.site, &task, &data).unwrap();
    assert!(std::fs::read_to_string(draft.path)
        .unwrap()
        .contains("TI  - Corrected title"));
    data["fields"]["doi"] = json!(["10.1234/wrong"]);
    assert_eq!(
        capture(&store.root, &session.site, &task, &data)
            .unwrap_err()
            .code,
        "SOURCE_IDENTITY_CONFLICT"
    );
    data["title"] = json!("Paper");
    assert_eq!(
        capture(&store.root, &session.site, &task, &data)
            .unwrap_err()
            .code,
        "SOURCE_IDENTITY_CONFLICT"
    );
}

#[test]
fn report_keeps_all_source_queue_runs_and_reconstructs_full_long_export_checkpoint() {
    use calamine::{open_workbook_auto, Reader};
    let (_dir, store, session, task) = fixture();
    let mut data = page();
    data["visible_text"] = json!("完整原始依据🧬".repeat(6000));
    let (draft, info) = capture(&store.root, &session.site, &task, &data).unwrap();
    store
        .remember_source_capture(&session, &draft, &info)
        .unwrap();
    let flow = store.source_flow(&session.site).unwrap();
    let op = store.arm_source_export(&session, &flow, &data).unwrap();
    let saved = store
        .source_export_update(
            op["id"].as_str().unwrap(),
            "failed",
            None,
            Some(&Failure::new(
                "PAGE_UNSUPPORTED",
                "fixture has no export control",
            )),
        )
        .unwrap();
    let mut first = store.create_source_queue("one", "ei").unwrap();
    first["status"] = json!("paused");
    store.save_source_queue_progress(&mut first).unwrap();
    store.source_queue_control("cancel").unwrap();
    let mut second = store.create_source_queue("one", "ei").unwrap();
    second["status"] = json!("completed");
    store.save_source_queue_progress(&mut second).unwrap();
    let path = store.root.join("source-report.xlsx");
    files::export_report_with_source_runs(&store, &path).unwrap();
    let mut book = open_workbook_auto(&path).unwrap();
    let acquisitions = book.worksheet_range("渠道获取记录").unwrap();
    let matching: Vec<_> = acquisitions
        .rows()
        .skip(1)
        .filter(|r| r[11].to_string() == op["id"].as_str().unwrap())
        .collect();
    assert!(matching.len() > 1);
    let joined: String = matching.iter().map(|r| r[12].to_string()).collect();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&joined).unwrap(),
        saved
    );
    let queues = book.worksheet_range("来源批量队列").unwrap();
    for original in store.source_queues().unwrap() {
        let joined: String = queues
            .rows()
            .skip(1)
            .filter(|r| r[0].to_string() == original["id"].as_str().unwrap())
            .map(|r| r[3].to_string())
            .collect();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&joined).unwrap(),
            original
        );
    }
    assert_eq!(store.source_queues().unwrap().len(), 2);
}

#[test]
fn shared_excel_resave_preserves_original_bytes_and_source_channel() {
    let (_dir, store, _session, task) = fixture();
    let path = store.root.join("source.xls");
    let raw=b"<table><tr><td>Title</td><td>Author</td></tr><tr><td>Paper</td><td>Alice</td></tr></table>";
    std::fs::write(&path, raw).unwrap();
    let draft = cnki_excel::normalize_for_channel(
        &store.root,
        &task,
        "ei",
        &path,
        "EI original.xls",
        "utf-8",
    )
    .unwrap();
    assert_eq!(draft.channel, "ei");
    assert_eq!(draft.format, "xlsx");
    assert_eq!(std::fs::read(&path).unwrap(), raw);
    let receipt = draft.resave.as_ref().unwrap();
    assert_eq!(receipt.original_sha256, hash(raw));
    cnki_excel::verify(
        &store.root,
        Some(receipt),
        std::path::Path::new(&draft.path),
    )
    .unwrap();
}
