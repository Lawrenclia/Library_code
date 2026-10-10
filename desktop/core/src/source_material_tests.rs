//! Deferred original-export handoff and recovery scenarios; no live platform.
use crate::{
    source_files::{self, ReadOptions, Selection},
    *,
};
use serde_json::{json, Value};
use std::{fs, path::Path};
fn fixture() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    let record=serde_json::from_value(json!({"row":2,"owner":"one","sa_id":"s1","title":"Paper","doi":"10.1234/test","wos":"","staff_id":"001","matches":0,"item_ids":"","mark":"pending","reason":"original reason","skipped":false,"done":false,"source":"list.xlsx"})).unwrap();
    store.import(vec![record], "input".into()).unwrap();
    (dir, store)
}
fn bind(store: &Store, channel: &str, format: &str, text: &str) -> source_files::OriginalExport {
    let task = store.task("s1").unwrap();
    let file = store
        .root
        .join(format!("input-{}.{format}", uuid::Uuid::new_v4()));
    fs::write(&file, text).unwrap();
    let draft =
        source_files::prepare(&store.root, &task, channel, &file, ReadOptions::default()).unwrap();
    let selection = if format == "csv" {
        Selection {
            sheet: "CSV".into(),
            header_row: 1,
            row: 2,
            end_row: 2,
            title_column: Some(0),
            doi_column: Some(1),
            wos_column: None,
            text_title: String::new(),
        }
    } else {
        Selection {
            sheet: "文本".into(),
            header_row: 0,
            row: 1,
            end_row: text.lines().count() as u32,
            title_column: None,
            doi_column: None,
            wos_column: None,
            text_title: "Paper".into(),
        }
    };
    let mut task = task;
    let evidence = source_files::bind(
        &store.root,
        &task,
        &draft,
        selection,
        "https://example.org/record".into(),
        "Checked complete title and record identifiers".into(),
        true,
    )
    .unwrap();
    let id = evidence.id.clone();
    task.evidence.push(evidence);
    store.save(&mut task, "source_bound_fixture").unwrap();
    source_files::original_exports(&store.root, &task)
        .unwrap()
        .into_iter()
        .find(|e| e.evidence_id == id)
        .unwrap()
}
#[test]
fn whole_file_and_selected_record_survive_handoff_without_ai_or_ready_upload() {
    let (dir, store) = fixture();
    let raw="Title,DOI,Abstract\nPaper,10.1234/test,Full abstract\nDifferent paper,10.1234/other,Other record\n";
    let original = bind(&store, "ei", "csv", raw);
    let before = store.task("s1").unwrap();
    let prepared = submission::prepare(
        &store,
        "s1",
        &format!("source-export:{}", original.evidence_id),
        before.revision,
    )
    .unwrap();
    let product: materials::Product =
        serde_json::from_value(prepared["packet"]["material"].clone()).unwrap();
    assert_eq!(fs::read(&product.path).unwrap(), raw.as_bytes());
    assert_eq!(Path::new(&product.path).extension().unwrap(), "csv");
    assert_eq!(product.kind, "original_pending");
    assert_eq!(product.validation["ready"], false);
    assert_eq!(prepared["packet"]["channel"], "ei");
    assert_eq!(prepared["packet"]["instructions"], "SA补充-s1");
    assert_eq!(prepared["can_upload"], false);
    let audit: Value = serde_json::from_slice(&fs::read(&product.audit).unwrap()).unwrap();
    assert_eq!(audit["plan"]["original_source"], json!(original));
    let after = store.task("s1").unwrap();
    assert_eq!(before.stage, after.stage);
    assert!(after.classification.is_none() && after.artifact.is_none() && !after.record.done);
    assert!(submission::check_upload(&store, &after).is_err());
    let report = dir.path().join("sources.xlsx");
    files::export_report(&[after.clone()], &report).unwrap();
    use calamine::Reader;
    let mut book = calamine::open_workbook_auto(&report).unwrap();
    let rows = book.worksheet_range("任务与来源").unwrap();
    assert!(rows
        .get_value((1, 10))
        .unwrap()
        .to_string()
        .contains("导出范围"));
    let again = submission::prepare(&store, "s1", &product.recipe, after.revision).unwrap();
    assert_eq!(again["packet"]["id"], prepared["packet"]["id"]);
    assert_eq!(store.task("s1").unwrap().revision, after.revision);
}
#[test]
fn explicit_export_choice_is_frozen_in_batch_and_reused_after_store_reopen() {
    let (dir, store) = fixture();
    bind(&store, "ei", "csv", "Title,DOI\nPaper,10.1234/test\n");
    let chosen = bind(&store, "cscd", "txt", "Paper\nFull source record\n");
    let frozen = materials::facts(&store.root, &store.task("s1").unwrap()).unwrap();
    assert_eq!(
        materials::prepare(&store, "s1", &frozen, None)
            .unwrap_err()
            .code,
        "AMBIGUOUS_RESULT"
    );
    let task = store.task("s1").unwrap();
    submission::prepare(
        &store,
        "s1",
        &format!("source-export:{}", chosen.evidence_id),
        task.revision,
    )
    .unwrap();
    let batch = store.start_material_batch("one").unwrap();
    assert_eq!(
        batch.targets[0]
            .original_source
            .as_ref()
            .unwrap()
            .evidence_id,
        chosen.evidence_id
    );
    let product = store.material_target(&batch.id).unwrap();
    let revision = store.task("s1").unwrap().revision;
    drop(store);
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    reopened.resume_material_batch(&batch.id).unwrap();
    let reused = reopened.material_target(&batch.id).unwrap();
    assert!(reused.reused);
    assert_eq!(product.path, reused.path);
    assert_eq!(reopened.task("s1").unwrap().revision, revision);
    let ended = reopened
        .finish_material_target(&batch.id, Ok(reused))
        .unwrap();
    assert_eq!(ended.cursor, 1);
    assert_eq!(ended.targets.len(), 1);
    assert_eq!(
        ended.outcomes[0].product.as_ref().unwrap().kind,
        "original_pending"
    );
    let bytes = fs::read(&chosen.receipt.archive_path).unwrap();
    fs::write(&chosen.receipt.archive_path, b"altered original").unwrap();
    assert!(submission::options(&reopened, "s1").is_err());
    assert_eq!(fs::read(&product.path).unwrap(), bytes);
}

#[test]
fn database_excel_remains_original_bytes_and_does_not_become_a_generated_template() {
    let (_dir, store) = fixture();
    let path = store.root.join("database-export.xlsx");
    let mut book = rust_xlsxwriter::Workbook::new();
    let sheet = book.add_worksheet();
    sheet.set_name("Records").unwrap();
    sheet.write_string(0, 0, "Title").unwrap();
    sheet.write_string(0, 1, "DOI").unwrap();
    sheet.write_string(0, 2, "Full authors").unwrap();
    sheet.write_string(1, 0, "Paper").unwrap();
    sheet.write_string(1, 1, "10.1234/test").unwrap();
    sheet.write_string(1, 2, "First; Second; Third").unwrap();
    sheet.write_string(2, 0, "Different paper").unwrap();
    book.add_worksheet()
        .set_name("Export instructions")
        .unwrap()
        .write_string(0, 0, "Preserve this original worksheet")
        .unwrap();
    book.save(&path).unwrap();
    let original_bytes = fs::read(&path).unwrap();
    let mut task = store.task("s1").unwrap();
    let draft =
        source_files::prepare(&store.root, &task, "cnki", &path, ReadOptions::default()).unwrap();
    let selection = Selection {
        sheet: "Records".into(),
        header_row: 1,
        row: 2,
        end_row: 2,
        title_column: Some(0),
        doi_column: Some(1),
        wos_column: None,
        text_title: String::new(),
    };
    task.evidence.push(
        source_files::bind(
            &store.root,
            &task,
            &draft,
            selection,
            "https://example.org/record".into(),
            "Checked all title and identifier fields".into(),
            true,
        )
        .unwrap(),
    );
    store.save(&mut task, "excel_source_fixture").unwrap();
    let batch = store.start_material_batch("one").unwrap();
    assert!(batch.targets[0].template.is_none());
    let product = store.material_target(&batch.id).unwrap();
    assert_eq!(product.kind, "original_pending");
    assert_eq!(Path::new(&product.path).extension().unwrap(), "xlsx");
    assert_eq!(fs::read(product.path).unwrap(), original_bytes);
    assert_eq!(store.task("s1").unwrap().stage, Stage::Pending);
}
