use crate::*;
use serde_json::{json, Value};

fn setup(key: &str, sa_value: &str) -> (Task, Value, Value) {
    let reason = if key == "first_author" {
        "第一作者标记不一致"
    } else {
        "交大是否第一单位不一致"
    };
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
            reason: reason.into(),
            skipped: false,
            done: false,
            source: "".into(),
        },
        "input".into(),
    );
    let author_sa = format!("工号：00001\n是否第一作者：{sa_value}\n是否通讯作者：否");
    let label = if key == "first_author" {
        "作者信息"
    } else {
        "交大是否第一单位"
    };
    let comparison = if key == "first_author" {
        json!([{ "label": label, "sa": author_sa, "library": "署名：Tester\n工号：00001\n是否第一作者：否\n是否通讯作者：否" }])
    } else {
        json!([{ "label": "作者信息", "sa": author_sa, "library": "署名：Tester\n工号：00001\n是否第一作者：否\n是否通讯作者：否" }, { "label": label, "sa": sa_value, "library": "否" }])
    };
    let sa = json!({"row":{"saLzkId":"sa1","itemId":"item1","matchCount":1,"gh":"00001","titleValue":"Paper","reason":reason,"markStatus":"待处理"},"comparison":comparison});
    issues::prepare(&mut task, &sa).unwrap();
    let scholar = json!({"id":"scholar1","wno":"00001"});
    let prepared = json!({"sa":sa,"identity":{"staff_id":"00001","scholar":scholar},"names":["Tester"],"result":{
        "item_id":"item1","staff_id":"00001","scholar":scholar,"can_reorder_authors":true,"can_reorder_institutions":true,
        "authors":[{"index":0,"id":"author0","fullname":"Other","order":1,"eligible":false,"fields":[]},{"index":1,"id":"author1","fullname":"Tester","order":2,"eligible":true,"fields":[]}],
        "institutions":[{"index":0,"order":1,"address":"Other university","first_institution_value":"否"},{"index":1,"order":2,"address":"SJTU","first_institution_value":"是"}],
        "snapshot":{"fields":[{"fieldName":"author"},{"fieldName":"authorInstitution"}],"form":{
            "id":"item1","modelId":"model1","modelName":"期刊论文","datasetIds":["dataset1"],"dataSources":["WOS"],"fullTexts":[{"id":"file1","name":"original.pdf"}],
            "metadata":{"title":["Paper"],"doi":["10.1234/test"],"abstract":["Complete original abstract"],"pages":["100-120"],
                "author":[{"id":"author0","fullname":"Other","order":1,"institutionOrderNums":"1","scholarId":"","commonFirst":false,"correspondent":true,"email":"other@example.invalid"},
                  {"id":"author1","fullname":"Tester","order":2,"institutionOrderNums":["1","2"],"scholarId":"scholar1","commonFirst":false,"ownFirst":false,"commonCorrespondent":true,"email":"tester@example.invalid"}],
                "authorInstitution":[{"id":"unit0","order":1,"address":"Other university","topInstitutionId":"other"},{"id":"unit1","order":2,"address":"SJTU","topInstitutionId":"sjtu-synthetic"}]}}}}});
    (task, sa, prepared)
}
fn build(
    t: &mut Task,
    sa: &Value,
    p: &Value,
    key: &str,
    operation: &str,
    order: &Value,
) -> Result<Value> {
    metadata::order_payload(
        t,
        sa,
        p,
        key,
        1,
        operation,
        order,
        "原文 PDF",
        "已核对完整署名、单位顺序及工号身份",
        "按原文完整顺序修正",
    )
}
fn result(payload: &Value) -> Value {
    json!({"verified":true,"item_id":"item1","staff_id":"00001","author_id":"author1","key":payload["key"],"value":payload["value"],"operation":payload["operation"],"before":payload["expected_snapshot"]["form"],"after":payload["expected_form"]})
}
#[test]
fn author_reordering_preserves_every_identity_role_affiliation_and_publication_field() {
    let (mut t, sa, p) = setup("first_author", "是");
    let cmd = build(
        &mut t,
        &sa,
        &p,
        "first_author",
        "author_order",
        &json!([1, 0]),
    )
    .unwrap();
    let mut expected = p["result"]["snapshot"]["form"].clone();
    let old = expected["metadata"]["author"].clone();
    expected["metadata"]["author"] = json!([old[1], old[0]]);
    expected["metadata"]["author"][0]["order"] = json!(1);
    expected["metadata"]["author"][1]["order"] = json!(2);
    assert_eq!(cmd["expected_form"], expected);
    assert_eq!(
        p["result"]["snapshot"]["form"]["metadata"]["author"][1]["order"],
        2
    );
    assert_eq!(t.evidence.len(), 1);
    assert_eq!(t.evidence[0].id, cmd["evidence_id"]);
    metadata::assert_result(&cmd, &result(&cmd)).unwrap();
}
#[test]
fn institution_reordering_remaps_all_author_links_without_changing_their_entities() {
    let (mut t, sa, p) = setup("first_institution", "是");
    let cmd = build(
        &mut t,
        &sa,
        &p,
        "first_institution",
        "institution_order",
        &json!([1, 0]),
    )
    .unwrap();
    let form = &cmd["expected_form"];
    assert_eq!(form["metadata"]["authorInstitution"][0]["id"], "unit1");
    assert_eq!(form["metadata"]["authorInstitution"][1]["id"], "unit0");
    assert_eq!(form["metadata"]["author"][0]["institutionOrderNums"], "2");
    assert_eq!(
        form["metadata"]["author"][1]["institutionOrderNums"],
        json!(["2", "1"])
    );
    assert_eq!(form["metadata"]["author"][1]["commonCorrespondent"], true);
    assert_eq!(
        form["fullTexts"],
        p["result"]["snapshot"]["form"]["fullTexts"]
    );
    assert_eq!(
        form["metadata"]["abstract"],
        json!(["Complete original abstract"])
    );
    metadata::assert_result(&cmd, &result(&cmd)).unwrap();
}
#[test]
fn order_requires_a_changed_complete_original_permutation() {
    let (_, _, p) = setup("first_author", "是");
    let form = &p["result"]["snapshot"]["form"];
    for order in [
        json!(null),
        json!([0, 1]),
        json!([1]),
        json!([1, 1]),
        json!([1, 2]),
        json!([1, -1]),
        json!([1, "0"]),
        json!([1, 0, 2]),
    ] {
        assert!(
            metadata_order::reordered_form(form, "author_order", &order).is_err(),
            "{order}"
        );
    }
    let mut inconsistent = form.clone();
    inconsistent["metadata"]["author"][0]["order"] = json!(2);
    assert_eq!(
        metadata_order::reordered_form(&inconsistent, "author_order", &json!([1, 0]))
            .unwrap_err()
            .code,
        "PAGE_UNSUPPORTED"
    );
}
#[test]
fn ambiguous_or_invalid_original_unit_references_refuse_all_reordering() {
    let (_, _, p) = setup("first_institution", "是");
    for refs in [
        json!(""),
        json!("01"),
        json!("1,1"),
        json!("1,3"),
        json!(" 1"),
        json!([]),
        json!([1]),
        json!(["1", ""]),
    ] {
        let mut form = p["result"]["snapshot"]["form"].clone();
        form["metadata"]["author"][0]["institutionOrderNums"] = refs;
        assert_eq!(
            metadata_order::reordered_form(&form, "institution_order", &json!([1, 0]))
                .unwrap_err()
                .code,
            "PAGE_UNSUPPORTED"
        );
    }
}
#[test]
fn wrong_source_value_hidden_roles_and_uneditable_previews_never_create_write_evidence() {
    for change in [
        "wrong_value",
        "no_control",
        "wrong_author",
        "missing_source",
    ] {
        let (mut t, sa, mut p) = setup(
            "first_author",
            if change == "wrong_value" {
                "否"
            } else {
                "是"
            },
        );
        match change {
            "no_control" => p["result"]["can_reorder_authors"] = json!(false),
            "wrong_author" => p["result"]["authors"][1]["id"] = json!("other"),
            _ => (),
        };
        let r = if change == "missing_source" {
            metadata::order_payload(
                &mut t,
                &sa,
                &p,
                "first_author",
                1,
                "author_order",
                &json!([1, 0]),
                "",
                "proof",
                "note",
            )
        } else {
            build(
                &mut t,
                &sa,
                &p,
                "first_author",
                "author_order",
                &json!([1, 0]),
            )
        };
        assert!(r.is_err(), "{change}");
        assert!(t.evidence.is_empty());
    }
    let (mut t, sa, mut p) = setup("first_author", "否");
    p["result"]["snapshot"]["form"]["metadata"]["author"][1]["ownFirst"] = json!(true);
    // Even a change among other rows cannot clear a source author's hidden flag.
    p["result"]["snapshot"]["form"]["metadata"]["author"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"author2","fullname":"Third","order":3}));
    assert_eq!(
        build(
            &mut t,
            &sa,
            &p,
            "first_author",
            "author_order",
            &json!([2, 1, 0])
        )
        .unwrap_err()
        .code,
        "REVIEW_REQUIRED"
    );
    assert!(t.evidence.is_empty());
}
#[test]
fn unit_preview_must_match_the_complete_original_and_source_business_value() {
    for change in ["wrong_address", "missing_unit", "no_helper", "wrong_value"] {
        let (mut t, sa, mut p) = setup("first_institution", "是");
        match change {
            "wrong_address" => p["result"]["institutions"][1]["address"] = json!("Wrong"),
            "missing_unit" => {
                p["result"]["institutions"].as_array_mut().unwrap().pop();
            }
            "no_helper" => p["result"]["institutions"][1]["first_institution_value"] = Value::Null,
            _ => p["result"]["institutions"][1]["first_institution_value"] = json!("否"),
        };
        assert!(
            build(
                &mut t,
                &sa,
                &p,
                "first_institution",
                "institution_order",
                &json!([1, 0])
            )
            .is_err(),
            "{change}"
        );
        assert!(t.evidence.is_empty());
    }
}
#[test]
fn order_readback_rejects_partial_saves_other_changes_and_forged_plans() {
    let (mut t, sa, p) = setup("first_institution", "是");
    let cmd = build(
        &mut t,
        &sa,
        &p,
        "first_institution",
        "institution_order",
        &json!([1, 0]),
    )
    .unwrap();
    for change in [
        "links",
        "id",
        "role",
        "attachments",
        "abstract",
        "operation",
        "before",
    ] {
        let mut r = result(&cmd);
        match change {
            "links" => {
                r["after"]["metadata"]["author"][1]["institutionOrderNums"] = json!(["1", "2"])
            }
            "id" => r["after"]["metadata"]["authorInstitution"][0]["id"] = json!("new-unit"),
            "role" => r["after"]["metadata"]["author"][1]["commonCorrespondent"] = json!(false),
            "attachments" => r["after"]["fullTexts"] = json!([]),
            "abstract" => r["after"]["metadata"]["abstract"] = json!([]),
            "operation" => r["operation"] = json!("author_order"),
            _ => r["before"]["metadata"]["pages"] = json!([]),
        };
        assert!(metadata::assert_result(&cmd, &r).is_err(), "{change}");
    }
    let mut forged = cmd.clone();
    forged["expected_form"]["metadata"]["pages"] = json!([]);
    assert!(metadata::assert_result(&forged, &result(&forged)).is_err());
    let mut equivalent = result(&cmd);
    equivalent["after"]["metadata"]["author"][1]["institutionOrderNums"] = json!("2,1");
    metadata::assert_result(&cmd, &equivalent).unwrap();
}
