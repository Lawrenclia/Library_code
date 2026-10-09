use crate::*;
use serde_json::{json, Value};
fn setup() -> (Task, Value, Value) {
    let record:Record=serde_json::from_value(json!({"row":2,"owner":"owner","sa_id":"merge-1","title":"Paper","doi":"10.example/a","wos":"","staff_id":"001","matches":2,"item_ids":"a,b","mark":"待处理","reason":"重复数据","skipped":false,"done":false,"source":"list.xlsx"})).unwrap();
    let mut task = Task::new(record, "input".into());
    task.evidence.push(Evidence {
        id: "proof".into(),
        kind: "human_review".into(),
        source: "original paper".into(),
        text: "same paper; retain main publication/author/unit fields".into(),
        created: 0,
    });
    let sa = json!({"row":{"saLzkId":task.id,"gh":"001","titleValue":"Paper","markStatus":"待处理","itemId":"a,b","matchCount":2,"remark":"original"},"comparison":[{"label":"DOI","sa":"10.example/a","library":"10.example/a"}]});
    let a = json!({"id":"a","model_name":"journal","metadata":{"title":["Paper"],"doi":["10.example/a"],"abstract":["complete abstract"],"author":[{"id":"author-1","order":1,"correspondent":true}],"authorInstitution":[{"id":"unit-1","order":1}]}});
    let b = json!({"id":"b","model_name":"science","metadata":{"title":["Paper corrected"],"doi":["10.example/a"],"pages":["10-20"]}});
    let prepared = json!({"sa":sa,"row":sa["row"],"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),"result":{"title":"Paper","matched_ids":["a","b"],"title_similarity":93.5,"group":{"id":"g","primary_id":"a","items":[a,b]}}});
    let p=merge::payload(&task,&sa,&prepared,&json!({"source_id":"b","target_id":"a","evidence_id":"proof","identity_confirmed":true,"retained":"retain complete main fields"})).unwrap();
    (task, sa, p)
}
fn after(sa: &Value) -> Value {
    let mut s = sa.clone();
    s["row"]["itemId"] = json!("a");
    s["row"]["matchCount"] = json!(1);
    s
}
fn result(p: &Value) -> Value {
    json!({"verified":true,"source_id":"b","target_id":"a","master_after":{"id":"a","model_name":"journal","fields":p["expected_master"]["metadata"]},"pool_after":[]})
}
#[test]
fn exact_merge_recovers_complete_context_without_finishing_sa() {
    let (mut t, s, p) = setup();
    t.stage = Stage::Unknown;
    merge::assert_result(&t, &p, &result(&p), &after(&s)).unwrap();
    assert_eq!(
        p["prepared"]["result"]["group"]["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(!t.record.done);
}
#[test]
fn same_title_cannot_hide_lost_publication_author_or_unit_fields() {
    let (t, s, p) = setup();
    for key in ["doi", "abstract", "author", "authorInstitution"] {
        let mut r = result(&p);
        r["master_after"]["fields"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(merge::assert_result(&t, &p, &r, &after(&s)).is_err());
    }
    let mut r = result(&p);
    r["master_after"]["fields"]["author"][0]["correspondent"] = json!(false);
    assert!(merge::assert_result(&t, &p, &r, &after(&s)).is_err());
}
#[test]
fn original_and_source_titles_are_allowed_but_other_field_changes_are_not() {
    let (t, s, p) = setup();
    let mut r = result(&p);
    r["master_after"]["fields"]["title"] = json!(["Paper corrected", "Paper"]);
    r["master_after"]["fields"]["pages"] = json!(["10-20"]);
    merge::assert_result(&t, &p, &r, &after(&s)).unwrap();
    r["master_after"]["fields"]["pages"] = json!(["unrelated"]);
    assert!(merge::assert_result(&t, &p, &r, &after(&s)).is_err());
}
#[test]
fn original_sa_source_or_other_fields_and_input_remain_bound() {
    let (t, s, p) = setup();
    let mut changed = after(&s);
    changed["row"]["remark"] = json!("changed");
    assert!(merge::assert_result(&t, &p, &result(&p), &changed).is_err());
    changed = after(&s);
    changed["comparison"][0]["sa"] = json!("changed");
    assert!(merge::assert_result(&t, &p, &result(&p), &changed).is_err());
    let mut other = t.clone();
    other.input_hash = "new".into();
    assert!(merge::assert_result(&other, &p, &result(&p), &after(&s)).is_err());
    other = t;
    other.record.owner = "changed".into();
    assert!(merge::assert_result(&other, &p, &result(&p), &after(&s)).is_err());
}
#[test]
fn old_partial_intent_changed_source_proof_and_selection_cannot_recover() {
    let (mut t, s, p) = setup();
    let mut old = p.clone();
    old.as_object_mut().unwrap().remove("schema");
    assert!(merge::assert_result(&t, &old, &result(&p), &after(&s)).is_err());
    let mut changed = p.clone();
    changed["expected_master"]["metadata"]["doi"] = json!(["forged"]);
    assert!(merge::assert_result(&t, &changed, &result(&p), &after(&s)).is_err());
    t.evidence[0].text = "replaced".into();
    assert!(merge::assert_result(&t, &p, &result(&p), &after(&s)).is_err());
}
#[test]
fn source_still_present_wrong_master_and_uncommitted_sa_stay_unknown() {
    let (t, s, p) = setup();
    assert!(merge::assert_result(&t, &p, &result(&p), &s).is_err());
    let mut r = result(&p);
    r["pool_after"] = json!([{"items":[p["expected_source"]]}]);
    assert!(merge::assert_result(&t, &p, &r, &after(&s)).is_err());
    r = result(&p);
    r["master_after"]["model_name"] = json!("wrong");
    assert!(merge::assert_result(&t, &p, &r, &after(&s)).is_err());
}
#[test]
fn remaining_third_match_is_preserved_and_not_completed() {
    let (mut t, mut s, mut p) = setup();
    t.record.matches = 3;
    t.record.item_ids = "a,b,c".into();
    s["row"]["itemId"] = json!("a,b,c");
    s["row"]["matchCount"] = json!(3);
    p["prepared"]["row"] = s["row"].clone();
    p["prepared"]["sa"] = s.clone();
    p["prepared"]["result"]["matched_ids"] = json!(["a", "b", "c"]);
    p["prepared"]["record_fingerprint"] = json!(t.record.fingerprint());
    let q=merge::payload(&t,&s,&p["prepared"],&json!({"source_id":"b","target_id":"a","evidence_id":"proof","identity_confirmed":true,"retained":"retain main"})).unwrap();
    let mut live = s.clone();
    live["row"]["itemId"] = json!("a,c");
    live["row"]["matchCount"] = json!(2);
    merge::assert_result(&t, &q, &result(&q), &live).unwrap();
    assert!(!t.record.done);
}
