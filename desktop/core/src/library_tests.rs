use crate::*;
use serde_json::{json, Value};

fn task() -> Task {
    Task::new(
        serde_json::from_value(
            json!({"row":2,"owner":"测试","sa_id":"sa-library","title":"Incorrect title",
        "doi":"10.1234/test","wos":"","staff_id":"001","matches":0,"item_ids":"","mark":"待处理",
        "reason":"","skipped":false,"done":false,"source":"list.xlsx"}),
        )
        .unwrap(),
        "input".into(),
    )
}
fn result(t: &Task, rows: Vec<Value>) -> Value {
    let target = library::target(t, "Correct paper title").unwrap();
    let queries: Vec<_> = ["title", "doi"]
        .into_iter()
        .map(|kind| {
            json!({"kind":kind,"field":kind,
        "value":target[kind],"precise":kind!="title","total":rows.len(),
        "pages":[{"current":1,"size":10,"total":rows.len(),"records":rows}]})
        })
        .collect();
    json!({"verified":true,"sa_id":t.id,"source":"http://www.ir.lib.sjtu.edu.cn/advancedSearch",
        "institution_id":"1244586319225556993","target":target,"queries":queries,"items":rows,"checked_at":now()})
}
fn review(route: Route, platform_id: &str) -> Review {
    Review {
        route,
        evidence_id: "proof".into(),
        library_checked: true,
        platform_id: platform_id.into(),
        affiliation_confirmed: true,
        identity_confirmed: true,
        issues_resolved: true,
        note: "已核对".into(),
    }
}
fn item() -> Value {
    json!({"id":"item-001","metadata":{"title":["Correct paper title"],"doi":["10.1234/test"],
    "pages":["10-20"],"abstract":["完整摘要必须保留"],"author":[{"fullname":"Test Author","order":1}]}})
}
fn save(t: &mut Task, r: &Value) {
    library::record(t, &r["target"], r).unwrap();
}

#[test]
fn checkbox_without_actual_front_query_is_not_absence() {
    let t = task();
    assert_eq!(
        library::review(&t, &review(Route::Missing, ""))
            .unwrap_err()
            .code,
        "SEARCH_REQUIRED"
    );
}
#[test]
fn all_queries_zero_allow_review_but_any_identifier_hit_blocks_upload() {
    let mut t = task();
    let empty = result(&t, vec![]);
    save(&mut t, &empty);
    library::review(&t, &review(Route::Missing, "")).unwrap();
    library::assert_absent(&t).unwrap();
    let mut hit = result(&t, vec![item()]);
    hit["queries"][0]["total"] = json!(0);
    hit["queries"][0]["pages"][0]["total"] = json!(0);
    hit["queries"][0]["pages"][0]["records"] = json!([]);
    save(&mut t, &hit);
    assert!(library::assert_absent(&t).is_err());
    assert!(library::review(&t, &review(Route::Missing, "")).is_err());
}
#[test]
fn selected_existing_id_must_be_real_and_identity_confirmed() {
    let mut t = task();
    let r = result(&t, vec![item()]);
    save(&mut t, &r);
    assert!(library::review(&t, &review(Route::CorrectedExisting, "arbitrary")).is_err());
    let mut selected = review(Route::CorrectedExisting, "item-001");
    library::review(&t, &selected).unwrap();
    selected.identity_confirmed = false;
    assert!(library::review(&t, &selected).is_err());
    assert!(
        t.platform_id.is_empty(),
        "query does not select an item automatically"
    );
}
#[test]
fn missing_pages_wrong_task_scope_and_incomplete_coverage_never_become_evidence() {
    for mutation in 0..6 {
        let mut t = task();
        let mut r = result(&t, vec![]);
        match mutation {
            0 => r["queries"][0]["pages"] = json!([]),
            1 => r["sa_id"] = json!("other"),
            2 => r["institution_id"] = json!("other"),
            3 => r["queries"] = json!([r["queries"][0]]),
            4 => r["queries"][0]["precise"] = json!(true),
            _ => r["source"] = json!("http://example.test/#/advancedSearch"),
        };
        assert!(library::record(&mut t, &r["target"], &r).is_err());
        assert!(t.evidence.is_empty());
    }
}
#[test]
fn full_metadata_persists_and_item_union_cannot_repeat_ids() {
    let mut t = task();
    let r = result(&t, vec![item()]);
    save(&mut t, &r);
    assert_eq!(
        library::latest(&t).unwrap()["items"][0]["metadata"]["abstract"][0],
        "完整摘要必须保留"
    );
    let mut r = result(
        &t,
        vec![
            item(),
            json!({"id":"item-002","metadata":{"title":["Another paper"]}}),
        ],
    );
    r["items"][1] = item();
    assert!(library::record(&mut t, &r["target"], &r).is_err());
}
#[test]
fn stale_time_or_changed_input_and_identifier_require_new_query() {
    let mut t = task();
    let r = result(&t, vec![]);
    save(&mut t, &r);
    t.input_hash = "new-input".into();
    assert_eq!(library::latest(&t).unwrap_err().code, "INPUT_CHANGED");
    t.input_hash = "input".into();
    t.record.doi = "10.1234/other".into();
    assert!(library::latest(&t).is_err());
    let mut expired = result(&t, vec![]);
    expired["checked_at"] = json!(now() - 25 * 60 * 60 * 1000);
    assert!(library::record(&mut t, &expired["target"], &expired).is_err());
}
#[test]
fn pushed_missing_task_requires_new_front_result_containing_selected_item() {
    let mut t = task();
    let empty = result(&t, vec![]);
    save(&mut t, &empty);
    t.stage = Stage::Pushed;
    assert!(library::review(&t, &review(Route::Missing, "item-001")).is_err());
    let found = result(&t, vec![item()]);
    save(&mut t, &found);
    library::review(&t, &review(Route::Missing, "item-001")).unwrap();
    assert!(
        library::assert_absent(&t).is_err(),
        "post-push lookup never authorizes another upload"
    );
}
