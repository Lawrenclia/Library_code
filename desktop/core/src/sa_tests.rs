use crate::*;
use serde_json::{json, Value};
const ITEM: &str = "1244586319225556123";
fn task() -> Task {
    let record:Record=serde_json::from_value(json!({"row":2,"owner":"测试","sa_id":"sa-zero","title":"Paper","doi":"","wos":"",
        "staff_id":"001","matches":0,"item_ids":"","mark":"待处理","reason":"","skipped":false,"done":false,"source":"list.xlsx"})).unwrap();
    let mut t = Task::new(record, "input".into());
    t.route = Route::CorrectedExisting;
    t.stage = Stage::AwaitingReview;
    t.platform_id = ITEM.into();
    t.evidence.push(Evidence {
        id: "human".into(),
        kind: "human_review".into(),
        source: "原文".into(),
        text: "确认同一篇交大成果".into(),
        created: now(),
    });
    t.review = Some(Review {
        route: Route::CorrectedExisting,
        evidence_id: "human".into(),
        library_checked: true,
        platform_id: ITEM.into(),
        affiliation_confirmed: true,
        identity_confirmed: true,
        issues_resolved: true,
        note: "明确核对本库候选".into(),
    });
    let target = library::target(&t, "Paper").unwrap();
    let item = json!({"id":ITEM,"metadata":{"title":["Paper"]}});
    let result = json!({"verified":true,"sa_id":t.id,"source":"http://www.ir.lib.sjtu.edu.cn/advancedSearch",
        "institution_id":"1244586319225556993","target":target,"queries":[{"kind":"title","field":"title","value":"Paper","precise":false,
        "total":1,"pages":[{"current":1,"size":10,"total":1,"records":[item]}]}],"items":[item],"checked_at":now()});
    library::record(&mut t, &target, &result).unwrap();
    t
}
fn snapshot(linked: bool) -> Value {
    json!({"row":{"saLzkId":"sa-zero","gh":"001","titleValue":"Paper","markStatus":"待处理",
    "matchCount":if linked{1}else{0},"itemId":if linked{ITEM}else{""},"reason":""},"comparison":[]})
}
#[test]
fn roster_zero_live_one_requires_issue_review_without_rewriting_roster() {
    let mut t = task();
    assert!(!t.needs_issue_review());
    let s = snapshot(true);
    t.sa_snapshot = Some(s.clone());
    assert!(t.needs_issue_review());
    assert!(
        t.assert_complete().is_err(),
        "global checkbox cannot skip first live plan"
    );
    issues::prepare(&mut t, &s).unwrap();
    t.assert_complete().unwrap();
    assert_eq!(t.record.matches, 0);
}
#[test]
fn link_payload_binds_actual_front_selection_and_already_linked_is_read_only() {
    let t = task();
    let p = sa::link_payload(&t, &snapshot(false)).unwrap();
    assert_eq!(p["already_linked"], false);
    assert_eq!(p["reviewed"], true);
    assert!(!p["library_evidence_id"].as_str().unwrap().is_empty());
    assert_eq!(
        sa::link_payload(&t, &snapshot(true)).unwrap()["already_linked"],
        true
    );
    let mut other = snapshot(true);
    other["row"]["itemId"] = json!("9999999999999999999");
    assert!(sa::link_payload(&t, &other).is_err());
}
#[test]
fn unknown_link_only_recovers_exact_task_staff_title_target_and_input() {
    let mut t = task();
    let p = sa::link_payload(&t, &snapshot(false)).unwrap();
    t.stage = Stage::Unknown;
    sa::verify_link(&t, &p, &snapshot(true)).unwrap();
    assert!(sa::link_payload(&t, &snapshot(false)).is_err());
    for key in ["saLzkId", "gh", "titleValue", "itemId", "markStatus"] {
        let mut changed = snapshot(true);
        changed["row"][key] = json!("changed");
        assert!(sa::verify_link(&t, &p, &changed).is_err());
    }
    t.input_hash = "changed-input".into();
    assert!(sa::verify_link(&t, &p, &snapshot(true)).is_err());
}
#[test]
fn missing_link_needs_pushed_phase_and_non_submission_routes_cannot_link() {
    for route in [
        Route::NonSjtu,
        Route::NotFound,
        Route::ZeroReview,
        Route::Missing,
    ] {
        let mut t = task();
        t.route = route.clone();
        t.review.as_mut().unwrap().route = route;
        assert!(sa::link_payload(&t, &snapshot(false)).is_err());
    }
    let mut t = task();
    t.route = Route::Missing;
    t.review.as_mut().unwrap().route = Route::Missing;
    t.stage = Stage::Pushed;
    sa::link_payload(&t, &snapshot(false)).unwrap();
    t.evidence.retain(|e| e.kind != "library_search");
    assert!(sa::link_payload(&t, &snapshot(false)).is_err());
}
#[test]
fn live_issue_context_cannot_use_an_unselected_platform_item() {
    let mut t = task();
    let mut wrong = snapshot(true);
    wrong["row"]["itemId"] = json!("9999999999999999999");
    assert_eq!(
        issues::prepare(&mut t, &wrong).unwrap_err().code,
        "IDENTITY_CONFLICT"
    );
    assert!(t.issue_plan.is_none());
}
fn new_query(task: &mut Task, mutate: impl FnOnce(&mut Value)) {
    let mut result = library::latest(task).unwrap();
    mutate(&mut result);
    let target = result["target"].clone();
    library::record(task, &target, &result).unwrap();
}
#[test]
fn link_recovery_requires_unchanged_complete_selected_front_item() {
    let mut t = task();
    let p = sa::link_payload(&t, &snapshot(false)).unwrap();
    new_query(&mut t, |r| {
        r["items"][0]["metadata"]["abstract"] = json!(["Changed publication"]);
        r["queries"][0]["pages"][0]["records"][0] = r["items"][0].clone();
    });
    t.stage = Stage::Unknown;
    assert_eq!(
        sa::verify_link(&t, &p, &snapshot(true)).unwrap_err().code,
        "REMOTE_RESULT_UNKNOWN"
    );
}
#[test]
fn link_recovery_refuses_missing_original_candidate() {
    let mut t = task();
    let p = sa::link_payload(&t, &snapshot(false)).unwrap();
    new_query(&mut t, |r| {
        r["items"] = json!([]);
        r["queries"][0]["total"] = json!(0);
        r["queries"][0]["pages"][0]["total"] = json!(0);
        r["queries"][0]["pages"][0]["records"] = json!([]);
    });
    assert!(sa::verify_link(&t, &p, &snapshot(true)).is_err());
}
#[test]
fn link_recovery_protects_sa_other_fields_and_source_values() {
    let t = task();
    let mut before = snapshot(false);
    before["row"]["remark"] = json!("original");
    before["row"]["doiValue"] = json!("10.example/original");
    before["comparison"] = json!([{"label":"DOI","sa":"10.example/original","library":""}]);
    let p = sa::link_payload(&t, &before).unwrap();
    let mut live = before.clone();
    live["row"]["itemId"] = json!(ITEM);
    live["row"]["matchCount"] = json!(1);
    live["row"]["reason"] = json!("newly calculated issue");
    live["row"]["updateTime"] = json!("new timestamp");
    sa::verify_link(&t, &p, &live).unwrap();
    for key in ["remark", "doiValue"] {
        let mut changed = live.clone();
        changed["row"][key] = json!("changed");
        assert!(sa::verify_link(&t, &p, &changed).is_err());
    }
    live["comparison"][0]["sa"] = json!("10.example/changed");
    assert!(sa::verify_link(&t, &p, &live).is_err());
}
#[test]
fn link_recovery_refuses_replaced_review_input_or_old_partial_intent() {
    let t = task();
    let p = sa::link_payload(&t, &snapshot(false)).unwrap();
    let mut changed = t.clone();
    changed.review.as_mut().unwrap().note = "new review".into();
    assert!(sa::verify_link(&changed, &p, &snapshot(true)).is_err());
    changed = t.clone();
    changed
        .evidence
        .iter_mut()
        .find(|e| e.id == "human")
        .unwrap()
        .text = "new source".into();
    assert!(sa::verify_link(&changed, &p, &snapshot(true)).is_err());
    changed = t.clone();
    changed.record.owner = "new owner".into();
    assert!(sa::verify_link(&changed, &p, &snapshot(true)).is_err());
    let mut old = p.clone();
    old.as_object_mut().unwrap().remove("schema");
    assert!(sa::verify_link(&t, &old, &snapshot(true)).is_err());
    old = p.clone();
    old["previous_stage"] = json!("unknown");
    assert!(sa::verify_link(&t, &old, &snapshot(true)).is_err());
}
#[test]
fn link_recovery_does_not_replace_original_query_evidence() {
    let mut t = task();
    let p = sa::link_payload(&t, &snapshot(false)).unwrap();
    let id = p["library_evidence_id"].as_str().unwrap();
    t.evidence.iter_mut().find(|e| e.id == id).unwrap().text = "replaced".into();
    assert!(sa::verify_link(&t, &p, &snapshot(true)).is_err());
    let t = task();
    let mut p = sa::link_payload(&t, &snapshot(false)).unwrap();
    p["selected_item"]["metadata"]["title"] = json!(["Other paper"]);
    assert!(sa::verify_link(&t, &p, &snapshot(true)).is_err());
}
#[test]
fn unknown_missing_link_restores_only_original_pushed_checkpoint() {
    let mut t = task();
    t.route = Route::Missing;
    t.review.as_mut().unwrap().route = Route::Missing;
    t.stage = Stage::Pushed;
    let p = sa::link_payload(&t, &snapshot(false)).unwrap();
    t.stage = Stage::Unknown;
    new_query(&mut t, |_| {});
    sa::verify_link(&t, &p, &snapshot(true)).unwrap();
    assert_eq!(p["previous_stage"], json!("pushed"));
    assert_eq!(t.record.matches, 0);
    assert!(!t.record.done);
}
