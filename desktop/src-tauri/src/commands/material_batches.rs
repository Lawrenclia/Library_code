use super::local;
use crate::engine::Engine;
use library_core::{queue::QueueStatus, *};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State, WebviewWindow};

fn save_report(engine: &Engine, id: &str) -> Result<()> {
    let batch = engine.store.material_batch(id)?;
    let folder = engine.store.root.join("materials").join("batches").join(
        uuid::Uuid::parse_str(&batch.id)
            .map_err(Failure::storage)?
            .to_string(),
    );
    std::fs::create_dir_all(&folder)?;
    if !folder
        .canonicalize()?
        .starts_with(engine.store.root.canonicalize()?)
    {
        return Err(Failure::new("MATERIAL_CHANGED", "材料报告目录被重定向。"));
    }
    let tasks = engine
        .store
        .tasks()?
        .into_iter()
        .filter(|t| batch.targets.iter().any(|r| r.id == t.id))
        .collect::<Vec<_>>();
    files::export_report_with_materials(
        &tasks,
        &[],
        &[],
        &[],
        &[batch],
        &folder.join("材料与来源.xlsx"),
    )
}
async fn run(engine: &Engine, app: &AppHandle, id: &str) -> Result<()> {
    loop {
        let batch = engine.store.material_batch(id)?;
        if batch.status != QueueStatus::Running {
            break;
        }
        if batch.pause_requested || engine.pause.load(Ordering::SeqCst) {
            engine.store.pause_material_batch(id)?;
            break;
        }
        let result = engine.store.material_target(id);
        engine.store.finish_material_target(id, result)?;
        engine.changed(app);
        tokio::task::yield_now().await;
    }
    save_report(engine, id)
}
#[tauri::command]
pub(crate) async fn prepare_material_batch(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    owner: String,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire_materials()?;
    let batch = state.store.start_material_batch(&owner)?;
    state.changed(&app);
    let result = run(&state, &app, &batch.id).await;
    state.changed(&app);
    result
}
#[tauri::command]
pub(crate) async fn resume_material_batch(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire_materials()?;
    if state.store.material_batch(&id)?.status == QueueStatus::Running {
        state.store.recover_material_batch()?;
    }
    state.store.resume_material_batch(&id)?;
    state.changed(&app);
    let result = run(&state, &app, &id).await;
    state.changed(&app);
    result
}
#[tauri::command]
pub(crate) fn cancel_material_batch(
    window: WebviewWindow,
    app: AppHandle,
    state: State<Engine>,
    id: String,
) -> Result<()> {
    local(&window)?;
    let _lease = state.acquire_materials()?;
    state.store.cancel_material_batch(&id)?;
    save_report(&state, &id)?;
    state.changed(&app);
    Ok(())
}
