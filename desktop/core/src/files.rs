use crate::*;
use calamine::{open_workbook_auto, Data, Reader};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

/// Re-parse the actual archive before recovering any remote import operation.
/// The original immutable attempt, not a newer task or browser draft, owns it.
pub fn read_import_archive(task: &Task, payload: &serde_json::Value) -> Result<Vec<u8>> {
    if payload["sa_id"] != task.id || payload["instructions"] != format!("SA补充-{}", task.id) {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "原导入载荷与当前任务不一致，保持结果未知。",
        ));
    }
    let artifact = task
        .artifact
        .as_ref()
        .ok_or_else(|| Failure::new("FILE_INVALID", "缺少原导入归档，保持结果未知。"))?;
    let path = Path::new(&artifact.path);
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| Failure::new("FILE_INVALID", "原导入归档不可读取，保持结果未知。"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 524288 {
        return Err(Failure::new(
            "FILE_INVALID",
            "原导入归档不是有效单篇文件，保持结果未知。",
        ));
    }
    let raw = std::fs::read(path)
        .map_err(|_| Failure::new("FILE_INVALID", "原导入归档不可读取，保持结果未知。"))?;
    let candidate = parse_wos(&raw)?;
    let actual = serde_json::to_value(&candidate)?;
    if raw.is_empty()
        || raw.len() > 524288
        || actual != serde_json::to_value(&artifact.candidate)?
        || payload["candidate"] != actual
        || (payload.get("contentSha").is_some() && payload["contentSha"] != candidate.sha256)
    {
        return Err(Failure::new(
            "FILE_INVALID",
            "原导入归档、哈希或执行载荷已变化，保持结果未知。",
        ));
    }
    Ok(raw)
}

pub fn read_roster(path: &Path) -> Result<(Vec<Record>, String)> {
    let input_hash = hash(&std::fs::read(path)?);
    if path.extension().and_then(|s| s.to_str()) != Some("xlsx") {
        return Err(Failure::new(
            "INPUT_INVALID",
            "请选择 list.xlsx 格式的名单。",
        ));
    }
    let mut book = open_workbook_auto(path).map_err(Failure::storage)?;
    let choices: Vec<_> = book
        .worksheets()
        .into_iter()
        .filter(|(_, r)| {
            r.rows()
                .next()
                .map(|row| row.iter().any(|x| x.to_string().trim() == "sa_lzk表ID"))
                .unwrap_or(false)
        })
        .collect();
    if choices.len() != 1 {
        return Err(Failure::new(
            "INPUT_INVALID",
            "需恰好有一个含 sa_lzk表ID 的工作表。",
        ));
    }
    let (_, range) = &choices[0];
    let labels: Vec<_> = range
        .rows()
        .next()
        .unwrap()
        .iter()
        .map(|x| x.to_string().trim().to_string())
        .collect();
    let column = |name: &str| -> Result<usize> {
        let a: Vec<_> = labels
            .iter()
            .enumerate()
            .filter(|(_, s)| s.as_str() == name)
            .map(|(i, _)| i)
            .collect();
        if a.len() != 1 {
            Err(Failure::new(
                "INPUT_INVALID",
                format!("缺失或重复表头：{name}"),
            ))
        } else {
            Ok(a[0])
        }
    };
    let keys = [
        "负责人",
        "sa_lzk表ID",
        "题名",
        "DOI",
        "WOS_ID",
        "工号",
        "匹配到的条目数量",
        "平台唯一号",
        "标记状态",
        "标记为待处理原因",
    ];
    let cols: Vec<_> = keys.iter().map(|k| column(k)).collect::<Result<_>>()?;
    let remark_cols: Vec<_> = labels
        .iter()
        .enumerate()
        .filter(|(_, s)| s.as_str() == "备注")
        .map(|(i, _)| i)
        .collect();
    let remark = if remark_cols.len() == 1 {
        Some(remark_cols[0])
    } else if remark_cols.len() == 2
        && remark_cols[0] == 0
        && labels.get(1).map(|s| s.as_str()) == Some("负责人")
    {
        Some(0)
    } else {
        None
    };
    let source = labels.iter().position(|s| s == "数据来源");
    let mut ids = HashSet::new();
    let mut records = vec![];
    for (index, row) in range.rows().enumerate().skip(1) {
        for column in [cols[1], cols[5], cols[7]] {
            let imprecise = match row.get(column) {
                Some(Data::Float(v)) => !v.is_finite() || v.fract() != 0. || v.abs() >= 1e15,
                Some(Data::Int(v)) => v.unsigned_abs() >= 1_000_000_000_000_000,
                _ => false,
            };
            if imprecise {
                return Err(Failure::new(
                    "INPUT_INVALID",
                    format!(
                        "第 {} 行的 {} 为可能丢失精度的数字，请使用原始完整文本编号。",
                        index + 1,
                        labels[column]
                    ),
                ));
            }
        }
        let get = |i: usize| {
            row.get(i)
                .map(|v| v.to_string().trim().to_string())
                .unwrap_or_default()
        };
        let sa_id = get(cols[1]);
        if sa_id.is_empty() {
            if row.iter().all(|x| matches!(x, Data::Empty)) {
                continue;
            }
            return Err(Failure::new(
                "INPUT_INVALID",
                format!("第 {} 行缺少 SA ID。", index + 1),
            ));
        }
        if !ids.insert(sa_id.clone()) {
            return Err(Failure::new(
                "INPUT_INVALID",
                format!("重复 SA ID：{sa_id}"),
            ));
        }
        let matches = get(cols[6]).parse::<u32>().map_err(|_| {
            Failure::new(
                "INPUT_INVALID",
                format!("第 {} 行匹配数不是整数。", index + 1),
            )
        })?;
        if get(cols[0]).is_empty() || get(cols[2]).is_empty() {
            return Err(Failure::new(
                "INPUT_INVALID",
                format!("第 {} 行负责人或题名缺失。", index + 1),
            ));
        }
        let workflow = remark.and_then(|i| row.get(i));
        let numeric = match workflow {
            Some(Data::Int(v)) => Some(*v as f64),
            Some(Data::Float(v)) => Some(*v),
            _ => None,
        };
        records.push(Record {
            row: (index + 1) as u32,
            owner: get(cols[0]),
            sa_id,
            title: get(cols[2]),
            doi: get(cols[3]),
            wos: get(cols[4]),
            staff_id: get(cols[5]),
            matches,
            item_ids: get(cols[7]),
            mark: get(cols[8]),
            reason: get(cols[9]),
            skipped: numeric == Some(2.),
            done: numeric == Some(1.),
            source: source.map(get).unwrap_or_default(),
        });
    }
    if records.is_empty() {
        return Err(Failure::new("INPUT_INVALID", "名单没有任务。"));
    }
    if input_hash != hash(&std::fs::read(path)?) {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "读取期间名单发生变化，请重新导入。",
        ));
    }
    Ok((records, input_hash))
}
pub fn archive_roster(root: &Path, path: &Path, expected_hash: &str) -> Result<String> {
    let raw = std::fs::read(path)?;
    if hash(&raw) != expected_hash {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "名单在读取后变化，未导入任务。",
        ));
    }
    let folder = root.join("inputs");
    std::fs::create_dir_all(&folder)?;
    let target = folder.join(format!("{expected_hash}.xlsx"));
    if target.exists() {
        if std::fs::symlink_metadata(&target)?.file_type().is_symlink()
            || hash(&std::fs::read(&target)?) != expected_hash
        {
            return Err(Failure::new("FILE_CHANGED", "已归档的名单版本发生变化。"));
        }
    } else {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        f.write_all(&raw)?;
        f.sync_all()?;
    }
    Ok(target.to_string_lossy().into())
}

pub fn parse_wos(raw: &[u8]) -> Result<Candidate> {
    if raw.is_empty() || raw.len() > 512 * 1024 {
        return Err(Failure::new("FILE_INVALID", "单篇 TXT 为空或超过 512 KB。"));
    }
    let text = if raw.starts_with(&[0xff, 0xfe]) || raw.starts_with(&[0xfe, 0xff]) {
        if raw.len() % 2 != 0 {
            return Err(Failure::new("FILE_INVALID", "UTF-16 文件长度不合法。"));
        }
        let le = raw[0] == 0xff;
        let units: Vec<_> = raw[2..]
            .chunks_exact(2)
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| Failure::new("FILE_INVALID", "TXT 编码无效。"))?
    } else {
        std::str::from_utf8(raw)
            .map_err(|_| Failure::new("FILE_INVALID", "TXT 编码无效。"))?
            .trim_start_matches('\u{feff}')
            .to_string()
    };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|_| Failure::new("FILE_INVALID", "TXT 表头无效。"))?
        .clone();
    if headers.iter().collect::<HashSet<_>>().len() != headers.len() {
        return Err(Failure::new("FILE_INVALID", "TXT 表头重复。"));
    }
    let rows = reader
        .records()
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Failure::new("FILE_INVALID", "TXT 列数或引号格式错误。"))?;
    if rows.len() != 1 {
        return Err(Failure::new(
            "FILE_INVALID",
            "请选择表头加一篇文献的完整记录制表符导出。",
        ));
    }
    let fields: BTreeMap<_, _> = headers
        .iter()
        .zip(rows[0].iter())
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    for key in ["TI", "AU", "AF", "SO", "PY", "C1", "UT", "DI"] {
        if !fields.contains_key(key) {
            return Err(Failure::new(
                "FILE_INVALID",
                format!("缺少完整记录字段 {key}。"),
            ));
        }
    }
    let get = |k: &str| fields[k].clone();
    for key in ["TI", "AU", "SO", "PY", "UT"] {
        if get(key).is_empty() {
            return Err(Failure::new(
                "INCOMPLETE_METADATA",
                format!("字段 {key} 为空。"),
            ));
        }
    }
    if !regex::Regex::new(r"^(19|20)\d{2}$")
        .unwrap()
        .is_match(&get("PY"))
        || !regex::Regex::new(r"^WOS:\d{15}$")
            .unwrap()
            .is_match(&normalized_wos(&get("UT")))
    {
        return Err(Failure::new("FILE_INVALID", "年份或 WOS ID 不合法。"));
    }
    let doi = normalized_doi(&get("DI"));
    if !doi.is_empty()
        && !regex::Regex::new(r"^10\.\d{4,9}/\S+$")
            .unwrap()
            .is_match(&doi)
    {
        return Err(Failure::new("FILE_INVALID", "DOI 格式无效。"));
    }
    let affiliation = get("C1");
    let sjtu = regex::Regex::new(
        r"(?i)\bShanghai\s+(Jiao\s*Tong|Jiaotong)\s+Univ(ersity)?\b|上海交通大学",
    )
    .unwrap()
    .is_match(&affiliation);
    Ok(Candidate {
        title: get("TI"),
        doi,
        wos: normalized_wos(&get("UT")),
        authors: if get("AF").is_empty() {
            get("AU")
        } else {
            get("AF")
        },
        year: get("PY"),
        journal: get("SO"),
        affiliation,
        sjtu,
        sha256: hash(raw),
        fields,
    })
}
pub fn verify_identity(record: &Record, c: &Candidate) -> Result<bool> {
    let doi = normalized_doi(&record.doi);
    let wos = normalized_wos(&record.wos);
    if (!doi.is_empty() && doi != c.doi) || (!wos.is_empty() && wos != c.wos) {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "文件的 DOI/WOS ID 与名单冲突，未采纳。",
        ));
    }
    Ok((!doi.is_empty() || !wos.is_empty()) && norm(&record.title) == norm(&c.title))
}
pub fn ensure_metadata_evidence(task: &mut Task) -> Result<bool> {
    let Some(artifact) = &task.artifact else {
        return Ok(false);
    };
    let raw = std::fs::read(&artifact.path)?;
    if hash(&raw) != artifact.candidate.sha256 {
        return Err(Failure::new(
            "FILE_INVALID",
            "归档内容已变化，不能作为 AI 或学者署名的来源。",
        ));
    }
    let actual = parse_wos(&raw)?;
    if actual.fields != artifact.candidate.fields {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "缓存元数据与原始文件不一致，请重新核验来源。",
        ));
    }
    let text = serde_json::to_string_pretty(&actual.fields)?;
    if task
        .evidence
        .iter()
        .any(|e| e.kind == "metadata" && e.source == artifact.record_url && e.text == text)
    {
        return Ok(false);
    }
    task.evidence.push(Evidence {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "metadata".into(),
        source: artifact.record_url.clone(),
        text,
        created: now(),
    });
    Ok(true)
}
pub fn archive(root: &Path, raw: &[u8]) -> Result<std::path::PathBuf> {
    use std::io::Write;
    let folder = root.join("artifacts");
    std::fs::create_dir_all(&folder)?;
    let path = folder.join(format!("{}.txt", hash(raw)));
    if path.exists() {
        if std::fs::read(&path)? != raw {
            return Err(Failure::new("FILE_INVALID", "归档文件已变化。"));
        }
    } else {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(raw)?;
        file.sync_all()?;
    }
    Ok(path)
}
pub fn export_report(tasks: &[Task], path: &Path) -> Result<()> {
    export_report_with_queues(tasks, &[], path)
}
fn report_missing(task: &Task) -> String {
    let Some(ai) = &task.classification else {
        return String::new();
    };
    let mut missing: Vec<String> = ai["missing"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if let Some(material) = task
        .evidence
        .iter()
        .rev()
        .filter(|e| e.kind == "material_validation")
        .filter_map(|e| serde_json::from_str::<serde_json::Value>(&e.text).ok())
        .find(|m| {
            m["schema"] == "template_material_v1"
                && m["classification_hash"] == hash(ai.to_string().as_bytes())
                && m["input_hash"] == task.input_hash
                && m["record_fingerprint"] == task.record.fingerprint()
        })
    {
        if let Some(a) = material["validation"]["missing"].as_array() {
            missing.extend(a.iter().filter_map(|v| v.as_str().map(str::to_string)));
        }
        for kind in ["invalid", "requires_review"] {
            if let Some(a) = material["validation"][kind].as_array() {
                missing.extend(a.iter().map(|v| {
                    format!(
                        "{}：{}",
                        v["column"].as_str().unwrap_or("字段"),
                        v["reason"].as_str().unwrap_or("待核对")
                    )
                }));
            }
        }
    }
    serde_json::to_string(&missing).unwrap_or_default()
}
pub fn export_report_with_queues(
    tasks: &[Task],
    queues: &[crate::queue::DownloadQueue],
    path: &Path,
) -> Result<()> {
    export_report_with_history(tasks, queues, &[], path)
}
pub fn export_report_with_history(
    tasks: &[Task],
    queues: &[crate::queue::DownloadQueue],
    legacy: &[serde_json::Value],
    path: &Path,
) -> Result<()> {
    export_report_with_runs(tasks, queues, legacy, &[], path)
}
pub fn export_report_with_runs(
    tasks: &[Task],
    queues: &[crate::queue::DownloadQueue],
    legacy: &[serde_json::Value],
    ai: &[crate::ai_queue::AiQueue],
    path: &Path,
) -> Result<()> {
    export_report_with_materials(tasks, queues, legacy, ai, &[], path)
}
pub fn export_report_with_materials(
    tasks: &[Task],
    queues: &[crate::queue::DownloadQueue],
    legacy: &[serde_json::Value],
    ai: &[crate::ai_queue::AiQueue],
    materials: &[crate::material_batch::Batch],
    path: &Path,
) -> Result<()> {
    use rust_xlsxwriter::Workbook;
    let mut book = Workbook::new();
    let sheet = book.add_worksheet();
    sheet.set_name("任务与来源").map_err(Failure::storage)?;
    let headers = [
        "原表行号",
        "负责人",
        "SA ID",
        "原题名",
        "核实题名",
        "成果类型",
        "渠道",
        "来源链接",
        "原始文件",
        "哈希",
        "缺项",
        "业务分支",
        "任务阶段",
        "平台唯一号",
        "批次",
        "失败原因",
        "核验结论",
        "AI 模型置信度",
        "AI 分类理由",
        "渠道建议与确认条件",
        "AI 建议状态",
    ];
    for (i, h) in headers.iter().enumerate() {
        sheet
            .write_string(0, i as u16, *h)
            .map_err(Failure::storage)?;
    }
    for (row, t) in tasks.iter().enumerate() {
        let a = t.artifact.as_ref();
        let originals = crate::source_files::current_receipts(t);
        let original_pairs = originals
            .iter()
            .map(|s| (s.archive_path.clone(), s.sha256.clone()))
            .chain(a.map(|a| (a.path.clone(), a.candidate.sha256.clone())))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let original_paths = original_pairs
            .iter()
            .map(|p| p.0.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let original_hashes = original_pairs
            .iter()
            .map(|p| p.1.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let ai = t.classification.as_ref();
        let fields = vec![
            t.record.row.to_string(),
            t.record.owner.clone(),
            t.id.clone(),
            t.record.title.clone(),
            a.map(|a| a.candidate.title.clone()).unwrap_or_default(),
            ai.and_then(|v| v["type"].as_str()).unwrap_or("").into(),
            ai.and_then(|v| v["channel"].as_str()).unwrap_or("").into(),
            t.evidence
                .iter()
                .map(|e| e.source.as_str())
                .filter(|s| !s.is_empty())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join("\n"),
            original_paths,
            original_hashes,
            report_missing(t),
            serde_json::to_value(&t.route)?.as_str().unwrap().into(),
            serde_json::to_value(&t.stage)?.as_str().unwrap().into(),
            t.platform_id.clone(),
            t.batch.as_ref().map(|v| v.to_string()).unwrap_or_default(),
            t.last_error
                .as_ref()
                .map(|e| format!("{}: {}", e.code, e.message))
                .unwrap_or_default(),
            t.review
                .as_ref()
                .map(|r| r.note.clone())
                .unwrap_or_default(),
            ai.and_then(|v| v["confidence"].as_str())
                .unwrap_or("未记录")
                .into(),
            ai.and_then(|v| v["reason"].as_str()).unwrap_or("").into(),
            ai.and_then(|v| v["channel_reason"].as_str())
                .unwrap_or("")
                .into(),
            if ai.is_some() {
                "建议待复核"
            } else {
                "未分类"
            }
            .into(),
        ];
        for (col, v) in fields.iter().enumerate() {
            sheet
                .write_string((row + 1) as u32, col as u16, v)
                .map_err(Failure::storage)?;
        }
    }
    sheet.set_freeze_panes(1, 0).map_err(Failure::storage)?;
    sheet.set_column_width(3, 55.).map_err(Failure::storage)?;
    let sources = book.add_worksheet();
    sources.set_name("原始来源依据").map_err(Failure::storage)?;
    for (col, label) in [
        "SA ID",
        "论文 ID",
        "来源 ID",
        "类型",
        "来源链接或材料",
        "取得时间",
        "依据片段",
        "片段序号",
        "完整依据哈希",
    ]
    .iter()
    .enumerate()
    {
        sources
            .write_string(0, col as u16, *label)
            .map_err(Failure::storage)?;
    }
    let mut row = 1;
    for t in tasks {
        for e in &t.evidence {
            let chars: Vec<char> = e.text.chars().collect();
            let chunks: Vec<String> = if chars.is_empty() {
                vec![String::new()]
            } else {
                chars.chunks(15000).map(|s| s.iter().collect()).collect()
            };
            for (index, chunk) in chunks.into_iter().enumerate() {
                let values = [
                    t.id.clone(),
                    t.paper_id.clone(),
                    e.id.clone(),
                    e.kind.clone(),
                    e.source.clone(),
                    e.created.to_string(),
                    chunk,
                    (index + 1).to_string(),
                    hash(e.text.as_bytes()),
                ];
                for (col, value) in values.iter().enumerate() {
                    sources
                        .write_string(row, col as u16, value)
                        .map_err(Failure::storage)?;
                }
                row += 1;
            }
        }
    }
    sources.set_freeze_panes(1, 0).map_err(Failure::storage)?;
    sources.set_column_width(6, 80.).map_err(Failure::storage)?;
    let fields = book.add_worksheet();
    fields.set_name("AI 字段来源").map_err(Failure::storage)?;
    for (col, label) in [
        "SA ID",
        "模板 ID",
        "字段名",
        "填写值",
        "引用来源 ID",
        "片段序号",
        "完整填写值 SHA256",
    ]
    .iter()
    .enumerate()
    {
        fields
            .write_string(0, col as u16, *label)
            .map_err(Failure::storage)?;
    }
    let mut row = 1;
    for t in tasks {
        if let Some(ai) = &t.classification {
            if let Some(values) = ai["fields"].as_object() {
                for (name, field) in values {
                    let text = field["value"].as_str().unwrap_or("");
                    let chars: Vec<_> = text.chars().collect();
                    let chunks = if chars.is_empty() {
                        vec![String::new()]
                    } else {
                        chars
                            .chunks(15000)
                            .map(|c| c.iter().collect())
                            .collect::<Vec<String>>()
                    };
                    for (part, chunk) in chunks.into_iter().enumerate() {
                        let values = [
                            t.id.clone(),
                            ai["template_id"].as_str().unwrap_or("").into(),
                            name.clone(),
                            chunk,
                            field["evidence_ids"].to_string(),
                            (part + 1).to_string(),
                            hash(text.as_bytes()),
                        ];
                        for (col, value) in values.iter().enumerate() {
                            fields
                                .write_string(row, col as u16, value)
                                .map_err(Failure::storage)?;
                        }
                        row += 1;
                    }
                }
            }
        }
    }
    fields.set_freeze_panes(1, 0).map_err(Failure::storage)?;
    let history = book.add_worksheet();
    history.set_name("下载队列结果").map_err(Failure::storage)?;
    for (col, label) in [
        "队列 ID",
        "原负责人",
        "原范围",
        "队列状态",
        "原顺序",
        "SA ID",
        "原题名",
        "原输入版本哈希",
        "原记录指纹",
        "逐篇结果",
        "错误代码",
        "记录完成时间",
        "队列建立时间",
        "队列更新时间",
        "完整记录片段",
        "片段序号",
        "完整记录哈希",
    ]
    .iter()
    .enumerate()
    {
        history
            .write_string(0, col as u16, *label)
            .map_err(Failure::storage)?;
    }
    let mut row = 1;
    for queue in queues {
        for (index, target) in queue.targets.iter().enumerate() {
            let outcome = queue.outcomes.iter().find(|o| o.id == target.id);
            let error = outcome.and_then(|o| o.error.as_ref()).or_else(|| {
                if index == queue.cursor {
                    queue.last_error.as_ref()
                } else {
                    None
                }
            });
            let complete = serde_json::to_string(
                &serde_json::json!({"target":target,"outcome":outcome,"current_error":error}),
            )?;
            let chars = complete.chars().collect::<Vec<_>>();
            for (part, chunk) in chars.chunks(15000).enumerate() {
                let values = [
                    queue.id.clone(),
                    queue.owner.clone(),
                    if queue.retry_skipped {
                        "跳过论文"
                    } else {
                        "待补论文"
                    }
                    .into(),
                    serde_json::to_value(&queue.status)?
                        .as_str()
                        .unwrap()
                        .into(),
                    (index + 1).to_string(),
                    target.id.clone(),
                    target
                        .record
                        .as_ref()
                        .map(|r| r.title.clone())
                        .unwrap_or_default(),
                    target.input_hash.clone(),
                    target.fingerprint.clone(),
                    outcome
                        .map(|o| o.status.as_str())
                        .unwrap_or("not_executed")
                        .into(),
                    error.map(|e| e.code.clone()).unwrap_or_default(),
                    outcome.map(|o| o.finished.to_string()).unwrap_or_default(),
                    queue.created.to_string(),
                    queue.updated.to_string(),
                    chunk.iter().collect(),
                    (part + 1).to_string(),
                    hash(complete.as_bytes()),
                ];
                for (col, value) in values.iter().enumerate() {
                    history
                        .write_string(row, col as u16, value)
                        .map_err(Failure::storage)?;
                }
                row += 1;
            }
        }
    }
    history.set_freeze_panes(1, 0).map_err(Failure::storage)?;
    history.set_column_width(6, 50.).map_err(Failure::storage)?;
    history
        .set_column_width(14, 80.)
        .map_err(Failure::storage)?;
    if !legacy.is_empty() {
        let audit = book.add_worksheet();
        audit.set_name("旧版资料归档").map_err(Failure::storage)?;
        for (col, label) in [
            "迁移批次",
            "旧版目录",
            "名单哈希",
            "完整迁移记录 SHA256",
            "分段序号",
            "完整迁移记录 JSON（按序连接）",
        ]
        .iter()
        .enumerate()
        {
            audit
                .write_string(0, col as u16, *label)
                .map_err(Failure::storage)?;
        }
        let mut row = 1;
        for (index, snapshot) in legacy.iter().enumerate() {
            let raw = serde_json::to_string(snapshot)?;
            // Unicode scalar chunks keep each cell below Excel's UTF-16 limit.
            let chars: Vec<_> = raw.chars().collect();
            for (part, chunk) in chars.chunks(15000).enumerate() {
                let values = [
                    (index + 1).to_string(),
                    snapshot["root"].as_str().unwrap_or("").into(),
                    snapshot["roster_hash"].as_str().unwrap_or("").into(),
                    hash(raw.as_bytes()),
                    (part + 1).to_string(),
                    chunk.iter().collect(),
                ];
                for (col, value) in values.iter().enumerate() {
                    audit
                        .write_string(row, col as u16, value)
                        .map_err(Failure::storage)?;
                }
                row += 1;
            }
        }
        audit.set_freeze_panes(1, 0).map_err(Failure::storage)?;
        audit.set_column_width(5, 80.).map_err(Failure::storage)?;
    }
    if !ai.is_empty() {
        let sheet = book.add_worksheet();
        sheet.set_name("AI 批量结果").map_err(Failure::storage)?;
        for (col, label) in [
            "队列 ID",
            "原负责人",
            "范围",
            "模型",
            "状态",
            "已记录",
            "原总数",
            "完整记录 SHA256",
            "片段序号",
            "完整记录 JSON（按序连接）",
        ]
        .iter()
        .enumerate()
        {
            sheet
                .write_string(0, col as u16, *label)
                .map_err(Failure::storage)?;
        }
        let mut row = 1;
        for q in ai {
            let raw = serde_json::to_string(q)?;
            let digest = hash(raw.as_bytes());
            let chars: Vec<_> = raw.chars().collect();
            for (part, chunk) in chars.chunks(15000).enumerate() {
                let values = [
                    q.id.clone(),
                    q.owner.clone(),
                    if q.zero_only { "zero" } else { "all" }.into(),
                    q.config["model"].as_str().unwrap_or("").into(),
                    serde_json::to_value(&q.status)?.as_str().unwrap().into(),
                    q.cursor.to_string(),
                    q.targets.len().to_string(),
                    digest.clone(),
                    (part + 1).to_string(),
                    chunk.iter().collect(),
                ];
                for (col, value) in values.iter().enumerate() {
                    sheet
                        .write_string(row, col as u16, value)
                        .map_err(Failure::storage)?;
                }
                row += 1;
            }
        }
        sheet.set_freeze_panes(1, 0).map_err(Failure::storage)?;
        sheet.set_column_width(9, 80.).map_err(Failure::storage)?;
    }
    if !materials.is_empty() {
        let index = book.add_worksheet();
        index.set_name("材料文件索引").map_err(Failure::storage)?;
        for (column, label) in [
            "原范围 ID",
            "负责人",
            "SA ID",
            "原表行号",
            "题名",
            "材料类型",
            "材料路径",
            "文件 SHA256",
            "来源回执",
            "必填缺项",
            "格式问题",
            "待核对要求",
            "失败原因",
            "原文件/模板来源",
            "是否复用",
        ]
        .iter()
        .enumerate()
        {
            index
                .write_string(0, column as u16, *label)
                .map_err(Failure::storage)?;
        }
        let mut row = 1;
        for batch in materials {
            for result in &batch.outcomes {
                let target = batch.targets.iter().find(|t| t.id == result.id);
                let p = result.product.as_ref();
                let list = |key: &str| p.map(|p| p.validation[key].to_string()).unwrap_or_default();
                let source = target
                    .and_then(|t| t.template.as_ref())
                    .map(|t| format!("{} / {} / {}", t.name, t.path, t.fingerprint))
                    .unwrap_or_else(|| {
                        target
                            .and_then(|t| t.artifact.as_ref())
                            .map(|a| format!("{} / {}", a.source, a.record_url))
                            .unwrap_or_default()
                    });
                for (column, value) in [
                    batch.id.clone(),
                    batch.owner.clone(),
                    result.id.clone(),
                    target.map(|t| t.record.row.to_string()).unwrap_or_default(),
                    target.map(|t| t.record.title.clone()).unwrap_or_default(),
                    p.map(|p| {
                        match p.kind.as_str() {
                            "original" => "原始数据库导出",
                            "field_valid" => "模板字段校验通过",
                            _ => "草稿待核对",
                        }
                        .to_string()
                    })
                    .unwrap_or_default(),
                    p.map(|p| p.path.clone()).unwrap_or_default(),
                    p.map(|p| p.sha256.clone()).unwrap_or_default(),
                    p.map(|p| p.audit.clone()).unwrap_or_default(),
                    list("missing"),
                    list("invalid"),
                    list("requires_review"),
                    result
                        .error
                        .as_ref()
                        .map(|e| format!("{}: {}", e.code, e.message))
                        .unwrap_or_default(),
                    source,
                    p.map(|p| p.reused.to_string()).unwrap_or_default(),
                ]
                .iter()
                .enumerate()
                {
                    // Full values remain in the chunked receipt sheet below.
                    index
                        .write_string(
                            row,
                            column as u16,
                            value.chars().take(15000).collect::<String>(),
                        )
                        .map_err(Failure::storage)?;
                }
                row += 1;
            }
        }
        index.set_freeze_panes(1, 0).map_err(Failure::storage)?;
        for col in [4, 6, 8, 13] {
            index.set_column_width(col, 50.).map_err(Failure::storage)?;
        }
        let sheet = book.add_worksheet();
        sheet.set_name("材料整理结果").map_err(Failure::storage)?;
        for (column, label) in [
            "原范围 ID",
            "负责人",
            "状态",
            "已记录",
            "总数",
            "完整记录 SHA256",
            "片段序号",
            "原范围与完整结果 JSON",
        ]
        .iter()
        .enumerate()
        {
            sheet
                .write_string(0, column as u16, *label)
                .map_err(Failure::storage)?;
        }
        let mut row = 1;
        for batch in materials {
            let raw = serde_json::to_string(batch)?;
            let digest = hash(raw.as_bytes());
            let chars: Vec<_> = raw.chars().collect();
            for (part, chunk) in chars.chunks(15000).enumerate() {
                for (column, value) in [
                    batch.id.clone(),
                    batch.owner.clone(),
                    serde_json::to_value(&batch.status)?
                        .as_str()
                        .unwrap()
                        .into(),
                    batch.cursor.to_string(),
                    batch.targets.len().to_string(),
                    digest.clone(),
                    (part + 1).to_string(),
                    chunk.iter().collect(),
                ]
                .iter()
                .enumerate()
                {
                    sheet
                        .write_string(row, column as u16, value)
                        .map_err(Failure::storage)?;
                }
                row += 1;
            }
        }
        sheet.set_freeze_panes(1, 0).map_err(Failure::storage)?;
        sheet.set_column_width(7, 80.).map_err(Failure::storage)?;
    }
    book.save(path).map_err(Failure::storage)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn rec() -> Record {
        Record {
            row: 2,
            owner: "测试".into(),
            sa_id: "sa1".into(),
            title: "Paper".into(),
            doi: "10.1234/test".into(),
            wos: "".into(),
            staff_id: "01".into(),
            matches: 0,
            item_ids: "".into(),
            mark: "待处理".into(),
            reason: "".into(),
            skipped: false,
            done: false,
            source: "".into(),
        }
    }
    fn raw() -> Vec<u8> {
        b"TI\tAU\tAF\tSO\tPY\tC1\tUT\tDI\nPaper\tLi X\tLi, X\tJournal\t2026\tShanghai Jiao Tong University\tWOS:000123456789012\t10.1234/test\n".to_vec()
    }
    #[test]
    fn import_recovery_reparses_original_archive_and_immutable_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = archive(dir.path(), &raw()).unwrap();
        let mut task = Task::new(rec(), "input".into());
        task.artifact = Some(Artifact {
            path: path.to_string_lossy().into(),
            source: "WOS".into(),
            record_url: "source".into(),
            downloaded: 1,
            candidate: parse_wos(&raw()).unwrap(),
            identity_confirmed: true,
        });
        let payload = serde_json::json!({"sa_id":task.id,"instructions":format!("SA补充-{}",task.id),
            "candidate":task.artifact.as_ref().unwrap().candidate,"contentSha":hash(&raw())});
        assert_eq!(read_import_archive(&task, &payload).unwrap(), raw());
        for (key, value) in [
            ("sa_id", "other"),
            ("instructions", "SA补充-other"),
            ("contentSha", "wrong"),
        ] {
            let mut wrong = payload.clone();
            wrong[key] = value.into();
            assert!(read_import_archive(&task, &wrong).is_err());
        }
        let mut wrong = payload.clone();
        wrong["candidate"]["fields"]["AF"] = "Other author".into();
        assert!(read_import_archive(&task, &wrong).is_err());
        // An original imported/pushed payload has no upload-only contentSha.
        let mut batch = payload.clone();
        batch.as_object_mut().unwrap().remove("contentSha");
        assert_eq!(read_import_archive(&task, &batch).unwrap(), raw());
        let changed = String::from_utf8(raw())
            .unwrap()
            .replace("Journal", "Different journal");
        std::fs::write(&path, changed).unwrap();
        assert_eq!(
            read_import_archive(&task, &batch).unwrap_err().code,
            "FILE_INVALID"
        );
        // Changing the persisted artifact to match new bytes cannot rewrite the
        // original immutable attempt's candidate.
        task.artifact.as_mut().unwrap().candidate =
            parse_wos(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            read_import_archive(&task, &batch).unwrap_err().code,
            "FILE_INVALID"
        );
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            read_import_archive(&task, &batch).unwrap_err().code,
            "FILE_INVALID"
        );
    }
    #[test]
    fn original_file_and_identity() {
        let c = parse_wos(&raw()).unwrap();
        assert!(c.sjtu);
        assert!(verify_identity(&rec(), &c).unwrap());
        let mut r = rec();
        r.doi = "10.1234/other".into();
        assert_eq!(
            verify_identity(&r, &c).unwrap_err().code,
            "IDENTITY_CONFLICT"
        );
    }
    #[test]
    fn old_sources_restore_all_original_fields_and_refuse_changed_archives() {
        let dir = tempfile::tempdir().unwrap();
        let path = archive(dir.path(), &raw()).unwrap();
        let mut task = Task::new(rec(), "input".into());
        task.artifact = Some(Artifact {
            path: path.to_string_lossy().into(),
            source: "WOS".into(),
            record_url: "https://source/record".into(),
            downloaded: 1,
            candidate: parse_wos(&raw()).unwrap(),
            identity_confirmed: true,
        });
        task.evidence.push(Evidence {
            id: "old".into(),
            kind: "metadata".into(),
            source: "https://source/record".into(),
            text: "Paper\nWOS:000123456789012\nShanghai Jiao Tong University".into(),
            created: 0,
        });
        assert!(ensure_metadata_evidence(&mut task).unwrap());
        let source = task.evidence.last().unwrap();
        let fields: serde_json::Value = serde_json::from_str(&source.text).unwrap();
        assert_eq!(fields["AF"], "Li, X");
        assert_eq!(fields["DI"], "10.1234/test");
        assert_eq!(fields["PY"], "2026");
        assert_eq!(task.evidence[0].id, "old");
        assert!(!ensure_metadata_evidence(&mut task).unwrap());
        validate_alias_source(&task, "Li, X", &task.evidence.last().unwrap().id).unwrap();
        std::fs::write(path, b"changed").unwrap();
        assert_eq!(
            ensure_metadata_evidence(&mut task).unwrap_err().code,
            "FILE_INVALID"
        );
    }
    #[test]
    fn multirecord_rejected() {
        let mut a = raw();
        let row = raw().split(|x| *x == b'\n').nth(1).unwrap().to_vec();
        a.extend(row);
        a.push(b'\n');
        assert_eq!(parse_wos(&a).unwrap_err().code, "FILE_INVALID");
    }
    #[test]
    fn upload_requires_library_absence() {
        let mut t = Task::new(rec(), "input".into());
        let c = parse_wos(&raw()).unwrap();
        t.artifact = Some(Artifact {
            path: "file".into(),
            source: "WOS".into(),
            record_url: "source".into(),
            downloaded: 0,
            candidate: c,
            identity_confirmed: true,
        });
        t.evidence.push(Evidence {
            id: "e".into(),
            kind: "library_search".into(),
            source: "source".into(),
            text: "查无".into(),
            created: 0,
        });
        let mut r = Review {
            route: Route::Missing,
            evidence_id: "e".into(),
            library_checked: false,
            platform_id: "".into(),
            affiliation_confirmed: true,
            identity_confirmed: true,
            issues_resolved: false,
            note: "核对".into(),
        };
        assert!(t.validate_review(&r).is_err());
        r.library_checked = true;
        assert!(
            t.validate_review(&r).is_err(),
            "manual absence text is not a verified query"
        );
        let target =
            crate::library::target(&t, &t.artifact.as_ref().unwrap().candidate.title).unwrap();
        let queries: Vec<_> = ["title", "doi", "wos"]
            .into_iter()
            .filter(|k| !target[k].as_str().unwrap_or("").is_empty())
            .map(|k| {
                json!({"kind":k,"field":k,"value":target[k],"precise":k!="title","total":0,
                "pages":[{"current":1,"size":10,"total":0,"records":[]}]})
            })
            .collect();
        let result = json!({"verified":true,"sa_id":t.id,"source":"http://www.ir.lib.sjtu.edu.cn/advancedSearch",
            "institution_id":"1244586319225556993","target":target,"queries":queries,"items":[],"checked_at":crate::now()});
        crate::library::record(&mut t, &target, &result).unwrap();
        t.validate_review(&r).unwrap();
        t.route = Route::Missing;
        t.review = Some(r);
        t.import_ready().unwrap();
        assert!(t.assert_complete().is_err());
    }
    #[test]
    fn restart_blocks_duplicate_write() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path()).unwrap();
        s.import(vec![rec()], "input".into()).unwrap();
        let mut t = s.task("sa1").unwrap();
        s.begin_attempt(&mut t, "import_submit").unwrap();
        s.recover().unwrap();
        let mut restored = s.task("sa1").unwrap();
        assert_eq!(restored.stage, Stage::Unknown);
        assert!(s.begin_attempt(&mut restored, "import_submit").is_err());
    }
    #[test]
    fn recovery_keeps_write_payload_and_original_response() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        store.import(vec![rec()], "input".into()).unwrap();
        let mut task = store.task("sa1").unwrap();
        let attempt = store.begin_attempt(&mut task, "add_alias").unwrap();
        let payload = json!({"staff_id":"001","alias":"Li, X","previous_stage":"downloaded"});
        store.attempt_payload(&attempt, payload.clone()).unwrap();
        store
            .end_attempt(&attempt, "unknown", json!({"message":"timeout"}))
            .unwrap();
        let reopened = Store::new(dir.path()).unwrap();
        reopened.recover().unwrap();
        assert_eq!(reopened.task("sa1").unwrap().stage, Stage::Unknown);
        reopened
            .resolve_actions("sa1", &["import_submit"], json!({"verified":true}))
            .unwrap();
        assert_eq!(
            reopened.unresolved("sa1").unwrap().len(),
            1,
            "an unrelated readback cannot resolve an alias write"
        );
        reopened
            .resolve_actions("sa1", &["add_alias"], json!({"verified":true}))
            .unwrap();
        let db = rusqlite::Connection::open(dir.path().join("workspace.sqlite3")).unwrap();
        let raw: String = db
            .query_row("SELECT data FROM attempts WHERE id=?", [attempt], |r| {
                r.get(0)
            })
            .unwrap();
        let audit: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(audit["payload"], payload);
        assert_eq!(audit["response"]["message"], "timeout");
        assert_eq!(audit["verification"]["verified"], true);
    }
    #[test]
    fn source_update_preserves_state() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path()).unwrap();
        s.import(vec![rec()], "input".into()).unwrap();
        let mut t = s.task("sa1").unwrap();
        t.stage = Stage::Downloaded;
        s.save(&mut t, "downloaded").unwrap();
        let mut r = rec();
        r.source = "WOS".into();
        s.import(vec![r], "changedfile".into()).unwrap();
        assert_eq!(s.task("sa1").unwrap().stage, Stage::Downloaded);
    }
    #[test]
    fn stale_revision_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path()).unwrap();
        s.import(vec![rec()], "input".into()).unwrap();
        let mut a = s.task("sa1").unwrap();
        let mut b = a.clone();
        s.save(&mut a, "review").unwrap();
        assert_eq!(s.save(&mut b, "review").unwrap_err().code, "TASK_CHANGED");
    }
    #[test]
    fn not_found_never_completes() {
        let mut t = Task::new(rec(), "input".into());
        t.route = Route::NotFound;
        t.review = Some(Review {
            route: Route::NotFound,
            evidence_id: "e".into(),
            library_checked: true,
            platform_id: "".into(),
            affiliation_confirmed: false,
            identity_confirmed: false,
            issues_resolved: true,
            note: "未查询到".into(),
        });
        assert!(t.assert_complete().is_err());
    }
    #[test]
    fn verified_paper_is_shared_but_sa_states_stay_independent() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path()).unwrap();
        let mut second = rec();
        second.sa_id = "sa2".into();
        second.owner = "另一个负责人".into();
        s.import(vec![rec(), second.clone()], "input".into())
            .unwrap();
        let mut first = s.task("sa1").unwrap();
        let path = archive(dir.path(), &raw()).unwrap();
        first.artifact = Some(Artifact {
            path: path.to_string_lossy().into(),
            source: "WOS".into(),
            record_url: "https://source/record".into(),
            downloaded: 1,
            candidate: parse_wos(&raw()).unwrap(),
            identity_confirmed: true,
        });
        first.stage = Stage::Downloaded;
        s.save(&mut first, "downloaded").unwrap();
        let shared = s.reusable_artifact(&second).unwrap().unwrap();
        assert_eq!(
            shared.candidate.sha256,
            first.artifact.unwrap().candidate.sha256
        );
        assert_eq!(s.task("sa2").unwrap().stage, Stage::Pending);
        assert!(s.task("sa2").unwrap().artifact.is_none());
        second.doi = "10.1234/other".into();
        assert!(s.reusable_artifact(&second).unwrap().is_none());
    }
    #[test]
    fn alias_must_have_actual_paper_source() {
        let mut t = Task::new(rec(), "input".into());
        t.evidence.push(Evidence {
            id: "e".into(),
            kind: "alias_verified".into(),
            source: "平台别名".into(),
            text: "Li, X".into(),
            created: 1,
        });
        assert!(validate_alias_source(&t, "Li, X", "e").is_err());
        t.evidence[0].kind = "metadata".into();
        validate_alias_source(&t, "Li, X", "e").unwrap();
        assert!(validate_alias_source(&t, "Invented Name", "e").is_err());
        assert!(validate_alias_source(&t, "Li, X\n", "e").is_err());
    }
    #[test]
    fn report_keeps_human_sources_and_field_citations() {
        let mut t = Task::new(rec(), "input".into());
        t.evidence.push(Evidence {
            id: "proof".into(),
            kind: "human_review".into(),
            source: "https://source.example/record".into(),
            text: "Paper source".into(),
            created: 1,
        });
        t.classification = Some(
            json!({"type":"期刊论文","channel":"general","missing":[],"template_id":"template1","fields":{"题名":{"value":"Paper","evidence_ids":["proof"]}}}),
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.xlsx");
        export_report(&[t], &path).unwrap();
        let mut book = open_workbook_auto(path).unwrap();
        assert_eq!(book.sheet_names().len(), 4);
        assert!(book.sheet_names().iter().any(|name| name == "下载队列结果"));
        let sources = book.worksheet_range("原始来源依据").unwrap();
        assert_eq!(sources.get_value((1, 2)).unwrap().to_string(), "proof");
        let fields = book.worksheet_range("AI 字段来源").unwrap();
        assert_eq!(fields.get_value((1, 4)).unwrap().to_string(), "[\"proof\"]");
        let report = book.worksheet_range("任务与来源").unwrap();
        assert_eq!(
            report.get_value((1, 7)).unwrap().to_string(),
            "https://source.example/record"
        );
    }
    #[test]
    fn multiple_verified_file_versions_need_review() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        let mut second = rec();
        second.sa_id = "sa2".into();
        store.import(vec![rec(), second], "input".into()).unwrap();
        for (id, bytes) in [
            ("sa1", raw()),
            (
                "sa2",
                String::from_utf8(raw())
                    .unwrap()
                    .replace("Journal", "Journal second export")
                    .into_bytes(),
            ),
        ] {
            let mut task = store.task(id).unwrap();
            task.artifact = Some(Artifact {
                path: archive(dir.path(), &bytes)
                    .unwrap()
                    .to_string_lossy()
                    .into(),
                source: "WOS".into(),
                record_url: "https://source/record".into(),
                downloaded: 1,
                candidate: parse_wos(&bytes).unwrap(),
                identity_confirmed: true,
            });
            store.save(&mut task, "downloaded").unwrap();
        }
        assert_eq!(
            store.reusable_artifact(&rec()).unwrap_err().code,
            "AMBIGUOUS_RESULT"
        );
    }
    #[test]
    fn actual_material_problems_survive_restart_and_excel_but_do_not_attach_to_new_ai_fields() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        store.import(vec![rec()], "input".into()).unwrap();
        let mut task = store.task("sa1").unwrap();
        let ai = json!({"type":"期刊论文","channel":"general","missing":[],"template_id":"template1","fields":{"发表日期":{"value":"2023-02-29","evidence_ids":["proof"]}}});
        task.classification = Some(ai.clone());
        let original = json!({"schema":"template_material_v1","sa_id":task.id,"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),"classification_hash":hash(ai.to_string().as_bytes()),"output":"draft.xlsx","output_hash":"original-output-hash","validation":{"missing":["题名"],"invalid":[{"column":"发表日期","reason":"实际日期无效","rule_source":"Sheet1!G1"}],"requires_review":[{"column":"代码","reason":"动态枚举需核对","rule_source":"Sheet1!AA3"}],"ready":false}});
        task.evidence.push(Evidence {
            id: "material".into(),
            kind: "material_validation".into(),
            source: "materials/audit.json".into(),
            text: original.to_string(),
            created: 1,
        });
        store.save(&mut task, "material_exported").unwrap();
        drop(store);
        let store = Store::new(dir.path()).unwrap();
        let mut task = store.task("sa1").unwrap();
        let output = dir.path().join("report.xlsx");
        export_report(&[task.clone()], &output).unwrap();
        let mut book = open_workbook_auto(&output).unwrap();
        let summary = book.worksheet_range("任务与来源").unwrap();
        let problems = summary.get_value((1, 10)).unwrap().to_string();
        for text in ["题名", "实际日期无效", "动态枚举需核对"] {
            assert!(problems.contains(text), "{problems}");
        }
        let sources = book.worksheet_range("原始来源依据").unwrap();
        assert_eq!(
            sources.get_value((1, 6)).unwrap().to_string(),
            original.to_string()
        );
        task.classification.as_mut().unwrap()["fields"]["发表日期"]["value"] = json!("2024-02-29");
        assert_eq!(report_missing(&task), "[]"); // Historical draft remains evidence; it is not the new AI proposal.
        task.classification = Some(ai);
        task.input_hash = "new-input".into();
        assert_eq!(report_missing(&task), "[]");
    }
    #[test]
    fn report_preserves_long_unicode_source_without_truncation() {
        let mut task = Task::new(rec(), "input".into());
        let text = "依据文献🧬".repeat(8000);
        task.evidence.push(Evidence {
            id: "long".into(),
            kind: "metadata".into(),
            source: "原始记录".into(),
            text: text.clone(),
            created: 1,
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.xlsx");
        export_report(&[task], &path).unwrap();
        let mut book = open_workbook_auto(path).unwrap();
        let sheet = book.worksheet_range("原始来源依据").unwrap();
        let joined: String = sheet.rows().skip(1).map(|row| row[6].to_string()).collect();
        assert_eq!(joined, text);
        for row in sheet.rows().skip(1) {
            assert_eq!(row[8].to_string(), hash(text.as_bytes()));
        }
    }
    #[test]
    fn roster_rejects_imprecise_numbers_but_keeps_full_text_ids() {
        use rust_xlsxwriter::Workbook;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("list.xlsx");
        for text_id in [false, true] {
            let mut book = Workbook::new();
            let sheet = book.add_worksheet();
            for (col, label) in [
                "负责人",
                "sa_lzk表ID",
                "题名",
                "DOI",
                "WOS_ID",
                "工号",
                "匹配到的条目数量",
                "平台唯一号",
                "标记状态",
                "标记为待处理原因",
            ]
            .iter()
            .enumerate()
            {
                sheet.write_string(0, col as u16, *label).unwrap();
            }
            sheet.write_string(1, 0, "负责人").unwrap();
            sheet.write_string(1, 2, "Paper").unwrap();
            sheet.write_string(1, 5, "001").unwrap();
            sheet.write_number(1, 6, 0.).unwrap();
            if text_id {
                sheet.write_string(1, 1, "1234567890123456789").unwrap();
            } else {
                sheet.write_number(1, 1, 1234567890123456789_f64).unwrap();
            }
            book.save(&path).unwrap();
            if text_id {
                let (rows, _) = read_roster(&path).unwrap();
                assert_eq!(rows[0].sa_id, "1234567890123456789");
                assert_eq!(rows[0].staff_id, "001");
            } else {
                assert_eq!(read_roster(&path).unwrap_err().code, "INPUT_INVALID");
            }
        }
    }
}
