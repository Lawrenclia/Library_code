//! Read only the documented Python journals. Never load credentials or browser profiles.
use crate::*;
use calamine::{open_workbook_auto, Reader};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub sa_id: String,
    pub title: String,
    pub histories: usize,
    pub materials: usize,
    pub phases: Vec<String>,
    pub input_changed: bool,
    pub needs_readback: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Preview {
    pub root: String,
    pub fingerprint: String,
    pub roster_count: usize,
    pub journal_count: usize,
    pub import_count: usize,
    pub orphan_count: usize,
    pub classification_count: usize,
    pub prepared_count: usize,
    pub material_file_count: usize,
    pub unbound_material_count: usize,
    pub material_warnings: Vec<String>,
    pub entries: Vec<Entry>,
}
pub struct Plan {
    pub preview: Preview,
    pub records: Vec<Record>,
    pub keys: BTreeMap<String, String>,
    pub snapshot: Value,
    pub artifacts: BTreeMap<String, Artifact>,
    pub materials: crate::legacy_materials::Bundle,
}

// Python's json.dumps(..., ensure_ascii=False, sort_keys=True) uses spaces.
pub(crate) fn python_json(value: &Value) -> String {
    match value {
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!("{}: {}", json!(k), python_json(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Array(xs) => format!(
            "[{}]",
            xs.iter().map(python_json).collect::<Vec<_>>().join(", ")
        ),
        _ => value.to_string(),
    }
}
pub fn record_key(record: &Record, query: &str) -> String {
    let mut value = serde_json::to_value(record).unwrap();
    for key in ["done", "skipped", "source"] {
        value.as_object_mut().unwrap().remove(key);
    }
    value["query"] = json!(query);
    hash(python_json(&value).as_bytes())
}
fn safe_file(root: &Path, relative: &Path) -> Result<Option<PathBuf>> {
    let path = root.join(relative);
    if !path.exists() {
        return Ok(None);
    }
    if std::fs::symlink_metadata(&path)?.file_type().is_symlink() || !path.is_file() {
        return Err(Failure::new(
            "MIGRATION_INVALID",
            "旧版输入必须为目录内的实际文件。",
        ));
    }
    let canonical = path.canonicalize()?;
    if !canonical.starts_with(root) {
        return Err(Failure::new(
            "MIGRATION_INVALID",
            "旧版文件路径超出所选目录。",
        ));
    }
    Ok(Some(canonical))
}
fn read_db(path: &Path, imports: bool) -> Result<Value> {
    if std::fs::metadata(path)?.len() > 128 * 1024 * 1024 {
        return Err(Failure::new(
            "MIGRATION_INVALID",
            "旧日志超过 128 MB，请先核对文件。",
        ));
    }
    let mut db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let tx = db.transaction()?;
    let mut rows = Vec::new();
    let mut events = Vec::new();
    if imports {
        let mut query = tx.prepare("SELECT sa_id,record_key,data FROM imports ORDER BY sa_id")?;
        for row in query.query_map([], |r| Ok(json!({"sa_id":r.get::<_,String>(0)?,"record_key":r.get::<_,String>(1)?,"raw":r.get::<_,String>(2)?})))? {
            let mut row = row?;
            row["data"] = serde_json::from_str(row["raw"].as_str().unwrap())?;
            rows.push(row);
        }
    } else {
        let mut query =
            tx.prepare("SELECT task_key,sa_id,state,note,updated FROM progress ORDER BY task_key")?;
        for row in query.query_map([], |r|Ok(json!({"record_key":r.get::<_,String>(0)?,"sa_id":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"note":r.get::<_,String>(3)?,"updated":r.get::<_,String>(4)?})))? { rows.push(row?); }
        let mut query =
            tx.prepare("SELECT id,time,task_key,action,detail FROM events ORDER BY id")?;
        for row in query.query_map([], |r|Ok(json!({"id":r.get::<_,i64>(0)?,"time":r.get::<_,String>(1)?,"record_key":r.get::<_,String>(2)?,"action":r.get::<_,String>(3)?,"detail":r.get::<_,String>(4)?})))? { events.push(row?); }
    }
    tx.commit()?;
    Ok(json!({"rows":rows,"events":events}))
}
pub fn inspect(root: &Path) -> Result<Plan> {
    let root = root.canonicalize()?;
    let roster = safe_file(&root, Path::new("list.xlsx"))?.ok_or_else(|| {
        Failure::new(
            "MIGRATION_INVALID",
            "请选择含 list.xlsx 和 runtime 的旧版代码目录。",
        )
    })?;
    let before = hash(&std::fs::read(&roster)?);
    let (records, input_hash) = files::read_roster(&roster)?;
    let mut book = open_workbook_auto(&roster).map_err(Failure::storage)?;
    let sheets = book.worksheets();
    let (_, range) = sheets
        .iter()
        .find(|(_, r)| {
            r.rows()
                .next()
                .is_some_and(|row| row.iter().any(|v| v.to_string().trim() == "sa_lzk表ID"))
        })
        .unwrap();
    let header: Vec<_> = range
        .rows()
        .next()
        .unwrap()
        .iter()
        .map(|x| x.to_string())
        .collect();
    let queries: Vec<_> = header
        .iter()
        .enumerate()
        .filter(|(_, s)| s.trim().starts_with("查询方式"))
        .map(|(i, _)| i)
        .collect();
    if queries.len() != 1 {
        return Err(Failure::new(
            "MIGRATION_INVALID",
            "旧名单必须有唯一的查询方式列，才能核对旧任务版本。",
        ));
    }
    let keys = records
        .iter()
        .map(|r| {
            let value = range
                .rows()
                .nth((r.row - 1) as usize)
                .and_then(|row| row.get(queries[0]))
                .map(|x| x.to_string())
                .unwrap_or_default();
            (r.sa_id.clone(), record_key(r, value.trim()))
        })
        .collect::<BTreeMap<_, _>>();
    if before != input_hash || before != hash(&std::fs::read(&roster)?) {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "迁移读取期间旧名单发生变化。",
        ));
    }
    let mut sources = Vec::new();
    let mut journal_count = 0;
    let mut import_count = 0;
    let mut artifacts = BTreeMap::new();
    for (relative, imports) in [
        ("runtime/progress.sqlite3", false),
        ("runtime/wos-imports/imports.sqlite3", true),
        ("runtime/wos-downloads/imports.sqlite3", true),
    ] {
        let Some(path) = safe_file(&root, Path::new(relative))? else {
            continue;
        };
        let data = read_db(&path, imports)?;
        if imports {
            import_count += data["rows"].as_array().unwrap().len();
        } else {
            journal_count += data["rows"].as_array().unwrap().len();
        }
        if imports {
            for row in data["rows"].as_array().unwrap() {
                let state = &row["data"];
                let phase = state["phase"].as_str().unwrap_or("");
                if ![
                    "exported",
                    "upload_intent",
                    "import_intent",
                    "imported",
                    "push_intent",
                    "pushed",
                ]
                .contains(&phase)
                {
                    return Err(Failure::new(
                        "MIGRATION_INVALID",
                        "旧导入日志阶段无法识别；原日志未改动。",
                    ));
                }
                let sha = state["candidate"]["sha256"].as_str().unwrap_or("");
                if sha.len() != 64
                    || !sha
                        .chars()
                        .all(|x| x.is_ascii_hexdigit() && !x.is_ascii_uppercase())
                {
                    return Err(Failure::new("MIGRATION_INVALID", "旧归档哈希无效。"));
                }
                let relative_file = Path::new(relative)
                    .parent()
                    .unwrap()
                    .join(format!("{sha}.txt"));
                let archived = safe_file(&root, &relative_file)?.ok_or_else(|| {
                    Failure::new(
                        "MIGRATION_INVALID",
                        "旧日志的原始 TXT 缺失，未迁移任何任务。",
                    )
                })?;
                if std::fs::metadata(&archived)?.len() > 512 * 1024 {
                    return Err(Failure::new(
                        "FILE_INVALID",
                        "旧归档超过单篇 TXT 大小限制。",
                    ));
                }
                let raw = std::fs::read(&archived)?;
                let candidate = files::parse_wos(&raw)?;
                if candidate.sha256 != sha {
                    return Err(Failure::new("FILE_CHANGED", "旧导出文件与日志哈希不一致。"));
                }
                if normalized_wos(state["candidate"]["wos"].as_str().unwrap_or(""))
                    != normalized_wos(&candidate.wos)
                    || normalized_doi(state["candidate"]["doi"].as_str().unwrap_or(""))
                        != normalized_doi(&candidate.doi)
                {
                    return Err(Failure::new(
                        "MIGRATION_INVALID",
                        "旧日志标识符与原始 TXT 不一致。",
                    ));
                }
                artifacts.insert(
                    format!("{relative}:{}", row["sa_id"].as_str().unwrap_or("")),
                    Artifact {
                        path: archived.to_string_lossy().into(),
                        source: "WOS · 旧版迁移".into(),
                        record_url: "".into(),
                        downloaded: now(),
                        candidate,
                        identity_confirmed: false,
                    },
                );
            }
        }
        sources.push(json!({"path":relative,"imports":imports,"snapshot":data}));
    }
    let materials = crate::legacy_materials::inspect(&root, &records, &input_hash)?;
    if sources.is_empty() && materials.files.is_empty() {
        return Err(Failure::new(
            "MIGRATION_INVALID",
            "所选目录没有支持的旧版进度或导入日志。",
        ));
    }
    let snapshot = json!({"version":2,"root":root.to_string_lossy(),"roster_hash":input_hash,"records":records,"keys":keys,"sources":sources,"materials":materials});
    let mut entries = Vec::new();
    let mut orphans = std::collections::BTreeSet::new();
    for source in &sources {
        for row in source["snapshot"]["rows"].as_array().unwrap() {
            if !keys.contains_key(row["sa_id"].as_str().unwrap_or("")) {
                orphans.insert(row["sa_id"].to_string());
            }
        }
    }
    for record in &records {
        let rows = rows_for(&snapshot, &record.sa_id);
        entries.push(Entry {
            sa_id: record.sa_id.clone(),
            title: record.title.clone(),
            histories: rows.len(),
            materials: materials
                .records
                .iter()
                .filter(|m| m.task_ids.contains(&record.sa_id))
                .count(),
            phases: rows
                .iter()
                .filter_map(|r| r["data"]["phase"].as_str().or_else(|| r["state"].as_str()))
                .map(String::from)
                .collect(),
            input_changed: rows
                .iter()
                .any(|row| row["record_key"] != keys[&record.sa_id]),
            needs_readback: rows.iter().any(|row| write_history(row)),
        });
    }
    Ok(Plan {
        preview: Preview {
            root: root.to_string_lossy().into(),
            fingerprint: hash(snapshot.to_string().as_bytes()),
            roster_count: records.len(),
            journal_count,
            import_count,
            orphan_count: orphans.len(),
            classification_count: materials
                .records
                .iter()
                .filter(|m| m.kind == "classification")
                .count(),
            prepared_count: materials
                .records
                .iter()
                .filter(|m| m.kind == "submission")
                .count(),
            material_file_count: materials.files.len(),
            unbound_material_count: materials
                .records
                .iter()
                .filter(|m| m.task_ids.is_empty())
                .count(),
            material_warnings: materials.warnings.clone(),
            entries,
        },
        records,
        keys,
        snapshot,
        artifacts,
        materials,
    })
}
pub fn rows_for<'a>(snapshot: &'a Value, id: &str) -> Vec<&'a Value> {
    snapshot["sources"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["snapshot"]["rows"].as_array().unwrap())
        .filter(|r| r["sa_id"] == id)
        .collect()
}
pub fn write_history(row: &Value) -> bool {
    row["data"]["phase"]
        .as_str()
        .is_some_and(|s| s != "exported")
        || row["state"]
            .as_str()
            .is_some_and(|s| s.contains("认领") || s.contains("批准") || s == "已完成")
}
/// Preserve a complete verified claim receipt from the same journal row/version.
/// A name/order-only intent is insufficient to reconstruct an original author ID.
pub fn claim_checkpoint(snapshot: &Value, id: &str) -> Result<Option<Value>> {
    let mut found = Vec::new();
    for source in snapshot["sources"].as_array().into_iter().flatten() {
        if source["imports"] == true {
            continue;
        }
        for row in source["snapshot"]["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["sa_id"] == id)
        {
            if row["state"] != "认领已核验" {
                continue;
            }
            let matches: Vec<_> = source["snapshot"]["events"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|e| {
                    e["record_key"] == row["record_key"]
                        && e["action"] == row["state"]
                        && e["time"] == row["updated"]
                })
                .collect();
            if matches.len() != 1 {
                return Err(Failure::new(
                    "MIGRATION_CONFLICT",
                    "旧认领已核验日志没有唯一同版本完整事件；先核对历史。",
                ));
            }
            let raw = matches[0]["detail"]
                .as_str()
                .ok_or_else(|| Failure::new("MIGRATION_INVALID", "旧认领回执不是文本。"))?;
            let receipt: Value = serde_json::from_str(raw)
                .map_err(|_| Failure::new("MIGRATION_INVALID", "旧认领回执不是完整 JSON。"))?;
            found.push(json!({"schema":"legacy_claim_checkpoint_v1","source":source["path"],"record_key":row["record_key"],"event":matches[0],"receipt":receipt}));
        }
    }
    if found.len() > 1 {
        return Err(Failure::new(
            "MIGRATION_CONFLICT",
            "同一 SA 有多个旧认领回执，请先核对。",
        ));
    }
    Ok(found.pop())
}
pub fn is_claim_checkpoint(payload: &Value) -> bool {
    payload["legacy_rows"].as_array().is_some_and(|rows| {
        !rows.is_empty()
            && rows.iter().all(|r| {
                ["认领待提交", "认领已核验"].contains(&r["state"].as_str().unwrap_or(""))
                    && !r["data"].is_object()
            })
    })
}
/// Older desktop migrations used legacy_sa for every journal write. Recover
/// their immutable intent from the archived migration, without migrating again.
pub fn hydrate_claim_checkpoint(store: &Store, payload: &Value) -> Result<Value> {
    if !is_claim_checkpoint(payload) {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "该旧检查点不是独立认领记录，请使用对应回读步骤。",
        ));
    }
    let fingerprint = payload["legacy_migration"]
        .as_str()
        .ok_or_else(|| Failure::new("MIGRATION_INVALID", "旧认领意图没有迁移来源。"))?;
    let id = payload["sa_id"]
        .as_str()
        .ok_or_else(|| Failure::new("MIGRATION_INVALID", "旧认领意图没有 SA ID。"))?;
    let archived = store
        .legacy_snapshot(fingerprint)?
        .ok_or_else(|| Failure::new("MIGRATION_INVALID", "原迁移归档已缺失。"))?;
    let rows = rows_for(&archived, id);
    if payload["legacy_rows"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| !rows.contains(&r))
    {
        return Err(Failure::new(
            "MIGRATION_CONFLICT",
            "原认领意图与保存的迁移版本不同。",
        ));
    }
    let checkpoint = claim_checkpoint(&archived, id)?;
    let expected = json!(checkpoint);
    if payload["claim_checkpoint"].is_object() && payload["claim_checkpoint"] != expected {
        return Err(Failure::new(
            "MIGRATION_CONFLICT",
            "旧认领回执与原迁移归档不同。",
        ));
    }
    let mut result = payload.clone();
    result["claim_checkpoint"] = expected;
    Ok(result)
}
fn claim_snapshot_identity(task: &Task, payload: &Value, live: &Value) -> Result<()> {
    let row = &live["row"];
    if payload["sa_id"] != task.id
        || payload["input_hash"] != task.input_hash
        || payload["input_matches"] != true
        || row["saLzkId"] != task.id
        || row["gh"] != task.record.staff_id
        || row["titleValue"].as_str().map(norm) != Some(norm(&task.record.title))
        || !["待处理", "已处理"].contains(&row["markStatus"].as_str().unwrap_or(""))
        || task.record.matches != 1
        || matched_ids(row)?.len() != 1
        || matched_ids(row)?
            != matched_ids(
                &json!({"matchCount":task.record.matches,"itemId":task.record.item_ids}),
            )?
    {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "旧认领检查点、名单和实时 SA 不一致，保持待确认。",
        ));
    }
    Ok(())
}
pub fn claim_readback(task: &Task, payload: &Value, live: &Value) -> Result<Value> {
    claim_snapshot_identity(task, payload, live)?;
    let checkpoint = &payload["claim_checkpoint"];
    if checkpoint["schema"] != "legacy_claim_checkpoint_v1" {
        return Err(Failure::new(
            "REVIEW_REQUIRED",
            "旧日志缺少完整已核验认领回执，不能仅凭姓名与顺序确认，也不会重发认领。",
        ));
    }
    let receipt = &checkpoint["receipt"];
    let (index, prepared) = crate::claim::verified_checkpoint(task, receipt, live)?;
    Ok(
        json!({"sa_id":task.id,"expected":live["row"],"staff_id":task.record.staff_id,
        "roster_staff_id":task.record.staff_id,"sa_text":prepared["sa_text"],"prepared":prepared,
        "author_index":index,"checkpoint_mode":"verified_claim_receipt"}),
    )
}
pub fn verify_claim_checkpoint(
    task: &Task,
    payload: &Value,
    before: &Value,
    result: &Value,
    after: &Value,
) -> Result<()> {
    claim_snapshot_identity(task, payload, after)?;
    if before != after {
        return Err(Failure::new(
            "TASK_CHANGED",
            "旧认领只读核验期间 SA 发生变化，保持待确认。",
        ));
    }
    let request = claim_readback(task, payload, before)?;
    crate::claim::assert_checkpoint_result(
        task,
        &payload["claim_checkpoint"]["receipt"],
        &request["prepared"],
        result,
        after,
    )
}
pub fn verify_sa_checkpoint(task: &Task, payload: &Value, snapshot: &Value) -> Result<()> {
    let row = &snapshot["row"];
    if payload["sa_id"] != task.id
        || payload["input_matches"] != true
        || payload["input_hash"] != task.input_hash
        || row["saLzkId"] != task.id
        || row["gh"] != task.record.staff_id
        || norm(row["titleValue"].as_str().unwrap_or("")) != norm(&task.record.title)
    {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "旧版检查点、当前任务与实时 SA 的输入不一致。",
        ));
    }
    matched_ids(row)?;
    let note = row["remark"].as_str().unwrap_or("");
    let expected = payload["legacy_rows"].as_array().is_some_and(|rows| {
        rows.iter().any(|r| {
            ["已完成", "认领结案待核验"].contains(&r["state"].as_str().unwrap_or(""))
                && r["note"] == note
                && !note.is_empty()
        })
    });
    if row["markStatus"] != "已处理" || !expected {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "尚未回读到旧日志对应的已处理状态与准确备注；保留检查点，不重发认领或结案。",
        ));
    }
    Ok(())
}
pub fn assert_no_prior_import(store: &Store, sa_id: &str, candidate: &Candidate) -> Result<()> {
    for snapshot in store.legacy_snapshots()? {
        for source in snapshot["sources"].as_array().unwrap() {
            for row in source["snapshot"]["rows"].as_array().unwrap() {
                let prior = &row["data"]["candidate"];
                let same = normalized_wos(prior["wos"].as_str().unwrap_or(""))
                    == normalized_wos(&candidate.wos)
                    || (!candidate.doi.is_empty()
                        && normalized_doi(prior["doi"].as_str().unwrap_or(""))
                            == normalized_doi(&candidate.doi));
                if same && write_history(row) {
                    return Err(Failure::new("LEGACY_WRITE_EXISTS",format!("旧版已有本篇论文的导入操作（SA {}）；先核对历史批次与平台条目，不创建新批次。当前 SA {sa_id}。",row["sa_id"].as_str().unwrap_or(""))));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    fn record() -> Record {
        serde_json::from_value(json!({"row":2,"owner":"测试","sa_id":"0001","title":"Paper","doi":"10.1234/test","wos":"","staff_id":"001","matches":0,"item_ids":"","mark":"待处理","reason":"测试","done":false,"skipped":false,"source":""})).unwrap()
    }
    fn fixture(phase: &str, orphan: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let r = record();
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        let headers = [
            "备注",
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
            "查询方式（DOI和WOS_ID：1   题名： 2）",
        ];
        for (i, h) in headers.iter().enumerate() {
            sheet.write_string(0, i as u16, *h).unwrap();
        }
        for (i, s) in [
            "",
            "测试",
            "0001",
            "Paper",
            "10.1234/test",
            "",
            "001",
            "0",
            "",
            "待处理",
            "测试",
            "2",
        ]
        .iter()
        .enumerate()
        {
            sheet.write_string(1, i as u16, *s).unwrap();
        }
        book.save(dir.path().join("list.xlsx")).unwrap();
        std::fs::create_dir_all(dir.path().join("runtime/wos-imports")).unwrap();
        let db = Connection::open(dir.path().join("runtime/progress.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE progress(task_key TEXT PRIMARY KEY,sa_id TEXT,state TEXT,note TEXT,updated TEXT);CREATE TABLE events(id INTEGER PRIMARY KEY,time TEXT,task_key TEXT,action TEXT,detail TEXT);").unwrap();
        let key = record_key(&r, "2");
        db.execute(
            "INSERT INTO progress VALUES(?,?,?,?,?)",
            params![
                key,
                r.sa_id,
                "待核验",
                "完整原文依据",
                "2026-10-09T12:00:00+08:00"
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO events VALUES(1,?,?,?,?)",
            params![
                "2026-10-09",
                key,
                "待核验",
                json!({"source":"中文原始依据","complete":true}).to_string()
            ],
        )
        .unwrap();
        let raw=b"TI\tAU\tAF\tSO\tPY\tC1\tUT\tDI\nPaper\tTest, A\tAlice Test\tJournal\t2026\tShanghai Jiao Tong Univ\tWOS:000123456789012\t10.1234/test\n";
        let candidate = files::parse_wos(raw).unwrap();
        let folder = dir.path().join("runtime/wos-imports");
        std::fs::write(folder.join(format!("{}.txt", candidate.sha256)), raw).unwrap();
        let db = Connection::open(folder.join("imports.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE imports(sa_id TEXT PRIMARY KEY,record_key TEXT,data TEXT);")
            .unwrap();
        let state = json!({"phase":phase,"candidate":candidate,"identity_confirmed":true,"instructions":"SA补充-0001","batch":if phase=="exported"{Value::Null}else{json!({"id":"batch-1","status":2})}});
        db.execute(
            "INSERT INTO imports VALUES(?,?,?)",
            params![
                if orphan { "old-orphan" } else { "0001" },
                key,
                state.to_string()
            ],
        )
        .unwrap();
        dir
    }
    #[test]
    fn key_matches_actual_python_record_unicode_and_spacing() {
        assert_eq!(
            record_key(&record(), "2"),
            "33af9d971fd80495f7a231f1f647e361fa7835f786bd8feaf73e1fc04488eb07"
        );
        let mut r = record();
        r.done = true;
        r.skipped = true;
        r.source = "new source".into();
        assert_eq!(record_key(&r, "2"), record_key(&record(), "2"));
        r.staff_id = "0001".into();
        assert_ne!(record_key(&r, "2"), record_key(&record(), "2"));
    }
    fn material_fixture(checkpoint: bool) -> tempfile::TempDir {
        let root = fixture("exported", false);
        // Test a material-only legacy project, with no progress/import databases.
        for relative in [
            "runtime/progress.sqlite3",
            "runtime/wos-imports/imports.sqlite3",
        ] {
            std::fs::remove_file(root.path().join(relative)).unwrap();
        }
        let sha = hash(&std::fs::read(root.path().join("list.xlsx")).unwrap());
        let id = hash(python_json(&json!({"doi":"10.1234/test","title":"Paper"})).as_bytes());
        let classify = root
            .path()
            .join("runtime/classification")
            .join("a".repeat(64));
        let submit = root.path().join("runtime/submission").join("b".repeat(64));
        std::fs::create_dir_all(&classify).unwrap();
        std::fs::create_dir_all(submit.join("待补草稿")).unwrap();
        let paper = json!({"id":id,"title":"Paper","doi":"10.1234/test","rows":[2]});
        std::fs::write(classify.join("分类结果.json"),json!({"model":"old-model","source":{"sha256":sha},"records":[{"id":id,"title":"Paper","doi":"10.1234/test","rows":[2],"classification":{"type":"期刊论文","confidence":"低","missing_evidence":["机构署名"]}}]}).to_string()).unwrap();
        std::fs::write(classify.join("events.jsonl"), b"").unwrap();
        std::fs::write(
            submit.join("零匹配队列.json"),
            json!({"sha256":sha,"papers":[paper]}).to_string(),
        )
        .unwrap();
        let raw = b"TI\tDI\nPaper\t10.1234/test\n";
        std::fs::write(submit.join("original.txt"), raw).unwrap();
        std::fs::copy(
            root.path().join("list.xlsx"),
            submit.join("normalized.xlsx"),
        )
        .unwrap();
        std::fs::copy(
            root.path().join("list.xlsx"),
            submit.join("待补草稿/期刊论文.xlsx"),
        )
        .unwrap();
        let mut prepared = paper.clone();
        prepared["metadata"] = json!({"fields":{"abstract":{"value":"长摘要🙂引用保留".repeat(5000),"source_ids":["source-1"]}},"missing":["机构署名"]});
        prepared["sources"] = json!([{"id":"source-1","provider":"WOS","url":"https://example.test/record","fields":{"TI":"Paper"},"identity_verified":false}]);
        prepared["status"] = json!("待补全");
        prepared["export"] = json!({"file":"original.txt","sha256":hash(raw),"import_file":"normalized.xlsx","channel":"wos-txt"});
        let (name, data) = if checkpoint {
            (
                "progress.json",
                json!({"signature":"old","records":{id:prepared}}),
            )
        } else {
            (
                "提交准备.json",
                json!({"records":[prepared],"notice":"仅准备，未上传"}),
            )
        };
        std::fs::write(submit.join(name), data.to_string()).unwrap();
        root
    }
    #[test]
    fn materials_preserve_originals_and_citations_without_promoting_old_ai_after_restart() {
        let old = material_fixture(false);
        let plan = inspect(old.path()).unwrap();
        assert_eq!(
            (
                plan.preview.classification_count,
                plan.preview.prepared_count,
                plan.preview.unbound_material_count
            ),
            (1, 1, 0)
        );
        assert_eq!(plan.preview.journal_count, 0);
        let target = tempfile::tempdir().unwrap();
        let store = Store::new(target.path()).unwrap();
        store.apply_legacy(&plan).unwrap();
        let store = Store::new(target.path()).unwrap();
        let task = store.task("0001").unwrap();
        assert_eq!(task.stage, Stage::Pending);
        assert!(task.artifact.is_none() && task.classification.is_none());
        let evidence = task
            .evidence
            .iter()
            .filter(|e| e.kind == "legacy_material")
            .collect::<Vec<_>>();
        assert_eq!(evidence.len(), 2);
        let prepared: Value = serde_json::from_str(&evidence[1].text).unwrap();
        assert_eq!(prepared["record"], plan.materials.records[1].record);
        assert_eq!(prepared["platform_verified"], false);
        assert_eq!(prepared["review_required"], true);
        assert!(source_files::verified_evidence(target.path(), &task)
            .unwrap()
            .is_empty());
        let snapshots = store.legacy_snapshots().unwrap();
        for file in &plan.materials.files {
            let archived = &snapshots[0]["material_archives"][&file.relative];
            let raw = std::fs::read(archived["archive_path"].as_str().unwrap()).unwrap();
            assert_eq!(hash(&raw), file.sha256);
            assert_eq!(raw, std::fs::read(old.path().join(&file.relative)).unwrap());
        }
        assert_eq!(
            crate::legacy_materials::summary(&snapshots)[0]["records"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let revision = task.revision;
        assert_eq!(store.apply_legacy(&plan).unwrap()["already_imported"], true);
        assert_eq!(store.task("0001").unwrap().revision, revision);
        let report = target.path().join("report.xlsx");
        files::export_report_with_history(&[task], &[], &snapshots, &report).unwrap();
        let mut book = open_workbook_auto(&report).unwrap();
        let sheet = book.worksheet_range("旧版资料归档").unwrap();
        let reconstructed = sheet
            .rows()
            .skip(1)
            .map(|r| r[5].to_string())
            .collect::<String>();
        assert_eq!(
            serde_json::from_str::<Value>(&reconstructed).unwrap(),
            snapshots[0]
        );
        assert_eq!(
            sheet.rows().nth(1).unwrap()[3].to_string(),
            hash(reconstructed.as_bytes())
        );
        assert!(sheet.height() > 2);
    }
    #[test]
    fn incomplete_preparation_checkpoint_keeps_exports_and_requires_review() {
        let old = material_fixture(true);
        let plan = inspect(old.path()).unwrap();
        let material = &plan.materials.records[1];
        assert_eq!(material.task_ids, vec!["0001"]);
        assert!(material.file.ends_with("progress.json"));
        assert!(material.files.iter().any(|f| f.ends_with("original.txt")));
        assert!(!material.warnings.is_empty());
        assert_eq!(plan.preview.prepared_count, 1);
    }
    #[test]
    fn wrong_roster_or_rows_or_paper_keys_are_unbound_history() {
        for changed in ["hash", "rows", "id", "queue"] {
            let old = material_fixture(false);
            let path = old.path().join("runtime/submission").join("b".repeat(64));
            let (name, key) = if changed == "hash" || changed == "queue" {
                ("零匹配队列.json", "papers")
            } else {
                ("提交准备.json", "records")
            };
            let p = path.join(name);
            let mut data: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
            match changed {
                "hash" => data["sha256"] = json!("other"),
                "rows" => data[key][0]["rows"] = json!([3]),
                "id" => data[key][0]["id"] = json!("other"),
                _ => data[key][0]["title"] = json!("other"),
            };
            std::fs::write(p, data.to_string()).unwrap();
            let plan = inspect(old.path()).unwrap();
            assert_eq!(plan.preview.unbound_material_count, 1);
            assert!(plan.materials.records[1].task_ids.is_empty());
            let target = tempfile::tempdir().unwrap();
            let store = Store::new(target.path()).unwrap();
            store.apply_legacy(&plan).unwrap();
            assert_eq!(store.task("0001").unwrap().evidence.len(), 1);
            assert_eq!(
                crate::legacy_materials::summary(&store.legacy_snapshots().unwrap())[0]["records"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
        }
    }
    #[test]
    fn changed_manifest_or_export_or_existing_archive_never_commits_migration() {
        for changed in ["manifest", "export", "archive"] {
            let old = material_fixture(false);
            let plan = inspect(old.path()).unwrap();
            let target = tempfile::tempdir().unwrap();
            let store = Store::new(target.path()).unwrap();
            let file = &plan.materials.files[0];
            match changed {
                "manifest" => {
                    std::fs::write(old.path().join(&file.relative), b"{}").unwrap();
                }
                "export" => {
                    std::fs::write(
                        old.path()
                            .join("runtime/submission")
                            .join("b".repeat(64))
                            .join("original.txt"),
                        b"changed",
                    )
                    .unwrap();
                }
                _ => {
                    let dir = target.path().join("legacy-files");
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(
                        dir.join(format!("{}.{}", file.sha256, file.format)),
                        b"changed",
                    )
                    .unwrap();
                }
            }
            assert!(store.apply_legacy(&plan).is_err());
            assert!(store.tasks().unwrap().is_empty());
            assert!(store.legacy_snapshots().unwrap().is_empty());
        }
    }
    #[test]
    fn malformed_and_duplicate_manifest_or_external_export_refuse_migration() {
        for changed in ["json", "duplicate", "external"] {
            let old = material_fixture(false);
            let p = old
                .path()
                .join("runtime/submission")
                .join("b".repeat(64))
                .join("提交准备.json");
            if changed == "json" {
                std::fs::write(&p, b"invalid").unwrap();
            } else {
                let mut data: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
                let outside = tempfile::NamedTempFile::with_suffix(".txt").unwrap();
                if changed == "duplicate" {
                    let record = data["records"][0].clone();
                    data["records"].as_array_mut().unwrap().push(record);
                } else {
                    data["records"][0]["export"]["file"] = json!(outside.path().to_string_lossy());
                }
                std::fs::write(&p, data.to_string()).unwrap();
                assert!(inspect(old.path()).is_err());
                continue;
            }
            assert!(inspect(old.path()).is_err());
        }
    }
    #[test]
    fn legacy_sa_completion_requires_exact_live_identity_status_note_and_input() {
        let task = Task::new(record(), "input".into());
        let payload = json!({"sa_id":"0001","input_matches":true,"input_hash":"input","legacy_rows":[{"state":"认领结案待核验","note":"已认领"}]});
        let snapshot = json!({"row":{"saLzkId":"0001","gh":"001","titleValue":"Paper","markStatus":"已处理","remark":"已认领","matchCount":1,"itemId":"1234567890123456789"}});
        verify_sa_checkpoint(&task, &payload, &snapshot).unwrap();
        for key in ["saLzkId", "gh", "titleValue", "markStatus", "remark"] {
            let mut changed = snapshot.clone();
            changed["row"][key] = json!("other");
            assert!(verify_sa_checkpoint(&task, &payload, &changed).is_err());
        }
        let mut changed = payload.clone();
        changed["input_matches"] = json!(false);
        assert!(verify_sa_checkpoint(&task, &changed, &snapshot).is_err());
    }
    #[test]
    fn migration_copies_original_files_and_complete_journals_once_without_source_changes() {
        let root = fixture("exported", false);
        let target = tempfile::tempdir().unwrap();
        let store = Store::new(target.path()).unwrap();
        let before = hash(&std::fs::read(root.path().join("runtime/progress.sqlite3")).unwrap());
        let plan = inspect(root.path()).unwrap();
        assert!(!plan.preview.entries[0].input_changed);
        store.apply_legacy(&plan).unwrap();
        let task = store.task("0001").unwrap();
        assert_eq!(task.stage, Stage::Downloaded);
        assert!(task.artifact.as_ref().unwrap().identity_confirmed);
        assert!(Path::new(&task.artifact.as_ref().unwrap().path).starts_with(target.path()));
        let raw = &task
            .evidence
            .iter()
            .find(|e| e.kind == "legacy_history")
            .unwrap()
            .text;
        assert!(raw.contains("中文原始依据"));
        let revision = task.revision;
        assert_eq!(store.apply_legacy(&plan).unwrap()["already_imported"], true);
        assert_eq!(store.task("0001").unwrap().revision, revision);
        assert_eq!(
            hash(&std::fs::read(root.path().join("runtime/progress.sqlite3")).unwrap()),
            before
        );
    }
    #[test]
    fn old_push_needs_readback_and_orphan_writes_block_same_paper_after_restart() {
        for orphan in [false, true] {
            let root = fixture("pushed", orphan);
            let target = tempfile::tempdir().unwrap();
            let store = Store::new(target.path()).unwrap();
            let plan = inspect(root.path()).unwrap();
            store.apply_legacy(&plan).unwrap();
            let reopened = Store::new(target.path()).unwrap();
            reopened.recover().unwrap();
            let candidate = plan.artifacts.values().next().unwrap().candidate.clone();
            assert_eq!(
                assert_no_prior_import(&reopened, "new-sa", &candidate)
                    .unwrap_err()
                    .code,
                "LEGACY_WRITE_EXISTS"
            );
            if !orphan {
                let task = reopened.task("0001").unwrap();
                assert_eq!(task.stage, Stage::Unknown);
                let pending = reopened.unresolved("0001").unwrap();
                assert_eq!(pending.len(), 1);
                assert_eq!(pending[0]["action"], "import_push");
            } else {
                assert_eq!(plan.preview.orphan_count, 1);
                assert_eq!(
                    reopened.legacy_snapshots().unwrap()[0]["sources"][1]["snapshot"]["rows"][0]
                        ["sa_id"],
                    "old-orphan"
                );
            }
        }
    }
    #[test]
    fn task_conflict_rolls_back_migration_and_keeps_current_work() {
        let root = fixture("exported", false);
        let target = tempfile::tempdir().unwrap();
        let store = Store::new(target.path()).unwrap();
        let mut r = record();
        r.title = "Other paper".into();
        store.import(vec![r], "current".into()).unwrap();
        let before = store.task("0001").unwrap();
        assert_eq!(
            store
                .apply_legacy(&inspect(root.path()).unwrap())
                .unwrap_err()
                .code,
            "MIGRATION_CONFLICT"
        );
        assert!(store.legacy_snapshots().unwrap().is_empty());
        assert_eq!(store.task("0001").unwrap().revision, before.revision);
    }
    #[test]
    fn changed_legacy_key_preserves_history_and_blocks_old_intent() {
        let root = fixture("push_intent", false);
        let db = Connection::open(root.path().join("runtime/wos-imports/imports.sqlite3")).unwrap();
        db.execute("UPDATE imports SET record_key='older-input'", [])
            .unwrap();
        let target = tempfile::tempdir().unwrap();
        let store = Store::new(target.path()).unwrap();
        let plan = inspect(root.path()).unwrap();
        assert!(plan.preview.entries[0].input_changed);
        store.apply_legacy(&plan).unwrap();
        let task = store.task("0001").unwrap();
        assert_eq!(task.last_error.unwrap().code, "INPUT_CHANGED");
        assert_eq!(task.stage, Stage::Unknown);
        assert!(!task.artifact.unwrap().identity_confirmed);
    }
    #[test]
    fn malformed_or_modified_archives_and_unknown_phases_never_migrate() {
        let root = fixture("unknown-phase", false);
        assert!(inspect(root.path()).is_err());
        let root = fixture("exported", false);
        let plan = inspect(root.path()).unwrap();
        std::fs::write(&plan.artifacts.values().next().unwrap().path, b"modified").unwrap();
        assert!(inspect(root.path()).is_err());
        let root = fixture("exported", false);
        let first = inspect(root.path()).unwrap();
        let db = Connection::open(root.path().join("runtime/progress.sqlite3")).unwrap();
        db.execute("UPDATE progress SET note='changed'", [])
            .unwrap();
        assert_ne!(
            inspect(root.path()).unwrap().preview.fingerprint,
            first.preview.fingerprint
        );
    }
}
