//! Lossless original-file intake. Container reading is not a database export driver.
use crate::*;
use calamine::{open_workbook_auto, Reader};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_CELLS: usize = 200_000;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadOptions {
    pub encoding: String,
    pub delimiter: String,
}
impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            encoding: "utf-8".into(),
            delimiter: "comma".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Draft {
    pub id: String,
    pub task_id: String,
    pub input_hash: String,
    pub record_fingerprint: String,
    pub task_revision: i64,
    pub channel: String,
    pub original_name: String,
    pub path: String,
    pub sha256: String,
    pub format: String,
    pub options: ReadOptions,
}
struct Table {
    name: String,
    start_row: u32,
    start_col: u32,
    rows: Vec<Vec<String>>,
    formulas: BTreeMap<(u32, u32), String>,
}
pub struct Document {
    tables: Vec<Table>,
    encoding: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Selection {
    pub sheet: String,
    pub header_row: u32,
    pub row: u32,
    pub end_row: u32,
    pub title_column: Option<u32>,
    pub doi_column: Option<u32>,
    pub wos_column: Option<u32>,
    pub text_title: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Field {
    pub column: u32,
    pub address: String,
    pub label: String,
    pub value: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub schema: String,
    pub task_id: String,
    pub input_hash: String,
    pub record_fingerprint: String,
    pub channel: String,
    pub original_name: String,
    pub archive_path: String,
    pub sha256: String,
    pub format: String,
    pub options: ReadOptions,
    pub actual_encoding: String,
    pub selection: Selection,
    pub fields: Vec<Field>,
    pub text: String,
    pub title: String,
    pub doi: String,
    pub wos: String,
    pub source_url: String,
    pub binding_note: String,
    pub institution_verified: bool,
}
/// A bound database export, retaining the complete file and selected record.
/// Container verification does not prove suitability for platform import.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OriginalExport {
    pub evidence_id: String,
    pub evidence_hash: String,
    pub receipt: Receipt,
}
fn fail(message: impl Into<String>) -> Failure {
    Failure::new("SOURCE_INVALID", message)
}
pub fn channel_formats(id: &str) -> Result<Vec<String>> {
    crate::catalog::channels()
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .and_then(|c| c["formats"].as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .ok_or_else(|| fail("未登记的来源渠道。"))
}
fn bytes(path: &Path) -> Result<Vec<u8>> {
    let m = std::fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() as usize > MAX_BYTES {
        return Err(fail("请选择不超过 16 MB 的实际文件。"));
    }
    let raw = std::fs::read(path)?;
    if raw.is_empty() || raw.len() > MAX_BYTES {
        return Err(fail("来源为空或超过 16 MB。"));
    }
    Ok(raw)
}
fn decode(raw: &[u8], option: &str) -> Result<(String, String)> {
    let encoding = match option {
        "utf-8" => encoding_rs::UTF_8,
        "gb18030" => encoding_rs::GB18030,
        "utf-16le" => encoding_rs::UTF_16LE,
        "utf-16be" => encoding_rs::UTF_16BE,
        _ => return Err(fail("未支持的字符编码。")),
    };
    let (text, actual, errors) = encoding.decode(raw);
    if errors || text.contains('\0') {
        return Err(fail(
            "字符解码失败，请选择原文件编码；不会用替换字符采纳乱码。",
        ));
    }
    Ok((text.into_owned(), actual.name().into()))
}
fn column(mut col: u32) -> String {
    let mut out = String::new();
    loop {
        out.insert(0, (b'A' + (col % 26) as u8) as char);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out
}
pub fn read(path: &Path, format: &str, options: &ReadOptions) -> Result<Document> {
    let raw = bytes(path)?;
    let mut cells = 0;
    let mut tables = vec![];
    let encoding;
    match format {
        "xlsx" => {
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&raw))
                .map_err(|_| fail("文件不是有效 XLSX。"))?;
            let mut expanded = 0u64;
            for i in 0..zip.len() {
                expanded += zip.by_index(i).map_err(Failure::storage)?.size();
                if expanded > 64 * 1024 * 1024 {
                    return Err(fail("XLSX 解压内容超过 64 MB，请缩小导出范围。"));
                }
            }
            let mut book = open_workbook_auto(path).map_err(|_| fail("无法读取原始 Excel。"))?;
            for (name, range) in book.worksheets() {
                let formulas = book.worksheet_formula(&name).map_err(Failure::storage)?;
                let start = range.start().unwrap_or((0, 0));
                let mut formula_map = BTreeMap::new();
                if let Some(origin) = formulas.start() {
                    for (ri, row) in formulas.rows().enumerate() {
                        for (ci, value) in row.iter().enumerate() {
                            if !value.is_empty() {
                                formula_map.insert(
                                    (origin.0 + ri as u32, origin.1 + ci as u32),
                                    value.clone(),
                                );
                            }
                        }
                    }
                }
                cells += range.get_size().0.saturating_mul(range.get_size().1);
                if cells > MAX_CELLS {
                    return Err(fail("来源超过 200000 个单元格，请缩小原始导出范围。"));
                }
                tables.push(Table {
                    name,
                    start_row: start.0,
                    start_col: start.1,
                    rows: range
                        .rows()
                        .map(|r| r.iter().map(ToString::to_string).collect())
                        .collect(),
                    formulas: formula_map,
                });
            }
            encoding = "XLSX".into();
        }
        "csv" => {
            let (text, actual) = decode(&raw, &options.encoding)?;
            encoding = actual;
            let delimiter = match options.delimiter.as_str() {
                "comma" => b',',
                "tab" => b'\t',
                "semicolon" => b';',
                _ => return Err(fail("请选择逗号、制表符或分号分隔。")),
            };
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .delimiter(delimiter)
                .from_reader(text.as_bytes());
            let mut rows = vec![];
            for record in reader.records() {
                let row = record.map_err(|_| fail("CSV 列数或引号不一致，请核对分隔符。"))?;
                cells += row.len();
                if cells > MAX_CELLS {
                    return Err(fail("来源超过 200000 个单元格。"));
                }
                rows.push(row.iter().map(str::to_string).collect());
            }
            tables.push(Table {
                name: "CSV".into(),
                start_row: 0,
                start_col: 0,
                rows,
                formulas: BTreeMap::new(),
            });
        }
        "txt" => {
            let (text, actual) = decode(&raw, &options.encoding)?;
            encoding = actual;
            let rows: Vec<_> = text
                .split('\n')
                .map(|s| vec![s.strip_suffix('\r').unwrap_or(s).to_string()])
                .collect();
            if rows.len() > MAX_CELLS {
                return Err(fail("文本行数过多，请缩小导出范围。"));
            }
            tables.push(Table {
                name: "文本".into(),
                start_row: 0,
                start_col: 0,
                rows,
                formulas: BTreeMap::new(),
            });
        }
        _ => return Err(fail("仅支持登记的原始 Excel、CSV 或 TXT。")),
    }
    if tables.iter().all(|t| t.rows.is_empty()) {
        return Err(fail("文件没有可读取的记录。"));
    }
    Ok(Document { tables, encoding })
}
pub fn prepare(
    root: &Path,
    task: &Task,
    channel: &str,
    path: &Path,
    options: ReadOptions,
) -> Result<Draft> {
    let format = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !channel_formats(channel)?.contains(&format) {
        return Err(fail("该文件后缀不属于所选渠道的登记格式。"));
    }
    let raw = bytes(path)?;
    let sha = hash(&raw);
    read(path, &format, &options)?;
    if hash(&bytes(path)?) != sha {
        return Err(fail("预览期间原文件变化，请重新选择。"));
    }
    let folder = root.join("source-files");
    std::fs::create_dir_all(&folder)?;
    let target = folder.join(format!("{sha}.{format}"));
    if target.exists() {
        if bytes(&target)? != raw {
            return Err(fail("原文件归档已变化，不能覆盖。"));
        }
    } else {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        f.write_all(&raw)?;
        f.sync_all()?;
    }
    Ok(Draft {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: task.id.clone(),
        input_hash: task.input_hash.clone(),
        record_fingerprint: task.record.fingerprint(),
        task_revision: task.revision,
        channel: channel.into(),
        original_name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        path: target.to_string_lossy().into(),
        sha256: sha,
        format,
        options,
    })
}
fn owned(root: &Path, path: &str, sha: &str, format: &str) -> Result<PathBuf> {
    if sha.len() != 64
        || !sha.bytes().all(|b| b.is_ascii_hexdigit())
        || !["xlsx", "csv", "txt"].contains(&format)
    {
        return Err(fail("来源归档身份无效。"));
    }
    let expected = root.join("source-files").join(format!("{sha}.{format}"));
    if Path::new(path).canonicalize()? != expected.canonicalize()?
        || bytes(&expected).map(|v| hash(&v))? != sha
    {
        return Err(fail("原始归档或哈希变化，不采纳来源。"));
    }
    Ok(expected)
}
pub fn check_draft(root: &Path, task: &Task, draft: &Draft) -> Result<Document> {
    if task.id != draft.task_id
        || task.input_hash != draft.input_hash
        || task.record.fingerprint() != draft.record_fingerprint
        || task.revision != draft.task_revision
    {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "来源预览属于旧任务或旧版本，请重新预览原文件。",
        ));
    }
    let path = owned(root, &draft.path, &draft.sha256, &draft.format)?;
    read(&path, &draft.format, &draft.options)
}
impl Document {
    fn table(&self, sheet: &str) -> Result<&Table> {
        self.tables
            .iter()
            .find(|t| t.name == sheet)
            .ok_or_else(|| fail("选择实际工作表。"))
    }
    pub fn page(
        &self,
        draft: &Draft,
        sheet: Option<&str>,
        header_row: u32,
        page: u32,
    ) -> Result<Value> {
        let table = match sheet.filter(|s| !s.is_empty()) {
            Some(s) => self.table(s)?,
            None => self.tables.first().ok_or_else(|| fail("缺少工作表。"))?,
        };
        let header_row = if header_row == 0 {
            table.start_row + 1
        } else {
            header_row
        };
        let offset = if draft.format == "txt" {
            0
        } else {
            header_row
                .checked_sub(table.start_row + 1)
                .ok_or_else(|| fail("表头行不在实际数据内。"))? as usize
                + 1
        };
        let headers = if draft.format == "txt" {
            vec![]
        } else {
            table.rows.get(offset-1).ok_or_else(||fail("表头行不存在。"))?.iter().enumerate().map(|(i,s)|json!({"column":table.start_col+i as u32,"name":s,"label":format!("{} [{}列]",s,column(table.start_col+i as u32))})).collect()
        };
        let count = table.rows.len().saturating_sub(offset);
        let start = (page as usize)
            .checked_mul(50)
            .ok_or_else(|| fail("页码无效。"))?;
        if start >= count && page > 0 {
            return Err(fail("预览页超出原始数据。"));
        }
        let rows: Vec<_> = table
            .rows
            .iter()
            .enumerate()
            .skip(offset + start)
            .take(50)
            .map(|(i, row)| json!({"row":table.start_row+i as u32+1,"values":row}))
            .collect();
        Ok(
            json!({"draft":draft,"sheets":self.tables.iter().map(|t|json!({"name":t.name,"first_row":t.start_row+1,"rows":t.rows.len()})).collect::<Vec<_>>(),"sheet":table.name,"header_row":header_row,"columns":headers,"rows":rows,"page":page,"total":count,"actual_encoding":self.encoding}),
        )
    }
    fn extract(
        &self,
        format: &str,
        s: &Selection,
    ) -> Result<(Vec<Field>, String, String, String, String)> {
        let table = self.table(&s.sheet)?;
        let row = s
            .row
            .checked_sub(table.start_row + 1)
            .ok_or_else(|| fail("所选行不存在。"))? as usize;
        let values = table.rows.get(row).ok_or_else(|| fail("所选行不存在。"))?;
        if format == "txt" {
            let end = s
                .end_row
                .checked_sub(table.start_row + 1)
                .ok_or_else(|| fail("文本结束行不存在。"))? as usize;
            if end < row || end >= table.rows.len() {
                return Err(fail("选择准确的起止文本行。"));
            }
            let text = table.rows[row..=end]
                .iter()
                .map(|r| r[0].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if text.len() > 1024 * 1024
                || s.text_title.trim().is_empty()
                || !norm(&text).contains(&norm(&s.text_title))
            {
                return Err(fail("文本范围须包含填写的原文题名，且不超过 1 MB。"));
            }
            return Ok((
                vec![],
                text,
                s.text_title.trim().into(),
                String::new(),
                String::new(),
            ));
        }
        if s.row <= s.header_row || s.end_row != s.row {
            return Err(fail("从表头之后明确选择一条记录。"));
        }
        let header = s
            .header_row
            .checked_sub(table.start_row + 1)
            .and_then(|i| table.rows.get(i as usize))
            .ok_or_else(|| fail("表头行不存在。"))?;
        if values.len() != header.len() {
            return Err(fail("选中记录与表头列数不符。"));
        }
        let mut fields = vec![];
        for (i, value) in values.iter().enumerate() {
            let col = table.start_col + i as u32;
            if table.formulas.contains_key(&(s.row - 1, col)) {
                return Err(fail(
                    "选中记录含公式结果，不能作为原始数据库记录；请导出静态数据。",
                ));
            }
            fields.push(Field {
                column: col,
                address: format!("{}{r}", column(col), r = s.row),
                label: header[i].clone(),
                value: value.clone(),
            });
        }
        let get = |col: Option<u32>| -> Result<String> {
            match col {
                None => Ok(String::new()),
                Some(c) => fields
                    .iter()
                    .find(|v| v.column == c)
                    .map(|v| v.value.clone())
                    .ok_or_else(|| fail("选取的身份字段不在表内。")),
            }
        };
        let title = get(s.title_column)?;
        if serde_json::to_vec(&fields)?.len() > 1024 * 1024 {
            return Err(fail("所选记录超过 1 MB，请缩小原始记录；不会截断字段。"));
        }
        if title.trim().is_empty() {
            return Err(fail("选择来源中的真实题名列。"));
        }
        let doi = get(s.doi_column)?;
        let wos = get(s.wos_column)?;
        Ok((fields, String::new(), title, doi, wos))
    }
}
pub fn bind(
    root: &Path,
    task: &Task,
    draft: &Draft,
    selection: Selection,
    source_url: String,
    note: String,
    confirmed: bool,
) -> Result<Evidence> {
    if !confirmed || note.trim().is_empty() || note.len() > 4000 {
        return Err(fail("确认本条论文与所选记录对应，并填写具体绑定依据。"));
    }
    if !source_url.is_empty() {
        let url = url::Url::parse(&source_url).map_err(|_| fail("来源链接无效。"))?;
        if !["http", "https"].contains(&url.scheme())
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(fail("来源链接须为不含账户信息的网页地址。"));
        }
    }
    let document = check_draft(root, task, draft)?;
    let (fields, text, title, doi, wos) = document.extract(&draft.format, &selection)?;
    if title.chars().count() > 1000 {
        return Err(fail("原文题名超过 1000 字符。"));
    }
    let doi = normalized_doi(&doi);
    let wos = normalized_wos(&wos);
    if (!doi.is_empty()
        && (!regex::Regex::new(r"^10\.[0-9]{4,9}/\S+$")
            .unwrap()
            .is_match(&doi)
            || (!task.record.doi.is_empty() && normalized_doi(&task.record.doi) != doi)))
        || (!wos.is_empty()
            && (!regex::Regex::new(r"^WOS:[0-9]{15}$")
                .unwrap()
                .is_match(&wos)
                || (!task.record.wos.is_empty() && normalized_wos(&task.record.wos) != wos)))
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "来源标识符与名单冲突或格式无效，未采纳。",
        ));
    }
    let receipt = Receipt {
        schema: "original_source_v1".into(),
        task_id: task.id.clone(),
        input_hash: task.input_hash.clone(),
        record_fingerprint: task.record.fingerprint(),
        channel: draft.channel.clone(),
        original_name: draft.original_name.clone(),
        archive_path: draft.path.clone(),
        sha256: draft.sha256.clone(),
        format: draft.format.clone(),
        options: draft.options.clone(),
        actual_encoding: document.encoding,
        selection,
        fields,
        text,
        title,
        doi,
        wos,
        source_url: source_url.clone(),
        binding_note: note,
        institution_verified: false,
    };
    Ok(Evidence {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "external_metadata".into(),
        source: if source_url.is_empty() {
            draft.path.clone()
        } else {
            source_url
        },
        text: serde_json::to_string(&receipt)?,
        created: now(),
    })
}
pub fn current_receipts(task: &Task) -> Vec<Receipt> {
    task.evidence
        .iter()
        .filter(|e| e.kind == "external_metadata")
        .filter_map(|e| serde_json::from_str::<Receipt>(&e.text).ok())
        .filter(|r| {
            r.schema == "original_source_v1"
                && r.task_id == task.id
                && r.input_hash == task.input_hash
                && r.record_fingerprint == task.record.fingerprint()
        })
        .collect()
}
pub fn verify_for_ai(root: &Path, task: &Task) -> Result<std::collections::BTreeSet<String>> {
    let mut current = std::collections::BTreeSet::new();
    for evidence in task
        .evidence
        .iter()
        .filter(|e| e.kind == "external_metadata")
    {
        let receipt: Receipt = serde_json::from_str(&evidence.text)
            .map_err(|_| fail("原始来源记录不完整，请核对归档。"))?;
        if receipt.schema != "original_source_v1" {
            return Err(fail("原始来源版本不支持。"));
        }
        if receipt.task_id != task.id
            || receipt.input_hash != task.input_hash
            || receipt.record_fingerprint != task.record.fingerprint()
        {
            continue;
        }
        if !channel_formats(&receipt.channel)?.contains(&receipt.format)
            || receipt.institution_verified
        {
            return Err(fail("来源渠道或归属声明与原始来源规则不符。"));
        }
        let path = owned(
            root,
            &receipt.archive_path,
            &receipt.sha256,
            &receipt.format,
        )?;
        let document = read(&path, &receipt.format, &receipt.options)?;
        let (fields, text, title, doi, wos) =
            document.extract(&receipt.format, &receipt.selection)?;
        if fields != receipt.fields
            || text != receipt.text
            || title != receipt.title
            || normalized_doi(&doi) != receipt.doi
            || normalized_wos(&wos) != receipt.wos
            || document.encoding != receipt.actual_encoding
        {
            return Err(fail("来源缓存与原始归档不符，不能发送给 AI。"));
        }
        current.insert(evidence.id.clone());
    }
    Ok(current)
}

pub fn original_exports(root: &Path, task: &Task) -> Result<Vec<OriginalExport>> {
    let current = verify_for_ai(root, task)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut exports = Vec::new();
    for evidence in task
        .evidence
        .iter()
        .rev()
        .filter(|e| current.contains(&e.id))
    {
        let receipt: Receipt = serde_json::from_str(&evidence.text)?;
        // General/other are template or unclassified source containers.
        if matches!(receipt.channel.as_str(), "general" | "other") {
            continue;
        }
        let key = json!({"hash":receipt.sha256,"channel":receipt.channel,"selection":receipt.selection,"options":receipt.options}).to_string();
        if seen.insert(key) {
            exports.push(OriginalExport {
                evidence_id: evidence.id.clone(),
                evidence_hash: hash(evidence.text.as_bytes()),
                receipt,
            });
        }
    }
    Ok(exports)
}
pub fn read_original_export(
    root: &Path,
    task: &Task,
    evidence_id: &str,
) -> Result<(OriginalExport, Vec<u8>)> {
    let export = original_exports(root, task)?
        .into_iter()
        .find(|e| e.evidence_id == evidence_id)
        .ok_or_else(|| fail("原始导出不属于本篇当前绑定来源，请重新选择。"))?;
    let receipt = &export.receipt;
    let path = owned(
        root,
        &receipt.archive_path,
        &receipt.sha256,
        &receipt.format,
    )?;
    let raw = bytes(&path)?;
    if hash(&raw) != receipt.sha256 {
        return Err(fail("读取期间原始导出变化，未采纳。"));
    }
    Ok((export, raw))
}

/// One factual-source gate for both API requests and later material exports.
pub fn verified_evidence(root: &Path, task: &Task) -> Result<Vec<Evidence>> {
    let current = verify_for_ai(root, task)?;
    Ok(task
        .evidence
        .iter()
        .filter(|e| {
            (e.kind != "external_metadata" || current.contains(&e.id))
                && !matches!(
                    e.kind.as_str(),
                    "alias_verified"
                        | "claim_verified"
                        | "legacy_claim_verified"
                        | "source_download"
                        | "sa_read"
                        | "material_validation"
                        | "material_export"
                        | "submission_packet"
                        | "legacy_material"
                        | "legacy_history"
                        | "ai_classification"
                        | "ai_failure"
                        | "roster_input"
                )
        })
        .cloned()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task() -> Task {
        Task::new(
            Record {
                row: 2,
                owner: "Source tester".into(),
                sa_id: "source-task".into(),
                title: "Paper".into(),
                doi: "10.1234/test".into(),
                wos: "WOS:000123456789012".into(),
                staff_id: "001".into(),
                matches: 0,
                item_ids: String::new(),
                mark: String::new(),
                reason: String::new(),
                skipped: false,
                done: false,
                source: String::new(),
            },
            "roster-hash".into(),
        )
    }
    fn csv_selection() -> Selection {
        Selection {
            sheet: "CSV".into(),
            header_row: 1,
            row: 2,
            end_row: 2,
            title_column: Some(0),
            doi_column: Some(1),
            wos_column: Some(2),
            text_title: String::new(),
        }
    }
    fn csv_draft(root: &Path, t: &Task) -> Draft {
        let path = root.join("original.csv");
        std::fs::write(
            &path,
            b"Title,DOI,WOS,Abstract\nPaper,10.1234/test,WOS:000123456789012,full abstract\n",
        )
        .unwrap();
        prepare(root, t, "ei", &path, ReadOptions::default()).unwrap()
    }
    fn evidence(root: &Path, t: &Task, d: &Draft, s: Selection) -> Result<Evidence> {
        bind(
            root,
            t,
            d,
            s,
            "https://example.org/record".into(),
            "Identifiers and full title checked".into(),
            true,
        )
    }
    #[test]
    fn registry_formats_are_explicit_and_original_bytes_are_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let t = task();
        let csv = csv_draft(dir.path(), &t);
        assert_eq!(
            std::fs::read(&csv.path).unwrap(),
            std::fs::read(dir.path().join("original.csv")).unwrap()
        );
        assert_eq!(csv.sha256, hash(&std::fs::read(&csv.path).unwrap()));
        for c in crate::catalog::channels().as_array().unwrap() {
            assert_eq!(
                channel_formats(c["id"].as_str().unwrap()).unwrap(),
                c["formats"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            );
            assert_eq!(c["capabilities"]["local_source"], "implemented");
        }
        assert!(channel_formats("invented").is_err());
        assert!(prepare(
            dir.path(),
            &t,
            "wos_excel",
            &dir.path().join("original.csv"),
            ReadOptions::default()
        )
        .is_err());
        assert_eq!(t.stage, Stage::Pending);
        assert!(t.artifact.is_none());
    }
    #[test]
    fn excel_uses_physical_coordinates_preserves_duplicates_blanks_and_all_fields() {
        let dir = tempfile::tempdir().unwrap();
        let t = task();
        let path = dir.path().join("raw.xlsx");
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet.set_name("Actual").unwrap();
        for (i, h) in ["Title", "DOI", "WOS", "Name", "Name", "", "Abstract"]
            .iter()
            .enumerate()
        {
            sheet.write_string(3, i as u16 + 2, *h).unwrap();
        }
        let long = "完整摘要；不能省略。".repeat(1800);
        for (i, v) in [
            "Paper",
            "10.1234/test",
            "WOS:000123456789012",
            "Li",
            "Wang",
            "unlabelled",
            &long,
        ]
        .iter()
        .enumerate()
        {
            sheet.write_string(4, i as u16 + 2, *v).unwrap();
        }
        sheet.write_string(5, 2, "Formula paper").unwrap();
        sheet.write_formula(5, 3, "=1+1").unwrap();
        book.add_worksheet().set_name("Second").unwrap();
        book.save(&path).unwrap();
        let d = prepare(dir.path(), &t, "cnki", &path, ReadOptions::default()).unwrap();
        let doc = check_draft(dir.path(), &t, &d).unwrap();
        let p = doc.page(&d, Some("Actual"), 4, 0).unwrap();
        assert_eq!(p["rows"][0]["row"], 5);
        assert_eq!(p["columns"][0]["column"], 2);
        assert_eq!(p["columns"][3]["label"], "Name [F列]");
        assert_eq!(p["columns"][4]["label"], "Name [G列]");
        let mut s = csv_selection();
        s.sheet = "Actual".into();
        s.header_row = 4;
        s.row = 5;
        s.end_row = 5;
        s.title_column = Some(2);
        s.doi_column = Some(3);
        s.wos_column = Some(4);
        let e = evidence(dir.path(), &t, &d, s.clone()).unwrap();
        let r: Receipt = serde_json::from_str(&e.text).unwrap();
        assert_eq!(r.fields.len(), 7);
        assert_eq!(r.fields[5].label, "");
        assert_eq!(r.fields[5].value, "unlabelled");
        assert_eq!(r.fields[6].address, "I5");
        assert_eq!(r.fields[6].value, long);
        assert!(!r.institution_verified);
        assert!(doc.page(&d, Some("invented"), 4, 0).is_err());
        s.row = 6;
        s.end_row = 6;
        assert!(evidence(dir.path(), &t, &d, s).is_err());
    }
    #[test]
    fn csv_quoted_multiline_records_and_pagination_do_not_select_first_record() {
        let dir = tempfile::tempdir().unwrap();
        let t = task();
        let path = dir.path().join("rows.csv");
        let mut raw = "Title;DOI;WOS;Abstract\nPaper;10.1234/test;WOS:000123456789012;\"line 1; comma,\nline 2\"\n".to_string();
        for n in 0..65 {
            raw.push_str(&format!("Other {n};;;detail\n"));
        }
        std::fs::write(&path, raw).unwrap();
        let options = ReadOptions {
            delimiter: "semicolon".into(),
            ..ReadOptions::default()
        };
        let d = prepare(dir.path(), &t, "ei", &path, options).unwrap();
        let doc = check_draft(dir.path(), &t, &d).unwrap();
        let first = doc.page(&d, None, 1, 0).unwrap();
        let second = doc.page(&d, None, 1, 1).unwrap();
        assert_eq!(first["total"], 66);
        assert_eq!(first["rows"].as_array().unwrap().len(), 50);
        assert_eq!(second["rows"][0]["row"], 52);
        assert!(doc.page(&d, None, 1, 2).is_err());
        assert_eq!(first["rows"][0]["values"][3], "line 1; comma,\nline 2");
        let r: Receipt =
            serde_json::from_str(&evidence(dir.path(), &t, &d, csv_selection()).unwrap().text)
                .unwrap();
        assert_eq!(r.fields[3].value, "line 1; comma,\nline 2");
        let mut s = csv_selection();
        s.row = 1;
        s.end_row = 1;
        assert!(evidence(dir.path(), &t, &d, s).is_err());
        assert!(prepare(dir.path(), &t, "ei", &path, ReadOptions::default()).is_err());
    }
    #[test]
    fn decoding_is_explicit_bom_aware_and_refuses_replacement_characters() {
        let text = "中文题名\n完整摘要";
        let (encoded, _, errors) = encoding_rs::GB18030.encode(text);
        assert!(!errors);
        assert_eq!(decode(&encoded, "gb18030").unwrap().0, text);
        assert!(decode(&encoded, "utf-8").is_err());
        let mut utf16 = vec![0xff, 0xfe];
        for c in text.encode_utf16() {
            utf16.extend(c.to_le_bytes());
        }
        let decoded = decode(&utf16, "utf-8").unwrap();
        assert_eq!(decoded.0, text);
        assert_eq!(decoded.1, "UTF-16LE");
        assert_eq!(
            decode(
                &[&[0xef, 0xbb, 0xbf][..], text.as_bytes()].concat(),
                "utf-8"
            )
            .unwrap()
            .0,
            text
        );
        assert!(decode(&[0xff], "utf-8").is_err());
        assert!(decode(b"bad\0data", "utf-8").is_err());
        assert!(decode(b"text", "guess").is_err());
    }
    #[test]
    fn txt_range_preserves_only_selected_original_lines_and_requires_literal_title() {
        let dir = tempfile::tempdir().unwrap();
        let t = task();
        let path = dir.path().join("raw.txt");
        std::fs::write(
            &path,
            "Other paper\r\nOther abstract\r\nPaper\r\nFull selected abstract\r\nOther end",
        )
        .unwrap();
        let d = prepare(dir.path(), &t, "cssci", &path, ReadOptions::default()).unwrap();
        let mut s = Selection {
            sheet: "文本".into(),
            header_row: 1,
            row: 3,
            end_row: 4,
            title_column: None,
            doi_column: None,
            wos_column: None,
            text_title: "Paper".into(),
        };
        let r: Receipt =
            serde_json::from_str(&evidence(dir.path(), &t, &d, s.clone()).unwrap().text).unwrap();
        assert_eq!(r.text, "Paper\nFull selected abstract");
        assert!(r.fields.is_empty());
        s.text_title = "Invented title".into();
        assert!(evidence(dir.path(), &t, &d, s.clone()).is_err());
        s.text_title = "Paper".into();
        s.end_row = 2;
        assert!(evidence(dir.path(), &t, &d, s).is_err());
    }
    #[test]
    fn identity_conflicts_confirmation_old_revisions_and_changed_archive_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = task();
        let d = csv_draft(dir.path(), &t);
        assert!(bind(
            dir.path(),
            &t,
            &d,
            csv_selection(),
            String::new(),
            "checked".into(),
            false
        )
        .is_err());
        assert!(bind(
            dir.path(),
            &t,
            &d,
            csv_selection(),
            String::new(),
            String::new(),
            true
        )
        .is_err());
        assert!(bind(
            dir.path(),
            &t,
            &d,
            csv_selection(),
            "file:///secret".into(),
            "checked".into(),
            true
        )
        .is_err());
        t.record.doi = "10.1234/different".into();
        let mut changed = d.clone();
        changed.record_fingerprint = t.record.fingerprint();
        assert_eq!(
            evidence(dir.path(), &t, &changed, csv_selection())
                .unwrap_err()
                .code,
            "IDENTITY_CONFLICT"
        );
        t = task();
        t.revision += 1;
        assert!(check_draft(dir.path(), &t, &d).is_err());
        t = task();
        t.input_hash = "changed".into();
        assert!(check_draft(dir.path(), &t, &d).is_err());
        t = task();
        std::fs::write(&d.path, b"changed").unwrap();
        assert!(check_draft(dir.path(), &t, &d).is_err());
    }
    #[test]
    fn source_and_preview_survive_sqlite_reopen_and_stale_citations_cannot_fill_materials() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::new(dir.path()).unwrap();
        let mut t = task();
        store
            .import(vec![t.record.clone()], t.input_hash.clone())
            .unwrap();
        t = store.task(&t.id).unwrap();
        let d = csv_draft(dir.path(), &t);
        store.set_setting("source_previews", json!([d])).unwrap();
        drop(store);
        let store = crate::store::Store::new(dir.path()).unwrap();
        let saved: Vec<Draft> =
            serde_json::from_value(store.setting("source_previews").unwrap().unwrap()).unwrap();
        let e = evidence(dir.path(), &t, &saved[0], csv_selection()).unwrap();
        let id = e.id.clone();
        t.evidence.push(e);
        t.evidence.push(Evidence {
            id: "material".into(),
            kind: "material_validation".into(),
            source: String::new(),
            text: "exported".into(),
            created: 0,
        });
        store.save(&mut t, "source_attached").unwrap();
        drop(store);
        let store = crate::store::Store::new(dir.path()).unwrap();
        t = store.task(&t.id).unwrap();
        assert_eq!(t.stage, Stage::Pending);
        assert!(t.artifact.is_none());
        assert!(t.review.is_none());
        assert!(
            check_draft(dir.path(), &t, &saved[0]).is_err(),
            "Saved preview cannot attach again after revision changes"
        );
        let eligible = verified_evidence(dir.path(), &t).unwrap();
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].id, id);
        let suggestion = json!({"type":"期刊论文","confidence":"中","reason":"来源支持","channel_reason":"需确认实际模板","channel":"general","evidence_ids":[id],"missing":[],"fields":{"Title":{"value":"Paper","evidence_ids":[id]}}});
        let schema = json!({"columns":["Title"]});
        assert!(crate::catalog::validate_ai(&suggestion, &eligible, Some(&schema)).is_ok());
        let mut forged = t.clone();
        let mut receipt: Receipt = serde_json::from_str(&forged.evidence[0].text).unwrap();
        receipt.fields[3].value = "invented abstract".into();
        forged.evidence[0].text = serde_json::to_string(&receipt).unwrap();
        assert!(verified_evidence(dir.path(), &forged).is_err());
        t.input_hash = "new roster".into();
        let eligible = verified_evidence(dir.path(), &t).unwrap();
        assert!(eligible.is_empty());
        assert!(crate::catalog::validate_ai(&suggestion, &eligible, Some(&schema)).is_err());
    }
    #[test]
    fn source_report_reconstructs_all_long_fields_and_pairs_archive_with_hash() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = task();
        let path = dir.path().join("raw.csv");
        let long = "Unabridged source abstract. ".repeat(1300);
        std::fs::write(
            &path,
            format!("Title,DOI,WOS,Abstract\nPaper,10.1234/test,WOS:000123456789012,{long}\n"),
        )
        .unwrap();
        let d = prepare(dir.path(), &t, "ei", &path, ReadOptions::default()).unwrap();
        t.evidence
            .push(evidence(dir.path(), &t, &d, csv_selection()).unwrap());
        let text = t.evidence[0].text.clone();
        let output = dir.path().join("report.xlsx");
        crate::files::export_report(&[t], &output).unwrap();
        let mut book = open_workbook_auto(&output).unwrap();
        let main = book.worksheet_range("任务与来源").unwrap();
        assert_eq!(main.get_value((1, 8)).unwrap().to_string(), d.path);
        assert_eq!(main.get_value((1, 9)).unwrap().to_string(), d.sha256);
        let sources = book.worksheet_range("原始来源依据").unwrap();
        let rebuilt: String = sources.rows().skip(1).map(|r| r[6].to_string()).collect();
        assert_eq!(rebuilt, text);
        for row in sources.rows().skip(1) {
            assert_eq!(row[8].to_string(), hash(text.as_bytes()));
        }
        let r: Receipt = serde_json::from_str(&rebuilt).unwrap();
        assert_eq!(r.fields[3].value, long);
    }
}
