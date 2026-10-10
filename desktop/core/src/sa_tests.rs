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

fn complete_snapshot() -> Value {
    let mut result = snapshot(true);
    for key in [
        "id",
        "title",
        "doi",
        "doiValue",
        "wos",
        "wosValue",
        "claimStatus",
        "qr",
        "updateTime",
        "updateUsername",
        "remark",
    ] {
        result["row"][key] = json!("");
    }
    result["row"]["id"] = json!("record-001");
    result["comparison"] = json!([{"label":"题名","sa":"Paper","library":"Paper"}]);
    result
}

fn note_fixture() -> (Task, Value) {
    let mut t = task();
    t.route = Route::NotFound;
    t.platform_id.clear();
    t.evidence.clear();
    let mut before = complete_snapshot();
    before["row"]["matchCount"] = json!(0);
    before["row"]["itemId"] = json!("");
    before["row"]["remark"] = json!("原备注");
    before["comparison"] = json!([{"label":"题名","sa":"Paper","library":""}]);
    t.sa_snapshot = Some(before.clone());
    t.evidence.push(Evidence { id:"scope".into(),kind:"search_scope".into(),source:"https://example.org/results".into(),
        text:json!({"schema":"source_search_scope_v1","reported_by":"human","task_id":t.id,"input_hash":t.input_hash,"record_fingerprint":t.record.fingerprint(),
            "observation":{"channel":"wos_txt","source_url":"https://example.org/results","scope":"核心合集","query":"Paper","field":"title","observed_at":now(),
                "outcome":"zero_results","total":0,"explanation":"人工记录的实际零结果"}}).to_string(),created:now() });
    t.evidence.push(Evidence {
        id: "conclusion".into(),
        kind: "search_conclusion".into(),
        source: "检索页".into(),
        text: "按实际范围仍未定位".into(),
        created: now(),
    });
    t.review = Some(Review {
        route: Route::NotFound,
        evidence_id: "conclusion".into(),
        library_checked: false,
        platform_id: String::new(),
        affiliation_confirmed: false,
        identity_confirmed: false,
        issues_resolved: false,
        note: "未查询到该文献；保留原备注".into(),
    });
    (t, before)
}

#[test]
fn independent_note_requires_full_pending_readback_without_granting_completion() {
    let (t, before) = note_fixture();
    let payload = sa::not_found_note_payload(&t, &before).unwrap();
    assert_eq!(payload["write_sent"], false);
    assert_eq!(payload["platform_completed"], false);
    assert!(sa::complete_payload(&t, &before).is_err());
    let mut after = before.clone();
    after["row"]["remark"] = payload["note"].clone();
    after["row"]["updateTime"] = json!("new time");
    sa::verify_not_found_note(&t, &payload, &after).unwrap();
    for key in [
        "markStatus",
        "gh",
        "title",
        "id",
        "doiValue",
        "itemId",
        "reason",
    ] {
        let mut changed = after.clone();
        changed["row"][key] = json!(if key == "markStatus" {
            "已处理"
        } else {
            "changed"
        });
        assert!(sa::verify_not_found_note(&t, &payload, &changed).is_err());
    }
    let mut changed = after.clone();
    changed["comparison"][0]["sa"] = json!("Other paper");
    assert!(sa::verify_not_found_note(&t, &payload, &changed).is_err());
    assert!(sa::verify_not_found_note(
        &t,
        &payload,
        &json!({"row":{"markStatus":"待处理","remark":payload["note"]}})
    )
    .is_err());
    assert!(sa::verify_not_found_note(&t, &payload, &before).is_err());
    assert!(!t.record.done);
    assert_eq!(t.stage, Stage::AwaitingReview);
}

#[test]
fn note_handoff_rejects_changed_scope_review_version_and_remote_checkpoint() {
    let (t, before) = note_fixture();
    let payload = sa::not_found_note_payload(&t, &before).unwrap();
    let mut after = before.clone();
    after["row"]["remark"] = payload["note"].clone();
    for key in ["input", "record", "review", "scope", "batch", "stage"] {
        let mut changed = t.clone();
        match key {
            "input" => changed.input_hash = "other version".into(),
            "record" => changed.record.row += 1,
            "review" => changed.review.as_mut().unwrap().note.push_str("new note"),
            "scope" => changed.evidence[0].text = "invalid observation".into(),
            "batch" => changed.batch = Some(json!({"id":"batch-1"})),
            _ => changed.stage = Stage::Unknown,
        }
        assert!(sa::verify_not_found_note(&changed, &payload, &after).is_err());
    }
}

#[test]
fn manual_note_history_survives_restart_and_only_exact_fresh_readback_is_projected() {
    let (mut t, before) = note_fixture();
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    store
        .import(vec![t.record.clone()], t.input_hash.clone())
        .unwrap();
    t.revision = store.task(&t.id).unwrap().revision;
    let payload = sa::not_found_note_payload(&t, &before).unwrap();
    t.evidence.push(Evidence {
        id: "plan".into(),
        kind: "sa_note_handoff".into(),
        source: "机构库".into(),
        text: payload.to_string(),
        created: now(),
    });
    store.save(&mut t, "fixture_note_handoff").unwrap();
    let reopened = Store::new(dir.path()).unwrap();
    let mut current = reopened.task(&t.id).unwrap();
    assert_eq!(sa::latest_note_plan(&current).unwrap().0, "plan");
    assert_eq!(sa::note_status(&current).unwrap()["verified"], false);
    let mut after = before.clone();
    after["row"]["remark"] = payload["note"].clone();
    current.sa_snapshot = Some(after.clone());
    // Seeing a matching remark alone isn't a saved verification receipt.
    assert_eq!(sa::note_status(&current).unwrap()["verified"], false);
    sa::verify_not_found_note(&current, &payload, &after).unwrap();
    current.evidence.push(Evidence { id:"verified".into(),kind:"sa_note_verified".into(),source:"机构库".into(),
        text:json!({"schema":"sa_not_found_note_verified_v1","plan_id":"plan","payload":payload,"sa_after":after,"write_sent":false,"platform_completed":false}).to_string(),created:now() });
    store.save(&mut current, "fixture_note_verified").unwrap();
    let recovered = Store::new(dir.path()).unwrap().task(&current.id).unwrap();
    assert_eq!(sa::note_status(&recovered).unwrap()["verified"], true);
    assert!(!recovered.record.done);
    assert_eq!(recovered.stage, Stage::AwaitingReview);
    assert!(recovered.assert_complete().is_err());
    assert!(!classification::sources(dir.path(), &recovered)
        .unwrap()
        .iter()
        .any(|e| e.id == "plan" || e.id == "verified"));
    current.sa_snapshot.as_mut().unwrap()["row"]["remark"] = json!("changed later");
    assert_eq!(sa::note_status(&current).unwrap()["verified"], false);
}

#[test]
fn completion_readback_binds_full_original_row_and_proof_after_restart() {
    let mut t = task();
    let before = complete_snapshot();
    issues::prepare(&mut t, &before).unwrap();
    let payload = sa::complete_payload(&t, &before).unwrap();
    let mut after = before.clone();
    after["row"]["markStatus"] = json!("已处理");
    after["row"]["remark"] = payload["note"].clone();
    after["row"]["updateTime"] = json!("new server time");
    after["row"]["updateUsername"] = json!("operator");
    sa::verify_complete(&t, &payload, &after).unwrap();
    t.stage = Stage::Unknown;
    sa::verify_complete(&t, &payload, &after).unwrap();
    for key in [
        "id",
        "saLzkId",
        "gh",
        "itemId",
        "matchCount",
        "title",
        "titleValue",
        "doi",
        "doiValue",
        "wos",
        "wosValue",
        "reason",
        "claimStatus",
        "qr",
        "remark",
        "markStatus",
    ] {
        let mut changed = after.clone();
        changed["row"][key] = json!("changed");
        assert!(
            sa::verify_complete(&t, &payload, &changed).is_err(),
            "{key}"
        );
    }
    let mut missing = after.clone();
    missing["row"].as_object_mut().unwrap().remove("doi");
    assert!(sa::verify_complete(&t, &payload, &missing).is_err());
    let mut changed = after.clone();
    changed["comparison"][0]["library"] = json!("Other paper");
    assert!(sa::verify_complete(&t, &payload, &changed).is_err());
    let thin = json!({"sa_id":t.id,"expected":before["row"],"note":payload["note"]});
    assert!(sa::verify_complete(&t, &thin, &after).is_err());
    let mut changed = t.clone();
    changed.input_hash = "new-input".into();
    assert!(sa::verify_complete(&changed, &payload, &after).is_err());
    let mut changed = t.clone();
    changed.review.as_mut().unwrap().note = "different decision".into();
    assert!(sa::verify_complete(&changed, &payload, &after).is_err());
    let mut changed = t.clone();
    changed
        .evidence
        .iter_mut()
        .find(|e| e.id == "human")
        .unwrap()
        .text = "replaced proof".into();
    assert!(sa::verify_complete(&changed, &payload, &after).is_err());
    assert!(
        !t.record.done,
        "read-only verification does not mark the roster done"
    );
}

#[test]
fn completion_never_marks_not_found_or_replays_unknown_intent() {
    let before = complete_snapshot();
    for route in [Route::NotFound, Route::ZeroReview] {
        let mut t = task();
        t.route = route.clone();
        t.review.as_mut().unwrap().route = route;
        assert!(sa::complete_payload(&t, &before).is_err());
    }
    let mut t = task();
    issues::prepare(&mut t, &before).unwrap();
    for stage in [Stage::Unknown, Stage::Completed] {
        t.stage = stage;
        assert!(sa::complete_payload(&t, &before).is_err());
    }
}

#[test]
fn non_sjtu_completion_keeps_original_zero_match_and_full_identity() {
    let mut t = task();
    t.route = Route::NonSjtu;
    t.platform_id.clear();
    let review = t.review.as_mut().unwrap();
    review.route = Route::NonSjtu;
    review.platform_id.clear();
    review.note = "原文署名已核验：非交大成果".into();
    t.evidence
        .iter_mut()
        .find(|e| e.id == "human")
        .unwrap()
        .text = "原文完整单位署名无交大".into();
    let mut before = complete_snapshot();
    before["row"]["matchCount"] = json!(0);
    before["row"]["itemId"] = json!("");
    let payload = sa::complete_payload(&t, &before).unwrap();
    let mut after = before;
    after["row"]["markStatus"] = json!("已处理");
    after["row"]["remark"] = payload["note"].clone();
    t.stage = Stage::Unknown;
    sa::verify_complete(&t, &payload, &after).unwrap();
    assert_eq!(t.record.matches, 0);
    assert!(!t.record.done);
    after["row"]["matchCount"] = json!(1);
    after["row"]["itemId"] = json!(ITEM);
    assert!(sa::verify_complete(&t, &payload, &after).is_err());
}
