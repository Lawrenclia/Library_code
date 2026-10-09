//! Actual process termination around an alias save; compiled only for smoke tests.
use crate::engine::Engine;
use library_core::*;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
const ID: &str = "restart-alias";
pub fn valid_phase(phase: &str) -> bool {
    matches!(
        phase,
        "alias-start"
            | "alias-list-reject"
            | "alias-scholar-reject"
            | "alias-sa-reject"
            | "alias-resume"
            | "alias-absent-start"
            | "alias-absent-resume"
    )
}
fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Failure::new("TEST_FAILED", message))
    }
}
async fn open(app: &AppHandle, engine: &Engine) -> Result<()> {
    for role in ["sa", "scholar"] {
        engine.browser.open(app, &engine.store.root, role).await?;
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    Ok(())
}
pub async fn run(app: &AppHandle, engine: &Engine, phase: &str) -> Result<()> {
    if phase.ends_with("-start") {
        require(
            engine.store.tasks()?.is_empty(),
            "Alias test requires a fresh workspace",
        )?;
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
                item_ids: "item-alias".into(),
                mark: "待处理".into(),
                reason: "作者不一致".into(),
                skipped: false,
                done: false,
                source: "本地别名重启模拟".into(),
            }],
            "alias-original-input".into(),
        )?;
        let mut task = engine.store.task(ID)?;
        task.evidence.push(Evidence{id:"original-paper".into(),kind:"human_review".into(),source:"本地合成原始论文".into(),text:"Synthetic paper 的原始署名 Demo, X，完整工号 001 与学者身份已人工核对；不是按相似姓名推断。".into(),created:now()});
        engine.store.save(&mut task, "synthetic_original_source")?;
        open(app, engine).await?;
        let prepared = engine
            .step(app, ID, "prepare_alias", false, json!({}))
            .await?;
        require(
            prepared["staff_id"] == "001"
                && prepared["aliases"].as_array().is_some_and(|a| a.len() == 1),
            "Original complete alias list was not prepared",
        )?;
        engine
            .step(
                app,
                ID,
                "add_alias",
                true,
                json!({"alias":"Demo, X","evidence_id":"original-paper"}),
            )
            .await?;
        return Err(Failure::new(
            "TEST_FAILED",
            "Harness did not terminate the accepted alias request",
        ));
    }
    require(
        app.get_webview_window("sa").is_none() && app.get_webview_window("scholar").is_none(),
        "Old browser windows survived process restart",
    )?;
    let original = engine.store.unresolved(ID)?;
    require(
        original.len() == 1
            && original[0]["action"] == "add_alias"
            && engine.store.task(ID)?.stage == Stage::Unknown,
        "Exact original alias intent did not survive restart",
    )?;
    let payload: Value =
        serde_json::from_str::<Value>(original[0]["data"].as_str().unwrap())?["payload"].clone();
    require(
        payload["schema"] == "source_backed_alias_v1"
            && payload["source_evidence"]["id"] == "original-paper"
            && payload["staff_id"] == "001",
        "Original identity or paper source was lost",
    )?;
    for action in ["add_alias", "complete", "submit_claim", "prepare_alias"] {
        let before = engine.store.task(ID)?;
        let error = engine.step(app, ID, action, true, json!({})).await.err();
        require(
            error.as_ref().is_some_and(|e| {
                e.code
                    == if action == "prepare_alias" {
                        "INVALID_TRANSITION"
                    } else {
                        "REMOTE_RESULT_UNKNOWN"
                    }
            }),
            "Unknown alias did not block writes before page access",
        )?;
        require(
            engine.store.task(ID)?.revision == before.revision,
            "Rejected operation mutated original task",
        )?;
    }
    open(app, engine).await?;
    if phase == "alias-resume" {
        let result = engine
            .step(app, ID, "verify_alias", false, json!({}))
            .await?;
        let task = engine.store.task(ID)?;
        require(
            result["verified"] == true
                && task.stage
                    == serde_json::from_value::<Stage>(payload["previous_stage"].clone())?
                && !task.running
                && task.last_error.is_none()
                && engine.store.unresolved(ID)?.is_empty(),
            "Complete original alias readback did not restore stage atomically",
        )?;
        let proofs: Vec<_> = task
            .evidence
            .iter()
            .filter(|e| e.kind == "alias_verified")
            .collect();
        require(
            proofs.len() == 1 && !task.record.done && task.issue_reviews.is_empty(),
            "Alias save prematurely resolved issues or completed SA",
        )?;
        let proof: Value = serde_json::from_str(&proofs[0].text)?;
        require(
            proof["payload"] == payload && proof["result"] == result,
            "Complete original payload/source not preserved in verification",
        )?;
        require(
            engine
                .step(app, ID, "verify_alias", false, json!({}))
                .await
                .is_err(),
            "Consumed alias intent recovered twice",
        )?;
        let fresh = engine
            .step(app, ID, "prepare_alias", false, json!({}))
            .await?;
        require(
            fresh["aliases"]
                .as_array()
                .is_some_and(|rows| rows.len() == 2),
            "Subsequent fresh alias query lost complete list",
        )?;
        let repeat = engine
            .step(
                app,
                ID,
                "add_alias",
                true,
                json!({"alias":"Demo, X","evidence_id":"original-paper"}),
            )
            .await?;
        require(
            repeat["already_present"] == true,
            "Existing alias caused another save",
        )?;
    } else {
        let error = engine
            .step(app, ID, "verify_alias", false, json!({}))
            .await
            .err();
        let expected = match phase {
            "alias-scholar-reject" => "TASK_CHANGED",
            "alias-sa-reject" => "TASK_CHANGED",
            _ => "REMOTE_RESULT_UNKNOWN",
        };
        require(
            error.as_ref().is_some_and(|e| e.code == expected),
            "Changed/unsaved alias recovery was not rejected for the actual cause",
        )?;
        let task = engine.store.task(ID)?;
        let remaining = engine.store.unresolved(ID)?;
        require(
            remaining.len() == 1
                && remaining[0]["id"] == original[0]["id"]
                && task.stage == Stage::Unknown
                && !task.record.done
                && !task.evidence.iter().any(|e| e.kind == "alias_verified"),
            "Rejected readback changed original intent or created a verified conclusion",
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
