//! Actual app/API crash acceptance. All hooks are absent from production builds.
use crate::engine::Engine;
use library_core::{queue::QueueStatus, *};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager};

pub fn valid_phase(phase: &str) -> bool {
    matches!(
        phase,
        "ai-ordinary"
            | "ai-request"
            | "ai-request-resume"
            | "ai-save"
            | "ai-save-resume"
            | "ai-pause"
            | "ai-pause-resume"
            | "ai-rate"
            | "ai-rate-resume"
    )
}
fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Failure::new("TEST_FAILED", message))
    }
}
pub fn config() -> Result<Option<Value>> {
    if !std::env::var("DESKTOP_SMOKE_RESTART_PHASE").is_ok_and(|p| valid_phase(&p)) {
        return Ok(None);
    }
    let origin = crate::browser::fixture_origin().ok_or_else(|| {
        Failure::new(
            "TEST_FAILED",
            "AI fixture requires an explicit loopback origin",
        )
    })?;
    Ok(Some(
        json!({"base":format!("{}/api/v1",origin.origin().ascii_serialization()),"model":"isolated-ai-fixture","configured":true}),
    ))
}
pub async fn saved_boundary(
    engine: &Engine,
    attempt: &library_core::ai_queue::Attempt,
) -> Result<()> {
    if std::env::var("DESKTOP_SMOKE_RESTART_PHASE").ok().as_deref() != Some("ai-save")
        || attempt.task_id != "ai-native-1"
    {
        return Ok(());
    }
    let origin = crate::browser::fixture_origin()
        .ok_or_else(|| Failure::new("TEST_FAILED", "No loopback fixture"))?;
    let snapshot = json!({"process_id":std::process::id(),"queue":engine.store.latest_ai_queue()?,"tasks":engine.store.tasks()?});
    std::fs::write(
        engine.store.root.join("ai-saved-boundary.json"),
        serde_json::to_vec_pretty(&snapshot)?,
    )?;
    // The harness terminates this process after receiving the durable snapshot.
    reqwest::Client::new()
        .post(origin.join("restart/ai-saved").map_err(Failure::storage)?)
        .json(&snapshot)
        .send()
        .await
        .map_err(Failure::storage)?;
    Err(Failure::new(
        "TEST_FAILED",
        "Saved-result crash boundary unexpectedly returned",
    ))
}
fn initialize(engine: &Engine) -> Result<library_core::ai_queue::AiQueue> {
    require(
        engine.store.tasks()?.is_empty(),
        "New AI phase must use a new isolated workspace",
    )?;
    let records = (1..=4)
        .map(|n| Record {
            row: n + 1,
            owner: "ai-native-owner".into(),
            sa_id: format!("ai-native-{n}"),
            title: format!("Synthetic AI paper {n}"),
            doi: String::new(),
            wos: String::new(),
            staff_id: "000000000000001".into(),
            matches: 0,
            item_ids: String::new(),
            mark: "待处理".into(),
            reason: "Original roster reason — 原始依据不截断".into(),
            skipped: false,
            done: false,
            source: "isolated synthetic roster".into(),
        })
        .collect();
    engine
        .store
        .import(records, "isolated-ai-native-input".into())?;
    let mut first = engine.store.task("ai-native-1")?;
    first.evidence.push(Evidence {
        id: "long-original-source".into(),
        kind: "manual".into(),
        source: "isolated original research excerpt".into(),
        text: "Full original source 重要资料🧪。".repeat(1600),
        created: 1,
    });
    engine.store.save(&mut first, "isolated_source")?;
    engine.store.start_ai_queue(
        "ai-native-owner",
        true,
        crate::ai::public_config(engine)?,
        None,
    )
}
fn report(engine: &Engine) -> Result<()> {
    files::export_report_with_runs(
        &engine.store.tasks()?,
        &[],
        &[],
        &engine.store.ai_queues()?,
        &engine.store.root.join("ai-native-report.xlsx"),
    )
}
pub async fn run(app: &AppHandle, engine: &Engine, phase: &str) -> Result<()> {
    require(
        engine.acquire().is_err(),
        "AI must share the execution lock with other services",
    )?;
    let q = if phase.ends_with("-resume") {
        let q = engine
            .store
            .latest_ai_queue()?
            .ok_or_else(|| Failure::new("TEST_FAILED", "Original AI queue missing"))?;
        require(
            q.cursor == 1 && q.targets.len() == 4 && q.inflight.is_none(),
            "Startup did not retain the original AI scope",
        )?;
        let expected = if phase == "ai-request-resume" {
            "unconfirmed"
        } else if phase == "ai-rate-resume" {
            "failed"
        } else {
            "classified"
        };
        require(
            q.outcomes[0].status == expected,
            "Startup incorrectly accepted or repeated the first request",
        )?;
        require(
            !engine.store.task("ai-native-1")?.record.done,
            "AI cannot complete SA",
        )?;
        // New rows cannot join an explicitly resumed original queue.
        let mut extra = engine.store.task("ai-native-1")?.record;
        extra.sa_id = "ai-native-new".into();
        extra.row = 6;
        extra.title = "Later added paper".into();
        engine
            .store
            .import(vec![extra], "isolated-ai-native-input".into())?;
        engine
            .store
            .resume_ai_queue(&q.id, &crate::ai::public_config(engine)?, None)?;
        q
    } else {
        initialize(engine)?
    };
    if phase == "ai-pause" {
        let cloned = app.clone();
        let pause = tauri::async_runtime::spawn(async move {
            let engine = cloned.state::<Engine>();
            for _ in 0..500 {
                if engine
                    .store
                    .latest_ai_queue()?
                    .is_some_and(|q| q.inflight.is_some())
                {
                    engine.store.request_ai_pause()?;
                    engine.pause.store(true, Ordering::SeqCst);
                    engine.changed(&cloned);
                    return Ok::<(), Failure>(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            Err(Failure::new(
                "TEST_FAILED",
                "No inflight AI request to pause",
            ))
        });
        crate::ai::queue_loop(engine, app, &q.id).await?;
        pause.await.map_err(Failure::storage)??;
    } else {
        crate::ai::queue_loop(engine, app, &q.id).await?;
    }
    let finished = engine.store.ai_queue(&q.id)?;
    let tasks = engine.store.tasks()?;
    require(
        tasks.iter().all(|t| {
            t.stage == Stage::Pending && !t.record.done && t.record.staff_id == "000000000000001"
        }),
        "AI changed business state or roster precision",
    )?;
    require(finished.targets.len() == 4, "Original AI scope changed")?;
    match phase {
        "ai-request" | "ai-save" => {
            return Err(Failure::new(
                "TEST_FAILED",
                "Expected harness to terminate the owned process",
            ))
        }
        "ai-pause" => require(
            finished.status == QueueStatus::Paused
                && finished.cursor == 1
                && finished.outcomes[0].status == "classified",
            "Pause did not save the current request before stopping",
        )?,
        "ai-rate" => require(
            finished.status == QueueStatus::Blocked
                && finished.cursor == 1
                && finished.outcomes[0]
                    .error
                    .as_ref()
                    .is_some_and(|e| e.code == "AI_RATE_LIMIT"),
            "Rate limit did not stop later requests",
        )?,
        "ai-ordinary" => require(
            finished.status == QueueStatus::Completed
                && finished.cursor == 4
                && finished.outcomes[..3].iter().all(|o| o.status == "failed")
                && finished.outcomes[3].status == "classified",
            "Three ordinary failures prevented the fourth request",
        )?,
        _ => require(
            finished.status == QueueStatus::Completed
                && finished.cursor == 4
                && finished.outcomes[1..]
                    .iter()
                    .all(|o| o.status == "classified"),
            "Original remaining AI tasks did not complete",
        )?,
    }
    require(
        tasks
            .iter()
            .filter(|t| t.classification.is_some())
            .all(|t| t.evidence.iter().any(|e| e.kind == "ai_classification")),
        "Saved AI result lost its complete audit",
    )?;
    report(engine)
}
