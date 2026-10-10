//! Native callback / artifact-save crash boundaries, compiled only for fixtures.
use crate::engine::Engine;
use library_core::{queue::QueueStatus, *};
use tauri::{AppHandle, Manager};
const ID: &str = "receipt-paper";
pub fn valid_phase(phase: &str) -> bool {
    matches!(
        phase,
        "download-complete"
            | "download-complete-resume"
            | "download-requested"
            | "download-requested-resume"
    )
}
fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Failure::new("TEST_FAILED", message))
    }
}
pub async fn run(app: &AppHandle, engine: &Engine, phase: &str) -> Result<()> {
    if matches!(phase, "download-complete" | "download-requested") {
        require(
            engine.store.tasks()?.is_empty(),
            "Download crash test needs a fresh workspace",
        )?;
        engine.store.import(
            vec![Record {
                row: 2,
                owner: "receipt-fixture".into(),
                sa_id: ID.into(),
                title: "Synthetic paper".into(),
                doi: "10.1234/test".into(),
                wos: "WOS:000123456789012".into(),
                staff_id: "001".into(),
                matches: 0,
                item_ids: "".into(),
                mark: "待处理".into(),
                reason: "本地下载回执崩溃验收".into(),
                skipped: false,
                done: false,
                source: "本地合成 WOS 页面".into(),
            }],
            "native-receipt-input".into(),
        )?;
        engine.browser.open(app, &engine.store.root, "wos").await?;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        engine.queue(app, "receipt-fixture", false).await?;
        return Err(Failure::new(
            "TEST_FAILED",
            "Harness did not stop the download boundary",
        ));
    }
    require(
        app.get_webview_window("wos").is_none(),
        "Restart retained an old browser",
    )?;
    let queue = engine.store.latest_download_queue()?.unwrap();
    require(
        queue.status == QueueStatus::Interrupted && queue.cursor == 0,
        "Original queue cursor was consumed",
    )?;
    let task = engine.store.task(ID)?;
    let receipt = engine.store.native_downloads(ID)?;
    require(receipt.len() == 1, "Native download receipt was duplicated")?;
    if phase == "download-complete-resume" {
        require(
            task.stage == Stage::Downloaded
                && task.artifact.is_some()
                && receipt[0].state == "adopted",
            "Completed native file was not restored at production startup",
        )?;
        engine.resume_queue(app, &queue.id).await?;
        require(
            engine.store.download_queue(&queue.id)?.outcomes[0].status == "downloaded",
            "Queue did not reuse restored bytes without opening WOS",
        )?;
        let (tasks, queues) = engine.store.report_snapshot()?;
        files::export_report_with_queues(
            &tasks,
            &queues,
            &engine.store.root.join("download-receipt-report.xlsx"),
        )?;
    } else {
        require(
            task.artifact.is_none()
                && task
                    .last_error
                    .as_ref()
                    .is_some_and(|e| e.code == "DOWNLOAD_RESULT_UNKNOWN")
                && receipt[0].state == "requested",
            "Unconfirmed download was falsely completed",
        )?;
        require(
            engine
                .download_one(app, ID)
                .await
                .err()
                .is_some_and(|e| e.code == "DOWNLOAD_RESULT_UNKNOWN"),
            "Unconfirmed request was retried or searched",
        )?;
        require(
            engine.store.task(ID)?.artifact.is_none()
                && engine.store.native_downloads(ID)?.len() == 1,
            "Unknown request was modified",
        )?;
    }
    require(
        engine.store.unresolved(ID)?.is_empty(),
        "Download test performed a platform write",
    )?;
    Ok(())
}
