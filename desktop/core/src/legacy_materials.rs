//! Migrate documented Python material files without promoting old AI suggestions.
use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
const MAX_FILE: u64 = 20 * 1024 * 1024;
const MAX_TOTAL: u64 = 256 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Bundle {
    pub files: Vec<File>,
    pub records: Vec<Material>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct File {
    pub relative: String,
    pub sha256: String,
    pub bytes: u64,
    pub format: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Material {
    pub kind: String,
    pub file: String,
    pub sha256: String,
    pub roster_hash: String,
    pub paper_id: String,
    pub task_ids: Vec<String>,
    pub record: Value,
    pub files: Vec<String>,
    pub warnings: Vec<String>,
}
fn fail(message: impl Into<String>) -> Failure {
    Failure::new("MIGRATION_INVALID", message)
}
fn actual(root: &Path, path: &Path, directory: bool) -> Result<PathBuf> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink()
        || (if directory {
            !meta.is_dir()
        } else {
            !meta.is_file()
        })
    {
        return Err(fail("旧准备资料必须是所选目录内的实际文件/目录。"));
    }
    let path = path.canonicalize()?;
    if !path.starts_with(root) {
        return Err(fail("旧准备资料路径超出所选目录。"));
    }
    Ok(path)
}
fn bytes(root: &Path, path: &Path) -> Result<Vec<u8>> {
    let path = actual(root, path, false)?;
    if std::fs::metadata(&path)?.len() > MAX_FILE {
        return Err(fail("旧材料单文件超过 20 MB。"));
    }
    let raw = std::fs::read(path)?;
    if raw.len() as u64 > MAX_FILE {
        return Err(fail("旧材料超过 20 MB。"));
    }
    Ok(raw)
}
fn add(
    root: &Path,
    path: &Path,
    files: &mut BTreeMap<String, File>,
    total: &mut u64,
) -> Result<File> {
    let path = actual(root, path, false)?;
    let relative = path
        .strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    if let Some(f) = files.get(&relative) {
        return Ok(f.clone());
    }
    let raw = bytes(root, &path)?;
    *total += raw.len() as u64;
    if *total > MAX_TOTAL || files.len() >= 4000 {
        return Err(fail("旧准备资料超过迁移范围限制，请缩小目录。"));
    }
    let format = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["json", "jsonl", "md", "xlsx", "csv", "txt"].contains(&format.as_str()) {
        return Err(fail("未支持的旧材料后缀。"));
    }
    let f = File {
        relative: relative.clone(),
        sha256: hash(&raw),
        bytes: raw.len() as u64,
        format,
    };
    files.insert(relative, f.clone());
    Ok(f)
}
fn json_file(
    root: &Path,
    path: &Path,
    files: &mut BTreeMap<String, File>,
    total: &mut u64,
) -> Result<(File, Value)> {
    let f = add(root, path, files, total)?;
    let raw = bytes(root, path)?;
    if hash(&raw) != f.sha256 {
        return Err(Failure::new("INPUT_CHANGED", "旧 JSON 在读取期间变化。"));
    }
    let value = serde_json::from_slice(&raw).map_err(|_| fail("旧准备 JSON 无法解析，未迁移。"))?;
    Ok((f, value))
}
fn bind(
    rows: &Value,
    title: &str,
    doi: &str,
    paper_id: &str,
    roster_hash: &str,
    expected_hash: &str,
    records: &[Record],
) -> Vec<String> {
    if roster_hash != expected_hash
        || paper_id
            != hash(crate::legacy::python_json(&json!({"doi":doi,"title":title})).as_bytes())
    {
        return vec![];
    }
    let Some(rows) = rows.as_array().filter(|r| !r.is_empty()) else {
        return vec![];
    };
    let mut ids = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for row in rows {
        let Some(row) = row
            .as_u64()
            .filter(|r| *r >= 2 && *r <= u32::MAX as u64 && seen.insert(*r))
        else {
            return vec![];
        };
        let found: Vec<_> = records
            .iter()
            .filter(|r| r.row == row as u32 && r.title == title && r.doi == doi)
            .collect();
        if found.len() != 1 {
            return vec![];
        }
        ids.insert(found[0].sa_id.clone());
    }
    ids.into_iter().collect()
}
fn directories(root: &Path, path: &Path) -> Result<Vec<PathBuf>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let path = actual(root, path, true)?;
    let mut dirs = vec![];
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // Both Python workflows generate a SHA-256 signature directory.
        if name.len() == 64
            && name
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            dirs.push(actual(root, &entry.path(), true)?);
            if dirs.len() > 200 {
                return Err(fail("旧准备批次超过 200 个，请缩小迁移范围。"));
            }
        }
    }
    dirs.sort();
    Ok(dirs)
}
fn output_files(
    root: &Path,
    run: &Path,
    files: &mut BTreeMap<String, File>,
    total: &mut u64,
) -> Result<Vec<String>> {
    let mut found = vec![];
    for folder in ["字段齐备", "待补草稿"] {
        let path = run.join(folder);
        if !path.exists() {
            continue;
        }
        for entry in std::fs::read_dir(actual(root, &path, true)?)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("xlsx"))
            {
                found.push(add(root, &entry.path(), files, total)?.relative);
            }
        }
    }
    found.sort();
    Ok(found)
}
fn reference(
    root: &Path,
    run: &Path,
    value: &str,
    expected: Option<&str>,
    files: &mut BTreeMap<String, File>,
    total: &mut u64,
) -> Result<Option<File>> {
    let p = Path::new(value);
    let candidates = if p.is_absolute() {
        vec![p.to_path_buf()]
    } else {
        vec![root.join(p), run.join(p)]
    };
    let mut resolved = BTreeSet::new();
    for p in candidates {
        if p.exists() {
            resolved.insert(actual(root, &p, false)?);
        }
    }
    if resolved.len() > 1 {
        return Err(fail("旧导出文件相对路径不唯一。"));
    }
    let Some(path) = resolved.into_iter().next() else {
        return Ok(None);
    };
    if !path.extension().is_some_and(|e| {
        ["txt", "xlsx", "csv"].contains(&e.to_string_lossy().to_ascii_lowercase().as_str())
    }) {
        return Err(fail("旧记录引用的导出文件后缀无效。"));
    }
    let file = add(root, &path, files, total)?;
    if expected.is_some_and(|sha| sha != file.sha256) {
        return Err(Failure::new(
            "FILE_CHANGED",
            "旧准备记录中的导出哈希与文件不符。",
        ));
    }
    Ok(Some(file))
}
pub fn inspect(root: &Path, records: &[Record], roster_hash: &str) -> Result<Bundle> {
    let mut bundle = Bundle::default();
    let mut files = BTreeMap::new();
    let mut total = 0;
    for (kind, base, name) in [
        ("classification", "runtime/classification", "分类结果.json"),
        ("submission", "runtime/submission", "提交准备.json"),
    ] {
        for run in directories(root, &root.join(base))? {
            let mut path = run.join(name);
            let checkpoint =
                !path.exists() && kind == "submission" && run.join("progress.json").exists();
            if checkpoint {
                path = run.join("progress.json");
            }
            if !path.exists() {
                let mut count = 0;
                for relative in [
                    "progress.json",
                    "failures.json",
                    "status.json",
                    "events.jsonl",
                ] {
                    let p = run.join(relative);
                    if p.exists() {
                        add(root, &p, &mut files, &mut total)?;
                        count += 1;
                    }
                }
                if kind == "submission" {
                    let p = run.join("零匹配队列.json");
                    if p.exists() {
                        json_file(root, &p, &mut files, &mut total)?;
                        count += 1;
                    }
                    count += output_files(root, &run, &mut files, &mut total)?.len();
                }
                if count > 0 {
                    bundle.warnings.push(format!(
                        "{}：没有完整结果清单，断点文件已归档，未推断任务对应关系。",
                        run.strip_prefix(root).unwrap().to_string_lossy()
                    ));
                }
                continue;
            }
            let (manifest, mut data) = json_file(root, &path, &mut files, &mut total)?;
            if checkpoint {
                let saved = data["records"]
                    .as_object()
                    .ok_or_else(|| fail("旧准备断点缺少 records 对象。"))?;
                let mut items = vec![];
                for (key, item) in saved {
                    if item["id"].as_str() != Some(key.as_str()) {
                        return Err(fail("旧准备断点论文键不一致。"));
                    }
                    items.push(item.clone());
                }
                data["records"] = json!(items);
                bundle.warnings.push(format!(
                    "{}：迁入断点资料，旧批次尚未完成，仍需复核。",
                    manifest.relative
                ));
            }
            let items = data["records"]
                .as_array()
                .ok_or_else(|| fail("旧分类/准备记录缺少 records 数组。"))?;
            if items.len() > 10000 {
                return Err(fail("旧材料记录超过 10000 条。"));
            }
            let context_path = run.join("零匹配队列.json");
            let context = if kind == "submission" && context_path.exists() {
                Some(json_file(root, &context_path, &mut files, &mut total)?.1)
            } else {
                None
            };
            let input = if kind == "classification" {
                data["source"]["sha256"].as_str().unwrap_or("")
            } else {
                context
                    .as_ref()
                    .and_then(|v| v["sha256"].as_str())
                    .unwrap_or("")
            };
            let mut common = vec![manifest.relative.clone()];
            for relative in [
                "progress.json",
                "failures.json",
                "status.json",
                "events.jsonl",
                "分类建议.md",
                "导入渠道分类.md",
                "提交准备与来源.xlsx",
                "待收未匹配.json",
            ] {
                let p = run.join(relative);
                if p.exists() {
                    common.push(add(root, &p, &mut files, &mut total)?.relative);
                }
            }
            if context.is_some() {
                common.push(
                    context_path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
            if kind == "submission" {
                common.extend(output_files(root, &run, &mut files, &mut total)?);
            }
            let mut seen = BTreeSet::new();
            for item in items {
                let id = item["id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| fail("旧材料记录缺少论文 ID。"))?;
                if !seen.insert(id) {
                    return Err(fail("同一旧准备批次包含重复论文 ID。"));
                }
                let title = item["title"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| fail("旧材料题名缺失。"))?;
                let doi = item["doi"]
                    .as_str()
                    .ok_or_else(|| fail("旧材料 DOI 字段类型无效。"))?;
                let mut warnings = vec![];
                if checkpoint {
                    warnings.push("旧准备断点尚未完成；保留已有内容，不能视为可提交文件。".into());
                }
                let mut assets = common.clone();
                let context_matches = kind == "classification"
                    || context
                        .as_ref()
                        .and_then(|v| v["papers"].as_array())
                        .is_some_and(|papers| {
                            papers
                                .iter()
                                .filter(|p| {
                                    p["id"] == id
                                        && p["title"] == title
                                        && p["doi"] == doi
                                        && p["rows"] == item["rows"]
                                })
                                .count()
                                == 1
                        });
                let task_ids = if context_matches {
                    bind(&item["rows"], title, doi, id, input, roster_hash, records)
                } else {
                    vec![]
                };
                if task_ids.is_empty() {
                    warnings
                        .push("名单哈希、论文键、行号或队列不一致/缺失，保留未绑定历史。".into());
                }
                if let Some(export) = item.get("export").filter(|v| v.is_object()) {
                    let original = export["file"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| fail("旧导出记录缺少原文件路径。"))?;
                    let sha = export["sha256"]
                        .as_str()
                        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                        .ok_or_else(|| fail("旧导出记录缺少实际哈希。"))?;
                    match reference(root, &run, original, Some(sha), &mut files, &mut total)? {
                        Some(f) => assets.push(f.relative),
                        None => warnings.push(format!("原始导出文件缺失：{original}")),
                    }
                    if let Some(path) = export["import_file"].as_str().filter(|s| !s.is_empty()) {
                        match reference(root, &run, path, None, &mut files, &mut total)? {
                            Some(f) => assets.push(f.relative),
                            None => warnings.push(format!("旧另存文件缺失：{path}")),
                        }
                    }
                }
                assets.sort();
                assets.dedup();
                for warning in &warnings {
                    bundle
                        .warnings
                        .push(format!("{} / {id}：{warning}", manifest.relative));
                }
                bundle.records.push(Material {
                    kind: kind.into(),
                    file: manifest.relative.clone(),
                    sha256: manifest.sha256.clone(),
                    roster_hash: input.into(),
                    paper_id: id.into(),
                    task_ids,
                    record: item.clone(),
                    files: assets,
                    warnings,
                });
            }
        }
    }
    bundle.files = files.into_values().collect();
    Ok(bundle)
}
pub fn archive(root: &Path, old_root: &Path, bundle: &Bundle) -> Result<BTreeMap<String, Value>> {
    use std::io::Write;
    let target = root.join("legacy-files");
    let mut archived = BTreeMap::new();
    if bundle.files.is_empty() {
        return Ok(archived);
    }
    std::fs::create_dir_all(&target)?;
    let workspace = root.canonicalize()?;
    if std::fs::symlink_metadata(&target)?.file_type().is_symlink()
        || !target.canonicalize()?.starts_with(&workspace)
    {
        return Err(fail("旧材料归档目录已变化。"));
    }
    for f in &bundle.files {
        let raw = bytes(old_root, &old_root.join(&f.relative))?;
        if hash(&raw) != f.sha256 || raw.len() as u64 != f.bytes {
            return Err(Failure::new("INPUT_CHANGED", "旧准备材料在预览后变化。"));
        }
        let path = target.join(format!("{}.{}", f.sha256, f.format));
        if path.exists() {
            if std::fs::symlink_metadata(&path)?.file_type().is_symlink()
                || std::fs::read(&path)? != raw
            {
                return Err(Failure::new(
                    "FILE_CHANGED",
                    "工作台旧材料归档与原文件不符。",
                ));
            }
        } else {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(&raw)?;
            file.sync_all()?;
        }
        archived.insert(f.relative.clone(),json!({"original_file":f.relative,"archive_path":path.to_string_lossy(),"sha256":f.sha256,"bytes":f.bytes,"format":f.format}));
    }
    Ok(archived)
}
pub fn evidence(
    bundle: &Bundle,
    task: &Task,
    migration: &str,
    archived: &BTreeMap<String, Value>,
) -> Result<Vec<Evidence>> {
    bundle.records.iter().filter(|m|m.task_ids.contains(&task.id)).map(|m| {
        let files=m.files.iter().map(|p|archived.get(p).cloned().ok_or_else(||fail("旧材料归档对应关系缺失。"))).collect::<Result<Vec<_>>>()?;
        Ok(Evidence{id:uuid::Uuid::new_v4().to_string(),kind:"legacy_material".into(),source:archived[&m.file]["archive_path"].as_str().unwrap().into(),
            text:json!({"schema":"legacy_material_v1","migration":migration,"kind":m.kind,"original_manifest":m.file,"manifest_sha256":m.sha256,
                "original_roster_hash":m.roster_hash,"bound_record_fingerprint":task.record.fingerprint(),"paper_id":m.paper_id,"record":m.record,
                "files":files,"warnings":m.warnings,"review_required":true,"platform_verified":false}).to_string(),created:now()})
    }).collect()
}

/// Displayable inventory; full original content stays in the immutable archives.
pub fn summary(snapshots: &[Value]) -> Vec<Value> {
    snapshots.iter().filter(|s| s["materials"]["files"].as_array().is_some_and(|f|!f.is_empty())).map(|s| {
        let records = s["materials"]["records"].as_array().map(|items| items.iter().map(|m|json!({
            "kind":m["kind"],"title":m["record"]["title"],"paper_id":m["paper_id"],"task_ids":m["task_ids"],
            "warnings":m["warnings"],"manifest":m["file"]
        })).collect::<Vec<_>>()).unwrap_or_default();
        json!({"root":s["root"],"roster_hash":s["roster_hash"],"records":records,
            "files":s["material_archives"].as_object().map(|m|m.values().cloned().collect::<Vec<_>>()).unwrap_or_default(),
            "warnings":s["materials"]["warnings"],"review_required":true})
    }).collect()
}
