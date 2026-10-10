use crate::{materials::*, queue::QueueStatus, *};
use calamine::{open_workbook_auto, Reader};
use serde_json::{json, Value};
use std::{fs, path::Path};
#[test]
fn matching_updated_hash_cannot_hide_changed_template_cells() {
    let (_dir, store, schema) = fixture(false);
    suggestion(&store, &schema, "s1");
    let p = prepare_one(&store, &schema, "s1");
    let mut book = rust_xlsxwriter::Workbook::new();
    let sheet = book.add_worksheet();
    sheet.set_name(&schema.sheet).unwrap();
    sheet.write_string(0, 0, "Title").unwrap();
    sheet.write_string(0, 1, "Author").unwrap();
    sheet.write_string(1, 0, "Changed paper").unwrap();
    book.save(&p.path).unwrap();
    let receipt = Path::new(&p.path).parent().unwrap().join("prepared.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    v["sha256"] = json!(hash(&fs::read(&p.path).unwrap()));
    fs::write(&receipt, v.to_string()).unwrap();
    let frozen = facts(&store.root, &store.task("s1").unwrap()).unwrap();
    assert!(prepare(&store, "s1", &frozen, Some(&schema)).is_err());
}
fn fixture(required_author: bool) -> (tempfile::TempDir, Store, templates::Template) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    let records = (1..=4).map(|i| serde_json::from_value(json!({"row":i+1,"owner":"one","sa_id":format!("s{i}"),"title":"Paper","doi":"10.1234/test","wos":"","staff_id":"001","matches":0,"item_ids":"","mark":"pending","reason":"original reason","skipped":false,"done":false,"source":"list.xlsx"})).unwrap()).collect();
    store.import(records, "input".into()).unwrap();
    let path = dir.path().join("template.xlsx");
    let mut book = rust_xlsxwriter::Workbook::new();
    let sheet = book.add_worksheet();
    sheet.write_string(0, 0, "Title").unwrap();
    sheet.write_string(0, 1, "Author").unwrap();
    sheet.write_string(1, 0, "sample title").unwrap();
    sheet.write_string(1, 1, "sample author").unwrap();
    book.save(&path).unwrap();
    let required = if required_author {
        vec!["Title".into(), "Author".into()]
    } else {
        vec!["Title".into()]
    };
    let template = templates::inspect(&path, "", 0, required, "".into()).unwrap();
    store.set_setting("templates", json!([template])).unwrap();
    (dir, store, template)
}
fn suggestion(store: &Store, schema: &templates::Template, id: &str) {
    let mut t = store.task(id).unwrap();
    let sources = classification::sources(&store.root, &t).unwrap();
    let v = json!({"type":"期刊论文","channel":"general","confidence":"低","reason":"题名候选，待核实","channel_reason":"确认实际来源和模板","missing":["出版来源和署名"],"fields":{"Title":{"value":"Paper","evidence_ids":[sources[0].id]}},"evidence_ids":[sources[0].id]});
    catalog::validate_ai(&v, &sources, Some(&json!(schema))).unwrap();
    let v = classification::bind_result(&t, v, Some(&json!(schema)));
    t.evidence.push(classification::audit(
        &t,
        &v,
        &sources,
        "fixture",
        Some(&json!(schema)),
    ));
    t.classification = Some(v);
    store.save(&mut t, "ai_classified").unwrap();
}
fn prepare_one(store: &Store, schema: &templates::Template, id: &str) -> Product {
    let frozen = facts(&store.root, &store.task(id).unwrap()).unwrap();
    prepare(store, id, &frozen, Some(schema)).unwrap()
}
#[test]
fn actual_template_products_and_drafts_preserve_sources_and_business_state() {
    for author in [false, true] {
        let (_dir, store, schema) = fixture(author);
        suggestion(&store, &schema, "s1");
        let before = store.task("s1").unwrap();
        let p = prepare_one(&store, &schema, "s1");
        assert_eq!(p.kind, if author { "draft" } else { "field_valid" });
        let mut book = open_workbook_auto(&p.path).unwrap();
        let range = book.worksheet_range(&schema.sheet).unwrap();
        assert_eq!(range.get_value((1, 0)).unwrap().to_string(), "Paper");
        assert_eq!(range.get_value((1, 1)).unwrap().to_string(), "");
        assert_eq!(hash(&fs::read(&schema.path).unwrap()), schema.fingerprint);
        let after = store.task("s1").unwrap();
        assert_eq!(after.stage, before.stage);
        assert_eq!(after.record.reason, "original reason");
        assert!(after.review.is_none() && after.platform_id.is_empty());
        let audit: Value = serde_json::from_slice(&fs::read(&p.audit).unwrap()).unwrap();
        assert_eq!(audit["platform_verified"], false);
        assert_eq!(
            audit["evidence"],
            json!(classification::sources(&store.root, &after).unwrap())
        );
        let again = prepare_one(&store, &schema, "s1");
        assert!(again.reused);
        assert_eq!(again.sha256, p.sha256);
        assert_eq!(store.task("s1").unwrap().revision, after.revision);
    }
}
#[test]
fn recovery_adopts_published_file_before_cursor_once_without_new_scope() {
    let (dir, store, schema) = fixture(false);
    suggestion(&store, &schema, "s1");
    let q = store.start_material_batch("one").unwrap();
    let product = store.material_target(&q.id).unwrap();
    let revision = store.task("s1").unwrap().revision;
    drop(store);
    let store = Store::new(dir.path()).unwrap();
    store.recover().unwrap();
    assert_eq!(
        store.material_batch(&q.id).unwrap().status,
        QueueStatus::Interrupted
    );
    let mut added = store.task("s1").unwrap().record;
    added.sa_id = "added".into();
    added.row = 20;
    store.import(vec![added], "later-input".into()).unwrap();
    store.resume_material_batch(&q.id).unwrap();
    let adopted = store.material_target(&q.id).unwrap();
    assert!(adopted.reused);
    assert_eq!(adopted.path, product.path);
    assert_eq!(store.task("s1").unwrap().revision, revision);
    let ended = store.finish_material_target(&q.id, Ok(adopted)).unwrap();
    assert_eq!(ended.cursor, 1);
    assert_eq!(ended.targets.len(), 4);
}
#[test]
fn prepared_staging_is_adopted_without_rewriting_missing_output_or_duplicate_audit() {
    let (_dir, store, schema) = fixture(false);
    suggestion(&store, &schema, "s1");
    let product = prepare_one(&store, &schema, "s1");
    let folder = Path::new(&product.path).parent().unwrap();
    let receipt: Value =
        serde_json::from_slice(&fs::read(folder.join("prepared.json")).unwrap()).unwrap();
    fs::rename(
        &product.path,
        folder.join(receipt["staging"].as_str().unwrap()),
    )
    .unwrap();
    let mut task = store.task("s1").unwrap();
    task.evidence.retain(|e| e.kind != "material_validation");
    store
        .save(&mut task, "fixture_saved_before_publish")
        .unwrap();
    let p = prepare_one(&store, &schema, "s1");
    assert!(p.reused);
    assert_eq!(p.sha256, product.sha256);
    assert_eq!(
        store
            .task("s1")
            .unwrap()
            .evidence
            .iter()
            .filter(|e| e.kind == "material_validation")
            .count(),
        1
    );
}
#[test]
fn altered_products_or_validation_receipts_are_preserved_and_refused() {
    for target in ["output", "validation", "audit"] {
        let (_dir, store, schema) = fixture(true);
        suggestion(&store, &schema, "s1");
        let p = prepare_one(&store, &schema, "s1");
        let frozen = facts(&store.root, &store.task("s1").unwrap()).unwrap();
        let path = if target == "output" {
            Path::new(&p.path).to_path_buf()
        } else if target == "audit" {
            Path::new(&p.audit).to_path_buf()
        } else {
            Path::new(&p.path).parent().unwrap().join("prepared.json")
        };
        if target == "validation" {
            let mut v: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            v["validation"]["ready"] = json!(true);
            fs::write(&path, v.to_string()).unwrap();
        } else {
            fs::write(&path, b"manual change").unwrap();
        }
        let original = fs::read(&path).unwrap();
        assert!(prepare(&store, "s1", &frozen, Some(&schema)).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
}
#[test]
fn scope_keeps_remaining_after_three_errors_and_pause_then_resume() {
    let (_dir, store, schema) = fixture(false);
    suggestion(&store, &schema, "s4");
    let q = store.start_material_batch("one").unwrap();
    for i in 0..3 {
        let result = store.material_target(&q.id);
        assert!(result.is_err());
        if i == 0 {
            store.request_material_pause().unwrap();
        }
        let progress = store.finish_material_target(&q.id, result).unwrap();
        if i == 0 {
            assert_eq!(progress.status, QueueStatus::Paused);
            store.resume_material_batch(&q.id).unwrap();
        } else {
            assert_eq!(progress.status, QueueStatus::Running);
        }
    }
    let p = store.material_target(&q.id).unwrap();
    let ended = store.finish_material_target(&q.id, Ok(p)).unwrap();
    assert_eq!(ended.status, QueueStatus::Completed);
    assert_eq!(ended.cursor, 4);
    let mut long = ended.clone();
    long.targets[0].record.reason = "完整原因𠮷".repeat(10000);
    let path = store.root.join("sources.xlsx");
    files::export_report_with_materials(
        &store.tasks().unwrap(),
        &[],
        &[],
        &[],
        &[long.clone()],
        &path,
    )
    .unwrap();
    let mut book = open_workbook_auto(&path).unwrap();
    let sheet = book.worksheet_range("材料整理结果").unwrap();
    let raw = sheet
        .rows()
        .skip(1)
        .map(|r| r[7].to_string())
        .collect::<String>();
    assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), json!(long));
    assert_eq!(
        sheet.get_value((1, 5)).unwrap().to_string(),
        hash(raw.as_bytes())
    );
}
#[test]
fn changed_scope_or_template_and_incomplete_audit_do_not_generate_products() {
    let (_dir, store, schema) = fixture(false);
    suggestion(&store, &schema, "s1");
    let frozen = facts(&store.root, &store.task("s1").unwrap()).unwrap();
    let mut changed = store.task("s1").unwrap();
    changed.record.reason = "changed".into();
    store.save(&mut changed, "changed_input").unwrap();
    assert!(prepare(&store, "s1", &frozen, Some(&schema)).is_err());
    let mut changed = store.task("s1").unwrap();
    changed.evidence.retain(|e| e.kind != "ai_classification");
    store.save(&mut changed, "incomplete_audit").unwrap();
    let frozen = facts(&store.root, &changed).unwrap();
    assert!(prepare(&store, "s1", &frozen, Some(&schema)).is_err());
    suggestion(&store, &schema, "s1");
    let frozen = facts(&store.root, &store.task("s1").unwrap()).unwrap();
    fs::write(&schema.path, b"changed template").unwrap();
    assert!(prepare(&store, "s1", &frozen, Some(&schema)).is_err());
    assert!(!store.root.join("materials/products").exists());
}
#[test]
fn verified_original_export_takes_priority_and_unconfirmed_identity_cannot_fallback() {
    let (_dir, store, schema) = fixture(false);
    suggestion(&store, &schema, "s1");
    let raw = b"TI\tAU\tAF\tSO\tPY\tC1\tUT\tDI\r\nPaper\tLi X\tLi, X\tJournal\t2026\tShanghai Jiao Tong University\tWOS:000123456789012\t10.1234/test\r\n";
    let path = files::archive(&store.root, raw).unwrap();
    let mut task = store.task("s1").unwrap();
    task.artifact = Some(Artifact {
        path: path.to_string_lossy().into(),
        source: "WOS".into(),
        record_url: "https://example.test/record".into(),
        downloaded: 1,
        candidate: files::parse_wos(raw).unwrap(),
        identity_confirmed: false,
    });
    store.save(&mut task, "original_download").unwrap();
    let frozen = facts(&store.root, &task).unwrap();
    assert!(prepare(&store, "s1", &frozen, Some(&schema)).is_err());
    task.artifact.as_mut().unwrap().identity_confirmed = true;
    store.save(&mut task, "identity_confirmed").unwrap();
    let p = prepare_one(&store, &schema, "s1");
    assert_eq!(p.kind, "original");
    assert_eq!(fs::read(&p.path).unwrap(), raw);
    assert_eq!(p.sha256, hash(raw));
}
#[test]
fn automatic_notes_normalization_does_not_merge_distinct_requirements_or_ids() {
    let (_dir, _store, schema) = fixture(false);
    let mut original = json!(schema);
    original["notes"] = json!("manual\n模板原文格式要求：\nrules");
    let mut duplicate = original.clone();
    duplicate["notes"] = json!("manual\n模板原文格式要求：\nrules\n模板原文格式要求：\nrules");
    assert!(same_template(Some(&original), Some(&duplicate)));
    duplicate["notes"] = json!("manual\n模板原文格式要求：\nrules\n模板原文格式要求：\ndifferent");
    assert!(!same_template(Some(&original), Some(&duplicate)));
    duplicate = original.clone();
    duplicate["id"] = json!("other registration");
    assert!(!same_template(Some(&original), Some(&duplicate)));
}
