use crate::{claim::*, *};
use serde_json::{json, Value};
fn setup() -> (Task, Value, Value, Value) {
    let task = Task::new(
        Record {
            row: 2,
            owner: "owner".into(),
            sa_id: "claim-1".into(),
            title: "Paper".into(),
            doi: "10.example/a".into(),
            wos: "".into(),
            staff_id: "001".into(),
            matches: 1,
            item_ids: "item-1".into(),
            mark: "待处理".into(),
            reason: "作者不一致".into(),
            skipped: false,
            done: false,
            source: "roster".into(),
        },
        "input".into(),
    );
    let live = json!({"row":{"saLzkId":"claim-1","gh":"001","titleValue":"Paper","itemId":"item-1","matchCount":1,"markStatus":"待处理","remark":"original","reason":"作者不一致"},"comparison":[{"label":"认领状态","sa":"Tester(001)①","library":"未认领"}]});
    let authors = json!([{ "index":0,"id":"author-1","order":1,"fullname":"Demo","scholarId":"","eligible":true,"relations":[]},{"index":1,"id":"author-2","order":2,"fullname":"Other","scholarId":"other-scholar","eligible":true,"relations":[{"scholarId":"other-scholar","status":6}]}]);
    let metadata = json!({"title":["Paper"],"doi":["10.example/a"],"abstract":["Full original abstract"],"author":[{"id":"author-1","order":1,"fullname":"Demo","scholarId":null,"correspondent":false,"institutionOrderNums":"1"},{"id":"author-2","order":2,"fullname":"Other","scholarId":"other-scholar","correspondent":true,"institutionOrderNums":"2"}],"authorInstitution":[{"id":"unit-1","order":1,"address":"SJTU"},{"id":"unit-2","order":2,"address":"Other"}]});
    let prepared = json!({"row":live["row"],"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),"prepared":{"item_id":"item-1","staff_id":"001","sa_text":"Tester(001)①","person":{"id":"scholar-001","wno":"001","name":"Tester","names":["Tester","Demo"]},"authors":authors,"metadata":metadata}});
    let p = payload(&task, &live, &prepared, 0).unwrap();
    (task, live, prepared, p)
}
fn result(p: &Value) -> Value {
    let mut authors = p["prepared"]["authors"].clone();
    authors[0]["scholarId"] = "scholar-001".into();
    let mut metadata = p["prepared"]["metadata"].clone();
    metadata["author"][0]["scholarId"] = "scholar-001".into();
    json!({"verified":true,"claimed":true,"item_id":"item-1","staff_id":"001","scholar_id":"scholar-001","author":"Demo","author_id":"author-1","order":1,"person":p["prepared"]["person"],"authors":authors,"metadata":metadata})
}
#[test]
fn claim_binds_selected_id_complete_metadata_and_original_sa() {
    let (mut task, live, _, p) = setup();
    task.stage = Stage::Unknown;
    assert_result(&task, &p, &result(&p), &live).unwrap();
    assert_eq!(p["original_sa"], live);
    assert_eq!(
        p["prepared"]["metadata"]["abstract"][0],
        "Full original abstract"
    );
}
#[test]
fn same_order_and_name_with_different_author_id_never_recovers() {
    let (task, live, _, p) = setup();
    let mut changed = result(&p);
    changed["authors"][0]["id"] = "replacement".into();
    changed["metadata"]["author"][0]["id"] = "replacement".into();
    changed["author_id"] = "replacement".into();
    assert_eq!(
        assert_result(&task, &p, &changed, &live).unwrap_err().code,
        "REMOTE_RESULT_UNKNOWN"
    );
}
#[test]
fn unrelated_authors_relationships_and_complete_fields_cannot_be_lost() {
    let (task, live, _, p) = setup();
    for variant in 0..5 {
        let mut changed = result(&p);
        match variant {
            0 => changed["authors"][1]["scholarId"] = "changed".into(),
            1 => changed["authors"][1]["relations"] = json!([]),
            2 => changed["metadata"]["abstract"] = json!([]),
            3 => changed["metadata"]["author"][0]["correspondent"] = true.into(),
            _ => changed["metadata"]["authorInstitution"]
                .as_array_mut()
                .unwrap()
                .reverse(),
        };
        assert!(assert_result(&task, &p, &changed, &live).is_err());
    }
}
#[test]
fn actual_success_relation_is_allowed_but_negative_or_other_relation_is_not() {
    let (task, live, _, p) = setup();
    let mut proof = result(&p);
    proof["authors"][0]["relations"] = json!([{"scholarId":"scholar-001","status":6}]);
    assert_result(&task, &p, &proof, &live).unwrap();
    proof["authors"][0]["relations"][0]["status"] = 2.into();
    assert!(assert_result(&task, &p, &proof, &live).is_err());
    proof["authors"][0]["relations"] = json!([{"scholarId":"unexpected","status":6}]);
    assert!(assert_result(&task, &p, &proof, &live).is_err());
}
#[test]
fn sa_source_original_input_or_item_change_cannot_resolve_claim() {
    let (task, live, _, p) = setup();
    for key in ["gh", "titleValue", "itemId", "markStatus", "remark"] {
        let mut changed = live.clone();
        changed["row"][key] = "changed".into();
        assert!(assert_result(&task, &p, &result(&p), &changed).is_err());
    }
    let mut changed = live.clone();
    changed["comparison"][0]["sa"] = "Tester(0001)①".into();
    assert!(assert_result(&task, &p, &result(&p), &changed).is_err());
    let mut changed = task.clone();
    changed.input_hash = "new-input".into();
    assert!(assert_result(&changed, &p, &result(&p), &live).is_err());
    let mut changed = live;
    changed["row"]["claimStatus"] = true.into();
    changed["row"]["reason"] = "".into();
    changed["row"]["updateTime"] = "after-claim".into();
    assert_result(&task, &p, &result(&p), &changed).unwrap();
}
#[test]
fn missing_or_denied_targets_never_create_a_write_intent() {
    let (task, live, prepared, _) = setup();
    for variant in 0..4 {
        let mut changed = prepared.clone();
        match variant {
            0 => changed["prepared"]["authors"][0]["id"] = "".into(),
            1 => changed["prepared"]["authors"][0]["eligible"] = false.into(),
            2 => {
                changed["prepared"]["authors"][0]["relations"] =
                    json!([{"scholarId":"scholar-001","status":2}])
            }
            _ => {
                changed["prepared"]["metadata"]["author"][0]["data"] = json!({"scholarId":"manual"})
            }
        };
        assert!(payload(&task, &live, &changed, 0).is_err());
    }
    let mut changed = live;
    changed["comparison"][0]["sa"] = "Tester(999)①".into();
    let mut prepared = prepared;
    prepared["prepared"]["sa_text"] = "Tester(999)①".into();
    assert!(payload(&task, &changed, &prepared, 0).is_err());
}
#[test]
fn old_partial_intent_forged_proof_and_multiple_claims_remain_unknown() {
    let (task, live, _, p) = setup();
    let mut old = p.clone();
    old["prepared"].as_object_mut().unwrap().remove("metadata");
    assert!(assert_result(&task, &old, &result(&p), &live).is_err());
    let mut proof = result(&p);
    proof["person"]["name"] = "Different person".into();
    assert!(assert_result(&task, &p, &proof, &live).is_err());
    let mut proof = result(&p);
    proof["authors"][1]["scholarId"] = "scholar-001".into();
    assert!(assert_result(&task, &p, &proof, &live).is_err());
    let mut proof = result(&p);
    proof["verified"] = false.into();
    assert!(assert_result(&task, &p, &proof, &live).is_err());
}
