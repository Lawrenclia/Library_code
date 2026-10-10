use crate::*;
use serde_json::{json, Value};
fn prepared_task() -> (Task, Value, Value) {
    let mut task = Task::new(
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
            reason: "通讯作者标记不一致".into(),
            skipped: false,
            done: false,
            source: "".into(),
        },
        "input".into(),
    );
    let sa = json!({"row":{"saLzkId":"sa1","itemId":"item1","matchCount":1,"gh":"00001","titleValue":"Paper","reason":"通讯作者标记不一致","markStatus":"待处理"},"comparison":[{"label":"作者信息","sa":"工号：00001\n是否第一作者：否\n是否通讯作者：是","library":"署名：Tester\n工号：00001\n是否第一作者：否\n是否通讯作者：否"}]});
    issues::prepare(&mut task, &sa).unwrap();
    let scholar = json!({"id":"scholar1","wno":"00001","nameCn":"测试","nameEn":"Tester"});
    let prepared = json!({"sa":sa,"identity":{"staff_id":"00001","scholar":scholar,"aliases":[]},"names":["Tester"],"result":{"item_id":"item1","staff_id":"00001","scholar":scholar,"authors":[{"index":0,"id":"author1","fullname":"Tester","order":2,"eligible":true,"fields":["correspondent","commonFirst"]}],"snapshot":{"fields":[{"fieldName":"author"}],"form":{"id":"item1","modelId":"model1","modelName":"期刊论文","datasetIds":["institution1"],"dataSources":["WOS"],"fullTexts":[{"id":"attachment1"}],"metadata":{"title":["Paper"],"doi":["10.1234/test"],"pages":["1-10"],"author":[{"id":"author1","fullname":"Tester","order":2,"scholarId":"scholar1","institutionOrderNums":"1,2","correspondent":false,"commonFirst":false}],"authorInstitution":[{"order":1,"address":"University"}]}}}}});
    (task, sa, prepared)
}
fn payload(task: &mut Task, sa: &Value, p: &Value) -> Result<Value> {
    metadata::payload(
        task,
        sa,
        p,
        "corresponding_author",
        0,
        "原文",
        "该工号作者为通讯作者，已核对星号和邮箱",
        "经核对是通讯作者",
    )
}
fn result(payload: &Value) -> Value {
    let mut after = payload["expected_snapshot"]["form"].clone();
    after["metadata"]["author"][0]["correspondent"] = true.into();
    after["updateTime"] = "fresh-version".into();
    json!({"verified":true,"item_id":"item1","staff_id":"00001","author_id":"author1","key":"corresponding_author","value":true,"before":payload["expected_snapshot"]["form"],"after":after})
}
#[test]
fn metadata_edit_binds_source_staff_author_and_initial_sa_value() {
    let (mut t, s, p) = prepared_task();
    let cmd = payload(&mut t, &s, &p).unwrap();
    assert_eq!(cmd["value"], true);
    assert_eq!(cmd["author_id"], "author1");
    assert_eq!(cmd["staff_id"], "00001");
    assert_eq!(t.evidence.len(), 1);
    assert_eq!(t.evidence[0].id, cmd["evidence_id"]);
    assert!(is_write("save_metadata"));
    assert!(is_write("metadata_save"));
    assert!(!is_write("metadata_check"));
    metadata::assert_result(&cmd, &result(&cmd)).unwrap();
}
#[test]
fn metadata_edit_requires_evidence_and_actual_visible_control() {
    let (mut t, s, mut p) = prepared_task();
    assert_eq!(
        metadata::payload(
            &mut t,
            &s,
            &p,
            "corresponding_author",
            0,
            "",
            "依据",
            "备注"
        )
        .unwrap_err()
        .code,
        "EVIDENCE_REQUIRED"
    );
    assert!(t.evidence.is_empty());
    p["result"]["authors"][0]["fields"] = json!([]);
    assert_eq!(
        payload(&mut t, &s, &p).unwrap_err().code,
        "PAGE_UNSUPPORTED"
    );
    assert!(t.evidence.is_empty());
}
#[test]
fn metadata_edit_rejects_wrong_identity_stale_sa_and_author() {
    for change in ["staff", "scholar", "author", "sa"] {
        let (mut t, mut s, mut p) = prepared_task();
        match change {
            "staff" => p["identity"]["staff_id"] = "1".into(),
            "scholar" => p["identity"]["scholar"]["id"] = "wrong".into(),
            "author" => p["result"]["authors"][0]["fullname"] = "Other".into(),
            _ => s["row"]["reason"] = "new cause".into(),
        };
        assert!(payload(&mut t, &s, &p).is_err(), "{change}");
        assert!(t.evidence.is_empty());
    }
    let (mut t, s, mut p) = prepared_task();
    p["result"]["authors"][0]["eligible"] = false.into();
    assert_eq!(
        payload(&mut t, &s, &p).unwrap_err().code,
        "IDENTITY_CONFLICT"
    );
}
#[test]
fn metadata_result_rejects_other_field_loss_and_forged_targets() {
    let (mut t, s, p) = prepared_task();
    let cmd = payload(&mut t, &s, &p).unwrap();
    for change in ["pages", "affiliation", "attachments", "target", "role"] {
        let mut r = result(&cmd);
        match change {
            "pages" => r["after"]["metadata"]["pages"] = json!([]),
            "affiliation" => r["after"]["metadata"]["authorInstitution"] = json!([]),
            "attachments" => r["after"]["fullTexts"] = json!([]),
            "target" => r["author_id"] = "other".into(),
            _ => r["after"]["metadata"]["author"][0]["correspondent"] = false.into(),
        };
        assert_eq!(
            metadata::assert_result(&cmd, &r).unwrap_err().code,
            "REMOTE_RESULT_UNKNOWN",
            "{change}"
        );
    }
}
#[test]
fn metadata_encoding_only_allows_documented_order_number_conversion() {
    let (mut t, s, p) = prepared_task();
    let cmd = payload(&mut t, &s, &p).unwrap();
    let mut r = result(&cmd);
    r["after"]["metadata"]["author"][0]["institutionOrderNums"] = json!(["1", "2"]);
    metadata::assert_result(&cmd, &r).unwrap();
    r["after"]["metadata"]["author"][0]["institutionOrderNums"] = json!(["2", "1"]);
    assert!(metadata::assert_result(&cmd, &r).is_err());
}
#[test]
fn metadata_save_does_not_bypass_unknown_or_completed_stage_or_invent_unit_controls() {
    let (mut t, s, p) = prepared_task();
    assert_eq!(
        metadata::payload(
            &mut t,
            &s,
            &p,
            "first_institution",
            0,
            "原文",
            "依据",
            "备注"
        )
        .unwrap_err()
        .code,
        "PAGE_UNSUPPORTED"
    );
    for stage in [Stage::Unknown, Stage::Completed] {
        t.stage = stage;
        assert_eq!(
            payload(&mut t, &s, &p).unwrap_err().code,
            "INVALID_TRANSITION"
        );
    }
    assert!(t.evidence.is_empty());
}
