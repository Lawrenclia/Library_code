//! Real process restart at an accepted claim request; test-only local fixtures.
use crate::engine::Engine;
use library_core::*;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
const ID: &str = "restart-claim";
pub fn valid_phase(phase: &str) -> bool {
    matches!(
        phase,
        "claim-start"
            | "claim-id-reject"
            | "claim-metadata-reject"
            | "claim-person-reject"
            | "claim-sa-reject"
            | "claim-resume"
            | "claim-absent-start"
            | "claim-absent-resume"
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
    engine.browser.open(app, &engine.store.root, "sa").await?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    Ok(())
}
pub async fn run(app: &AppHandle, engine: &Engine, phase: &str) -> Result<()> {
    if phase.ends_with("-start") {
        require(
            engine.store.tasks()?.is_empty(),
            "Claim test requires a fresh workspace",
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
                item_ids: "item-claim".into(),
                mark: "待处理".into(),
                reason: "作者不一致".into(),
                skipped: false,
                done: false,
                source: "本地完整认领重启模拟".into(),
            }],
            "original-claim-input".into(),
        )?;
        open(app, engine).await?;
        let p = engine
            .step(app, ID, "prepare_claim", false, json!({}))
            .await?;
        require(
            p["suggested_index"] == Value::Null
                && p["prepared"]["authors"][0]["id"] == "claim-author-1",
            "Author was auto-selected or original ID was lost",
        )?;
        engine
            .step(app, ID, "submit_claim", true, json!({"author_index":0}))
            .await?;
        return Err(Failure::new(
            "TEST_FAILED",
            "Harness did not terminate the accepted claim request",
        ));
    }
    require(
        app.get_webview_window("sa").is_none(),
        "Old browser window survived process restart",
    )?;
    let original = engine.store.unresolved(ID)?;
    require(
        original.len() == 1
            && original[0]["action"] == "submit_claim"
            && engine.store.task(ID)?.stage == Stage::Unknown,
        "Original claim intent did not recover as Unknown",
    )?;
    let p: Value =
        serde_json::from_str::<Value>(original[0]["data"].as_str().unwrap())?["payload"].clone();
    require(
        p["schema"] == "original_author_claim_v1"
            && p["prepared"]["authors"][0]["id"] == "claim-author-1"
            && p["prepared"]["person"]["wno"] == "001",
        "Original author or complete staff identity was lost",
    )?;
    for action in ["submit_claim", "complete", "add_alias", "prepare_claim"] {
        let before = engine.store.task(ID)?;
        let error = engine.step(app, ID, action, true, json!({})).await.err();
        require(
            error.as_ref().is_some_and(|e| {
                e.code
                    == if action == "prepare_claim" {
                        "INVALID_TRANSITION"
                    } else {
                        "REMOTE_RESULT_UNKNOWN"
                    }
            }),
            "Unknown claim did not refuse new operations before page access",
        )?;
        require(
            engine.store.task(ID)?.revision == before.revision,
            "Rejected write changed original task",
        )?;
    }
    open(app, engine).await?;
    if phase == "claim-resume" {
        engine.step(app, ID, "verify_sa", false, json!({})).await?;
        let task = engine.store.task(ID)?;
        require(
            task.stage == Stage::Claimed
                && !task.record.done
                && !task.running
                && task.last_error.is_none()
                && engine.store.unresolved(ID)?.is_empty(),
            "Claim recovery did not atomically restore the claimed checkpoint",
        )?;
        let proofs: Vec<_> = task
            .evidence
            .iter()
            .filter(|e| e.kind == "claim_verified")
            .collect();
        require(
            proofs.len() == 1 && task.issue_reviews.is_empty(),
            "Claim recovery prematurely resolved issues or lost original evidence",
        )?;
        let proof: Value = serde_json::from_str(&proofs[0].text)?;
        require(
            proof["payload"] == p && proof["result"]["author_id"] == "claim-author-1",
            "Recovered claim did not preserve the original author/complete payload",
        )?;
        require(
            engine
                .step(app, ID, "verify_sa", false, json!({}))
                .await
                .is_err(),
            "Consumed claim intent verified twice",
        )?;
        require(
            engine
                .step(app, ID, "submit_claim", true, json!({"author_index":0}))
                .await
                .is_err(),
            "Old prepared author caused a duplicate claim",
        )?;
        let plan = engine
            .step(app, ID, "prepare_issues", false, json!({}))
            .await?;
        engine.step(app,ID,"review_issue",false,json!({"key":"author_claim","outcome":"claimed","source":"本地合成原文及完整工号认领回读","proof":"原作者 ID、署名、学者与完整工号 001、完整元数据均已核对并回读。","note":"准确作者与工号已认领并回读","expected":plan["live"]})).await?;
        require(
            issues::assert_resolved(&engine.store.task(ID)?, &plan["live"]).is_err(),
            "Claim alone incorrectly resolved the separate corresponding-author discrepancy",
        )?;
        engine.step(app,ID,"review_issue",false,json!({"key":"corresponding_author","outcome":"library_correct","source":"本地合成原文与原作者完整元数据","proof":"原作者 claim-author-1 与完整工号 001 对应，原文及原始元数据确认该作者不是通讯作者；另一个原作者的通讯标记仍保留。","note":"本库通讯作者标记正确，SA 本项来源有误；认领未修改作者角色。","expected":plan["live"]})).await?;
        engine.review(
            ID,
            Review {
                route: Route::Existing,
                evidence_id: "".into(),
                library_checked: true,
                platform_id: "item-claim".into(),
                affiliation_confirmed: true,
                identity_confirmed: true,
                issues_resolved: true,
                note: "原作者与完整工号已核对".into(),
            },
            "本地合成原始论文和实时完整认领信息".into(),
            "文献、原作者 ID、学者身份和交大归属已逐项核对。".into(),
        )?;
        engine.step(app, ID, "complete", true, json!({})).await?;
        require(
            engine.store.task(ID)?.stage == Stage::Completed,
            "Recovered claim did not continue through independent review to SA completion",
        )?;
    } else {
        let error = engine
            .step(app, ID, "verify_sa", false, json!({}))
            .await
            .err();
        let expected = match phase {
            "claim-id-reject" => "作者与学者关系",
            "claim-metadata-reject" => "其他元数据变化",
            "claim-person-reject" => "学者身份发生变化",
            "claim-sa-reject" => "SA 的其他字段变化",
            _ => "尚未回读到",
        };
        require(
            error.as_ref().is_some_and(|e| e.message.contains(expected)),
            "Changed/unsaved claim was not rejected for the actual cause",
        )?;
        let task = engine.store.task(ID)?;
        let pending = engine.store.unresolved(ID)?;
        require(
            pending.len() == 1
                && pending[0]["id"] == original[0]["id"]
                && task.stage == Stage::Unknown
                && !task.record.done
                && task.issue_reviews.is_empty()
                && !task.evidence.iter().any(|e| e.kind == "claim_verified"),
            "Rejected claim readback changed original intent or generated a verified conclusion",
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
