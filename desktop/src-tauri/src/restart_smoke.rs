//! Full process/WebView2 crash acceptance. Only compiled into the test binary.
//! Page requests go through production adapters. The external fixture retains
//! remote state while the Node harness terminates and restarts this process.
use crate::engine::Engine;
use library_core::*;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const ID: &str = "restart-import";
pub fn valid_phase(phase: &str) -> bool {
    crate::link_smoke::valid_phase(phase)
        || crate::merge_smoke::valid_phase(phase)
        || crate::claim_smoke::valid_phase(phase)
        || crate::alias_smoke::valid_phase(phase)
        || crate::queue_smoke::valid_phase(phase)
        || crate::download_smoke::valid_phase(phase)
        || crate::metadata_smoke::valid_phase(phase)
        || matches!(
            phase,
            "submit" | "push" | "verify" | "upload" | "upload-readback"
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
    for role in ["sa", "library", "import"] {
        engine.browser.open(app, &engine.store.root, role).await?;
    }
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    Ok(())
}
async fn initialize(app: &AppHandle, engine: &Engine) -> Result<()> {
    require(
        engine.store.tasks()?.is_empty(),
        "Initial phase must have a new isolated workspace",
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
            reason: "测试缺失".into(),
            skipped: false,
            done: false,
            source: "本地重启模拟页面".into(),
        }],
        "restart-synthetic-input".into(),
    )?;
    // Obtain the actual native TXT, rather than installing a fake task artifact.
    engine.browser.open(app, &engine.store.root, "wos").await?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    engine.queue(app, "restart-fixture", false).await?;
    require(
        engine.store.task(ID)?.stage == Stage::Downloaded,
        "Native TXT download failed",
    )?;
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
    engine.review(
        ID,
        Review {
            route: Route::Missing,
            evidence_id: "".into(),
            library_checked: true,
            platform_id: "".into(),
            affiliation_confirmed: true,
            identity_confirmed: true,
            issues_resolved: false,
            note: "本地合成原文与全部查库零条".into(),
        },
        "本地模拟 WOS 完整记录与前端".into(),
        "核对同篇、交大地址和全部查询零条".into(),
    )?;
    Ok(())
}
fn pending(engine: &Engine, action: &str) -> Result<Value> {
    let task = engine.store.task(ID)?;
    let attempts = engine.store.unresolved(ID)?;
    require(
        task.stage == Stage::Unknown
            && !task.running
            && attempts.len() == 1
            && attempts[0]["action"] == action,
        "Startup did not recover the exact original intent as unknown",
    )?;
    let original: Value = serde_json::from_str(attempts[0]["data"].as_str().unwrap())?;
    require(
        original["payload"]["sa_id"] == ID
            && original["payload"]["instructions"] == format!("SA补充-{ID}"),
        "Original payload lost task identity",
    )?;
    let raw = files::read_import_archive(&task, &original["payload"])?;
    require(
        hash(&raw) == task.artifact.as_ref().unwrap().candidate.sha256,
        "Original archive hash changed",
    )?;
    Ok(attempts[0].clone())
}
async fn refuse(engine: &Engine, app: &AppHandle, action: &str) -> Result<()> {
    require(
        engine.step(app, ID, action, true, json!({})).await.is_err(),
        "Unknown remote write was resent",
    )
}
async fn recover(app: &AppHandle, engine: &Engine, action: &str, expected: Stage) -> Result<()> {
    let original = pending(engine, action)?;
    // Reopening the application does not silently recreate work pages.
    require(
        engine.browser.states(app)["import"]["open"] == false,
        "Old WebView window survived the process",
    )?;
    refuse(engine, app, action).await?;
    open(app, engine).await?;
    // A corrupt persisted artifact must not turn a current batch into proof of
    // the old write. Restore the byte-identical archive for the positive case.
    let task = engine.store.task(ID)?;
    let path = &task.artifact.as_ref().unwrap().path;
    let raw = std::fs::read(path)?;
    let mut altered = raw.clone();
    let offset = altered.iter().position(|b| *b == b'S').unwrap();
    altered[offset] = b'Z';
    std::fs::write(path, &altered)?;
    let rejected = engine
        .step(app, ID, "verify_import", false, json!({}))
        .await;
    std::fs::write(path, &raw)?;
    require(
        rejected.is_err()
            && engine.store.task(ID)?.stage == Stage::Unknown
            && engine.store.unresolved(ID)?[0]["data"] == original["data"],
        "Changed archive did not preserve the original unknown attempt",
    )?;
    let result = engine
        .step(app, ID, "verify_import", false, json!({}))
        .await?;
    require(
        result["verified"] == true
            && engine.store.task(ID)?.stage == expected
            && engine.store.unresolved(ID)?.is_empty(),
        "Fresh batch readback failed after process restart",
    )?;
    // Readback of the same verified phase remains repeatable and write-free.
    engine
        .step(app, ID, "verify_import", false, json!({}))
        .await?;
    require(
        engine.store.task(ID)?.stage == expected,
        "Repeated batch readback changed the phase",
    )?;
    require(
        engine.step(app, ID, action, true, json!({})).await.is_err(),
        "Verified original operation could be repeated",
    )?;
    Ok(())
}
pub async fn run(app: &AppHandle, phase: &str) -> Result<()> {
    let engine = app.state::<Engine>();
    let _lease = engine.acquire()?;
    if crate::merge_smoke::valid_phase(phase) {
        return crate::merge_smoke::run(app, &engine, phase).await;
    }
    if crate::link_smoke::valid_phase(phase) {
        return crate::link_smoke::run(app, &engine, phase).await;
    }
    if crate::claim_smoke::valid_phase(phase) {
        return crate::claim_smoke::run(app, &engine, phase).await;
    }
    if crate::alias_smoke::valid_phase(phase) {
        return crate::alias_smoke::run(app, &engine, phase).await;
    }
    if crate::metadata_smoke::valid_phase(phase) {
        return crate::metadata_smoke::run(app, &engine, phase).await;
    }
    if crate::download_smoke::valid_phase(phase) {
        return crate::download_smoke::run(app, &engine, phase).await;
    }
    if crate::queue_smoke::valid_phase(phase) {
        return crate::queue_smoke::run(app, &engine, phase).await;
    }
    match phase {
        "submit" => {
            initialize(app, &engine).await?;
            engine
                .step(app, ID, "import_upload", true, json!({}))
                .await?;
            require(
                engine.store.task(ID)?.stage == Stage::Uploaded,
                "Upload receipt missing",
            )?;
            // Fixture accepts this request, withholds completion, and the
            // external harness kills the app while the durable intent exists.
            engine
                .step(app, ID, "import_submit", true, json!({}))
                .await?;
            return Err(Failure::new(
                "TEST_FAILED",
                "Crash boundary unexpectedly returned",
            ));
        }
        "push" => {
            recover(app, &engine, "import_submit", Stage::Imported).await?;
            engine.step(app, ID, "import_push", true, json!({})).await?;
            return Err(Failure::new(
                "TEST_FAILED",
                "Crash boundary unexpectedly returned",
            ));
        }
        "verify" => {
            recover(app, &engine, "import_push", Stage::Pushed).await?;
            let task = engine.store.task(ID)?;
            require(
                !task.record.done
                    && task
                        .evidence
                        .iter()
                        .filter(|e| e.kind == "import_verified")
                        .count()
                        == 4,
                "Readback evidence missing or push incorrectly completed SA",
            )?;
            require(
                std::fs::read_dir(engine.store.root.join("downloads"))?.count() == 1,
                "Restart downloaded the original metadata again",
            )?;
            files::export_report(
                &engine.store.tasks()?,
                &engine.store.root.join("restart-report.xlsx"),
            )?;
        }
        "upload" => {
            initialize(app, &engine).await?;
            engine
                .step(app, ID, "import_upload", true, json!({}))
                .await?;
            return Err(Failure::new(
                "TEST_FAILED",
                "Upload crash boundary unexpectedly returned",
            ));
        }
        "upload-readback" => {
            let original = pending(&engine, "import_upload")?;
            require(
                engine.browser.states(app)["import"]["open"] == false,
                "Upload window survived the process",
            )?;
            refuse(&engine, app, "import_upload").await?;
            open(app, &engine).await?;
            let error = engine
                .step(app, ID, "verify_import", false, json!({}))
                .await
                .unwrap_err();
            require(
                error.code == "REMOTE_RESULT_UNKNOWN"
                    && engine.store.task(ID)?.stage == Stage::Unknown
                    && engine.store.unresolved(ID)?[0]["data"] == original["data"],
                "Lost upload window falsely recovered without a real batch",
            )?;
            refuse(&engine, app, "import_upload").await?;
            refuse(&engine, app, "import_submit").await?;
            require(
                engine.store.task(ID)?.artifact.is_some(),
                "Unknown upload lost its original file",
            )?;
            files::export_report(
                &engine.store.tasks()?,
                &engine.store.root.join("restart-report.xlsx"),
            )?;
        }
        _ => return Err(Failure::new("TEST_FAILED", "Unknown restart phase")),
    }
    Ok(())
}
