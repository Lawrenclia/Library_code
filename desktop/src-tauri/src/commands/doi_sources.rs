use super::local;
use crate::engine::Engine;
use library_core::{doi_sources, *};
use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, State, WebviewWindow};

async fn fetch(request: &doi_sources::Request) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(35))
        .user_agent(concat!(
            "LibrarySAWorkspace/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/Lawrenclia/Library_code)"
        ))
        .build()
        .map_err(Failure::storage)?;
    let mut response = client
        .get(&request.url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| Failure::new("DOI_LOOKUP_FAILED", "DOI 来源连接失败或超时，未自动重试。"))?;
    match response.status().as_u16() {
        200 => {}
        404 => {
            return Err(Failure::new(
                "DOI_NOT_FOUND",
                "Crossref 未找到该 DOI 的登记记录；这不能证明论文不存在，请继续核对其他原始来源。",
            ))
        }
        429 => {
            return Err(Failure::new(
                "DOI_LOOKUP_LIMITED",
                "来源服务限流，稍后再查；未自动重复请求。",
            ))
        }
        _ => {
            return Err(Failure::new(
                "DOI_LOOKUP_FAILED",
                "来源服务没有返回成功记录，未绑定或自动重试。",
            ))
        }
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if response.url().as_str() != request.url
        || !(content_type == "application/json" || content_type.ends_with("+json"))
        || response
            .content_length()
            .is_some_and(|n| n > doi_sources::MAX_RESPONSE as u64)
    {
        return Err(Failure::new(
            "DOI_SOURCE_INVALID",
            "来源响应地址、格式或大小不符，未采纳。",
        ));
    }
    let mut raw = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Failure::new("DOI_LOOKUP_FAILED", "来源响应未完整读完，未采纳。"))?
    {
        if chunk.len() > doi_sources::MAX_RESPONSE.saturating_sub(raw.len()) {
            return Err(Failure::new(
                "DOI_SOURCE_INVALID",
                "来源响应超过 16 MB，未截断或采纳。",
            ));
        }
        raw.extend_from_slice(&chunk);
    }
    Ok(raw)
}

#[tauri::command]
pub(crate) async fn lookup_doi_source(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Engine>,
    id: String,
    refresh: bool,
) -> Result<Value> {
    local(&window)?;
    let _lease = state.acquire()?;
    if !refresh {
        if let Some(cached) = doi_sources::cached(&state.store, &id)? {
            return Ok(cached);
        }
    }
    let request = doi_sources::request(&state.store, &id)?;
    let result = match fetch(&request).await {
        Ok(raw) => doi_sources::attach(&state.store, &request, &raw),
        Err(error) => Err(error),
    };
    if let Err(error) = &result {
        doi_sources::record_failure(&state.store, &request, error)?;
    }
    state.changed(&app);
    result
}
