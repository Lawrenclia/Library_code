use crate::issues::*;
use crate::*;
use serde_json::{json, Value};
fn task() -> Task {
    Task::new(
        Record {
            row: 2,
            owner: "测试".into(),
            sa_id: "sa1".into(),
            title: "Paper".into(),
            doi: "".into(),
            wos: "".into(),
            staff_id: "00001".into(),
            matches: 1,
            item_ids: "item1".into(),
            mark: "待处理".into(),
            reason: "通讯作者标记不一致；第一作者标记不一致".into(),
            skipped: false,
            done: false,
            source: "".into(),
        },
        "input".into(),
    )
}
fn snapshot(reason: &str, sa: &str, library: &str, claimed: &str) -> Value {
    json!({"row":{"saLzkId":"sa1","itemId":",item1","matchCount":1,"gh":"00001","titleValue":"Paper","reason":reason},"comparison":[
        {"label":"题名","sa":"Paper","library":"Paper"},
        {"label":"作者信息","sa":sa,"library":library},
        {"label":"认领状态","sa":"测试(00001)","library":claimed},
        {"label":"交大是否第一单位","sa":"是","library":"是"},
        {"label":"DOI","sa":"10.example/a","library":"10.example/a"},
        {"label":"WOS记录号","sa":"WOS:000000000000001","library":"WOS:000000000000001"}]})
}
fn evidence(task: &mut Task) {
    task.evidence.push(Evidence {
        id: "paper".into(),
        kind: "human_review".into(),
        source: "原文".into(),
        text: "原文的作者角色与署名顺序".into(),
        created: now(),
    });
}
const SA: &str = "姓名：测试\n工号：00001\n是否第一作者：是\n是否通讯作者：是";
const LIB: &str = "署名：Tester\n工号：00001\n是否第一作者：否\n是否通讯作者：否";
#[test]
fn reasons_are_independent_and_global_checkbox_cannot_complete() {
    let mut t = task();
    let s = snapshot(&t.record.reason, SA, LIB, "已认领");
    let p = prepare(&mut t, &s).unwrap();
    assert_eq!(p["requirements"].as_array().unwrap().len(), 2);
    assert!(!p["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["key"] == "author_claim"));
    t.review = Some(Review {
        route: Route::Existing,
        evidence_id: "paper".into(),
        library_checked: true,
        platform_id: "item1".into(),
        affiliation_confirmed: true,
        identity_confirmed: true,
        issues_resolved: true,
        note: "总勾选不能取代逐项处理".into(),
    });
    t.sa_snapshot = Some(s);
    assert_eq!(t.assert_complete().unwrap_err().code, "REVIEW_REQUIRED");
}
#[test]
fn sa_correct_requires_actual_corrected_field_and_every_reason() {
    let mut t = task();
    let before = snapshot(&t.record.reason, SA, LIB, "已认领");
    prepare(&mut t, &before).unwrap();
    evidence(&mut t);
    assert!(resolve(
        &mut t,
        &before,
        "corresponding_author",
        "sa_correct",
        "paper",
        "原文确认是通讯作者"
    )
    .is_err());
    let after = snapshot(&t.record.reason, SA, SA, "已认领");
    resolve(
        &mut t,
        &after,
        "corresponding_author",
        "sa_correct",
        "paper",
        "经原文核对是通讯作者，已修正",
    )
    .unwrap();
    assert!(assert_resolved(&t, &after).is_err());
    resolve(
        &mut t,
        &after,
        "first_author",
        "sa_correct",
        "paper",
        "原文第一作者，已修正",
    )
    .unwrap();
    assert_resolved(&t, &after).unwrap();
    assert!(assert_resolved(&t, &before).is_err());
}
#[test]
fn library_correct_keeps_fields_but_requires_real_per_issue_evidence() {
    let mut t = task();
    let s = snapshot(
        "通讯作者标记不一致",
        SA,
        &LIB.replace("是否第一作者：否", "是否第一作者：是"),
        "已认领",
    );
    prepare(&mut t, &s).unwrap();
    assert!(resolve(
        &mut t,
        &s,
        "corresponding_author",
        "library_correct",
        "paper",
        "经原文核对不是通讯作者"
    )
    .is_err());
    evidence(&mut t);
    resolve(
        &mut t,
        &s,
        "corresponding_author",
        "library_correct",
        "paper",
        "经原文核对不是通讯作者",
    )
    .unwrap();
    assert_resolved(&t, &s).unwrap();
    let mut changed = s.clone();
    changed["row"]["itemId"] = "other-item".into();
    assert_eq!(
        assert_resolved(&t, &changed).unwrap_err().code,
        "TASK_CHANGED"
    );
}
#[test]
fn claim_requires_current_relationship_and_exact_staff_id() {
    let mut t = task();
    let s = snapshot("作者不一致", SA, SA, "未认领");
    prepare(&mut t, &s).unwrap();
    evidence(&mut t);
    assert!(resolve(&mut t, &s, "author_claim", "claimed", "paper", "已认领").is_err());
    let wrong = snapshot("作者不一致", SA, &SA.replace("00001", "00002"), "已认领");
    assert!(resolve(&mut t, &wrong, "author_claim", "claimed", "paper", "已认领").is_err());
    let correct = snapshot("作者不一致", SA, SA, "已认领");
    resolve(
        &mut t,
        &correct,
        "author_claim",
        "claimed",
        "paper",
        "本工号已认领",
    )
    .unwrap();
    assert_resolved(&t, &correct).unwrap();
}
#[test]
fn identifier_mismatch_needs_same_paper_source_not_string_similarity() {
    let mut t = task();
    let mut s = snapshot("", SA, SA, "已认领");
    s["comparison"][4]["library"] = "10.example/other".into();
    s["comparison"][5]["library"] = "WOS:000000000000002".into();
    let plan = prepare(&mut t, &s).unwrap();
    assert_eq!(plan["requirements"][0]["key"], "identifiers");
    evidence(&mut t);
    assert!(resolve(
        &mut t,
        &s,
        "identifiers",
        "library_correct",
        "paper",
        "两个标识符相似"
    )
    .is_err());
    resolve(
        &mut t,
        &s,
        "identifiers",
        "same_paper",
        "paper",
        "通过 DOI 对应原文核对为同一篇",
    )
    .unwrap();
    assert_resolved(&t, &s).unwrap();
    s["comparison"][4]["library"] = "10.example/new".into();
    assert!(assert_resolved(&t, &s).is_err());
}
#[test]
fn unknown_reasons_remain_explicit_and_new_reasons_block_completion() {
    let mut t = task();
    let s = snapshot("新的待处理原因", SA, SA, "已认领");
    let plan = prepare(&mut t, &s).unwrap();
    let key = plan["requirements"][0]["key"].as_str().unwrap();
    assert!(key.starts_with("other:"));
    evidence(&mut t);
    resolve(
        &mut t,
        &s,
        key,
        "other_confirmed",
        "paper",
        "具体原文核对后的结论",
    )
    .unwrap();
    assert_resolved(&t, &s).unwrap();
    let mut changed = s.clone();
    changed["row"]["reason"] = "新的待处理原因；通讯作者标记不一致".into();
    assert!(assert_resolved(&t, &changed).is_err());
}
#[test]
fn ambiguous_author_roles_and_missing_fields_never_pass() {
    let mut t = task();
    let s = snapshot(
        "通讯作者标记不一致",
        SA,
        &format!("{SA}\n是否通讯作者：否"),
        "已认领",
    );
    prepare(&mut t, &s).unwrap();
    evidence(&mut t);
    assert!(resolve(
        &mut t,
        &s,
        "corresponding_author",
        "library_correct",
        "paper",
        "署名相似"
    )
    .is_err());
    let mut bad = s.clone();
    bad["row"]["gh"] = 1.into();
    assert!(prepare(&mut t, &bad).is_err());
}

#[test]
fn first_institution_must_be_corrected_and_conclusions_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path()).unwrap();
    store.import(vec![task().record], "input".into()).unwrap();
    let mut t = store.task("sa1").unwrap();
    let mut before = snapshot("交大是否第一单位不一致", SA, SA, "已认领");
    before["comparison"][3]["library"] = "否".into();
    prepare(&mut t, &before).unwrap();
    evidence(&mut t);
    assert!(resolve(
        &mut t,
        &before,
        "first_institution",
        "sa_correct",
        "paper",
        "原文交大排第一单位"
    )
    .is_err());
    let mut after = before.clone();
    after["comparison"][3]["library"] = "是".into();
    resolve(
        &mut t,
        &after,
        "first_institution",
        "sa_correct",
        "paper",
        "原文交大第一单位，已修正",
    )
    .unwrap();
    t.sa_snapshot = Some(after.clone());
    store.save(&mut t, "issue_review").unwrap();
    let reopened = Store::new(dir.path()).unwrap();
    reopened.recover().unwrap();
    let restored = reopened.task("sa1").unwrap();
    assert_eq!(restored.issue_plan.as_ref().unwrap().baseline, before);
    assert_eq!(restored.issue_reviews.len(), 1);
    assert_resolved(&restored, &after).unwrap();
}
