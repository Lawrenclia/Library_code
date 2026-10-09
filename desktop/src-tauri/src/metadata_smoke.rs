//! Full application/WebView2 termination around real editor requests; test only.
use crate::engine::Engine;
use library_core::*;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
const ID: &str = "restart-metadata";
pub fn valid_phase(phase: &str) -> bool {
    matches!(
        phase,
        "metadata-author-start"
            | "metadata-role-start"
            | "metadata-role-resume"
            | "metadata-author-reject"
            | "metadata-author-sa-reject"
            | "metadata-author-resume"
            | "metadata-unit-start"
            | "metadata-unit-resume"
            | "metadata-absent-start"
            | "metadata-absent-resume"
    )
}
fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Failure::new("TEST_FAILED", message))
    }
}
async fn open(app: &AppHandle, engine: &Engine, initial: bool) -> Result<()> {
    engine.browser.open(app, &engine.store.root, "sa").await?;
    if initial {
        engine
            .browser
            .open(app, &engine.store.root, "scholar")
            .await?;
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    Ok(())
}
fn unresolved(engine: &Engine) -> Result<Value> {
    let task = engine.store.task(ID)?;
    let attempts = engine.store.unresolved(ID)?;
    require(
        task.stage == Stage::Unknown
            && !task.running
            && attempts.len() == 1
            && attempts[0]["action"] == "save_metadata",
        "Startup must retain exactly one unknown metadata intent",
    )?;
    let attempt = &attempts[0];
    let saved: Value = serde_json::from_str(attempt["data"].as_str().unwrap())?;
    require(
        saved["payload"]["sa_id"] == ID
            && saved["payload"]["staff_id"] == "001"
            && saved["payload"]["author_id"] == "native-author-2"
            && saved["payload"]["item_id"] == "item-metadata",
        "Original task, staff, item and author identity were lost",
    )?;
    Ok(attempt.clone())
}
pub async fn run(app: &AppHandle, engine: &Engine, phase: &str) -> Result<()> {
    if phase.ends_with("-start") {
        require(
            engine.store.tasks()?.is_empty(),
            "Initial metadata phase needs a fresh workspace",
        )?;
        let unit = phase == "metadata-unit-start";
        let role = phase == "metadata-role-start";
        engine.store.import(
            vec![Record {
                row: 2,
                owner: "restart-fixture".into(),
                sa_id: ID.into(),
                title: "Synthetic paper".into(),
                doi: "10.1234/test".into(),
                wos: "".into(),
                staff_id: "001".into(),
                matches: 1,
                item_ids: "item-metadata".into(),
                mark: "待处理".into(),
                reason: if unit {
                    "交大是否第一单位不一致"
                } else if role {
                    "通讯作者标记不一致"
                } else {
                    "第一作者标记不一致"
                }
                .into(),
                skipped: false,
                done: false,
                source: "本地字段保存重启模拟".into(),
            }],
            "metadata-restart-input".into(),
        )?;
        open(app, engine, true).await?;
        let prepared = engine
            .step(app, ID, "prepare_metadata", false, json!({}))
            .await?;
        require(
            prepared["result"]["authors"][1]["eligible"] == true,
            "Exact source author was not prepared",
        )?;
        engine.step(app, ID, "save_metadata", true,
            json!({"key":if unit {"first_institution"}else if role {"corresponding_author"}else{"first_author"},"operation":if unit {"institution_order"}else if role {"role"}else{"author_order"},
                "order":if role {json!([])}else{json!([1,0])},"author_index":1,"source":"本地合成完整原文","proof":"完整原文核对身份、原角色和全部署名单位顺序。",
                "note":if unit {"原文核对完整单位顺序与关联"}else if role {"原文核对通讯作者标记"}else{"原文核对完整作者顺序"}})).await?;
        return Err(Failure::new(
            "TEST_FAILED",
            "Harness did not terminate the accepted editor request",
        ));
    }
    require(
        app.get_webview_window("sa").is_none() && app.get_webview_window("scholar").is_none(),
        "Old page windows survived the application restart",
    )?;
    let original = unresolved(engine)?;
    let payload: Value =
        serde_json::from_str::<Value>(original["data"].as_str().unwrap())?["payload"].clone();
    for action in [
        "save_metadata",
        "prepare_metadata",
        "complete",
        "submit_claim",
        "add_alias",
    ] {
        let before = engine.store.task(ID)?;
        let error = engine.step(app, ID, action, true, json!({})).await.err();
        require(
            error.as_ref().is_some_and(|e| {
                if action == "prepare_metadata" {
                    e.code == "INVALID_TRANSITION"
                } else {
                    e.code == "REMOTE_RESULT_UNKNOWN"
                }
            }),
            "An unknown edit did not stop before page preparation",
        )?;
        require(
            engine.store.task(ID)?.revision == before.revision,
            "Refused edit touched the task or read work pages",
        )?;
        require(
            unresolved(engine)?["id"] == original["id"],
            "Original intent was replaced by the refused operation",
        )?;
    }
    open(app, engine, false).await?;
    if matches!(
        phase,
        "metadata-author-reject" | "metadata-author-sa-reject" | "metadata-absent-resume"
    ) {
        let error = engine
            .step(app, ID, "verify_metadata", false, json!({}))
            .await
            .err();
        require(
            error.is_some(),
            "Missing or unrelated metadata changes were accepted as the original write",
        )?;
        if phase == "metadata-author-sa-reject" {
            require(
                error.as_ref().is_some_and(|e| e.code == "TASK_CHANGED"),
                "Changed SA value was not rejected by source-value verification",
            )?;
        }
        let task = engine.store.task(ID)?;
        require(
            unresolved(engine)?["id"] == original["id"]
                && task.issue_reviews.is_empty()
                && !task.evidence.iter().any(|e| e.kind == "metadata_verified")
                && !task.record.done,
            "Rejected recovery changed the task, evidence or original unknown intent",
        )?;
    } else {
        let readback = engine
            .step(app, ID, "verify_metadata", false, json!({}))
            .await?;
        require(
            readback["verified"] == true && readback["operation"] == payload["operation"],
            "Complete original order was not verified",
        )?;
        let task = engine.store.task(ID)?;
        require(
            task.stage == serde_json::from_value::<Stage>(payload["previous_stage"].clone())?
                && task.issue_reviews.len() == 1
                && !task.running
                && task.last_error.is_none()
                && engine.store.unresolved(ID)?.is_empty(),
            "Verified recovery did not restore original stage atomically",
        )?;
        require(
            engine
                .step(app, ID, "verify_metadata", false, json!({}))
                .await
                .is_err(),
            "Consumed intent was verified again",
        )?;
        require(engine.step(app, ID, "save_metadata", true, json!({"key":payload["key"],"operation":payload["operation"],"order":payload["order"],
            "author_index":payload["author_index"],"source":payload["source"],"proof":payload["proof"],"note":payload["note"]})).await.is_err(), "Old prepared form caused a duplicate save")?;
        engine.review(
            ID,
            Review {
                route: Route::Existing,
                evidence_id: "".into(),
                library_checked: true,
                platform_id: "item-metadata".into(),
                affiliation_confirmed: true,
                identity_confirmed: true,
                issues_resolved: true,
                note: "原始保存结果已完整回读".into(),
            },
            "本地合成完整原文".into(),
            "工号、原身份、完整字段、署名单位关联和 SA 值一致。".into(),
        )?;
        engine.step(app, ID, "complete", true, json!({})).await?;
        require(
            engine.store.task(ID)?.stage == Stage::Completed,
            "Recovered source-backed issue did not reach SA completion",
        )?;
    }
    let (tasks, queues) = engine.store.report_snapshot()?;
    files::export_report_with_queues(
        &tasks,
        &queues,
        &engine.store.root.join(format!("{phase}-report.xlsx")),
    )?;
    Ok(())
}
