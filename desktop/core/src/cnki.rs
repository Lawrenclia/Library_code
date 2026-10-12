//! CNKI tagged bibliographic exports. Raw fields and the full response are retained.
use crate::*;
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Write, path::Path};
use url::Url;

pub const ENTRY: &str = "https://kns.cnki.net/kns8s/defaultresult/index";
pub fn is_site(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.host_str(), Some("kns.cnki.net" | "kns8.cnki.net"))
}
/// Search the roster title in the title field, preserving it as one URL value.
/// Older SU result checkpoints remain readable by select_result.
pub fn title_search(entry: &Url, title: &str) -> Result<Url> {
    if !is_site(entry) || title.trim().is_empty() {
        return Err(Failure::new(
            "INPUT_INVALID",
            "CNKI 官方入口或名单题名无效。",
        ));
    }
    let mut query = entry
        .join("/kns8s/defaultresult/index")
        .map_err(Failure::storage)?;
    query
        .query_pairs_mut()
        .append_pair("kw", title)
        .append_pair("korder", "TI");
    Ok(query)
}
fn fail(message: &str) -> Failure {
    Failure::new("CNKI_EXPORT_INVALID", message)
}

/// A complete, bounded result set is required before automatic selection.
/// The browser and Rust both check scope; a loaded first page is insufficient.
pub fn select_result(task: &Task, data: &Value) -> Result<Url> {
    let invalid = || {
        Failure::new(
            "SOURCE_SELECTION_REQUIRED",
            "CNKI 结果范围无法核对，请在窗口打开确认后的单篇详情。",
        )
    };
    let page = data["page_url"]
        .as_str()
        .and_then(|s| Url::parse(s).ok())
        .ok_or_else(invalid)?;
    let query = page
        .query_pairs()
        .find(|(k, _)| k == "kw")
        .map(|(_, v)| v.into_owned());
    if !is_site(&page)
        || !page.path().to_ascii_lowercase().contains("/defaultresult/")
        || query.as_deref().map(norm) != Some(norm(&task.record.title))
        || data["query"].as_str().map(norm) != Some(norm(&task.record.title))
        || data["complete"] != true
    {
        return Err(invalid());
    }
    let records = data["records"].as_array().ok_or_else(invalid)?;
    if records.len() > 100 || data["total"].as_u64() != Some(records.len() as u64) {
        return Err(invalid());
    }
    if records.is_empty() {
        return Err(Failure::new(
            "SOURCE_NOT_FOUND",
            "本次 CNKI 检索未返回记录；这不代表其他数据库或机构库中不存在。",
        ));
    }
    let mut matches = Vec::new();
    let mut urls = std::collections::BTreeSet::new();
    for record in records {
        let title = record["title"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(invalid)?;
        let url = record["url"]
            .as_str()
            .and_then(|s| Url::parse(s).ok())
            .ok_or_else(invalid)?;
        let path = url.path().to_ascii_lowercase();
        if !is_site(&url)
            || !(path.starts_with("/kcms/") || path.starts_with("/kcms2/"))
            || !urls.insert(url.to_string())
        {
            return Err(invalid());
        }
        if norm(title) == norm(&task.record.title) {
            matches.push(url);
        }
    }
    if matches.len() != 1 {
        return Err(Failure::new(
            "SOURCE_SELECTION_REQUIRED",
            "CNKI 没有唯一的精确题名匹配，请人工核对详情页；不会选择首条或相似题名。",
        ));
    }
    Ok(matches.remove(0))
}

#[derive(Debug)]
pub struct TaggedRecord {
    pub fields: Vec<(String, String)>,
    pub raw: String,
}
impl TaggedRecord {
    pub fn values(&self, tags: &[&str]) -> Vec<&str> {
        self.fields
            .iter()
            .filter(|(t, _)| tags.contains(&t.as_str()))
            .map(|(_, v)| v.as_str())
            .collect()
    }
}
/// Deliberately recognizes explicit record starts, not arbitrary text as a paper.
pub fn parse(text: &str) -> Result<Vec<TaggedRecord>> {
    let tag = regex::Regex::new(r"^(%[^\s]|[A-Z][A-Z0-9])(?:[ \t]+(.*))?$").unwrap();
    let mut records: Vec<TaggedRecord> = Vec::new();
    for original in text.trim_start_matches('\u{feff}').split_inclusive('\n') {
        let line = original.trim_end_matches(['\r', '\n']);
        if let Some(c) = tag.captures(line) {
            let key = c[1].to_string();
            if key == "%0" || key == "RT" {
                records.push(TaggedRecord {
                    fields: vec![],
                    raw: String::new(),
                });
            }
            let Some(record) = records.last_mut() else {
                if line.trim().is_empty() {
                    continue;
                }
                return Err(fail(
                    "TXT 缺少 CNKI EndNote (%0) 或 RefWorks (RT) 记录起始标记。",
                ));
            };
            record
                .fields
                .push((key, c.get(2).map(|v| v.as_str()).unwrap_or("").to_string()));
            record.raw.push_str(original);
        } else if let Some(record) = records.last_mut() {
            record.raw.push_str(original);
            // Keep continuation lines, including empty ones, in the preceding field.
            if let Some((_, value)) = record.fields.last_mut() {
                value.push('\n');
                value.push_str(line);
            }
        } else if !line.trim().is_empty() {
            return Err(fail(
                "TXT 不是带记录起始标记的 CNKI 题录，请选择普通文本或分隔表格。",
            ));
        }
    }
    if records.is_empty() {
        return Err(fail("CNKI 题录为空。"));
    }
    for r in &records {
        if r.values(&["%T", "T1"]).iter().all(|v| v.trim().is_empty()) {
            return Err(fail("CNKI 题录缺少题名，不能采纳。"));
        }
    }
    Ok(records)
}

pub fn rows(text: &str) -> Result<Vec<Vec<String>>> {
    let records = parse(text)?;
    let mut headers: Vec<String> = vec![];
    for r in &records {
        for (key, _) in &r.fields {
            if !headers.contains(key) {
                headers.push(key.clone());
            }
        }
    }
    let mut rows = vec![];
    for r in records {
        let mut values = BTreeMap::<String, Vec<String>>::new();
        for (key, value) in r.fields {
            values.entry(key).or_default().push(value);
        }
        let mut row: Vec<String> = headers
            .iter()
            .map(|h| values.get(h).map(|v| v.join("\n")).unwrap_or_default())
            .collect();
        row.push(r.raw);
        rows.push(row);
    }
    headers.push("原始完整记录".into());
    rows.insert(0, headers);
    Ok(rows)
}

fn archive(root: &Path, folder: &str, raw: &[u8], suffix: &str) -> Result<std::path::PathBuf> {
    let directory = root.join(folder);
    std::fs::create_dir_all(&directory)?;
    if !directory.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(fail("来源目录已被重定向。"));
    }
    let path = directory.join(format!("{}.{}", hash(raw), suffix));
    if path.exists() {
        if std::fs::read(&path)? != raw {
            return Err(fail("来源归档变化，不能覆盖。"));
        }
    } else {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        f.write_all(raw)?;
        f.sync_all()?;
    }
    Ok(path)
}

/// An export response is a separate acquisition mode, never a native DownloadEvent.
pub fn capture(root: &Path, task: &Task, data: &Value) -> Result<(source_files::Draft, Value)> {
    let page = data["page_url"]
        .as_str()
        .ok_or_else(|| fail("缺少详情页来源。"))?;
    let endpoint = data["export_url"]
        .as_str()
        .ok_or_else(|| fail("缺少导出来源。"))?;
    let page_url = Url::parse(page).map_err(|_| fail("详情页地址无效。"))?;
    let api = Url::parse(endpoint).map_err(|_| fail("导出地址无效。"))?;
    if !is_site(&page_url)
        || !is_site(&api)
        || page_url.origin() != api.origin()
        || !api.path().to_ascii_lowercase().ends_with("/getexport")
    {
        return Err(fail(
            "仅支持当前 CNKI 官方详情页提供的同源 GetExport。代理页面可手动导出下载。",
        ));
    }
    if !data["export_id"]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty() && s.len() < 2000)
        || !data["response_text"]
            .as_str()
            .is_some_and(|s| serde_json::from_str::<Value>(s).is_ok())
    {
        return Err(fail("导出编号或原始响应无效。"));
    }
    let text = data["text"]
        .as_str()
        .ok_or_else(|| fail("缺少完整标记题录。"))?;
    let records = parse(text)?;
    if records.len() != 1 {
        return Err(fail(
            "导出包含多篇记录，请先进入单篇详情页；不会自动选第一篇。",
        ));
    }
    let titles = records[0].values(&["%T", "T1"]);
    let dois: Vec<_> = records[0]
        .values(&["%R", "DO", "DI"])
        .iter()
        .map(|d| normalized_doi(d.trim()))
        .filter(|d| !d.is_empty())
        .collect();
    if titles.len() != 1
        || norm(titles[0].trim()) != norm(&task.record.title)
        || (!task.record.doi.is_empty()
            && dois.iter().any(|d| d != &normalized_doi(&task.record.doi)))
    {
        return Err(fail(
            "导出题名与本篇名单不一致或 DOI 冲突，请重新核对详情页。",
        ));
    }
    // Carry the exact response and acquisition identity inside the hash-protected
    // source TXT, so later AI rereads do not depend on a mutable sidecar alone.
    if !data["search"].is_null() {
        let selected = select_result(task, &data["search"])?;
        // Redirects may add language/session parameters, but must retain the
        // selected record's identifiers and path before its response is adopted.
        if selected.origin() != page_url.origin()
            || selected.path() != page_url.path()
            || selected
                .query_pairs()
                .any(|(k, v)| !page_url.query_pairs().any(|(pk, pv)| pk == k && pv == v))
        {
            return Err(fail("详情页已离开检索选中的记录，请重新核对。"));
        }
    }
    let provenance = json!({"mode":"cnki_export_response","page_url":page,"export_url":endpoint,"export_id":data["export_id"],"response_text":data["response_text"],"search":data["search"],"record":task.record,"input_hash":task.input_hash});
    let raw = serde_json::to_vec(&provenance)?;
    if raw.len() > 600_000 {
        return Err(fail("题录响应过大，请缩小导出范围。"));
    }
    let response = archive(root, "source-responses", &raw, "json")?;
    let full = format!(
        "{}\nXR {}\n",
        text.trim_end(),
        serde_json::to_string(&provenance)?
    );
    let saved = archive(root, "downloads/sources", full.as_bytes(), "txt")?;
    let mut draft = source_files::prepare(
        root,
        task,
        "cnki",
        &saved,
        source_files::ReadOptions {
            tagged_format: Some("cnki".into()),
            export_response: true,
            ..Default::default()
        },
    )?;
    draft.original_name = "CNKI-会话题录（供整理填表）.txt".into();
    let missing: Vec<_> = [
        ("作者", vec!["%A", "A1"]),
        ("作者单位", vec!["%+", "AD"]),
        ("出版年份", vec!["%D", "YR"]),
        (
            "来源出版物或出版社",
            vec!["%J", "%B", "%I", "JF", "JO", "T2", "PB"],
        ),
        ("页码或文献号", vec!["%P", "SP", "EP", "AR"]),
        ("DOI（需核对是否存在）", vec!["%R", "DO", "DI"]),
        ("摘要", vec!["%X", "AB"]),
        ("关键词", vec!["%K", "K1"]),
    ]
    .into_iter()
    .filter(|(_, tags)| records[0].values(tags).iter().all(|v| v.trim().is_empty()))
    .map(|(label, _)| label)
    .collect();
    Ok((
        draft,
        json!({"mode":"cnki_export_response","source_url":page,"response_path":response,"response_sha256":hash(&raw),"saved_path":saved,"missing":missing,"search":data["search"]}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn search_task() -> Task {
        Task::new(serde_json::from_value(json!({"row":2,"owner":"","sa_id":"cnki-search","title":"Paper","doi":"","wos":"","staff_id":"001","matches":0,"item_ids":"","mark":"","reason":"","done":false,"skipped":false,"source":""})).unwrap(), "input".into())
    }
    #[test]
    fn roster_search_uses_title_field_and_encodes_title_without_extra_parameters() {
        let title = "题名 & DOI? # A+B";
        for site in [
            "https://kns.cnki.net/old?kw=old&korder=SU",
            "https://kns8.cnki.net/",
        ] {
            let url = title_search(&Url::parse(site).unwrap(), title).unwrap();
            assert_eq!(url.path(), "/kns8s/defaultresult/index");
            assert_eq!(url.query_pairs().count(), 2);
            assert!(url.query_pairs().any(|(k, v)| k == "kw" && v == title));
            assert!(url.query_pairs().any(|(k, v)| k == "korder" && v == "TI"));
            assert_eq!(url.fragment(), None);
        }
        assert!(title_search(&Url::parse(ENTRY).unwrap(), " ").is_err());
        assert!(title_search(
            &Url::parse("https://kns.cnki.net.evil.invalid/").unwrap(),
            title
        )
        .is_err());
    }
    #[test]
    fn automatic_selection_requires_complete_unique_exact_scoped_results() {
        let task = search_task();
        let mut result = json!({"page_url":format!("{}?kw=Paper&korder=SU",ENTRY),"query":"Paper","complete":true,"total":2,"records":[{"title":"Paper","url":"https://kns.cnki.net/kcms2/article/abstract?v=one"},{"title":"Other","url":"https://kns.cnki.net/kcms2/article/abstract?v=two"}]});
        assert!(select_result(&task, &result)
            .unwrap()
            .as_str()
            .ends_with("v=one"));
        result["records"][1]["title"] = json!("Paper");
        assert_eq!(
            select_result(&task, &result).unwrap_err().code,
            "SOURCE_SELECTION_REQUIRED"
        );
        result["records"][1]["title"] = json!("Other");
        result["total"] = json!(20);
        assert!(select_result(&task, &result).is_err());
        result["total"] = json!(2);
        result["page_url"] = json!(format!("{}?kw=Other", ENTRY));
        assert!(select_result(&task, &result).is_err());
        result["page_url"] = json!(format!("{}?kw=Paper", ENTRY));
        result["records"][0]["url"] =
            json!("https://kns.cnki.net.evil.invalid/kcms2/article/abstract?v=one");
        assert!(select_result(&task, &result).is_err());
        result["records"] = json!([]);
        result["total"] = json!(0);
        assert_eq!(
            select_result(&task, &result).unwrap_err().code,
            "SOURCE_NOT_FOUND"
        );
    }
    #[test]
    fn preserves_repeated_authors_unknown_fields_and_multiline_abstracts() {
        let text = "%0 Journal Article\n%T Example\n%A 张三\n%A 李四\n%X first line\nsecond line\n%Z extra field\n";
        let records = parse(text).unwrap();
        assert_eq!(records[0].values(&["%A"]), vec!["张三", "李四"]);
        assert_eq!(records[0].values(&["%X"]), vec!["first line\nsecond line"]);
        assert_eq!(records[0].raw, text);
        let table = rows(text).unwrap();
        let authors = table[0].iter().position(|s| s == "%A").unwrap();
        assert_eq!(table[1][authors], "张三\n李四");
        assert_eq!(table[1].last().unwrap(), text);
    }
    #[test]
    fn separates_refworks_records_and_rejects_short_citations_without_titles() {
        let parsed =
            parse("RT Journal\nT1 One\nA1 张三\n\nRT Journal\nT1 Two\nAB full abstract\n").unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].values(&["T1"]), vec!["Two"]);
        assert!(parse("张三. One. Journal, 2026.").is_err());
        assert!(parse("%0 Journal Article\n%A 张三\n").is_err());
    }
    #[test]
    fn restricts_browser_protocol_to_official_https_hosts() {
        assert!(is_site(&Url::parse(ENTRY).unwrap()));
        for u in [
            "http://kns.cnki.net/",
            "https://kns.cnki.net.evil.invalid/",
            "https://user@kns.cnki.net/",
            "https://kns.cnki.net:8443/",
        ] {
            assert!(!is_site(&Url::parse(u).unwrap()));
        }
    }
}
