//! Compiled only with `smoke-test`; production has no loopback override.
use crate::{browser, engine::Engine};
use library_core::*;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
pub fn workspace_root() -> std::result::Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let id = match (
        std::env::var("DESKTOP_SMOKE_RESTART_PHASE").ok(),
        std::env::var("DESKTOP_SMOKE_RESTART_ID").ok(),
    ) {
        (None, None) => uuid::Uuid::new_v4(),
        (Some(phase), Some(id)) if crate::restart_smoke::valid_phase(&phase) => {
            let parsed = uuid::Uuid::parse_str(&id)?;
            if parsed.to_string() != id {
                return Err("Restart test requires a canonical UUID, not a path".into());
            }
            browser::fixture_origin().ok_or("Restart test requires a loopback origin")?;
            parsed
        }
        _ => return Err("Invalid restart smoke-test configuration".into()),
    };
    Ok(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../runtime/tauri-smoke")
        .join(id.to_string()))
}
pub fn setup(app: &AppHandle) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let origin = browser::fixture_origin().ok_or("Smoke test requires a loopback origin")?;
    let capability = json!({"identifier":"offline-fixture","local":false,"windows":["wos","sa","import","duplicate","scholar","library"],"remote":{"urls":[format!("{}/*",origin.origin().ascii_serialization())]},"permissions":["allow-browser-result"]});
    app.add_capability(capability.to_string())?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let phase = std::env::var("DESKTOP_SMOKE_RESTART_PHASE").ok();
        let result = if let Some(phase) = &phase {
            crate::restart_smoke::run(&app, phase).await
        } else {
            run(&app).await
        };
        let engine = app.state::<Engine>();
        let report = json!({"passed":result.is_ok(),"failure":result.err(),"process_id":std::process::id(),"phase":phase,"workspace":engine.snapshot(&app).ok()});
        let _ = std::fs::write(
            engine.store.root.join(
                phase
                    .as_ref()
                    .map(|p| format!("restart-{p}.json"))
                    .unwrap_or("native-smoke.json".into()),
            ),
            serde_json::to_vec_pretty(&report).unwrap(),
        );
        app.exit(if report["passed"] == true { 0 } else { 1 });
    });
    Ok(())
}
async fn run(app: &AppHandle) -> Result<()> {
    let engine = app.state::<Engine>();
    let records = (1..=4)
        .map(|i| Record {
            row: i + 1,
            owner: "offline-test".into(),
            sa_id: format!("smoke-{i}"),
            title: if i < 4 {
                format!("Missing {i}")
            } else {
                "Synthetic paper".into()
            },
            doi: if i == 4 {
                "10.1234/test".into()
            } else {
                "".into()
            },
            wos: "".into(),
            staff_id: "001".into(),
            matches: 0,
            item_ids: "".into(),
            mark: "待处理".into(),
            reason: "测试".into(),
            skipped: false,
            done: false,
            source: "本地合成页面".into(),
        })
        .collect();
    engine.store.import(records, "offline".into())?;
    engine.browser.open(app, &engine.store.root, "wos").await?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let _lease = engine.acquire()?;
    engine.queue(app, "offline-test", false).await?;
    let tasks = engine.store.tasks()?;
    if tasks.len() != 4
        || tasks[..3]
            .iter()
            .any(|t| t.last_error.as_ref().map(|e| e.code.as_str()) != Some("NO_RESULT"))
    {
        return Err(Failure::new("TEST_FAILED", "前三条应分别是明确零结果。"));
    }
    let artifact = tasks[3]
        .artifact
        .as_ref()
        .ok_or_else(|| Failure::new("TEST_FAILED", "连续三条失败后未取得第四条文件。"))?;
    if !artifact.identity_confirmed
        || !std::path::Path::new(&artifact.path).starts_with(&engine.store.root)
    {
        return Err(Failure::new("TEST_FAILED", "下载路径或身份核验不符。"));
    }
    let sources = tasks[3]
        .evidence
        .iter()
        .filter(|e| e.kind == "metadata")
        .collect::<Vec<_>>();
    if sources.len() != 1
        || serde_json::from_str::<serde_json::Value>(&sources[0].text)?
            != json!(artifact.candidate.fields)
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "来源记录没有保留完整原始导出字段。",
        ));
    }
    let mut second = tasks[3].record.clone();
    second.sa_id = "smoke-shared".into();
    second.owner = "second-owner".into();
    second.row = 6;
    engine.store.import(vec![second], "second-input".into())?;
    app.get_webview_window("wos")
        .unwrap()
        .close()
        .map_err(Failure::storage)?;
    engine.queue(app, "second-owner", false).await?;
    let shared = engine.store.task("smoke-shared")?;
    if shared.artifact.as_ref().map(|a| &a.path) != Some(&artifact.path)
        || shared.stage != Stage::Downloaded
        || shared.record.done
        || std::fs::read_dir(engine.store.root.join("downloads"))?.count() != 1
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "不同 SA 没有独立复用核验文件，或发生重复下载。",
        ));
    }
    verify_full_import(app, &engine).await?;
    verify_duplicate(app, &engine).await?;
    verify_existing(app, &engine).await?;
    verify_metadata(app, &engine).await?;
    verify_library(app, &engine).await?;
    verify_zero_to_unique(app, &engine, "smoke-4", false).await?;
    // Start at a seeded verified-push checkpoint; this does not exercise upload/push.
    let original = engine.store.task("smoke-4")?;
    let mut record = original.record.clone();
    record.sa_id = "smoke-pushed".into();
    record.row = 10;
    record.done = false;
    engine
        .store
        .import(vec![record], "pushed-checkpoint".into())?;
    let mut pushed = engine.store.task("smoke-pushed")?;
    pushed.artifact = original.artifact.clone();
    pushed.route = Route::Missing;
    pushed.stage = Stage::Pushed;
    pushed.batch = Some(json!({"id":"fixture-verified-push","status":2}));
    pushed.evidence.push(Evidence {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "fixture_push_checkpoint".into(),
        source: "本地模拟 · 预置推送后检查点，未执行导入/推送".into(),
        text: "仅测试推送后的前端查库、关联与 SA 核验".into(),
        created: now(),
    });
    engine.store.save(&mut pushed, "fixture_push_checkpoint")?;
    engine
        .step(
            app,
            "smoke-pushed",
            "library_search",
            false,
            json!({"title":"Synthetic paper"}),
        )
        .await?;
    engine.review(
        "smoke-pushed",
        Review {
            route: Route::Missing,
            evidence_id: "".into(),
            library_checked: true,
            platform_id: "1244586319225556123".into(),
            affiliation_confirmed: true,
            identity_confirmed: true,
            issues_resolved: true,
            note: "推送后明确核对实际条目与交大身份".into(),
        },
        "本地合成前端与原文".into(),
        "选择同一平台论文，原始名单仍为零匹配".into(),
    )?;
    verify_zero_to_unique(app, &engine, "smoke-pushed", false).await?;
    verify_input_version(app, &engine).await?;
    files::export_report(
        &engine.store.tasks()?,
        &engine.store.root.join("native-report.xlsx"),
    )?;
    Ok(())
}

async fn verify_full_import(app: &AppHandle, engine: &Engine) -> Result<()> {
    let original = engine.store.task("smoke-4")?;
    let mut record = original.record.clone();
    record.sa_id = "smoke-import".into();
    record.row = 12;
    engine
        .store
        .import(vec![record], "full-import-input".into())?;
    let mut task = engine.store.task("smoke-import")?;
    task.artifact = original.artifact;
    task.stage = Stage::Downloaded;
    engine.store.save(&mut task, "fixture_verified_artifact")?;
    for role in ["sa", "import", "library"] {
        engine.browser.open(app, &engine.store.root, role).await?;
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let id = "smoke-import";
    engine
        .step(
            app,
            id,
            "library_search",
            false,
            json!({"title":"Synthetic paper"}),
        )
        .await?;
    engine.review(
        id,
        Review {
            route: Route::Missing,
            evidence_id: "".into(),
            library_checked: true,
            platform_id: "".into(),
            affiliation_confirmed: true,
            identity_confirmed: true,
            issues_resolved: false,
            note: "合成原文确认交大归属；完整前端查询全部零条".into(),
        },
        "本地合成原始 TXT 与前端查询".into(),
        "已按原始 WOS 入藏号、DOI 与地址核验单篇交大成果".into(),
    )?;
    if engine
        .step(app, id, "import_push", true, json!({}))
        .await
        .is_ok()
        || engine
            .step(app, id, "import_submit", true, json!({}))
            .await
            .is_ok()
    {
        return Err(Failure::new("TEST_FAILED", "未上传就可以导入或推送。"));
    }
    let lost = engine.step(app, id, "import_upload", true, json!({})).await;
    if lost.as_ref().err().map(|e| e.code.as_str()) != Some("REMOTE_RESULT_UNKNOWN") {
        return Err(Failure::new(
            "TEST_FAILED",
            "模拟丢失上传确认没有保留未知状态。",
        ));
    }
    let reopened = Store::new(&engine.store.root)?;
    reopened.recover()?;
    if reopened.task(id)?.stage != Stage::Unknown
        || reopened.unresolved(id)?.len() != 1
        || engine
            .step(app, id, "import_upload", true, json!({}))
            .await
            .is_ok()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "未确认上传恢复后仍可重复上传。",
        ));
    }
    let upload = engine
        .step(app, id, "verify_import", false, json!({}))
        .await?;
    if upload["uploaded"] != true
        || engine.store.task(id)?.stage != Stage::Uploaded
        || !engine.store.unresolved(id)?.is_empty()
        || engine
            .step(app, id, "import_upload", true, json!({}))
            .await
            .is_ok()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "上传检查点不正确或允许重复上传。",
        ));
    }
    let repeated = engine
        .step(app, id, "verify_import", false, json!({}))
        .await?;
    if repeated["uploaded"] != true
        || engine.store.task(id)?.stage != Stage::Uploaded
        || !engine.store.unresolved(id)?.is_empty()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "已确认上传的回读没有保持上传阶段。",
        ));
    }
    for (action, expected) in [
        ("import_submit", Stage::Imported),
        ("import_push", Stage::Pushed),
    ] {
        engine.step(app, id, action, true, json!({})).await?;
        let reopened = Store::new(&engine.store.root)?;
        reopened.recover()?;
        if reopened.task(id)?.stage != Stage::Unknown
            || reopened.unresolved(id)?.len() != 1
            || engine.step(app, id, action, true, json!({})).await.is_ok()
        {
            return Err(Failure::new(
                "TEST_FAILED",
                "已发出操作恢复后没有阻止重发。",
            ));
        }
        engine
            .step(app, id, "verify_import", false, json!({}))
            .await?;
        if reopened.task(id)?.stage != expected || !reopened.unresolved(id)?.is_empty() {
            return Err(Failure::new(
                "TEST_FAILED",
                "导入/推送回读没有原子恢复实际阶段。",
            ));
        }
    }
    if engine.store.task(id)?.record.done
        || engine
            .step(app, id, "complete", true, json!({}))
            .await
            .is_ok()
    {
        return Err(Failure::new("TEST_FAILED", "推送完成就提前完成了 SA。"));
    }
    engine
        .step(
            app,
            id,
            "library_search",
            false,
            json!({"title":"Synthetic paper"}),
        )
        .await?;
    engine.review(
        id,
        Review {
            route: Route::Missing,
            evidence_id: "".into(),
            library_checked: true,
            platform_id: "1244586319225556123".into(),
            affiliation_confirmed: true,
            identity_confirmed: true,
            issues_resolved: false,
            note: "推送后实际前端返回同一成果，继续独立 SA 核对".into(),
        },
        "本地合成前端与原始 TXT".into(),
        "明确选中真实返回的文本平台号，核对完整身份".into(),
    )?;
    verify_zero_to_unique(app, engine, id, true).await?;
    let task = engine.store.task(id)?;
    if task.stage != Stage::Completed
        || task.batch.as_ref().map(|b| &b["status"]) != Some(&json!(2))
        || task
            .evidence
            .iter()
            .filter(|e| e.kind == "import_receipt")
            .count()
            != 2
        || task
            .evidence
            .iter()
            .filter(|e| e.kind == "upload_verified")
            .count()
            != 2
        || task
            .evidence
            .iter()
            .filter(|e| e.kind == "import_verified")
            .count()
            != 2
        || !task
            .evidence
            .iter()
            .any(|e| e.kind == "import_receipt" && e.text.contains("push_settings"))
        || !task
            .evidence
            .iter()
            .any(|e| e.kind == "import_verified" && e.text.contains("WOS:000123456789012"))
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "完整上传到 SA 处理闭环未完成或丢失批次。",
        ));
    }
    Ok(())
}

async fn verify_input_version(app: &AppHandle, engine: &Engine) -> Result<()> {
    let mut record = engine.store.task("smoke-4")?.record;
    record.sa_id = "smoke-version".into();
    record.row = 11;
    record.title = "Old title".into();
    record.done = false;
    record.reason = "测试".into();
    engine
        .store
        .import(vec![record.clone()], "old-version-input".into())?;
    let mut task = engine.store.task(&record.sa_id)?;
    task.classification = Some(json!({"source":"previous version AI"}));
    task.evidence.push(Evidence {
        id: "old-version-proof".into(),
        kind: "human_review".into(),
        source: "旧版模拟原文".into(),
        text: "完整旧版依据".into(),
        created: now(),
    });
    engine.store.save(&mut task, "fixture_old_version")?;
    let mut incoming = record.clone();
    incoming.title = "Synthetic paper".into();
    incoming.owner = "new-version-owner".into();
    engine
        .store
        .import(vec![incoming.clone()], "new-version-input".into())?;
    let proposal = engine.store.pending_input(&record.sa_id)?.unwrap();
    let reopened = Store::new(&engine.store.root)?;
    reopened.recover()?;
    if reopened.pending_input(&record.sa_id)?.unwrap().id != proposal.id
        || reopened.task(&record.sa_id)?.record.title != "Old title"
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "新名单候选或旧任务未在重开数据库后保留。",
        ));
    }
    let review = Review {
        route: Route::NotFound,
        evidence_id: "".into(),
        library_checked: false,
        platform_id: "".into(),
        affiliation_confirmed: false,
        identity_confirmed: false,
        issues_resolved: true,
        note: "旧版本不能保存".into(),
    };
    if engine
        .review(&record.sa_id, review, "合成来源".into(), "合成依据".into())
        .is_ok()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "待确认的新名单版本没有阻止旧任务核验。",
        ));
    }
    engine.prepare_input_version(app, &record.sa_id).await?;
    incoming.owner = "third-version-owner".into();
    engine
        .store
        .import(vec![incoming], "third-version-input".into())?;
    if engine
        .accept_input_version(
            app,
            &record.sa_id,
            &proposal.id,
            "新名单安排",
            "新责任人已明确核对",
        )
        .await
        .is_ok()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "旧准备没有拒绝被替换的名单候选。",
        ));
    }
    let prepared = engine.prepare_input_version(app, &record.sa_id).await?;
    let next = engine
        .accept_input_version(
            app,
            &record.sa_id,
            prepared["proposal_id"].as_str().unwrap(),
            "本地合成新名单与任务安排",
            "完整工号、题名、DOI、匹配条目、原因均与实时 SA 一致，责任人由本名单安排",
        )
        .await?;
    if next.record.title != "Synthetic paper"
        || next.record.owner != "third-version-owner"
        || next.review.is_some()
        || next.classification.is_some()
        || next.issue_plan.is_some()
        || engine.store.pending_input(&record.sa_id)?.is_some()
        || !next
            .evidence
            .iter()
            .any(|e| e.kind == "input_version_history" && e.text.contains("完整旧版依据"))
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "新版本未原子接受、旧历史丢失或旧业务结论沿用。",
        ));
    }
    if engine
        .accept_input_version(
            app,
            &record.sa_id,
            prepared["proposal_id"].as_str().unwrap(),
            "来源",
            "依据",
        )
        .await
        .is_ok()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "已消费的版本准备仍可再次接受。",
        ));
    }
    Ok(())
}
async fn verify_zero_to_unique(
    app: &AppHandle,
    engine: &Engine,
    id: &str,
    claim: bool,
) -> Result<()> {
    let linked = if id == "smoke-pushed" {
        // Deliberately omit the local finish after a real fixture-page write.
        // Reopen SQLite and run the startup recovery routine, without restarting WebView2.
        let mut task = engine.store.task(id)?;
        let live = engine.read_sa(app, &mut task).await?;
        let payload = sa::link_payload(&task, &live)?;
        let attempt =
            engine
                .store
                .begin_attempt_with_payload(&mut task, "link", payload.clone())?;
        let receipt = engine
            .browser
            .execute(app, "sa", "link", payload.clone(), 60)
            .await?;
        if receipt["verified"] != true {
            return Err(Failure::new("TEST_FAILED", "未取得模拟关联写入回执。"));
        }
        let reopened = Store::new(&engine.store.root)?;
        reopened.recover()?;
        if reopened.task(id)?.stage != Stage::Unknown
            || engine.step(app, id, "link", true, json!({})).await.is_ok()
            || reopened.unresolved(id)?.len() != 1
        {
            return Err(Failure::new(
                "TEST_FAILED",
                "未确认的关联恢复后没有禁止重发。",
            ));
        }
        let pending = reopened.unresolved(id)?;
        let audit: Value = serde_json::from_str(pending[0]["data"].as_str().unwrap_or("{}"))?;
        if pending[0]["id"] != attempt || audit["payload"] != payload {
            return Err(Failure::new(
                "TEST_FAILED",
                "恢复后的关联执行记录或载荷已变化。",
            ));
        }
        engine.step(app, id, "verify_sa", false, json!({})).await?;
        let saved = reopened.unresolved(id)?;
        let recovered = reopened.task(id)?;
        if !saved.is_empty() || recovered.stage != Stage::Pushed || recovered.running {
            return Err(Failure::new("TEST_FAILED", "只读确认未恢复原先推送阶段。"));
        }
        let raw = recovered
            .evidence
            .iter()
            .rev()
            .find(|e| e.kind == "sa_link_verified")
            .ok_or_else(|| Failure::new("TEST_FAILED", "缺少关联只读恢复来源。"))?;
        let audit: Value = serde_json::from_str(&raw.text)?;
        if audit["payload"] != payload {
            return Err(Failure::new("TEST_FAILED", "关联恢复覆盖了原始执行载荷。"));
        }
        receipt
    } else {
        engine.step(app, id, "link", true, json!({})).await?
    };
    if linked["verified"] != true || engine.store.task(id)?.issue_plan.is_none() {
        return Err(Failure::new(
            "TEST_FAILED",
            "关联后未保存完整实时字段与原因清单。",
        ));
    }
    let again = engine.step(app, id, "link", true, json!({})).await?;
    if again["already_linked"] != true || !engine.store.unresolved(id)?.is_empty() {
        return Err(Failure::new(
            "TEST_FAILED",
            "同一平台号关联没有按只读回读确认。",
        ));
    }
    if engine
        .step(app, id, "complete", true, json!({}))
        .await
        .is_ok()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "原名单为零就绕过了实时待处理原因。",
        ));
    }
    let plan = engine
        .step(app, id, "prepare_issues", false, json!({}))
        .await?;
    for (key, note) in [
        ("corresponding_author", "原文确认不是通讯作者"),
        ("first_author", "原文确认不是第一作者"),
    ] {
        engine
            .step(
                app,
                id,
                "review_issue",
                false,
                json!({"key":key,"outcome":"library_correct","source":"合成原文与实时 SA",
            "proof":"依实际署名核对，本库标记正确，保留原值","note":note,"expected":plan["live"]}),
            )
            .await?;
    }
    if claim {
        let prepared = engine
            .step(app, id, "prepare_claim", false, json!({}))
            .await?;
        if prepared["suggested_index"] != Value::Null {
            return Err(Failure::new("TEST_FAILED", "自动选择了认领作者。"));
        }
        engine
            .step(app, id, "submit_claim", true, json!({"author_index":0}))
            .await?;
        let fresh = engine
            .step(app, id, "prepare_issues", false, json!({}))
            .await?;
        engine.step(app,id,"review_issue",false,json!({"key":"author_claim","outcome":"claimed","source":"合成学者工号与实时认领关系",
            "proof":"作者与完整工号 001 唯一一致，认领后已回读","note":"完整工号已核对并回读认领","expected":fresh["live"]})).await?;
    }
    let task = engine.store.task(id)?;
    let mut review = task.review.clone().unwrap();
    review.issues_resolved = false;
    engine.review(
        id,
        review,
        "合成原文与实时 SA".into(),
        "每个实时原因均有独立依据；界面的旧勾选不作最终判断".into(),
    )?;
    if !engine.store.task(id)?.review.unwrap().issues_resolved {
        return Err(Failure::new(
            "TEST_FAILED",
            "零匹配转唯一后，服务端仍按旧名单判断逐项结论。",
        ));
    }
    engine.step(app, id, "complete", true, json!({})).await?;
    let task = engine.store.task(id)?;
    if task.stage != Stage::Completed
        || task.record.matches != 0
        || task
            .evidence
            .iter()
            .filter(|e| e.kind == "issue_baseline")
            .count()
            != 1
        || engine
            .step(app, id, "complete", true, json!({}))
            .await
            .is_ok()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "名单事实被改写、初始清单重复，或已处理仍可再次写入。",
        ));
    }
    Ok(())
}

async fn verify_library(app: &AppHandle, engine: &Engine) -> Result<()> {
    engine
        .browser
        .open(app, &engine.store.root, "library")
        .await?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let id = "smoke-4";
    let mut review = Review {
        route: Route::Missing,
        evidence_id: "".into(),
        library_checked: true,
        platform_id: "".into(),
        affiliation_confirmed: true,
        identity_confirmed: true,
        issues_resolved: false,
        note: "本地合成前端查无记录".into(),
    };
    if engine
        .review(
            id,
            review.clone(),
            "合成依据".into(),
            "不能仅凭勾选断言缺失".into(),
        )
        .is_ok()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "没有真实本库查询就通过了缺失核验。",
        ));
    }
    let result = engine
        .step(
            app,
            id,
            "library_search",
            false,
            json!({"title":"Synthetic paper"}),
        )
        .await?;
    if result["queries"].as_array().map(|a| a.len()) != Some(3) || result["items"] != json!([]) {
        return Err(Failure::new(
            "TEST_FAILED",
            "没有独立核查题名、DOI 和 WOS 号。",
        ));
    }
    engine.review(
        id,
        review.clone(),
        "本地合成文献".into(),
        "身份与交大署名已核对，前端实际三次查询均为零条".into(),
    )?;
    engine.store.task(id)?.import_ready()?;
    app.get_webview_window("library").unwrap().eval("testConfig.zero=false;testConfig.rows=[{id:'1244586319225556123',modelName:'期刊论文',metadata:{title:['Synthetic paper'],doi:['10.1234/test'],pages:['10-20'],abstract:['完整摘要必须保留']}}];").map_err(Failure::storage)?;
    let result = engine
        .step(
            app,
            id,
            "library_search",
            false,
            json!({"title":"Synthetic paper"}),
        )
        .await?;
    if engine.store.task(id)?.import_ready().is_ok()
        || result["items"][0]["metadata"]["abstract"][0] != "完整摘要必须保留"
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "本库有候选仍能导入，或完整字段未保留。",
        ));
    }
    review.route = Route::CorrectedExisting;
    review.platform_id = "arbitrary-id".into();
    if engine
        .review(
            id,
            review.clone(),
            "合成依据".into(),
            "不存在的条目不能选择".into(),
        )
        .is_ok()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "任意填写的条目 ID 通过了核验。",
        ));
    }
    review.platform_id = "1244586319225556123".into();
    engine.review(
        id,
        review,
        "合成文献与前端".into(),
        "已明确核对本库候选为同一篇交大文献".into(),
    )?;
    let task = engine.store.task(id)?;
    if task.route != Route::CorrectedExisting
        || task
            .evidence
            .iter()
            .filter(|e| e.kind == "library_search")
            .count()
            != 2
        || task.import_ready().is_ok()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "纠正题名后已有分支没有阻止新导入。",
        ));
    }
    Ok(())
}

async fn verify_duplicate(app: &AppHandle, engine: &Engine) -> Result<()> {
    let mut record = engine.store.task("smoke-4")?.record;
    record.sa_id = "smoke-duplicate".into();
    record.owner = "duplicate-test".into();
    record.matches = 2;
    record.item_ids = ",item-primary,item-source".into();
    record.row = 7;
    engine
        .store
        .import(vec![record], "duplicate-input".into())?;
    let mut task = engine.store.task("smoke-duplicate")?;
    task.evidence.push(Evidence {
        id: "fixture-proof".into(),
        kind: "human_review".into(),
        source: "本地合成论文 · 同篇核对".into(),
        text: "两个模拟条目的 DOI、作者与年份一致，明确保留主条目的题名。".into(),
        created: now(),
    });
    engine.store.save(&mut task, "fixture_review")?;
    for role in ["sa", "duplicate"] {
        engine.browser.open(app, &engine.store.root, role).await?;
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let id = "smoke-duplicate";
    let scan = engine
        .step(app, id, "scan_duplicates", false, json!({}))
        .await?;
    if scan["groups"].as_array().map(|g| g.len()) != Some(1)
        || scan["matched_ids"] != json!(["item-primary", "item-source"])
        || scan["title_similarity"] != json!(93.5)
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "重复候选未保留实际相似度或 SA 匹配集合。",
        ));
    }
    engine
        .step(
            app,
            id,
            "prepare_duplicate",
            false,
            json!({"group_id":"group-1"}),
        )
        .await?;
    let request = json!({"source_id":"item-source","target_id":"item-primary","evidence_id":"fixture-proof","retained":"保留主条目 Synthetic paper 题名与完整字段。","identity_confirmed":true});
    let result = engine
        .step(app, id, "merge_duplicate", true, request.clone())
        .await?;
    let mut task = engine.store.task(id)?;
    if result["verified"] != true
        || task.platform_id != "item-primary"
        || task.merges.len() != 1
        || task.stage == Stage::Completed
        || !engine.store.unresolved(id)?.is_empty()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "合并没有原子保存回读与证据，或错误地完成 SA。",
        ));
    }
    let sa = engine.read_sa(app, &mut task).await?;
    if matched_ids(&sa["row"])? != vec!["item-primary".to_string()] {
        return Err(Failure::new(
            "TEST_FAILED",
            "平台合并后 SA 未回读为唯一主条目。",
        ));
    }
    if engine
        .step(app, id, "merge_duplicate", true, request)
        .await
        .is_ok()
    {
        return Err(Failure::new("TEST_FAILED", "过期合并准备仍被重复提交。"));
    }
    Ok(())
}

async fn verify_existing(app: &AppHandle, engine: &Engine) -> Result<()> {
    let mut record = engine.store.task("smoke-4")?.record;
    record.sa_id = "smoke-existing".into();
    record.matches = 1;
    record.item_ids = "item-existing".into();
    record.row = 8;
    record.reason = "通讯作者标记不一致；第一作者标记不一致".into();
    engine.store.import(vec![record], "existing-input".into())?;
    let id = "smoke-existing";
    let plan = engine
        .step(app, id, "prepare_issues", false, json!({}))
        .await?;
    if engine
        .store
        .task(id)?
        .evidence
        .iter()
        .filter(|e| e.kind == "issue_baseline")
        .count()
        != 1
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "逐项核对的首次 SA 与本库完整字段没有归入来源。",
        ));
    }
    if plan["requirements"].as_array().map(|r| r.len()) != Some(2) {
        return Err(Failure::new(
            "TEST_FAILED",
            "逐项清单没有独立保留两个作者原因。",
        ));
    }
    for (index, key) in ["corresponding_author", "first_author"].iter().enumerate() {
        engine.step(app,id,"review_issue",false,json!({"key":key,"outcome":"library_correct","source":"本地合成原文","proof":"本地合成论文明确未标记该角色。","note":if index==0{"经核对不是通讯作者"}else{"经核对不是第一作者"},"expected":plan["live"]})).await?;
        let review = Review {
            route: Route::Existing,
            evidence_id: "".into(),
            library_checked: true,
            platform_id: "item-existing".into(),
            affiliation_confirmed: true,
            identity_confirmed: true,
            issues_resolved: true,
            note: "按原文分别核对作者角色".into(),
        };
        engine.review(
            id,
            review,
            "本地合成原文".into(),
            "该条目的作者角色已逐项取证。".into(),
        )?;
        if index == 0
            && engine
                .step(app, id, "complete", true, json!({}))
                .await
                .is_ok()
        {
            return Err(Failure::new("TEST_FAILED", "仅解决一个原因就完成了 SA。"));
        }
    }
    engine.step(app, id, "complete", true, json!({})).await?;
    let mut task = engine.store.task(id)?;
    let fresh = engine.read_sa(app, &mut task).await?;
    let note = fresh["row"]["remark"].as_str().unwrap_or("");
    if task.stage != Stage::Completed
        || !task.record.done
        || task.issue_reviews.len() != 2
        || fresh["row"]["markStatus"] != "已处理"
        || !note.contains("不是通讯作者")
        || !note.contains("不是第一作者")
        || !engine.store.unresolved(id)?.is_empty()
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "逐项结论未带入 SA 完成回读或状态不一致。",
        ));
    }
    Ok(())
}
async fn verify_metadata(app: &AppHandle, engine: &Engine) -> Result<()> {
    let mut record = engine.store.task("smoke-4")?.record;
    record.sa_id = "smoke-metadata".into();
    record.matches = 1;
    record.item_ids = "item-metadata".into();
    record.row = 9;
    record.reason = "通讯作者标记不一致；第一作者标记不一致".into();
    engine.store.import(vec![record], "metadata-input".into())?;
    engine
        .browser
        .open(app, &engine.store.root, "scholar")
        .await?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let id = "smoke-metadata";
    for (index, key) in ["corresponding_author", "first_author"].iter().enumerate() {
        let prepared = engine
            .step(app, id, "prepare_metadata", false, json!({}))
            .await?;
        if prepared["result"]["authors"][1]["eligible"] != true
            || prepared["result"]["staff_id"] != "001"
        {
            return Err(Failure::new(
                "TEST_FAILED",
                "实际作者编辑页没有完整工号与明确作者对象。",
            ));
        }
        let request = json!({"key":key,"author_index":1,"source":"本地合成原文","proof":"合成原文明确标记 Tester 为通讯作者和共同第一作者。","note":if index==0{"经核对是通讯作者"}else{"经核对是共同第一作者"}});
        engine
            .step(app, id, "save_metadata", true, request.clone())
            .await?;
        let task = engine.store.task(id)?;
        if task.issue_reviews.len() != index + 1 || !engine.store.unresolved(id)?.is_empty() {
            return Err(Failure::new(
                "TEST_FAILED",
                "角色保存没有原子记录逐项结论和写入核验。",
            ));
        }
        if engine
            .step(app, id, "save_metadata", true, request)
            .await
            .is_ok()
        {
            return Err(Failure::new("TEST_FAILED", "过期作者编辑准备被重复提交。"));
        }
    }
    let task = engine.store.task(id)?;
    if task
        .evidence
        .iter()
        .filter(|e| e.kind == "metadata_verified")
        .count()
        != 2
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "完整编辑前后字段没有归入来源。",
        ));
    }
    engine.review(
        id,
        Review {
            route: Route::Existing,
            evidence_id: "".into(),
            library_checked: true,
            platform_id: "item-metadata".into(),
            affiliation_confirmed: true,
            identity_confirmed: true,
            issues_resolved: true,
            note: "原文证实两个作者角色，已修正本库".into(),
        },
        "本地合成原文".into(),
        "已独立核验编辑结果。".into(),
    )?;
    engine.step(app, id, "complete", true, json!({})).await?;
    let mut task = engine.store.task(id)?;
    let fresh = engine.read_sa(app, &mut task).await?;
    let note = fresh["row"]["remark"].as_str().unwrap_or("");
    if task.stage != Stage::Completed
        || !note.contains("是通讯作者")
        || !note.contains("是共同第一作者")
    {
        return Err(Failure::new(
            "TEST_FAILED",
            "角色修改没有进入最终 SA 完成回读。",
        ));
    }
    Ok(())
}
