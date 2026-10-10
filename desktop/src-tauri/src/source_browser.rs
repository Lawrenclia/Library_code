use crate::{
    browser::Browser,
    engine::{Engine, Lease},
};
use library_core::{source_downloads::Session, Failure, Result, Store};
use serde_json::json;
use std::path::PathBuf;
use tauri::{
    webview::{DownloadEvent, NewWindowResponse},
    AppHandle, Emitter, Manager, Webview, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

/// Keep the same Engine lease until native completion has been persisted.
pub(crate) struct Transfer {
    session_id: String,
    window_label: String,
    event_url: String,
    path: PathBuf,
    _lease: Lease,
}
fn watch_close(app: &AppHandle, store: &Store, browser: &Browser, window: &WebviewWindow) {
    let app = app.clone();
    let store = store.clone();
    let browser = browser.clone();
    let label = window.label().to_string();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            browser.interrupt_source_window(&app, &store, &label);
        }
    });
}
impl Browser {
    fn receive_source(
        &self,
        app: &AppHandle,
        store: &Store,
        session: &Session,
        view: Webview,
        event: DownloadEvent<'_>,
    ) -> bool {
        match event {
            DownloadEvent::Requested { url, destination } => {
                let result = (|| -> Result<library_core::source_downloads::Receipt> {
                    let mut transfers = self.source_transfers.lock().map_err(Failure::storage)?;
                    let engine = app.try_state::<Engine>().ok_or_else(|| {
                        Failure::new("BROWSER_UNAVAILABLE", "工作台运行服务不可用。")
                    })?;
                    let lease = engine.acquire_source_download()?;
                    let page = view.url().map_err(Failure::storage)?;
                    let receipt =
                        store.request_source_download(session, &page, &url, destination)?;
                    transfers.insert(
                        receipt.id.clone(),
                        Transfer {
                            session_id: session.id.clone(),
                            window_label: view.label().to_string(),
                            event_url: url.to_string(),
                            path: receipt.path.clone().into(),
                            _lease: lease,
                        },
                    );
                    Ok(receipt)
                })();
                match result {
                    Ok(receipt) => {
                        *destination = receipt.path.clone().into();
                        let _ = app.emit_to(
                            "main",
                            "source-download-started",
                            json!({"task_id":session.record.sa_id,"receipt":receipt}),
                        );
                        let _ = app.emit_to(
                            "main",
                            "workspace-changed",
                            json!({"source_download":true}),
                        );
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
                let owned = (|| -> Result<Option<(String, Transfer)>> {
                    let mut transfers = self.source_transfers.lock().map_err(Failure::storage)?;
                    let keys: Vec<_> = transfers
                        .iter()
                        .filter(|(_, t)| {
                            t.session_id == session.id
                                && t.window_label == view.label()
                                && t.event_url == url.as_str()
                                && path.as_ref().is_none_or(|p| p == &t.path)
                        })
                        .map(|(id, _)| id.clone())
                        .collect();
                    if keys.len() > 1 {
                        return Err(Failure::new(
                            "SOURCE_DOWNLOAD_INVALID",
                            "下载结束事件无法唯一对应当前原请求，请关闭来源窗口后核对。",
                        ));
                    }
                    Ok(keys
                        .first()
                        .and_then(|id| transfers.remove(id).map(|t| (id.clone(), t))))
                })();
                let payload = match owned {
                    Ok(None) => return true,
                    Ok(Some((id, transfer))) => {
                        let result = store.finish_source_download(
                            &session.id,
                            &url,
                            path.as_deref(),
                            success,
                        );
                        if result.is_err() {
                            let _ = store.interrupt_source_download_ids(&[id]);
                        }
                        drop(transfer);
                        match result {
                            Ok(Some(receipt)) => {
                                json!({"success":receipt.state=="completed","task_id":session.record.sa_id,"receipt":receipt,"error":receipt.error})
                            }
                            Ok(None) => {
                                json!({"success":false,"task_id":session.record.sa_id,"error":Failure::new("SOURCE_DOWNLOAD_INVALID","原下载请求已不在等待状态，未采纳结束事件。")})
                            }
                            Err(error) => {
                                json!({"success":false,"task_id":session.record.sa_id,"error":error})
                            }
                        }
                    }
                    Err(error) => {
                        json!({"success":false,"task_id":session.record.sa_id,"error":error})
                    }
                };
                let _ = app.emit_to("main", "source-download-event", payload);
                let _ = app.emit_to("main", "workspace-changed", json!({"source_download":true}));
                true
            }
            _ => false,
        }
    }
    fn interrupt_source_window(&self, app: &AppHandle, store: &Store, label: &str) {
        let result = (|| -> Result<bool> {
            let mut transfers = self.source_transfers.lock().map_err(Failure::storage)?;
            let ids: Vec<_> = transfers
                .iter()
                .filter(|(_, t)| t.window_label == label)
                .map(|(id, _)| id.clone())
                .collect();
            if ids.is_empty() {
                return Ok(false);
            }
            let owned: Vec<_> = ids.iter().filter_map(|id| transfers.remove(id)).collect();
            drop(transfers);
            let result = store.interrupt_source_download_ids(&ids);
            drop(owned);
            result?;
            Ok(true)
        })();
        let error = match result {
            Ok(false) => return,
            Ok(true) => Failure::new(
                "SOURCE_DOWNLOAD_INVALID",
                "来源窗口已关闭，原始下载尚未确认完成。文件与请求保留，请核对后继续。",
            ),
            Err(error) => error,
        };
        let _ = app.emit_to(
            "main",
            "source-download-event",
            json!({"success":false,"error":error}),
        );
        let _ = app.emit_to("main", "workspace-changed", json!({"source_download":true}));
    }
    pub fn open_source(&self, app: &AppHandle, store: &Store, session: Session) -> Result<()> {
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
        let download_browser = self.clone();
        let popup_app = app.clone();
        let popup_profile = profile.clone();
        let popup_store = store.clone();
        let popup_session = session.clone();
        let popup_browser = self.clone();
        let url = session.site.entry_url.parse().map_err(Failure::storage)?;
        let window = WebviewWindowBuilder::new(app, label, WebviewUrl::External(url))
            .title(format!(
                "原始来源 · {} · SA {}",
                session.site.channel, session.record.sa_id
            ))
            .inner_size(1180., 820.)
            .data_directory(profile)
            .on_navigation(|u| matches!(u.scheme(), "http" | "https" | "about"))
            .on_download(move |view, event| {
                download_browser.receive_source(
                    &download_app,
                    &download_store,
                    &download_session,
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
                let browser = popup_browser.clone();
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
                    browser.receive_source(&app, &store, &session, view, event)
                });
                match child.build() {
                    Ok(window) => {
                        watch_close(&popup_app, &popup_store, &popup_browser, &window);
                        NewWindowResponse::Create { window }
                    }
                    Err(_) => NewWindowResponse::Deny,
                }
            })
            .build()
            .map_err(Failure::storage)?;
        watch_close(app, store, self, &window);
        Ok(())
    }
}
