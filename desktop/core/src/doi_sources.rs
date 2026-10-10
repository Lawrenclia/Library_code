//! DOI registry metadata is a factual source, never an institutional import format.
use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, io::Write, path::Path};

pub const KIND: &str = "doi_metadata";
pub const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const MAX_WORK: usize = 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub task_id: String,
    pub task_revision: i64,
    pub input_hash: String,
    pub record_fingerprint: String,
    pub doi: String,
    pub url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub schema: String,
    pub request: Request,
    pub archive_path: String,
    pub sha256: String,
    pub fetched_at: u64,
    pub metadata: Value,
    pub missing: Vec<String>,
    pub matches_input_title: bool,
    pub institution_verified: bool,
}
fn fail(code: &str, message: &str) -> Failure {
    Failure::new(code, message)
}
fn doi(record: &Record) -> Result<String> {
    let value = normalized_doi(&record.doi);
    if value.len() > 1000
        || !regex::Regex::new(r"^10\.[0-9]{4,9}/\S+$")
            .unwrap()
            .is_match(&value)
    {
        return Err(fail(
            "DOI_REQUIRED",
            "名单没有有效 DOI，请先取得原始来源并核对名单；不按题名猜测 DOI。",
        ));
    }
    Ok(value)
}
pub fn endpoint(value: &str) -> Result<String> {
    // DOI remains data, including slashes and reserved characters; no user host.
    let mut url = url::Url::parse("https://api.crossref.org/works/").map_err(Failure::storage)?;
    url.path_segments_mut()
        .map_err(|_| fail("DOI_SOURCE_INVALID", "来源地址无效。"))?
        .pop_if_empty()
        .push(value);
    Ok(url.into())
}
fn eligible(store: &Store, task: &Task) -> Result<()> {
    if task.record.done
        || task.record.skipped
        || matches!(task.stage, Stage::Unknown | Stage::Completed)
        || store.pending_input(&task.id)?.is_some()
        || !store.unresolved(&task.id)?.is_empty()
    {
        return Err(fail(
            "INVALID_TRANSITION",
            "先核对本条名单版本和未确认操作，再补查来源。",
        ));
    }
    Ok(())
}
pub fn request(store: &Store, id: &str) -> Result<Request> {
    let task = store.task(id)?;
    eligible(store, &task)?;
    let value = doi(&task.record)?;
    Ok(Request {
        task_id: task.id.clone(),
        task_revision: task.revision,
        input_hash: task.input_hash.clone(),
        record_fingerprint: task.record.fingerprint(),
        url: endpoint(&value)?,
        doi: value,
    })
}
fn current(task: &Task, request: &Request) -> bool {
    request.task_id == task.id
        && request.input_hash == task.input_hash
        && request.record_fingerprint == task.record.fingerprint()
}
fn work(raw: &[u8], expected_doi: &str) -> Result<Value> {
    if raw.is_empty() || raw.len() > MAX_RESPONSE {
        return Err(fail("DOI_SOURCE_INVALID", "来源响应为空或超过 16 MB。"));
    }
    let envelope: Value = serde_json::from_slice(raw)
        .map_err(|_| fail("DOI_SOURCE_INVALID", "来源响应不是完整 JSON。"))?;
    let metadata = &envelope["message"];
    if envelope["status"] != "ok"
        || envelope["message-type"] != "work"
        || !metadata.is_object()
        || metadata["DOI"].as_str().map(normalized_doi).as_deref() != Some(expected_doi)
        || !metadata["title"].as_array().is_some_and(|a| {
            !a.is_empty()
                && a.iter()
                    .all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
        })
    {
        return Err(fail(
            "DOI_SOURCE_INVALID",
            "没有查到题名完整且 DOI 精确一致的单篇记录，未绑定来源。",
        ));
    }
    if serde_json::to_vec(metadata)?.len() > MAX_WORK {
        return Err(fail(
            "DOI_SOURCE_INVALID",
            "单篇元数据超过 1 MB，未截断或发送给 AI，请改用原始导出。",
        ));
    }
    Ok(metadata.clone())
}
fn missing(metadata: &Value) -> Vec<String> {
    let mut values = Vec::new();
    let authors = metadata["author"].as_array();
    if authors.is_none_or(|a| a.is_empty()) {
        values.push("作者".into());
    }
    if authors.is_none_or(|a| {
        a.is_empty()
            || a.iter().any(|author| {
                !author["affiliation"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty())
            })
    }) {
        values.push("完整作者单位".into());
    }
    for (key, label) in [
        ("container-title", "来源出版物"),
        ("abstract", "摘要"),
        ("published", "出版日期"),
    ] {
        let v = &metadata[key];
        if v.is_null()
            || v.as_str().is_some_and(|s| s.is_empty())
            || v.as_array().is_some_and(|a| a.is_empty())
        {
            values.push(label.into());
        }
    }
    values
}
fn titles_match(task: &Task, metadata: &Value) -> bool {
    metadata["title"].as_array().is_some_and(|titles| {
        titles.iter().any(|t| {
            t.as_str()
                .is_some_and(|s| norm(s) == norm(&task.record.title))
        })
    })
}
fn owned(root: &Path, receipt: &Receipt) -> Result<Vec<u8>> {
    let sha = &receipt.sha256;
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(fail("DOI_SOURCE_INVALID", "来源文件身份无效。"));
    }
    let path = root.join("doi-sources").join(format!("{sha}.json"));
    if !path
        .parent()
        .ok_or_else(|| fail("DOI_SOURCE_INVALID", "来源目录无效。"))?
        .canonicalize()?
        .starts_with(root.canonicalize()?)
    {
        return Err(fail("DOI_SOURCE_INVALID", "来源目录已移到应用目录之外。"));
    }
    let meta = std::fs::symlink_metadata(&path)?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() as usize > MAX_RESPONSE
        || Path::new(&receipt.archive_path).canonicalize()? != path.canonicalize()?
    {
        return Err(fail(
            "DOI_SOURCE_INVALID",
            "来源归档不在应用目录内或已变化。",
        ));
    }
    let raw = std::fs::read(path)?;
    if raw.len() > MAX_RESPONSE || hash(&raw) != *sha {
        return Err(fail("DOI_SOURCE_INVALID", "来源归档哈希变化。"));
    }
    Ok(raw)
}
pub fn verify(root: &Path, task: &Task) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    // Only the newest current response is an AI fact. Earlier versions remain in reports.
    for evidence in task.evidence.iter().rev().filter(|e| e.kind == KIND) {
        let receipt: Receipt = serde_json::from_str(&evidence.text)
            .map_err(|_| fail("DOI_SOURCE_INVALID", "DOI 来源记录不完整。"))?;
        if !current(task, &receipt.request) {
            continue;
        }
        if receipt.schema != "crossref_source_v1"
            || receipt.institution_verified
            || receipt.request.doi != doi(&task.record)?
            || receipt.request.url != endpoint(&receipt.request.doi)?
            || evidence.source != receipt.request.url
        {
            return Err(fail(
                "DOI_SOURCE_INVALID",
                "DOI 来源与当前名单或提供方地址不一致。",
            ));
        }
        let metadata = work(&owned(root, &receipt)?, &receipt.request.doi)?;
        if metadata != receipt.metadata
            || missing(&metadata) != receipt.missing
            || titles_match(task, &metadata) != receipt.matches_input_title
        {
            return Err(fail(
                "DOI_SOURCE_INVALID",
                "来源缓存与完整原响应不一致，不能用于分类或填写。",
            ));
        }
        ids.insert(evidence.id.clone());
        break;
    }
    Ok(ids)
}
pub fn cached(store: &Store, id: &str) -> Result<Option<Value>> {
    let task = store.task(id)?;
    eligible(store, &task)?;
    let ids = verify(&store.root, &task)?;
    Ok(task
        .evidence
        .iter()
        .find(|e| ids.contains(&e.id))
        .map(|e| json!({"evidence":e,"reused":true})))
}
/// Declared current archives for the audit report, without claiming disk verification.
pub fn current_receipts(task: &Task) -> Vec<Receipt> {
    task.evidence
        .iter()
        .filter(|e| e.kind == KIND)
        .filter_map(|e| serde_json::from_str::<Receipt>(&e.text).ok())
        .filter(|r| r.schema == "crossref_source_v1" && current(task, &r.request))
        .collect()
}
pub fn attach(store: &Store, request: &Request, raw: &[u8]) -> Result<Value> {
    let mut task = store.task(&request.task_id)?;
    eligible(store, &task)?;
    if !current(&task, request)
        || request.task_revision != task.revision
        || request.doi != doi(&task.record)?
        || request.url != endpoint(&request.doi)?
    {
        return Err(fail(
            "INPUT_CHANGED",
            "查询期间名单或任务变化，未绑定返回来源。",
        ));
    }
    let metadata = work(raw, &request.doi)?;
    let sha = hash(raw);
    let ids = verify(&store.root, &task)?;
    if let Some(old) = task.evidence.iter().find(|e| ids.contains(&e.id)) {
        let previous: Receipt = serde_json::from_str(&old.text)?;
        if previous.sha256 == sha {
            return Ok(json!({"evidence":old,"reused":true}));
        }
    }
    let folder = store.root.join("doi-sources");
    std::fs::create_dir_all(&folder)?;
    if !folder
        .canonicalize()?
        .starts_with(store.root.canonicalize()?)
    {
        return Err(fail(
            "DOI_SOURCE_INVALID",
            "来源目录已移到应用目录之外，未写入。",
        ));
    }
    let path = folder.join(format!("{sha}.json"));
    if path.exists() {
        let m = std::fs::symlink_metadata(&path)?;
        if !m.is_file() || m.file_type().is_symlink() || std::fs::read(&path)? != raw {
            return Err(fail("DOI_SOURCE_INVALID", "已有来源归档变化，未覆盖。"));
        }
    } else {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(raw)?;
        file.sync_all()?;
    }
    let receipt = Receipt {
        schema: "crossref_source_v1".into(),
        request: request.clone(),
        archive_path: path.to_string_lossy().into(),
        sha256: sha,
        fetched_at: now(),
        missing: missing(&metadata),
        matches_input_title: titles_match(&task, &metadata),
        metadata,
        institution_verified: false,
    };
    let evidence = Evidence {
        id: uuid::Uuid::new_v4().to_string(),
        kind: KIND.into(),
        source: request.url.clone(),
        text: serde_json::to_string(&receipt)?,
        created: now(),
    };
    task.evidence.push(evidence.clone());
    task.classification = None;
    // Both this new source and the complete older source record stay in the report.
    verify(&store.root, &task)?;
    eligible(store, &task)?;
    store.save(&mut task, "doi_source_attached")?;
    Ok(json!({"evidence":evidence,"reused":false,"task_revision":task.revision}))
}
/// A registry miss describes this one lookup, never a conclusion that the paper is absent.
pub fn record_failure(store: &Store, request: &Request, error: &Failure) -> Result<()> {
    let mut task = store.task(&request.task_id)?;
    eligible(store, &task)?;
    if !current(&task, request)
        || task.revision != request.task_revision
        || request.doi != doi(&task.record)?
        || request.url != endpoint(&request.doi)?
    {
        return Err(fail(
            "INPUT_CHANGED",
            "查询期间任务变化，失败结果未归入其他版本。",
        ));
    }
    task.evidence.push(Evidence { id:uuid::Uuid::new_v4().to_string(), kind:"doi_lookup".into(), source:request.url.clone(),
        text:json!({"schema":"doi_lookup_v1","request":request,"error":error,"paper_not_found":false}).to_string(), created:now() });
    store.save(&mut task, "doi_lookup_failed")
}
