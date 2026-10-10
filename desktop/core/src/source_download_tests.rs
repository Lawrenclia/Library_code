//! Deferred acceptance scenarios. Writing these tests does not run them.
use crate::{source_downloads::*, *};
use serde_json::json;
use std::{fs, path::Path};
use url::Url;
fn fixture() -> (tempfile::TempDir, Store, Session, Url, Url) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    let record=serde_json::from_value(json!({"row":2,"owner":"one","sa_id":"s1","title":"Paper","doi":"","wos":"","staff_id":"001","matches":0,"item_ids":"","mark":"pending","reason":"original reason","skipped":false,"done":false,"source":"list.xlsx"})).unwrap();
    store.import(vec![record], "input".into()).unwrap();
    store
        .save_source_site(Site {
            channel: "ei".into(),
            entry_url: "https://example.org/search".into(),
            download_origins: vec![],
        })
        .unwrap();
    let session = store.source_session("s1", "ei").unwrap();
    (
        dir,
        store,
        session,
        Url::parse("https://example.org/record").unwrap(),
        Url::parse("https://example.org/export").unwrap(),
    )
}
#[test]
fn native_receipt_is_owned_persistent_and_duplicate_completion_is_ignored() {
    let (dir, store, session, page, event) = fixture();
    let before = store.task("s1").unwrap();
    let receipt = store
        .request_source_download(&session, &page, &event, Path::new("original.csv"))
        .unwrap();
    fs::write(&receipt.path, b"Title,DOI\nPaper,10.1234/test\n").unwrap();
    assert!(store
        .finish_source_download(
            "foreign-session",
            &event,
            Some(Path::new(&receipt.path)),
            true
        )
        .unwrap()
        .is_none());
    assert!(store
        .source_download_for_preview("s1", &receipt.id)
        .is_err());
    let wrong = dir.path().join("unrelated.csv");
    fs::write(&wrong, b"unrelated").unwrap();
    assert!(store
        .finish_source_download(&session.id, &event, Some(&wrong), true)
        .unwrap()
        .is_none());
    let complete = store
        .finish_source_download(&session.id, &event, Some(Path::new(&receipt.path)), true)
        .unwrap()
        .unwrap();
    assert_eq!(complete.state, "completed");
    let after = store.task("s1").unwrap();
    assert!(store
        .finish_source_download(&session.id, &event, Some(Path::new(&receipt.path)), true)
        .unwrap()
        .is_none());
    assert_eq!(store.task("s1").unwrap().revision, after.revision);
    assert_eq!(after.stage, before.stage);
    assert!(after.artifact.is_none() && after.review.is_none() && !after.record.done);
    // A native download receipt is not factual metadata for AI.
    assert_eq!(
        classification::sources(&store.root, &after).unwrap().len(),
        1
    );
    drop(store);
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    let read = reopened
        .source_download_for_preview("s1", &receipt.id)
        .unwrap();
    assert_eq!(read.sha256, complete.sha256);
    fs::write(&receipt.path, b"changed").unwrap();
    assert!(reopened
        .source_download_for_preview("s1", &receipt.id)
        .is_err());
}
#[test]
fn window_interruption_and_restart_preserve_unknown_files_without_adoption() {
    let (dir, store, session, page, event) = fixture();
    let first = store
        .request_source_download(&session, &page, &event, Path::new("one.csv"))
        .unwrap();
    let second = store
        .request_source_download(&session, &page, &event, Path::new("two.csv"))
        .unwrap();
    fs::write(&first.path, b"complete-looking file without Finished event").unwrap();
    store
        .interrupt_source_download_ids(&[first.id.clone()])
        .unwrap();
    let states = store.source_downloads("s1").unwrap();
    assert_eq!(
        states.iter().find(|r| r.id == first.id).unwrap().state,
        "interrupted"
    );
    assert_eq!(
        states.iter().find(|r| r.id == second.id).unwrap().state,
        "requested"
    );
    assert!(store
        .finish_source_download(&session.id, &event, Some(Path::new(&first.path)), true)
        .unwrap()
        .is_none());
    drop(store);
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    assert!(reopened
        .source_downloads("s1")
        .unwrap()
        .iter()
        .all(|r| r.state == "interrupted" && r.sha256.is_none()));
    assert!(reopened
        .source_download_for_preview("s1", &first.id)
        .is_err());
    assert_eq!(
        fs::read(first.path).unwrap(),
        b"complete-looking file without Finished event"
    );
    let revision = reopened.task("s1").unwrap().revision;
    reopened.recover().unwrap();
    assert_eq!(reopened.task("s1").unwrap().revision, revision);
}
#[test]
fn origin_configuration_is_idempotent_and_session_cannot_follow_a_changed_record() {
    let (_dir, store, session, page, event) = fixture();
    let site = Site {
        channel: "ei".into(),
        entry_url: "https://example.org/search".into(),
        download_origins: (0..11)
            .map(|i| format!("https://cdn{i}.example.org"))
            .collect(),
    };
    let first = validate_site(site).unwrap();
    assert_eq!(json!(validate_site(first.clone()).unwrap()), json!(first));
    assert!(store
        .request_source_download(
            &session,
            &page,
            &Url::parse("https://foreign.example/export").unwrap(),
            Path::new("file.csv")
        )
        .is_err());
    assert!(store
        .request_source_download(&session, &page, &event, Path::new("file.txt"))
        .is_err());
    let mut changed = store.task("s1").unwrap();
    changed.record.staff_id = "002".into();
    store.save(&mut changed, "changed_input_fixture").unwrap();
    assert!(store
        .request_source_download(&session, &page, &event, Path::new("file.csv"))
        .is_err());
    assert!(store.source_downloads("s1").unwrap().is_empty());
}
