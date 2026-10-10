use crate::browser::Browser;
use library_core::{source_downloads::Session, Failure, Result, Store};
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{
    webview::{DownloadEvent, NewWindowResponse},
    AppHandle, Emitter, Webview, WebviewUrl, WebviewWindowBuilder,
};

fn receive(
    app: &AppHandle,
    store: &Store,
    session: &Session,
    busy: &AtomicBool,
    view: Webview,
    event: DownloadEvent<'_>,
) -> bool {
    match event {
        DownloadEvent::Requested { url, destination } => {
            let result = if busy.load(Ordering::SeqCst) {
                Err(Failure::new(
                    "BUSY",
                    "已有任务运行，请暂停后再下载原始来源。",
                ))
            } else {
                view.url().map_err(Failure::storage).and_then(|page| {
                    store.request_source_download(session, &page, &url, destination)
                })
            };
            match result {
                Ok(receipt) => {
                    *destination = receipt.path.into();
                    true
                }
                Err(error) => {
                    let _ = app.emit_to(
                        "main",
                        "source-download-event",
                        json!({"success":false,"task_id":session.record.sa_id,"error":error}),
                    );
                    false
                }
            }
        }
        DownloadEvent::Finished { url, path, success } => {
            let result = store.finish_source_download(&session.id, &url, path.as_deref(), success);
            let payload = match result {
                Ok(Some(receipt)) => {
                    json!({"success":receipt.state=="completed","task_id":session.record.sa_id,"receipt":receipt,"error":receipt.error})
                }
                Ok(None) => return true,
                Err(error) => json!({"success":false,"task_id":session.record.sa_id,"error":error}),
            };
            let _ = app.emit_to("main", "source-download-event", payload);
            let _ = app.emit_to("main", "workspace-changed", json!({"source_download":true}));
            true
        }
        _ => false,
    }
}
impl Browser {
    pub fn open_source(
        &self,
        app: &AppHandle,
        store: &Store,
        session: Session,
        busy: Arc<AtomicBool>,
    ) -> Result<()> {
        let profile = store
            .root
            .join("browser")
            .join(format!("source-{}-profile", session.site.channel));
        std::fs::create_dir_all(&profile)?;
        if !profile
            .canonicalize()?
            .starts_with(store.root.canonicalize()?)
        {
            return Err(Failure::new(
                "SOURCE_DOWNLOAD_INVALID",
                "来源会话目录已被重定向。",
            ));
        }
        let label = format!("source-{}", session.id);
        let download_store = store.clone();
        let download_session = session.clone();
        let download_app = app.clone();
        let download_busy = busy.clone();
        let popup_app = app.clone();
        let popup_profile = profile.clone();
        let popup_store = store.clone();
        let popup_session = session.clone();
        let popup_busy = busy.clone();
        let url = session.site.entry_url.parse().map_err(Failure::storage)?;
        WebviewWindowBuilder::new(app, label, WebviewUrl::External(url))
            .title(format!(
                "原始来源 · {} · SA {}",
                session.site.channel, session.record.sa_id
            ))
            .inner_size(1180., 820.)
            .data_directory(profile)
            .on_navigation(|u| matches!(u.scheme(), "http" | "https" | "about"))
            .on_download(move |view, event| {
                receive(
                    &download_app,
                    &download_store,
                    &download_session,
                    &download_busy,
                    view,
                    event,
                )
            })
            .on_new_window(move |url, features| {
                if !matches!(url.scheme(), "http" | "https") {
                    return NewWindowResponse::Deny;
                }
                let label = format!("source-popup-{}", uuid::Uuid::new_v4().simple());
                let app = popup_app.clone();
                let store = popup_store.clone();
                let session = popup_session.clone();
                let busy = popup_busy.clone();
                let child = WebviewWindowBuilder::new(
                    &popup_app,
                    label,
                    WebviewUrl::External("about:blank".parse().unwrap()),
                )
                .title("原始来源 · 机构访问窗口")
                .data_directory(popup_profile.clone())
                .window_features(features)
                .on_navigation(|u| matches!(u.scheme(), "http" | "https" | "about"))
                .on_download(move |view, event| {
                    receive(&app, &store, &session, &busy, view, event)
                });
                match child.build() {
                    Ok(window) => NewWindowResponse::Create { window },
                    Err(_) => NewWindowResponse::Deny,
                }
            })
            .build()
            .map_err(Failure::storage)?;
        Ok(())
    }
}
