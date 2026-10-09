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
