use crate::{alias::*, *};
use serde_json::{json, Value};
fn setup() -> (Task, Value, Value, Value) {
    let mut task = Task::new(
        Record {
            row: 2,
            owner: "owner".into(),
            sa_id: "sa-1".into(),
            title: "Paper".into(),
            doi: "".into(),
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
    task.evidence.push(Evidence {
        id: "source".into(),
        kind: "human_review".into(),
        source: "Original paper".into(),
        text: "Paper author: Demo, X; staff 001 identity verified".into(),
        created: 1,
    });
    let live = json!({"row":{"saLzkId":"sa-1","gh":"001","titleValue":"Paper","itemId":"item-1","matchCount":1,"markStatus":"待处理"}});
    let prepared = json!({"sa":live,"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),"staff_id":"001","scholar":{"id":"scholar-1","wno":"001","nameCn":"Tester","nameEn":"Tester"},"aliases":[{"id":"old","nameAlias":"Tester","defaultNameCn":1,"defaultNameEn":0}]});
    let p = payload(&task, &live, &prepared, "Demo, X", "source").unwrap();
    (task, live, prepared, p)
}
fn result(p: &Value) -> Value {
    let mut rows = p["expected_aliases"].as_array().unwrap().clone();
    let target = json!({"id":"new","nameAlias":"Demo, X","defaultNameCn":0,"defaultNameEn":0});
    rows.push(target.clone());
    json!({"verified":true,"scholar":p["expected_scholar"],"aliases":rows,"alias":target})
}
#[test]
fn full_alias_result_retains_original_identity_source_and_stage() {
    let (mut task, live, _, p) = setup();
    task.stage = Stage::Unknown;
    assert_result(&task, &p, &result(&p), &live).unwrap();
    assert_eq!(p["previous_stage"], "pending");
    assert_eq!(p["source_evidence"]["text"], task.evidence[0].text);
    assert_eq!(p["source_sha256"], hash(task.evidence[0].text.as_bytes()));
}
#[test]
fn alias_preparation_must_bind_the_complete_current_sa_and_input() {
    let (task, live, prepared, _) = setup();
    for key in ["input_hash", "record_fingerprint", "staff_id"] {
        let mut stale = prepared.clone();
        stale[key] = "changed".into();
        assert_eq!(
            payload(&task, &live, &stale, "Demo, X", "source")
                .unwrap_err()
                .code,
            "TASK_CHANGED"
        );
    }
    let mut changed = live.clone();
    changed["row"]["itemId"] = "different".into();
    assert_eq!(
        payload(&task, &changed, &prepared, "Demo, X", "source")
            .unwrap_err()
            .code,
        "TASK_CHANGED"
    );
}
#[test]
fn alias_recovery_refuses_changed_task_sa_and_source_without_mutation() {
    let (task, live, _, p) = setup();
    let proof = result(&p);
    let mut changed = task.clone();
    changed.record.title = "Other paper".into();
    assert!(assert_result(&changed, &p, &proof, &live).is_err());
    let mut changed = live.clone();
    changed["row"]["markStatus"] = "已处理".into();
    assert_eq!(
        assert_result(&task, &p, &proof, &changed).unwrap_err().code,
        "TASK_CHANGED"
    );
    let mut changed = task.clone();
    changed.evidence[0].source = "Substituted source".into();
    assert_eq!(
        assert_result(&changed, &p, &proof, &live).unwrap_err().code,
        "EVIDENCE_REQUIRED"
    );
    let mut changed = proof.clone();
    changed["scholar"]["nameEn"] = "Other".into();
    assert_eq!(
        assert_result(&task, &p, &changed, &live).unwrap_err().code,
        "IDENTITY_CONFLICT"
    );
}
#[test]
fn found_target_cannot_hide_lost_or_changed_original_aliases_or_defaults() {
    let (task, live, _, p) = setup();
    for mutation in 0..5 {
        let mut changed = result(&p);
        match mutation {
            0 => {
                changed["aliases"].as_array_mut().unwrap().remove(0);
            }
            1 => changed["aliases"][0]["defaultNameCn"] = 0.into(),
            2 => changed["aliases"][0]["nameAlias"] = "Changed".into(),
            3 => {
                changed["aliases"][1]["defaultNameEn"] = 1.into();
                changed["alias"] = changed["aliases"][1].clone();
            }
            _ => changed["aliases"].as_array_mut().unwrap().push(
                json!({"id":"third","nameAlias":"Unexpected","defaultNameCn":0,"defaultNameEn":0}),
            ),
        }
        assert_eq!(
            assert_result(&task, &p, &changed, &live).unwrap_err().code,
            "REMOTE_RESULT_UNKNOWN"
        );
    }
}
#[test]
fn existing_alias_requires_exact_original_list_and_accepts_no_addition() {
    let (task, live, mut prepared, _) = setup();
    prepared["aliases"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"new","nameAlias":"Demo, X","defaultNameCn":0,"defaultNameEn":0}));
    let p = payload(&task, &live, &prepared, "Demo, X", "source").unwrap();
    let proof = json!({"verified":true,"scholar":prepared["scholar"],"aliases":prepared["aliases"],"alias":prepared["aliases"][1]});
    assert_result(&task, &p, &proof, &live).unwrap();
    let mut changed = proof;
    changed["aliases"][1]["id"] = "replacement".into();
    changed["alias"] = changed["aliases"][1].clone();
    assert!(assert_result(&task, &p, &changed, &live).is_err());
}
#[test]
fn malformed_or_ambiguous_alias_entities_never_become_recovery_evidence() {
    let (task, live, _, p) = setup();
    for mutation in 0..4 {
        let mut changed = result(&p);
        match mutation {
            0 => changed["aliases"][1]["id"] = 123.into(),
            1 => changed["aliases"][1]["id"] = "old".into(),
            2 => changed["aliases"][1]["nameAlias"] = "Tester".into(),
            _ => changed["aliases"][1]["defaultNameEn"] = Value::Null,
        }
        assert_eq!(
            assert_result(&task, &p, &changed, &live).unwrap_err().code,
            "IDENTITY_CONFLICT"
        );
    }
}
#[test]
fn missing_target_old_incomplete_intent_and_unbacked_name_stay_unconfirmed() {
    let (task, live, prepared, p) = setup();
    let missing =
        json!({"verified":true,"scholar":p["expected_scholar"],"aliases":p["expected_aliases"]});
    assert_eq!(
        assert_result(&task, &p, &missing, &live).unwrap_err().code,
        "REMOTE_RESULT_UNKNOWN"
    );
    let mut old = p.clone();
    old.as_object_mut().unwrap().remove("source_evidence");
    assert!(assert_result(&task, &old, &result(&p), &live).is_err());
    assert_eq!(
        payload(&task, &live, &prepared, "Invented", "source")
            .unwrap_err()
            .code,
        "EVIDENCE_REQUIRED"
    );
}
