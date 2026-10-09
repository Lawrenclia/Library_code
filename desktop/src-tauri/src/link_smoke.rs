//! Full process crash at an accepted SA association, using local-only fixtures.
use crate::engine::Engine;
use library_core::*;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
const ID: &str = "restart-link";
const ITEM: &str = "1244586319225556123";
pub fn valid_phase(p: &str) -> bool {
    matches!(
        p,
        "link-start"
            | "link-sa-reject"
            | "link-source-reject"
            | "link-metadata-reject"
            | "link-missing-reject"
            | "link-staff-reject"
            | "link-resume"
            | "link-absent-start"
            | "link-absent-resume"
            | "link-pushed-start"
            | "link-pushed-resume"
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
    for role in ["sa", "library"] {
        engine.browser.open(app, &engine.store.root, role).await?;
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    Ok(())
}
fn review(engine: &Engine, route: Route, proof: &str) -> Result<Task> {
    engine.review(
        ID,
        Review {
            route,
            evidence_id: "".into(),
            library_checked: true,
            platform_id: ITEM.into(),
            affiliation_confirmed: true,
            identity_confirmed: true,
            issues_resolved: true,
            note: "准确核对本库条目与交大归属".into(),
        },
        "本地合成原始文献和机构库完整查询".into(),
        proof.into(),
    )
}
pub async fn run(app: &AppHandle, engine: &Engine, phase: &str) -> Result<()> {
    if phase.ends_with("-start") {
        require(
            engine.store.tasks()?.is_empty(),
            "Link start requires a fresh workspace",
        )?;
        engine.store.import(
            vec![Record {
                row: 2,
                owner: "restart-fixture".into(),
                sa_id: ID.into(),
                title: "Synthetic paper".into(),
                doi: "10.1234/test".into(),
                wos: "WOS:000123456789012".into(),
                staff_id: "001".into(),
                matches: 0,
                item_ids: "".into(),
                mark: "待处理".into(),
                reason: "通讯作者标记不一致".into(),
                skipped: false,
                done: false,
                source: "本地完整关联恢复模拟".into(),
            }],
            "original-link-input".into(),
        )?;
        if phase == "link-pushed-start" {
            engine.browser.open(app, &engine.store.root, "wos").await?;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            engine.queue(app, "restart-fixture", false).await?;
            let mut task = engine.store.task(ID)?;
            require(
                task.stage == Stage::Downloaded,
                "Pushed checkpoint requires a real native synthetic TXT",
            )?;
            task.route = Route::Missing;
            task.stage = Stage::Pushed;
            task.batch = Some(json!({"id":"synthetic-pushed-checkpoint","status":2}));
            task.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "fixture_push_checkpoint".into(),
                source: "本地模拟预置推送后检查点，未执行上传/导入/推送".into(),
                text: "只验证推送后的真实前端查询、关联与完整进程恢复".into(),
                created: now(),
            });
            engine.store.save(&mut task, "fixture_push_checkpoint")?;
        }
        open(app, engine).await?;
        engine
            .step(
                app,
                ID,
                "library_search",
                false,
                json!({"title":"Synthetic paper"}),
            )
            .await?;
        review(
            engine,
            if phase == "link-pushed-start" {
                Route::Missing
            } else {
                Route::CorrectedExisting
            },
            "完整题名、DOI、WOS 与原文作者单位确认同一篇交大成果，明确选择实际查询返回的唯一号。",
        )?;
        engine.step(app, ID, "link", true, json!({})).await?;
        return Err(Failure::new(
            "TEST_FAILED",
            "Harness did not stop the accepted link request",
        ));
    }
    require(
        app.get_webview_window("sa").is_none() && app.get_webview_window("library").is_none(),
        "Old SA/library WebView survived process restart",
    )?;
    let original = engine.store.unresolved(ID)?;
    require(
        original.len() == 1
            && original[0]["action"] == "link"
            && engine.store.task(ID)?.stage == Stage::Unknown,
        "Original link intent did not restore as Unknown",
    )?;
    let p =
        serde_json::from_str::<Value>(original[0]["data"].as_str().unwrap())?["payload"].clone();
    require(
        p["schema"] == "sa_link_v2"
            && p["selected_item"]["id"] == ITEM
            && p["original_sa"]["row"]["matchCount"] == 0,
        "Original selection or zero-match SA context was lost",
    )?;
    for action in ["link", "complete", "submit_claim", "add_alias"] {
        let before = engine.store.task(ID)?;
        let error = engine.step(app, ID, action, true, json!({})).await.err();
        require(
            error
                .as_ref()
                .is_some_and(|e| e.code == "REMOTE_RESULT_UNKNOWN"),
            "Unknown association allowed a write before page access",
        )?;
        require(
            engine.store.task(ID)?.revision == before.revision,
            "Rejected write changed original task",
        )?;
    }
    open(app, engine).await?;
    if matches!(phase, "link-resume" | "link-pushed-resume") {
        engine.step(app, ID, "verify_sa", false, json!({})).await?;
        let task = engine.store.task(ID)?;
        require(
            serde_json::to_value(&task.stage)? == p["previous_stage"]
                && !task.running
                && !task.record.done
                && task.record.matches == 0
                && engine.store.unresolved(ID)?.is_empty(),
            "Link readback did not restore original checkpoint without completing SA",
        )?;
        require(
            task.issue_plan.is_some() && task.issue_reviews.is_empty(),
            "Restored link skipped independent live issue review",
        )?;
        let evidence: Vec<_> = task
            .evidence
            .iter()
            .filter(|e| e.kind == "sa_link_verified")
            .collect();
        require(
            evidence.len() == 1
                && serde_json::from_str::<Value>(&evidence[0].text)?["payload"] == p,
            "Verified association lost original immutable payload",
        )?;
        require(
            engine
                .step(app, ID, "verify_sa", false, json!({}))
                .await
                .is_err(),
            "Consumed link intent verified twice",
        )?;
        require(
            engine
                .step(app, ID, "complete", true, json!({}))
                .await
                .is_err(),
            "Link recovery completed SA before resolving current issues",
        )?;
        let repeated = engine.step(app, ID, "link", true, json!({})).await?;
        require(
            repeated["already_linked"] == true,
            "Same target association was sent again",
        )?;
        let plan = engine
            .step(app, ID, "prepare_issues", false, json!({}))
            .await?;
        engine.step(app,ID,"review_issue",false,json!({"key":"corresponding_author","outcome":"library_correct","source":"本地合成原文与准确工号署名","proof":"完整工号 001 对应 Tester，原文确认不是通讯作者，本库元数据正确。","note":"本库通讯作者标记正确，SA 本项来源有误","expected":plan["live"]})).await?;
        review(
            engine,
            task.route,
            "文献身份、交大归属、完整工号、唯一条目与通讯作者原因分别核实，关联不代替最终核对。",
        )?;
        engine.step(app, ID, "complete", true, json!({})).await?;
        require(
            engine.store.task(ID)?.stage == Stage::Completed,
            "Recovered association did not finish independent SA review",
        )?;
    } else {
        let error = engine
            .step(app, ID, "verify_sa", false, json!({}))
            .await
            .err();
        let expected = match phase {
            "link-sa-reject" => "其他字段变化",
            "link-source-reject" => "原始比对值变化",
            "link-metadata-reject" | "link-missing-reject" => "原选择条目缺失或完整元数据变化",
            "link-staff-reject" => "完整工号",
            _ => "尚未回读到上次操作",
        };
        require(
            error.as_ref().is_some_and(|e| e.message.contains(expected)),
            "Link recovery did not refuse for the actual cause",
        )?;
        let task = engine.store.task(ID)?;
        let pending = engine.store.unresolved(ID)?;
        require(
            task.stage == Stage::Unknown
                && !task.record.done
                && task.record.matches == 0
                && task.issue_plan.is_none()
                && pending.len() == 1
                && pending[0]["id"] == original[0]["id"]
                && !task.evidence.iter().any(|e| e.kind == "sa_link_verified"),
            "Rejected recovery changed original intent or generated an accepted conclusion",
        )?;
    }
    files::export_report(
        &engine.store.tasks()?,
        &engine.store.root.join(format!("{phase}-report.xlsx")),
    )?;
    Ok(())
}
