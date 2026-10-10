//! Process/WebView2 termination around a source-backed duplicate merge.
//! This harness is absent from production builds and uses only local fixtures.
use crate::engine::Engine;
use library_core::*;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
const ID: &str = "restart-merge";
pub fn valid_phase(phase: &str) -> bool {
    matches!(
        phase,
        "merge-start"
            | "merge-master-reject"
            | "merge-pool-reject"
            | "merge-sa-reject"
            | "merge-comparison-reject"
            | "merge-ids-reject"
            | "merge-threshold-reject"
            | "merge-resume"
            | "merge-next-start"
            | "merge-next-resume"
            | "merge-absent-start"
            | "merge-absent-resume"
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
    for role in ["sa", "duplicate"] {
        engine.browser.open(app, &engine.store.root, role).await?;
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    Ok(())
}
pub async fn run(app: &AppHandle, engine: &Engine, phase: &str) -> Result<()> {
    if matches!(phase, "merge-start" | "merge-absent-start") {
        require(
            engine.store.tasks()?.is_empty(),
            "Merge crash test needs a fresh workspace",
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
                matches: 3,
                item_ids: ",item-primary,item-source,item-third".into(),
                mark: "待处理".into(),
                reason: "重复数据".into(),
                skipped: false,
                done: false,
                source: "本地合并完整进程恢复测试".into(),
            }],
            "merge-original-input".into(),
        )?;
        let mut t = engine.store.task(ID)?;
        t.evidence.push(Evidence { id: "original-paper".into(), kind: "human_review".into(), source: "本地合成原文".into(),
            text: "三个合成候选的 DOI、作者及年份对应同一篇原文，保留主条目完整字段和原候选摘要；不是仅按相似度选择。".into(), created: now() });
        engine.store.save(&mut t, "original_merge_source")?;
    }
    if phase.ends_with("-start") {
        if phase == "merge-next-start" {
            let t = engine.store.task(ID)?;
            require(
                t.merges.len() == 1
                    && t.stage == Stage::Pending
                    && engine.store.unresolved(ID)?.is_empty(),
                "Next merge did not continue from the first verified merge",
            )?;
        }
        open(app, engine).await?;
        let scan = engine
            .step(app, ID, "scan_duplicates", false, json!({}))
            .await?;
        require(
            scan["matched_ids"]
                .as_array()
                .is_some_and(|a| a.len() == if phase == "merge-next-start" { 2 } else { 3 }),
            "Fresh SA did not retain every remaining match",
        )?;
        engine
            .step(
                app,
                ID,
                "prepare_duplicate",
                false,
                json!({"group_id":"group-1"}),
            )
            .await?;
        engine.step(app, ID, "merge_duplicate", true, json!({"source_id":if phase == "merge-next-start" {"item-third"} else {"item-source"},
            "target_id":"item-primary","evidence_id":"original-paper","identity_confirmed":true,
            "retained":"保留 Synthetic paper 主条目完整字段、原候选摘要，并回读全部剩余匹配。"})).await?;
        return Err(Failure::new(
            "TEST_FAILED",
            "Harness did not terminate the accepted merge request",
        ));
    }
    require(
        app.get_webview_window("sa").is_none() && app.get_webview_window("duplicate").is_none(),
        "Old merge browser windows survived process restart",
    )?;
    let original = engine.store.unresolved(ID)?;
    let t = engine.store.task(ID)?;
    require(
        t.stage == Stage::Unknown
            && !t.running
            && original.len() == 1
            && original[0]["action"] == "merge_duplicate",
        "Original merge intent did not recover as unknown at startup",
    )?;
    let payload: Value =
        serde_json::from_str::<Value>(original[0]["data"].as_str().unwrap())?["payload"].clone();
    require(
        payload["schema"] == "merge_context_v1"
            && payload["input_hash"] == "merge-original-input"
            && payload["original_evidence"]["id"] == "original-paper"
            && payload["expected_group"]["items"]
                .as_array()
                .is_some_and(|r| r.len() >= 2),
        "Original source, group or input was not preserved",
    )?;
    for action in [
        "merge_duplicate",
        "complete",
        "import_upload",
        "submit_claim",
        "prepare_duplicate",
    ] {
        let before = engine.store.task(ID)?;
        let error = engine.step(app, ID, action, true, json!({})).await.err();
        require(
            error.as_ref().is_some_and(|e| {
                e.code
                    == if action == "prepare_duplicate" {
                        "INVALID_TRANSITION"
                    } else {
                        "REMOTE_RESULT_UNKNOWN"
                    }
            }),
            "Unknown merge did not block a new write before page access",
        )?;
        require(
            engine.store.task(ID)?.revision == before.revision,
            "Rejected write mutated the original task",
        )?;
    }
    open(app, engine).await?;
    if matches!(phase, "merge-resume" | "merge-next-resume") {
        let result = engine
            .step(app, ID, "verify_duplicate", false, json!({}))
            .await?;
        let task = engine.store.task(ID)?;
        let count = if phase == "merge-next-resume" { 2 } else { 1 };
        require(
            result["verified"] == true
                && task.stage
                    == serde_json::from_value::<Stage>(payload["previous_stage"].clone())?
                && task.merges.len() == count
                && task.platform_id == "item-primary"
                && !task.running
                && task.last_error.is_none()
                && engine.store.unresolved(ID)?.is_empty(),
            "Verified merge did not restore the original checkpoint atomically",
        )?;
        require(
            !task.record.done && task.record.matches == 3 && task.issue_reviews.is_empty(),
            "Merge recovery prematurely completed SA or overwrote roster matches",
        )?;
        let proofs: Vec<_> = task
            .evidence
            .iter()
            .filter(|e| e.kind == "duplicate_merge_verified")
            .collect();
        require(
            proofs.len() == count,
            "Merge evidence was duplicated or omitted",
        )?;
        let proof: Value = serde_json::from_str(&proofs.last().unwrap().text)?;
        require(
            proof["payload"] == payload
                && proof["result"] == result
                && matched_ids(&proof["sa_after"]["row"])?.len() == 3 - count,
            "Complete original intent/readback or remaining third match was lost",
        )?;
        require(
            engine
                .step(app, ID, "verify_duplicate", false, json!({}))
                .await
                .is_err(),
            "Consumed merge recovered twice",
        )?;
        require(
            engine
                .step(app, ID, "merge_duplicate", true, json!({}))
                .await
                .is_err(),
            "Stale prepared merge dispatched again",
        )?;
        let mut live = engine.store.task(ID)?;
        let sa = engine.read_sa(app, &mut live).await?;
        require(
            matched_ids(&sa["row"])?.len() == 3 - count,
            "Fresh SA did not preserve all remaining matches",
        )?;
        if count == 1 {
            let scan = engine
                .step(app, ID, "scan_duplicates", false, json!({}))
                .await?;
            require(
                scan["matched_ids"] == json!(["item-primary", "item-third"]),
                "Third match was not available for independent review",
            )?;
        } else {
            require(
                engine
                    .step(app, ID, "scan_duplicates", false, json!({}))
                    .await
                    .is_err(),
                "Unique match offered another duplicate merge",
            )?;
        }
    } else {
        let error = engine
            .step(app, ID, "verify_duplicate", false, json!({}))
            .await
            .err();
        let expected = if matches!(
            phase,
            "merge-sa-reject" | "merge-comparison-reject" | "merge-threshold-reject"
        ) {
            "TASK_CHANGED"
        } else {
            "REMOTE_RESULT_UNKNOWN"
        };
        require(
            error.as_ref().is_some_and(|e| e.code == expected),
            &format!(
                "Invalid merge recovery was not rejected for its actual cause: {:?}",
                error
            ),
        )?;
        let task = engine.store.task(ID)?;
        let remaining = engine.store.unresolved(ID)?;
        require(
            task.stage == Stage::Unknown
                && !task.record.done
                && task.merges.is_empty()
                && remaining.len() == 1
                && remaining[0]["id"] == original[0]["id"]
                && !task
                    .evidence
                    .iter()
                    .any(|e| e.kind == "duplicate_merge_verified"),
            "Rejected recovery changed the original intent or created a verified conclusion",
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
