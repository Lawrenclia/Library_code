use crate::*;
use serde_json::json;
use std::path::Path;

fn record(id: &str, row: u32) -> Record {
    Record {
        row,
        owner: "one".into(),
        sa_id: id.into(),
        title: "Paper".into(),
        doi: "".into(),
        wos: "".into(),
        staff_id: "001".into(),
        matches: 0,
        item_ids: "".into(),
        mark: "待处理".into(),
        reason: "缺失".into(),
        skipped: false,
        done: false,
        source: "original source".into(),
    }
}
fn write_roster(path: &Path, extra_headers: &[&str], duplicate: bool, invalid_count: bool) {
    use rust_xlsxwriter::Workbook;
    let mut book = Workbook::new();
    let sheet = book.add_worksheet();
    let headers = [
        "备注",
        "负责人",
        "sa_lzk表ID",
        "题名",
        "DOI",
        "WOS_ID",
        "工号",
        "匹配到的条目数量",
        "平台唯一号",
        "标记状态",
        "标记为待处理原因",
        "数据来源",
    ];
    for (i, h) in headers.iter().chain(extra_headers.iter()).enumerate() {
        sheet.write_string(4, 2 + i as u16, *h).unwrap();
    }
    for index in 0..2 {
        let row = 5 + index;
        let values = [
            "",
            "one",
            if index == 0 || duplicate {
                "sa-one"
            } else {
                "sa-two"
            },
            "Paper",
            "",
            "",
            "001",
            "",
            "",
            "待处理",
            "缺失",
            "full original source",
        ];
        for (i, v) in values.iter().enumerate() {
            sheet.write_string(row, 2 + i as u16, *v).unwrap();
        }
        sheet
            .write_number(row, 2, if index == 0 { 2. } else { 0. })
            .unwrap();
        if index == 0 && invalid_count {
            sheet.write_string(row, 9, "invalid").unwrap();
        } else {
            sheet
                .write_number(row, 9, if index == 0 { 0. } else { 2. })
                .unwrap();
        }
    }
    book.save(path).unwrap();
}

#[test]
fn original_sheet_row_offsets_and_numeric_skip_are_independent_from_match_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("list.xlsx");
    write_roster(&path, &[], false, false);
    let (records, _) = files::read_roster(&path).unwrap();
    assert_eq!(records[0].row, 6);
    assert_eq!(records[1].row, 7);
    assert!(records[0].skipped);
    assert_eq!(records[0].matches, 0);
    assert!(!records[1].skipped);
    assert_eq!(records[1].matches, 2);
    assert_eq!(
        Task::new(records[1].clone(), "hash".into()).route,
        Route::Duplicate
    );
    assert_eq!(records[0].source, "full original source");
    write_roster(&path, &[], false, true);
    assert!(files::read_roster(&path)
        .unwrap_err()
        .message
        .contains("第 6 行"));
    write_roster(&path, &[], true, false);
    let error = files::read_roster(&path).unwrap_err();
    assert!(error.message.contains("第 6、7 行"));
}

#[test]
fn ambiguous_source_or_workflow_headers_are_reported_instead_of_silently_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("list.xlsx");
    for extra in [vec!["数据来源"], vec!["备注", "备注"]] {
        write_roster(&path, &extra, false, false);
        assert_eq!(files::read_roster(&path).unwrap_err().code, "INPUT_INVALID");
    }
    // Preserve the explicitly supported two-remark layout: first numeric workflow,
    // second platform annotation. The workflow column remains the first one.
    write_roster(&path, &["备注"], false, false);
    assert!(files::read_roster(&path).unwrap().0[0].skipped);
}

#[test]
fn duplicate_or_missing_sa_rejects_the_whole_core_batch_without_changes_or_proposals() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    store
        .import(vec![record("existing", 2)], "original".into())
        .unwrap();
    let before = store.task("existing").unwrap();
    for invalid in [record("new", 5), record("", 5)] {
        let mut changed = record("existing", 3);
        changed.title = "Changed paper".into();
        let error = store
            .import(vec![changed, record("new", 4), invalid], "new-input".into())
            .unwrap_err();
        assert_eq!(error.code, "INPUT_INVALID");
        assert_eq!(json!(store.task("existing").unwrap()), json!(before));
        assert!(store.task("new").is_err());
        assert!(store.pending_input("existing").unwrap().is_none());
        let histories: i64 = store
            .connect()
            .unwrap()
            .query_row("SELECT count(*) FROM input_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(histories, 0);
    }
}

#[test]
fn changed_processed_mark_or_done_flag_requires_a_new_reviewed_version() {
    for change_done in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        let original = record("existing", 2);
        store
            .import(vec![original.clone()], "old-input".into())
            .unwrap();
        let mut incoming = original.clone();
        if change_done {
            incoming.done = true;
        } else {
            incoming.mark = "已处理".into();
        }
        assert_eq!(incoming.fingerprint(), original.fingerprint());
        store
            .import(vec![incoming.clone()], "new-input".into())
            .unwrap();
        let current = store.task("existing").unwrap();
        assert!(!current.record.done);
        assert_eq!(current.record.mark, "待处理");
        assert_eq!(current.stage, Stage::AwaitingReview);
        assert_eq!(current.last_error.unwrap().code, "INPUT_CHANGED");
        assert_eq!(current.input_hash, "old-input");
        let proposal = store.pending_input("existing").unwrap().unwrap();
        assert_eq!(json!(proposal.record), json!(incoming));
        assert_eq!(proposal.input_hash, "new-input");
        let reopened = Store::new(dir.path()).unwrap();
        assert!(reopened.pending_input("existing").unwrap().is_some());
        assert_ne!(reopened.task("existing").unwrap().stage, Stage::Completed);
    }
}
