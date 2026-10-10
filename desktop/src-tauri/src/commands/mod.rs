use library_core::{Failure, Result};
use tauri::WebviewWindow;
pub mod browsers;
pub mod material_batches;
pub mod materials;
pub mod models;
pub mod sources;
pub mod submission;
pub mod tasks;
pub mod workspace;

pub(super) fn local(w: &WebviewWindow) -> Result<()> {
    if w.label() != "main" {
        return Err(Failure::new(
            "PERMISSION_DENIED",
            "只能从工作台执行此操作。",
        ));
    }
    let u = w.url().map_err(Failure::storage)?;
    if !matches!(u.scheme(), "tauri" | "http" | "https")
        || (!matches!(
            u.host_str(),
            Some("tauri.localhost" | "127.0.0.1" | "localhost")
        ) && u.scheme() != "tauri")
    {
        return Err(Failure::new("PERMISSION_DENIED", "页面不属于本地工作台。"));
    }
    Ok(())
}
