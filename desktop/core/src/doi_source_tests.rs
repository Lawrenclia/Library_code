//! Deferred registry source scenarios. No external API requests are made by these tests.
use crate::*;
use serde_json::{json, Value};
use std::fs;
fn fixture() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    let record = serde_json::from_value(json!({"row":2,"owner":"private-owner","sa_id":"doi-task","title":"Old title","doi":"https://doi.org/10.1234/PAPER","wos":"","staff_id":"private-staff","matches":0,"item_ids":"","mark":"pending","reason":"original reason","skipped":false,"done":false,"source":"list.xlsx"})).unwrap();
    store.import(vec![record], "input-v1".into()).unwrap();
    (dir, store)
}
fn response(doi: &str, title: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"status":"ok","message-type":"work","message-version":"1.0.0","message":{
        "DOI":doi,"title":[title],"type":"journal-article","publisher":"Publisher","container-title":["Journal"],
        "author":[{"given":"A","family":"B","ORCID":"https://orcid.org/0000-0000-0000-0001","affiliation":[{"name":"Actual source institution"}]}],
        "abstract":"<jats:p>Complete original abstract</jats:p>","published":{"date-parts":[[2025,3,1]]},
        "reference":[{"key":"one","unstructured":"Long complete reference ".repeat(10000)}],"unknown-provider-field":{"kept":true}}})).unwrap()
}
#[test]
fn archive_and_complete_facts_survive_reopen_without_institution_or_task_progress() {
    let (dir, store) = fixture();
    let before = store.task("doi-task").unwrap();
    let request = doi_sources::request(&store, &before.id).unwrap();
    let url = url::Url::parse(&request.url).unwrap();
    assert_eq!(url.host_str(), Some("api.crossref.org"));
    assert_eq!(url.scheme(), "https");
    assert!(url.query().is_none() && url.fragment().is_none());
    assert!(!request.url.contains("private-owner") && !request.url.contains("private-staff"));
    let raw = response("10.1234/paper", "Correct complete title");
    let saved = doi_sources::attach(&store, &request, &raw).unwrap();
    let receipt: doi_sources::Receipt =
        serde_json::from_str(saved["evidence"]["text"].as_str().unwrap()).unwrap();
    assert_eq!(fs::read(&receipt.archive_path).unwrap(), raw);
    assert!(!receipt.institution_verified && !receipt.matches_input_title);
    assert!(receipt.missing.is_empty());
    assert_eq!(receipt.metadata["unknown-provider-field"]["kept"], true);
    let reopened = Store::new(dir.path()).unwrap();
    let after = reopened.task("doi-task").unwrap();
    assert_eq!(json!(before.record), json!(after.record));
    assert_eq!(before.stage, after.stage);
    assert!(after.artifact.is_none() && after.review.is_none() && after.classification.is_none());
    let facts = classification::sources(&reopened.root, &after).unwrap();
    assert!(facts
        .iter()
        .any(|e| e.id == saved["evidence"]["id"].as_str().unwrap()));
    assert!(!facts
        .iter()
        .any(|e| e.text.contains("private-staff") || e.text.contains("private-owner")));
    let request = doi_sources::request(&reopened, &after.id).unwrap();
    assert_eq!(
        doi_sources::attach(&reopened, &request, &raw).unwrap()["reused"],
        true
    );
    assert_eq!(reopened.task(&after.id).unwrap().revision, after.revision);
    assert!(submission::check_upload(&reopened, &after).is_err());
}
#[test]
fn latest_response_is_used_while_old_full_evidence_and_files_remain() {
    let (_dir, store) = fixture();
    let request = doi_sources::request(&store, "doi-task").unwrap();
    let old = doi_sources::attach(
        &store,
        &request,
        &response("10.1234/paper", "Older complete title"),
    )
    .unwrap();
    let request = doi_sources::request(&store, "doi-task").unwrap();
    let new = doi_sources::attach(
        &store,
        &request,
        &response("10.1234/paper", "New complete title"),
    )
    .unwrap();
    let task = store.task("doi-task").unwrap();
    let ids = doi_sources::verify(&store.root, &task).unwrap();
    assert_eq!(ids.len(), 1);
    assert!(ids.contains(new["evidence"]["id"].as_str().unwrap()));
    assert!(!ids.contains(old["evidence"]["id"].as_str().unwrap()));
    assert_eq!(doi_sources::current_receipts(&task).len(), 2);
    let report = store.root.join("source-report.xlsx");
    files::export_report(&[task], &report).unwrap();
    use calamine::Reader;
    let mut book = calamine::open_workbook_auto(&report).unwrap();
    let rows = book.worksheet_range("任务与来源").unwrap();
    let paths = rows.get_value((1, 8)).unwrap().to_string();
    let raw_old: doi_sources::Receipt =
        serde_json::from_str(old["evidence"]["text"].as_str().unwrap()).unwrap();
    assert!(paths.contains(&raw_old.archive_path));
}
#[test]
fn mismatched_identity_cache_and_live_input_version_are_rejected() {
    let (_dir, store) = fixture();
    let request = doi_sources::request(&store, "doi-task").unwrap();
    assert!(
        doi_sources::attach(&store, &request, &response("10.1234/other", "Other paper")).is_err()
    );
    let saved = doi_sources::attach(&store, &request, &response("10.1234/paper", "Paper")).unwrap();
    let mut task = store.task("doi-task").unwrap();
    let mut receipt: Value =
        serde_json::from_str(saved["evidence"]["text"].as_str().unwrap()).unwrap();
    receipt["metadata"]["author"][0]["family"] = json!("Invented name");
    task.evidence
        .iter_mut()
        .find(|e| e.kind == doi_sources::KIND)
        .unwrap()
        .text = receipt.to_string();
    assert!(classification::sources(&store.root, &task).is_err());
    let mut current = store.task("doi-task").unwrap();
    current.record.title = "Changed input title".into();
    store.save(&mut current, "fixture_input_changed").unwrap();
    assert_eq!(
        doi_sources::attach(&store, &request, &response("10.1234/paper", "Paper"))
            .unwrap_err()
            .code,
        "INPUT_CHANGED"
    );
    assert!(doi_sources::verify(&store.root, &current)
        .unwrap()
        .is_empty());
    let receipt: doi_sources::Receipt =
        serde_json::from_str(saved["evidence"]["text"].as_str().unwrap()).unwrap();
    fs::write(
        &receipt.archive_path,
        response("10.1234/paper", "Changed file"),
    )
    .unwrap();
    let mut original = store.task("doi-task").unwrap();
    original.record.title = "Old title".into();
    assert!(doi_sources::verify(&store.root, &original).is_err());
}
#[test]
fn failed_lookup_is_a_search_receipt_never_paper_absence_or_ai_fact() {
    let (_dir, store) = fixture();
    let before = store.task("doi-task").unwrap();
    let request = doi_sources::request(&store, &before.id).unwrap();
    doi_sources::record_failure(
        &store,
        &request,
        &Failure::new("DOI_NOT_FOUND", "No registry record"),
    )
    .unwrap();
    let after = store.task(&before.id).unwrap();
    assert_eq!(json!(before.record), json!(after.record));
    assert_eq!(before.stage, after.stage);
    assert!(classification::sources(&store.root, &after)
        .unwrap()
        .iter()
        .all(|e| e.kind != "doi_lookup"));
    let receipt: Value = serde_json::from_str(&after.evidence.last().unwrap().text).unwrap();
    assert_eq!(receipt["paper_not_found"], false);
}

#[test]
fn lookup_report_preserves_long_fields_history_and_saved_source_after_refresh_failure() {
    use calamine::Reader;
    let (_dir, store) = fixture();
    let title = "Original title 🙂 ".repeat(2500);
    let request = doi_sources::request(&store, "doi-task").unwrap();
    let saved = doi_sources::attach(&store, &request, &response("10.1234/paper", &title)).unwrap();
    let source_id = saved["evidence"]["id"].as_str().unwrap();
    let message = "Registry connection failed; keep original source. ".repeat(1000);
    let request = doi_sources::request(&store, "doi-task").unwrap();
    doi_sources::record_failure(
        &store,
        &request,
        &Failure::new("DOI_LOOKUP_FAILED", &message),
    )
    .unwrap();
    let mut task = store.task("doi-task").unwrap();
    let failure_id = task.evidence.last().unwrap().id.clone();
    let source: doi_sources::Receipt =
        serde_json::from_str(saved["evidence"]["text"].as_str().unwrap()).unwrap();
    // Reporting is an audit of saved records, not a hidden file validation step.
    fs::remove_file(&source.archive_path).unwrap();
    let path = store.root.join("doi-audit.xlsx");
    files::export_report(&[task.clone()], &path).unwrap();
    let mut book = calamine::open_workbook_auto(&path).unwrap();
    let sheet = book.worksheet_range("DOI 检索记录").unwrap();
    let join = |id: &str, col: usize| {
        sheet
            .rows()
            .skip(1)
            .filter(|r| r[3].to_string() == id)
            .map(|r| r[col].to_string())
            .collect::<String>()
    };
    assert_eq!(join(source_id, 8), json!([title]).to_string());
    assert_eq!(join(&failure_id, 13), message);
    let first = sheet
        .rows()
        .skip(1)
        .find(|r| r[3].to_string() == source_id)
        .unwrap();
    assert_eq!(first[14].to_string(), "是");
    assert!(first[15].to_string().starts_with("是"));
    assert!(sheet
        .rows()
        .skip(1)
        .filter(|r| r[3].to_string() == failure_id)
        .all(|r| !r[7].to_string().contains("论文不存在")));
    task.record.title = "New roster title".into();
    files::export_report(&[task], &path).unwrap();
    let mut book = calamine::open_workbook_auto(&path).unwrap();
    let sheet = book.worksheet_range("DOI 检索记录").unwrap();
    for id in [source_id, failure_id.as_str()] {
        let first = sheet
            .rows()
            .skip(1)
            .find(|r| r[3].to_string() == id)
            .unwrap();
        assert_eq!(first[14].to_string(), "否，历史名单");
        assert_eq!(first[15].to_string(), "否");
    }
}
