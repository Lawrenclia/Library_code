use library_core::{now, Failure, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{
    webview::{DownloadEvent, NewWindowResponse},
    AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tokio::sync::oneshot;
use url::Url;

const WOS: &str = include_str!("../../../extension/wos-adapter.js");
const SA: &str = include_str!("../../../extension/adapter.js");
const IMPORT: &str = include_str!("../../../extension/import-adapter.js");
const SCHOLAR: &str = include_str!("../browser/scholar-adapter.cjs");
const DUPLICATE: &str = include_str!("../browser/duplicate-adapter.cjs");
const METADATA: &str = include_str!("../browser/metadata-adapter.cjs");
const LIBRARY: &str = include_str!("../browser/library-adapter.cjs");
#[cfg(feature = "smoke-test")]
pub fn fixture_origin() -> Option<Url> {
    std::env::var("DESKTOP_SMOKE_ORIGIN")
        .ok()
        .and_then(|s| Url::parse(&s).ok())
        .filter(|u| {
            u.scheme() == "http"
                && u.host_str() == Some("127.0.0.1")
                && u.port().is_some()
                && u.path() == "/"
                && u.username().is_empty()
                && u.password().is_none()
        })
}
struct Pending {
    label: String,
    origin: String,
    expires: u64,
    sender: oneshot::Sender<Value>,
}
struct Capture {
    record_url: String,
    url: Option<String>,
    path: Option<PathBuf>,
    sender: oneshot::Sender<Result<PathBuf>>,
}
#[derive(Clone, Default)]
pub struct Browser {
    pending: Arc<Mutex<HashMap<String, Pending>>>,
    capture: Arc<Mutex<Option<Capture>>>,
    #[cfg(feature = "smoke-test")]
    lost_upload_reply: Arc<std::sync::atomic::AtomicBool>,
}
pub fn is_wos(u: &Url) -> bool {
    #[cfg(feature = "smoke-test")]
    if fixture_origin()
        .as_ref()
        .map(|origin| origin.origin() == u.origin())
        .unwrap_or(false)
        && u.path().starts_with("/wos/")
    {
        return true;
    }
    u.scheme() == "https"
        && u.username().is_empty()
        && u.password().is_none()
        && ["webofscience.clarivate.cn", "www.webofscience.com"]
            .contains(&u.host_str().unwrap_or(""))
        && u.port().is_none()
        && u.path().starts_with("/wos/")
}
pub fn core_record(u: &Url) -> bool {
    is_wos(u)
        && regex::Regex::new(r"^/wos/woscc/full-record/WOS:\d{15}(?:\(overlay:export/ext\))?/?$")
            .unwrap()
            .is_match(&u.path().replace("%3A", ":").replace("%3a", ":"))
}
fn valid(label: &str, url: &Url) -> bool {
    #[cfg(feature = "smoke-test")]
    if ["sa", "import", "duplicate", "scholar", "library"].contains(&label)
        && fixture_origin()
            .as_ref()
            .map(|origin| origin.origin() == url.origin())
            .unwrap_or(false)
        && ((label == "library" && url.path() == "/advancedSearch" && url.fragment().is_none())
            || (label != "library"
                && url.path() == format!("/native/{label}")
                && url.fragment()
                    == Some(match label {
                        "sa" => "/dataCompare/list",
                        "import" => "/collectItem/batchManage",
                        "scholar" => "/scholar/list",
                        _ => "/collectItem/duplicateData",
                    })))
    {
        return true;
    }
    match label {
        "wos" => is_wos(url),
        "library" => {
            ["http", "https"].contains(&url.scheme())
                && url.host_str() == Some("www.ir.lib.sjtu.edu.cn")
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
        }
        "sa" | "import" | "scholar" | "duplicate" => {
            ["http", "https"].contains(&url.scheme())
                && url.host_str() == Some("admin.ir.lib.sjtu.edu.cn")
                && url.username().is_empty()
                && url.password().is_none()
        }
        _ => false,
    }
}
impl Browser {
    pub fn states(&self, app: &AppHandle) -> Value {
        let mut data = json!({});
        for role in ["wos", "sa", "import", "scholar", "duplicate", "library"] {
            data[role]=app.get_webview_window(role).map(|w|{let url=w.url().ok();json!({"open":true,"usable":url.as_ref().map(|u|valid(role,u)).unwrap_or(false)})}).unwrap_or(json!({"open":false,"usable":false}));
        }
        data
    }
    pub async fn open(&self, app: &AppHandle, root: &std::path::Path, role: &str) -> Result<()> {
        #[cfg(feature = "smoke-test")]
        let fixture_url = fixture_origin().and_then(|origin| {
            let path = match role {
                "wos" => "/wos/woscc/basic-search",
                "sa" => "/native/sa#/dataCompare/list",
                "import" => "/native/import#/collectItem/batchManage",
                "duplicate" => "/native/duplicate#/collectItem/duplicateData",
                "scholar" => "/native/scholar#/scholar/list",
                "library" => "/advancedSearch",
                _ => return None,
            };
            Some(format!("{}{path}", origin.origin().ascii_serialization()))
        });
        let (url, title) = match role {
            "library" => (
                "http://www.ir.lib.sjtu.edu.cn/advancedSearch",
                "机构库前端 · 成果检索",
            ),
            "wos" => (
                "https://webofscience.clarivate.cn/wos/",
                "WOS · 机构访问与元数据",
            ),
            "sa" => (
                "http://admin.ir.lib.sjtu.edu.cn/#/dataCompare/list",
                "机构库 · SA 比对",
            ),
            "import" => (
                "http://admin.ir.lib.sjtu.edu.cn/#/collectItem/batchManage",
                "机构库 · 导入与批次管理",
            ),
            "scholar" => (
                "http://admin.ir.lib.sjtu.edu.cn/#/scholar/list",
                "机构库 · 学者管理与别名",
            ),
            "duplicate" => (
                "http://admin.ir.lib.sjtu.edu.cn/#/collectItem/duplicateData",
                "机构库 · 重复数据管理",
            ),
            _ => return Err(Failure::new("INVALID_CHANNEL", "未知浏览器通道。")),
        };
        #[cfg(feature = "smoke-test")]
        let url = fixture_url.as_deref().unwrap_or(url);
        if let Some(w) = app.get_webview_window(role) {
            w.show().map_err(Failure::storage)?;
            w.set_focus().map_err(Failure::storage)?;
            return Ok(());
        }
        let profile = root.join("browser").join(if role == "wos" {
            "wos-profile"
        } else {
            "library-profile"
        });
        std::fs::create_dir_all(&profile)?;
        let downloads = root.join("downloads");
        std::fs::create_dir_all(&downloads)?;
        let capture = self.capture.clone();
        let download_app = app.clone();
        let popup_app = app.clone();
        let popup_profile = profile.clone();
        WebviewWindowBuilder::new(app,role,WebviewUrl::External(url.parse().unwrap()))
            .title(title).inner_size(1180.,820.).data_directory(profile)
            .on_navigation(|u|matches!(u.scheme(),"http"|"https"|"about"))
            .on_new_window(move |url,features| {
                #[cfg(feature="smoke-test")]
                eprintln!("Native popup: {}",url);
                if !matches!(url.scheme(),"https"|"http"){return NewWindowResponse::Deny;}
                let label=format!("login-{}",uuid::Uuid::new_v4().simple());
                let child=WebviewWindowBuilder::new(&popup_app,label,WebviewUrl::External("about:blank".parse().unwrap()))
                    .title("机构访问 · 登录窗口").data_directory(popup_profile.clone()).window_features(features)
                    .on_navigation(|u|matches!(u.scheme(),"http"|"https"|"about"));
                match child.build(){Ok(window)=>NewWindowResponse::Create{window},Err(_)=>NewWindowResponse::Deny}
            })
            .on_download(move |view,event| {
                match event {
                    DownloadEvent::Requested{url,destination}=>{
                        #[cfg(feature="smoke-test")]
                        eprintln!("Native download requested: {} {:?}; page {:?}",url,destination,view.url());
                        let current=view.url().ok();
                        if !current.as_ref().map(is_wos).unwrap_or(false){#[cfg(feature="smoke-test")]eprintln!("Download rejected: page origin");return false;}
                        let current=current.unwrap();
                        let download_origin=if url.scheme()=="blob"{Url::parse(&url.as_str()[5..]).ok().map(|u|u.origin())}else{Some(url.origin())};
                        if download_origin!=Some(current.origin())||destination.extension().and_then(|s|s.to_str()).map(|s|!s.eq_ignore_ascii_case("txt")).unwrap_or(true){#[cfg(feature="smoke-test")]eprintln!("Download rejected: origin {:?} vs {:?}, extension {:?}",download_origin,current.origin(),destination.extension());return false;}
                        let mut lock=capture.lock().unwrap();
                        if let Some(c)=lock.as_mut(){
                            let canonical=current.as_str().replace("(overlay:export/ext)","");
                            if canonical.split('#').next()!=c.record_url.split('#').next()||c.path.is_some(){#[cfg(feature="smoke-test")]eprintln!("Download rejected: record {} vs {}",canonical,c.record_url);return false;}
                            *destination=downloads.join(format!("{}.txt",uuid::Uuid::new_v4().simple()));c.path=Some(destination.clone());c.url=Some(url.to_string());
                        }else{*destination=downloads.join(format!("manual-{}.txt",uuid::Uuid::new_v4().simple()));}
                        #[cfg(feature="smoke-test")]eprintln!("Download accepted: {:?}",destination);
                        true
                    },
                    DownloadEvent::Finished{url,path,success}=>{
                        #[cfg(feature="smoke-test")]
                        eprintln!("Native download finished: {} {:?} {}",url,path,success);
                        let mut lock=capture.lock().unwrap();
                        let matched=lock.as_ref().map(|c|c.url.as_deref()==Some(url.as_str())&&c.path.as_deref()==path.as_deref()).unwrap_or(false);
                        if matched {let c=lock.take().unwrap();let result=if success{path.clone().filter(|p|std::fs::metadata(p).map(|m|m.len()>0&&m.len()<=512*1024).unwrap_or(false)).ok_or_else(||Failure::new("FILE_INVALID","下载文件为空、过大或不存在。"))}else{Err(Failure::new("DOWNLOAD_FAILED","原生下载未完成。"))};let _=c.sender.send(result);}
                        use tauri::Emitter;let _=download_app.emit_to("main","download-event",json!({"success":success,"path":path.map(|p|p.to_string_lossy().to_string())}));true
                    },_=>false,
                }
            }).build().map_err(Failure::storage)?;
        Ok(())
    }
    pub fn reply(&self, window: &WebviewWindow, id: &str, result: Value) -> Result<()> {
        if result.to_string().len() > 700000 {
            return Err(Failure::new("REPLY_INVALID", "网页结果过大。"));
        }
        let mut pending = self.pending.lock().unwrap();
        let p = pending
            .get(id)
            .ok_or_else(|| Failure::new("REPLY_EXPIRED", "网页命令已结束。"))?;
        let url = window.url().map_err(Failure::storage)?;
        if window.label() != p.label
            || !valid(window.label(), &url)
            || url.origin().ascii_serialization() != p.origin
            || now() > p.expires
        {
            return Err(Failure::new("REPLY_INVALID", "网页来源与命令不一致。"));
        }
        let p = pending.remove(id).unwrap();
        let _ = p.sender.send(result);
        Ok(())
    }
    pub async fn execute(
        &self,
        app: &AppHandle,
        label: &str,
        action: &str,
        payload: Value,
        timeout: u64,
    ) -> Result<Value> {
        let w = app.get_webview_window(label).ok_or_else(|| {
            Failure::new(
                "BROWSER_DISCONNECTED",
                format!("先打开并登录 {label} 工作页。"),
            )
        })?;
        let url = w.url().map_err(Failure::storage)?;
        if !valid(label, &url) {
            return Err(Failure::new(
                "AUTH_REQUIRED",
                "当前页面仍在登录或已经离开工作区。",
            ));
        }
        let (source, func) = match label {
            "wos" => (WOS, "runWOSCommand"),
            "sa" if action.starts_with("metadata_") => (METADATA, "runMetadataCommand"),
            "sa" => (SA, "runSACommand"),
            "import" => (IMPORT, "runImportCommand"),
            "scholar" => (SCHOLAR, "runScholarCommand"),
            "duplicate" => (DUPLICATE, "runDuplicateCommand"),
            "library" => (LIBRARY, "runLibraryCommand"),
            _ => return Err(Failure::new("INVALID_CHANNEL", "通道错误。")),
        };
        #[cfg(feature = "smoke-test")]
        let fixture_source = fixture_origin()
            .filter(|origin| origin.origin() == url.origin())
            .and_then(|origin| match label {
                "wos" => Some(source.replace(
                    "\"https://www.webofscience.com\",\"https://webofscience.clarivate.cn\"",
                    &serde_json::to_string(&origin.origin().ascii_serialization()).unwrap(),
                )),
                "sa" | "import" | "duplicate" | "scholar" => Some(
                    source
                        .replace("\"admin.ir.lib.sjtu.edu.cn\"", "\"127.0.0.1\"")
                        .replace("'admin.ir.lib.sjtu.edu.cn'", "'127.0.0.1'"),
                ),
                "library" => Some(source.replace("'www.ir.lib.sjtu.edu.cn'", "'127.0.0.1'")),
                _ => None,
            });
        #[cfg(feature = "smoke-test")]
        let source = fixture_source.as_deref().unwrap_or(source);
        let id = uuid::Uuid::new_v4().to_string();
        // The page adapter reserves twelve seconds for reply delivery; its
        // working deadline must also fit the short native result polls.
        let expires = now() + timeout * 1000 + if label == "wos" { 12000 } else { 0 };
        let mut cmd = payload;
        cmd["action"] = action.into();
        cmd["id"] = id.clone().into();
        cmd["expires"] = expires.into();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(
            id.clone(),
            Pending {
                label: label.into(),
                origin: url.origin().ascii_serialization(),
                expires,
                sender: tx,
            },
        );
        let script=format!("{source}\n; (async()=>{{let result;try{{result=await {func}({cmd});}}catch(e){{result={{ok:false,error:String(e.message||e)}};}}await window.__TAURI_INTERNALS__.invoke('browser_result',{{requestId:{},result}});}})();",serde_json::to_string(&id)?);
        if let Err(e) = w.eval(&script) {
            self.pending.lock().unwrap().remove(&id);
            return Err(Failure::new("PAGE_UNSUPPORTED", e.to_string()));
        }
        let result = tokio::time::timeout(Duration::from_secs(timeout), rx).await;
        self.pending.lock().unwrap().remove(&id);
        let response = result
            .map_err(|_| {
                Failure::new(
                    if library_core::is_write(action) {
                        "REMOTE_RESULT_UNKNOWN"
                    } else {
                        "PAGE_TIMEOUT"
                    },
                    "网页结果未返回，请检查工作页。",
                )
            })?
            .map_err(|_| Failure::new("BROWSER_DISCONNECTED", "浏览器命令连接已结束。"))?;
        if response["ok"] != true {
            let mut error = page_error(
                response["error"]
                    .as_str()
                    .unwrap_or("网页没有返回有效结果。"),
            );
            if let Some(code) = response["code"].as_str() {
                error.code = code.to_owned();
            }
            error.submitted = response["submitted"].as_bool();
            return Err(error);
        }
        #[cfg(feature = "smoke-test")]
        if label == "import"
            && action == "import_upload"
            && response["data"]["uploaded"] == true
            && std::env::var("DESKTOP_SMOKE_LOSE_UPLOAD_RESPONSE").as_deref() == Ok("1")
            && fixture_origin().is_some_and(|origin| origin.origin() == url.origin())
            && !self
                .lost_upload_reply
                .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            let mut failure = Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "仅本地原生测试：丢弃一次已成功上传的确认回执。被丢弃的文件上传不得重发。",
            );
            failure.submitted = Some(true);
            return Err(failure);
        }
        Ok(response["data"].clone())
    }
    pub async fn search(&self, app: &AppHandle, payload: Value) -> Result<Value> {
        let w = app
            .get_webview_window("wos")
            .ok_or_else(|| Failure::new("BROWSER_DISCONNECTED", "请打开 WOS 并完成机构访问。"))?;
        let u = w.url().map_err(Failure::storage)?;
        if !is_wos(&u) {
            return Err(Failure::new(
                "AUTH_REQUIRED",
                "请先完成机构访问，返回 WOS 文献页。",
            ));
        }
        let origin = u.origin().ascii_serialization();
        let path = u.path().trim_end_matches('/');
        if ![
            "/wos",
            "/wos/woscc/basic-search",
            "/wos/woscc/advanced-search",
            "/wos/woscc/fielded-search",
        ]
        .contains(&path)
        {
            w.navigate(format!("{origin}/wos/woscc/basic-search").parse().unwrap())
                .map_err(Failure::storage)?;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let end = now() + 120000;
        let mut start_payload = payload.clone();
        start_payload["defer_click"] = true.into();
        let start = self
            .execute(app, "wos", "wos_start_search", start_payload.clone(), 35)
            .await;
        if let Err(e) = start {
            if e.message.contains("保留上一条零结果") {
                w.navigate(format!("{origin}/wos/woscc/basic-search").parse().unwrap())
                    .map_err(Failure::storage)?;
                tokio::time::sleep(Duration::from_secs(2)).await;
                self.execute(app, "wos", "wos_start_search", start_payload, 35)
                    .await?;
            } else {
                return Err(e);
            }
        }
        // The deferred click is issued outside the result callback: navigation can destroy
        // the JavaScript document without destroying the desktop command channel.
        w.eval("if(window.__wosNativeSearch){const p=window.__wosNativeSearch;delete window.__wosNativeSearch;if(p.button?.isConnected)p.button.click();}").map_err(Failure::storage)?;
        let mut navigated = false;
        while now() < end {
            tokio::time::sleep(Duration::from_millis(700)).await;
            let current = w.url().map_err(Failure::storage)?;
            if !is_wos(&current) || current.origin().ascii_serialization() != origin {
                return Err(Failure::new(
                    "AUTH_REQUIRED",
                    "检索已离开 WOS 工作区，请核验机构访问。",
                ));
            }
            let data = match self
                .execute(app, "wos", "wos_read_results", payload.clone(), 5)
                .await
            {
                Ok(d) => d,
                Err(e) if e.code == "PAGE_TIMEOUT" => continue,
                Err(e) => return Err(e),
            };
            match data["state"].as_str() {
                Some("loading") => continue,
                Some("zero") => return Err(Failure::new("NO_RESULT", "本次 WOS 检索未找到记录。")),
                Some("multiple") => {
                    return Err(Failure::new(
                        "AMBIGUOUS_RESULT",
                        "多个结果不能确认唯一目标，请核验。",
                    ))
                }
                Some("record") => {
                    let target = data["record_url"].as_str().and_then(|s| Url::parse(s).ok());
                    if !target.as_ref().map(core_record).unwrap_or(false) {
                        return Err(Failure::new(
                            "IDENTITY_CONFLICT",
                            "没有核实为核心合集记录。",
                        ));
                    }
                    return Ok(data);
                }
                Some("single") => {
                    if navigated {
                        return Err(Failure::new(
                            "PAGE_UNSUPPORTED",
                            "打开目标后仍停在结果列表。",
                        ));
                    }
                    let url = data["navigate_url"]
                        .as_str()
                        .and_then(|s| Url::parse(s).ok())
                        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "目标链接无效。"))?;
                    if !is_wos(&url)
                        || url.origin().ascii_serialization() != origin
                        || !core_record(&url)
                    {
                        return Err(Failure::new(
                            "IDENTITY_CONFLICT",
                            "目标链接不是核心合集的单篇 WOS 记录。",
                        ));
                    }
                    w.navigate(url).map_err(Failure::storage)?;
                    navigated = true;
                }
                _ => return Err(Failure::new("PAGE_UNSUPPORTED", "WOS 返回未知结果状态。")),
            }
        }
        Err(Failure::new(
            "PAGE_TIMEOUT",
            "WOS 结果等待超时，没有重复提交检索。",
        ))
    }
    pub async fn download(&self, app: &AppHandle, payload: Value) -> Result<(PathBuf, String)> {
        let prepared = self
            .execute(app, "wos", "wos_prepare_export", payload.clone(), 35)
            .await?;
        let record = prepared["record_url"]
            .as_str()
            .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "导出未提供记录来源。"))?
            .to_string();
        let (tx, rx) = oneshot::channel();
        {
            let mut cap = self.capture.lock().unwrap();
            if cap.is_some() {
                return Err(Failure::new("BUSY", "已有下载进行中。"));
            }
            *cap = Some(Capture {
                record_url: record.clone(),
                url: None,
                path: None,
                sender: tx,
            });
        }
        if let Err(e) = self.execute(app, "wos", "wos_download", payload, 35).await {
            self.capture.lock().unwrap().take();
            return Err(e);
        }
        let result = tokio::time::timeout(Duration::from_secs(40), rx).await;
        self.capture.lock().unwrap().take();
        let path = result
            .map_err(|_| Failure::new("DOWNLOAD_FAILED", "没有收到完整文件，未采纳其他下载。"))?
            .map_err(|_| Failure::new("BROWSER_DISCONNECTED", "下载通道已结束。"))??;
        Ok((path, record))
    }
}
fn page_error(message: &str) -> Failure {
    let code = if message.contains("未找到记录") {
        "NO_RESULT"
    } else if message.contains("唯一记录") {
        "AMBIGUOUS_RESULT"
    } else if message.contains("登录") {
        "AUTH_REQUIRED"
    } else if message.contains("超时") || message.contains("未完成") {
        "PAGE_TIMEOUT"
    } else if message.contains("冲突") || message.contains("不一致") {
        "IDENTITY_CONFLICT"
    } else {
        "PAGE_UNSUPPORTED"
    };
    Failure::new(code, message)
}
