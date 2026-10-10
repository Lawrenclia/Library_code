//! Real app/process queue recovery against an independently running loopback WOS.
//! No test phase is compiled into production builds.
use crate::engine::Engine;
use library_core::{queue::QueueStatus, *};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager};

pub fn valid_phase(phase: &str) -> bool {
    matches!(
        phase,
        "queue-start" | "queue-resume" | "queue-pause" | "queue-paused-resume"
    )
}
fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Failure::new("TEST_FAILED", message))
    }
}
fn record(id: &str, title: &str) -> Record {
    let identified = title == "Synthetic paper";
    Record {
        row: 2,
        owner: "queue-fixture".into(),
        sa_id: id.into(),
        title: title.into(),
        doi: if identified {
            "10.1234/test".into()
        } else {
            String::new()
        },
        wos: String::new(),
        staff_id: "001".into(),
        matches: 0,
        item_ids: String::new(),
        mark: "待处理".into(),
        reason: "本地队列恢复测试".into(),
        skipped: false,
        done: false,
        source: "本地合成 WOS 页面".into(),
    }
}
async fn open(app: &AppHandle, engine: &Engine) -> Result<()> {
    engine.browser.open(app, &engine.store.root, "wos").await?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    Ok(())
}
pub async fn run(app: &AppHandle, engine: &Engine, phase: &str) -> Result<()> {
    match phase {
        "queue-start" => {
            require(
                engine.store.tasks()?.is_empty(),
                "Queue test needs a fresh isolated workspace",
            )?;
            let mut records = (1..=7)
                .map(|i| {
                    record(
                        &format!("queue-{i}"),
                        if i <= 3 {
                            "Missing paper"
                        } else if i == 5 {
                            "Changed fifth"
                        } else {
                            "Synthetic paper"
                        },
                    )
                })
                .collect::<Vec<_>>();
            for (i, r) in records.iter_mut().enumerate() {
                r.row = i as u32 + 2;
                if i < 3 {
                    r.title = format!("Missing {}", i + 1);
                }
            }
            let mut other = record("queue-other", "Missing other");
            other.owner = "other-owner".into();
            records.push(other);
            let mut skipped = record("queue-skipped", "Synthetic paper");
            skipped.skipped = true;
            records.push(skipped);
            engine.store.import(records, "queue-input".into())?;
            open(app, engine).await?;
            // Server holds the fourth Search; the harness kills this process.
            engine.queue(app, "queue-fixture", false).await?;
            return Err(Failure::new("TEST_FAILED", "Queue crash boundary returned"));
        }
        "queue-resume" => {
            let original = engine
                .store
                .latest_download_queue()?
                .ok_or_else(|| Failure::new("TEST_FAILED", "Queue missing"))?;
            require(
                original.status == QueueStatus::Interrupted
                    && original.cursor == 3
                    && original.outcomes.len() == 3
                    && original.targets.len() == 7,
                "Original queue order/progress did not survive app termination",
            )?;
            require(
                original
                    .outcomes
                    .iter()
                    .all(|o| o.error.as_ref().is_some_and(|e| e.code == "NO_RESULT")),
                "Previous failures lost their structured outcomes",
            )?;
            require(
                engine.browser.states(app)["wos"]["open"] == false,
                "Old WebView2 survived termination",
            )?;
            require(
                engine
                    .queue(app, "other-owner", false)
                    .await
                    .unwrap_err()
                    .code
                    == "QUEUE_PENDING",
                "A new scope replaced an unfinished queue",
            )?;
            let mut changed = engine.store.task("queue-5")?.record;
            changed.title = "Different fifth".into();
            let mut skipped = engine.store.task("queue-6")?.record;
            skipped.skipped = true;
            engine.store.import(
                vec![changed, skipped, record("queue-new", "Synthetic paper")],
                "new-input-after-restart".into(),
            )?;
            open(app, engine).await?;
            engine.resume_queue(app, &original.id).await?;
            let done = engine.store.download_queue(&original.id)?;
            require(
                done.status == QueueStatus::Completed
                    && done.cursor == 7
                    && done.targets.len() == 7,
                "Resume re-filtered or expanded the original scope",
            )?;
            require(
                done.outcomes
                    .iter()
                    .map(|o| o.status.as_str())
                    .collect::<Vec<_>>()
                    == [
                        "review",
                        "review",
                        "review",
                        "downloaded",
                        "not_executed",
                        "not_executed",
                        "downloaded",
                    ],
                "Resume did not preserve previous failures or skip changed tasks",
            )?;
            require(
                done.targets
                    .iter()
                    .map(|t| (&t.id, &t.fingerprint, t.skipped))
                    .collect::<Vec<_>>()
                    == original
                        .targets
                        .iter()
                        .map(|t| (&t.id, &t.fingerprint, t.skipped))
                        .collect::<Vec<_>>(),
                "Frozen targets were rewritten",
            )?;
            require(
                engine.store.pending_input("queue-5")?.is_some()
                    && engine.store.task("queue-5")?.artifact.is_none(),
                "Changed input was overwritten instead of reviewed",
            )?;
            require(
                engine.store.task("queue-6")?.record.skipped
                    && engine.store.task("queue-6")?.artifact.is_none(),
                "New skip flag was ignored",
            )?;
            require(
                engine.store.task("queue-new")?.stage == Stage::Pending
                    && engine.store.task("queue-new")?.artifact.is_none(),
                "A newly imported task joined the old scope",
            )?;
            let downloaded = engine.store.task("queue-4")?.artifact.unwrap();
            require(
                engine.store.task("queue-7")?.artifact.unwrap().path == downloaded.path
                    && std::fs::read_dir(engine.store.root.join("downloads"))?.count() == 1,
                "Verified metadata was downloaded again instead of reused",
            )?;
            require(
                engine.resume_queue(app, &original.id).await.is_err(),
                "Completed queue could run twice",
            )?;
            // A genuine closed-browser channel failure keeps the current target.
            app.get_webview_window("wos")
                .unwrap()
                .close()
                .map_err(Failure::storage)?;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            engine.queue(app, "other-owner", false).await?;
            let blocked = engine.store.latest_download_queue()?.unwrap();
            require(
                blocked.status == QueueStatus::Blocked
                    && blocked.cursor == 0
                    && blocked
                        .last_error
                        .as_ref()
                        .is_some_and(|e| e.code == "BROWSER_DISCONNECTED"),
                "Channel failure consumed the target or lost the error",
            )?;
            open(app, engine).await?;
            // acquire() resets the user's runtime pause for a new command. This
            // test holds one lease across phases, so emulate that reset here.
            engine.pause.store(false, Ordering::SeqCst);
            engine.resume_queue(app, &blocked.id).await?;
            require(
                engine.store.download_queue(&blocked.id)?.status == QueueStatus::Completed,
                "Recovered channel did not resume its current target",
            )?;
            let (tasks, queues) = engine.store.report_snapshot()?;
            files::export_report_with_queues(
                &tasks,
                &queues,
                &engine.store.root.join("queue-report.xlsx"),
            )?;
        }
        "queue-pause" => {
            require(
                engine.store.tasks()?.is_empty(),
                "Pause test needs a fresh workspace",
            )?;
            engine.store.import(
                vec![
                    record("queue-pause-1", "Synthetic paper"),
                    record("queue-pause-2", "Missing after pause"),
                ],
                "pause-input".into(),
            )?;
            open(app, engine).await?;
            let pause_store = engine.store.clone();
            let pause_flag = engine.pause.clone();
            let pause = tauri::async_runtime::spawn(async move {
                let until = now() + 20000;
                while now() < until {
                    if pause_store.task("queue-pause-1")?.running {
                        pause_store.request_download_pause()?;
                        pause_flag.store(true, Ordering::SeqCst);
                        return Ok(());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                }
                Err(Failure::new(
                    "TEST_FAILED",
                    "Search did not begin before pause",
                ))
            });
            let ran = engine.queue(app, "queue-fixture", false).await;
            let paused = pause.await.map_err(Failure::storage)?;
            ran?;
            paused?;
            let q = engine.store.latest_download_queue()?.unwrap();
            require(
                q.status == QueueStatus::Paused
                    && q.pause_requested
                    && q.cursor == 0
                    && engine.store.task("queue-pause-1")?.stage == Stage::Pending,
                "Pause during Search discarded or consumed the unfinished target",
            )?;
            std::fs::write(
                engine.store.root.join("queue-pause-ready.json"),
                serde_json::to_vec_pretty(&q)?,
            )?;
            println!("QUEUE_PAUSE_READY");
            // Deliberate crash boundary; the harness terminates its owned tree.
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
            return Err(Failure::new(
                "TEST_FAILED",
                "Paused process was not terminated",
            ));
        }
        "queue-paused-resume" => {
            let q = engine.store.latest_download_queue()?.unwrap();
            require(
                q.status == QueueStatus::Paused && q.cursor == 0 && q.pause_requested,
                "User pause was lost at restart",
            )?;
            require(
                engine.browser.states(app)["wos"]["open"] == false,
                "Old paused browser survived restart",
            )?;
            open(app, engine).await?;
            engine.resume_queue(app, &q.id).await?;
            let done = engine.store.download_queue(&q.id)?;
            require(
                done.status == QueueStatus::Completed
                    && done.cursor == 2
                    && done.outcomes[0].status == "downloaded"
                    && done.outcomes[1]
                        .error
                        .as_ref()
                        .is_some_and(|e| e.code == "NO_RESULT"),
                "Paused queue did not resume from its exact cursor",
            )?;
            require(
                std::fs::read_dir(engine.store.root.join("downloads"))?.count() == 1,
                "Pause generated a duplicate download",
            )?;
        }
        _ => return Err(Failure::new("TEST_FAILED", "Unknown queue phase")),
    }
    Ok(())
}
