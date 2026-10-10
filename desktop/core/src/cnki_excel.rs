//! Explicit CNKI Excel resave. Originals stay immutable; conversions are audited.
use crate::*;
use calamine::{Data, Reader};
use scraper::{ElementRef, Html, Node, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resave {
    pub schema: String,
    pub original_path: String,
    pub original_sha256: String,
    pub original_name: String,
    pub original_format: String,
    pub encoding: String,
    pub recipe: String,
    pub saved_path: String,
    pub saved_sha256: String,
    pub content_hash: String,
}
fn fail(message: &str) -> Failure {
    Failure::new("CNKI_EXCEL_INVALID", message)
}
fn bytes(path: &Path) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() > source_files::MAX_BYTES as u64
    {
        return Err(fail("CNKI Excel 不是普通文件或超过 16 MB。"));
    }
    let mut raw = Vec::new();
    fs::File::open(path)?
        .take(source_files::MAX_BYTES as u64 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > source_files::MAX_BYTES || raw.is_empty() {
        return Err(fail("CNKI Excel 为空或读取期间变大。"));
    }
    Ok(raw)
}
fn owned_directory(root: &Path, folder: &str) -> Result<PathBuf> {
    let path = root.join(folder);
    fs::create_dir_all(&path)?;
    if !path.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(fail("另存目录已被重定向。"));
    }
    Ok(path)
}
fn store_original(root: &Path, raw: &[u8], format: &str) -> Result<PathBuf> {
    let path = owned_directory(root, "source-originals")?.join(format!("{}.{}", hash(raw), format));
    if path.exists() {
        if bytes(&path)? != raw {
            return Err(fail("原件归档变化，不能覆盖。"));
        }
    } else {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(raw)?;
        file.sync_all()?;
    }
    Ok(path)
}
pub fn html_document(raw: &[u8], encoding: &str) -> Result<source_files::Document> {
    let (text, _) = source_files::decode(raw, encoding)?;
    let html = Html::parse_document(&text);
    let tables = Selector::parse("table").unwrap();
    let rows = Selector::parse("tr").unwrap();
    let unsafe_content =
        Selector::parse("script,style,img,input,textarea,select,iframe,object").unwrap();
    let mut sheets = Vec::new();
    let mut count = 0usize;
    let mut expanded = 0usize;
    for table in html.select(&tables) {
        if table
            .descendants()
            .skip(1)
            .any(|n| ElementRef::wrap(n).is_some_and(|e| e.value().name() == "table"))
        {
            return Err(fail(
                "HTML Excel 含嵌套表格，不能推断列位置，请在 Excel 手动另存。",
            ));
        }
        let mut values = Vec::new();
        for row in table.select(&rows) {
            let mut cells = Vec::new();
            for cell in row
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|c| matches!(c.value().name(), "td" | "th"))
            {
                if ["rowspan", "colspan"]
                    .iter()
                    .any(|a| cell.value().attr(a).is_some_and(|s| s != "1"))
                    || cell.select(&unsafe_content).next().is_some()
                {
                    return Err(fail("HTML Excel 含合并单元格或非文本内容，请在 Excel 核对后另存，程序不会丢弃字段。"));
                }
                let mut value = String::new();
                for node in cell.descendants() {
                    match node.value() {
                        Node::Text(text) => value.push_str(text),
                        Node::Element(e) if matches!(e.name(), "br" | "p" | "div" | "li") => {
                            value.push('\n')
                        }
                        _ => {}
                    }
                }
                if value.encode_utf16().count() > 32767 {
                    return Err(fail("单个元数据字段超过 Excel 单元格长度，不能截断另存。"));
                }
                count += 1;
                if count > 200_000 {
                    return Err(fail("Excel 超过 200000 个单元格，请缩小导出范围。"));
                }
                cells.push(value);
            }
            if !cells.is_empty() {
                values.push(cells);
            }
        }
        if values.is_empty() {
            continue;
        }
        let columns = values.iter().map(Vec::len).max().unwrap();
        expanded = expanded.saturating_add(values.len().saturating_mul(columns));
        if expanded > 200_000 {
            return Err(fail("Excel 表格展开过大。"));
        }
        for row in &mut values {
            row.resize(columns, String::new());
        }
        sheets.push((format!("CNKI {}", sheets.len() + 1), values));
    }
    if sheets.is_empty() {
        return Err(fail("文件没有元数据表格，可能是登录或错误页面。"));
    }
    Ok(source_files::Document::from_tables(sheets))
}
fn document(path: &Path, format: &str, encoding: &str) -> Result<source_files::Document> {
    let raw = bytes(path)?;
    if format == "xls" && !raw.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        return html_document(&raw, encoding);
    }
    let document = source_files::read(path, format, &source_files::ReadOptions::default())?;
    let mut book = calamine::open_workbook_auto(path).map_err(Failure::storage)?;
    for (_, range) in book.worksheets() {
        if range
            .rows()
            .flatten()
            .any(|cell| matches!(cell, Data::DateTime(_) | Data::Error(_)))
        {
            return Err(fail(
                "Excel 含日期类型或错误单元格，自动另存可能改变显示含义，请在 Excel 核对后另存。",
            ));
        }
    }
    Ok(document)
}

pub fn normalize(
    root: &Path,
    task: &Task,
    path: &Path,
    original_name: &str,
    encoding: &str,
) -> Result<source_files::Draft> {
    let format = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !matches!(format.as_str(), "xls" | "xlsx") {
        return Err(fail("另存只接受 CNKI Excel，题录 TXT 不转换为原始 Excel。"));
    }
    let raw = bytes(path)?;
    let original = store_original(root, &raw, &format)?;
    let document = document(&original, &format, encoding)?;
    let content_hash = hash(document.content().to_string().as_bytes());
    let recipe = hash(json!({"schema":"cnki_excel_resave_v1","original":hash(&raw),"format":format,"encoding":encoding,"content_hash":content_hash}).to_string().as_bytes());
    let folder = owned_directory(root, "source-resaves")?;
    let saved = folder.join(format!("{recipe}.xlsx"));
    let ledger = folder.join(format!("{recipe}.json"));
    let receipt = if ledger.exists() {
        let r: Resave = serde_json::from_slice(&bytes(&ledger)?)?;
        if r.recipe != recipe
            || r.original_sha256 != hash(&raw)
            || r.content_hash != content_hash
            || r.original_format != format
            || r.encoding != encoding
        {
            return Err(fail("原另存回执与当前原件不一致，不能覆盖。"));
        }
        verify(root, Some(&r), &saved)?;
        r
    } else {
        if saved.exists() {
            return Err(fail(
                "已有另存文件但缺少完成回执，保留文件，不能覆盖或推断成功。",
            ));
        }
        let result = document.resave_bytes()?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&saved)?;
        file.write_all(&result)?;
        file.sync_all()?;
        let reread = source_files::read(&saved, "xlsx", &source_files::ReadOptions::default())?;
        if reread.content() != document.content() {
            return Err(fail("另存前后的完整单元格不一致，文件保留但未采纳。"));
        }
        let receipt = Resave {
            schema: "cnki_excel_resave_v1".into(),
            original_path: original.to_string_lossy().into(),
            original_sha256: hash(&raw),
            original_name: original_name.into(),
            original_format: format,
            encoding: encoding.into(),
            recipe,
            saved_path: saved.to_string_lossy().into(),
            saved_sha256: hash(&result),
            content_hash,
        };
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&ledger)?;
        f.write_all(&serde_json::to_vec(&receipt)?)?;
        f.sync_all()?;
        receipt
    };
    if bytes(path)? != raw {
        return Err(fail("原件在另存期间变化，请重新选择。"));
    }
    let mut draft = source_files::prepare(
        root,
        task,
        "cnki",
        &saved,
        source_files::ReadOptions::default(),
    )?;
    draft.original_name = format!("{}（另存 XLSX）", original_name);
    draft.resave = Some(receipt);
    Ok(draft)
}

pub fn verify(root: &Path, receipt: Option<&Resave>, saved: &Path) -> Result<()> {
    let Some(r) = receipt else {
        return Ok(());
    };
    if r.schema != "cnki_excel_resave_v1"
        || !matches!(r.original_format.as_str(), "xls" | "xlsx")
        || [
            &r.original_sha256,
            &r.saved_sha256,
            &r.content_hash,
            &r.recipe,
        ]
        .iter()
        .any(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(fail("另存原件身份不完整。"));
    }
    let original = root
        .join("source-originals")
        .join(format!("{}.{}", r.original_sha256, r.original_format));
    let normalized = root
        .join("source-resaves")
        .join(format!("{}.xlsx", r.recipe));
    let ledger = root
        .join("source-resaves")
        .join(format!("{}.json", r.recipe));
    if !normalized.canonicalize()?.starts_with(root.canonicalize()?)
        || normalized.canonicalize()? != Path::new(&r.saved_path).canonicalize()?
        || serde_json::from_slice::<Value>(&bytes(&ledger)?)? != json!(r)
        || hash(&bytes(&normalized)?) != r.saved_sha256
    {
        return Err(fail("另存完成回执或保存文件变化，不能采纳。"));
    }
    if !original.canonicalize()?.starts_with(root.canonicalize()?)
        || original.canonicalize()? != Path::new(&r.original_path).canonicalize()?
        || hash(&bytes(&original)?) != r.original_sha256
        || hash(&bytes(saved)?) != r.saved_sha256
    {
        return Err(fail("另存文件或原件变化，不能用于绑定、AI 或提交材料。"));
    }
    let before = document(&original, &r.original_format, &r.encoding)?.content();
    let after = source_files::read(saved, "xlsx", &source_files::ReadOptions::default())?.content();
    let recipe = hash(json!({"schema":"cnki_excel_resave_v1","original":r.original_sha256,"format":r.original_format,"encoding":r.encoding,"content_hash":r.content_hash}).to_string().as_bytes());
    if before != after
        || hash(before.to_string().as_bytes()) != r.content_hash
        || recipe != r.recipe
    {
        return Err(fail("完整单元格与原件不一致，不能采用另存文件。"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_unicode_entities_breaks_and_keeps_formula_like_titles_as_text() {
        let doc = html_document(
            b"<table><tr><th>Title<th>Authors<tr><td>=SUM(1,2)<td>A&amp;B<br>C</table>",
            "utf-8",
        )
        .unwrap();
        assert_eq!(doc.content()[0]["cells"][2][2], "=SUM(1,2)");
        assert_eq!(doc.content()[0]["cells"][3][2], "A&B\nC");
    }
    #[test]
    fn refuses_missing_tables_merged_cells_and_nontext_data() {
        for text in [
            "<h1>Sign in</h1>",
            "<table><tr><td colspan=2>Title</table>",
            "<table><tr><td><img src=x></table>",
            "<table><tr><td><table><tr><td>x</table></table>",
        ] {
            assert!(html_document(text.as_bytes(), "utf-8").is_err());
        }
    }
}
