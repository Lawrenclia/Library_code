use crate::*;
use calamine::{open_workbook_auto, Reader};
use quick_xml::{
    events::{BytesEnd, BytesStart, BytesText, Event},
    Reader as XmlReader, Writer,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    io::{Cursor, Read, Write},
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub fingerprint: String,
    pub name: String,
    pub path: String,
    pub sheet: String,
    pub header_row: u32,
    pub columns: Vec<String>,
    pub required: Vec<String>,
    pub notes: String,
}
pub fn inspect(
    path: &Path,
    sheet_name: &str,
    header_row: u32,
    required: Vec<String>,
    notes: String,
) -> Result<Template> {
    let mut book = open_workbook_auto(path).map_err(Failure::storage)?;
    let sheets = book.worksheets();
    let (sheet, range) = if sheet_name.is_empty() {
        sheets.first()
    } else {
        sheets.iter().find(|(name, _)| name == sheet_name)
    }
    .ok_or_else(|| Failure::new("TEMPLATE_INVALID", "模板工作表不存在。"))?;
    if range.start().map(|v| v.1).unwrap_or(0) != 0 {
        return Err(Failure::new("TEMPLATE_INVALID", "模板表头须从 A 列开始。"));
    }
    let relative = header_row
        .checked_sub(range.start().map(|v| v.0).unwrap_or(0))
        .ok_or_else(|| Failure::new("TEMPLATE_INVALID", "表头行不存在。"))?;
    let columns: Vec<_> = range
        .rows()
        .nth(relative as usize)
        .ok_or_else(|| Failure::new("TEMPLATE_INVALID", "表头行不存在。"))?
        .iter()
        .map(|v| v.to_string().trim().to_string())
        .collect();
    if columns.is_empty()
        || columns.iter().any(|s| s.is_empty())
        || columns.iter().collect::<HashSet<_>>().len() != columns.len()
    {
        return Err(Failure::new("TEMPLATE_INVALID", "表头有空列或重复列。"));
    }
    if required.iter().any(|s| !columns.contains(s)) {
        return Err(Failure::new("TEMPLATE_INVALID", "必填列不在模板表头内。"));
    }
    let mut notes = notes;
    let format_notes: Vec<_> = range
        .rows()
        .take(relative as usize)
        .flat_map(|row| {
            row.iter().enumerate().filter_map(|(i, cell)| {
                let text = cell.to_string();
                if text.trim().is_empty() {
                    None
                } else {
                    columns.get(i).map(|name| format!("{name}：{text}"))
                }
            })
        })
        .collect();
    if !format_notes.is_empty() {
        notes.push_str("\n模板原文格式要求：\n");
        notes.push_str(&format_notes.join("\n"));
    }
    let fingerprint = hash(&std::fs::read(path)?);
    let id = hash(
        serde_json::to_string(&(&fingerprint, sheet, header_row, &columns, &required, &notes))?
            .as_bytes(),
    );
    Ok(Template {
        id,
        fingerprint,
        name: path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        path: path.to_string_lossy().into(),
        sheet: sheet.clone(),
        header_row,
        columns,
        required,
        notes,
    })
}
fn attr(e: &BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| {
            std::str::from_utf8(a.value.as_ref())
                .ok()
                .and_then(|s| quick_xml::escape::unescape(s).ok().map(|v| v.into_owned()))
        })
}
fn sheet_path(book: &str, rels: &str, name: &str) -> Result<String> {
    let mut r = XmlReader::from_str(book);
    let mut id = None;
    loop {
        match r.read_event().map_err(Failure::storage)? {
            Event::Start(e) | Event::Empty(e)
                if e.local_name().as_ref() == b"sheet"
                    && attr(&e, b"name").as_deref() == Some(name) =>
            {
                id = attr(&e, b"r:id")
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let id = id.ok_or_else(|| Failure::new("TEMPLATE_INVALID", "无法定位模板工作表。"))?;
    let mut r = XmlReader::from_str(rels);
    loop {
        match r.read_event().map_err(Failure::storage)? {
            Event::Start(e) | Event::Empty(e) if attr(&e, b"Id").as_deref() == Some(&id) => {
                let target = attr(&e, b"Target")
                    .ok_or_else(|| Failure::new("TEMPLATE_INVALID", "缺少工作表地址。"))?;
                if target.contains("..") || attr(&e, b"TargetMode").as_deref() == Some("External") {
                    return Err(Failure::new("TEMPLATE_INVALID", "不支持外部工作表。"));
                }
                return Ok(if target.starts_with('/') {
                    target.trim_start_matches('/').into()
                } else {
                    format!("xl/{target}")
                });
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Err(Failure::new("TEMPLATE_INVALID", "无法解析模板工作表关系。"))
}
fn column(mut n: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    s
}
fn cell(
    writer: &mut Writer<Vec<u8>>,
    ref_name: &str,
    value: &str,
    style: Option<&str>,
) -> Result<()> {
    let mut start = BytesStart::new("c");
    start.push_attribute(("r", ref_name));
    start.push_attribute(("t", "inlineStr"));
    if let Some(s) = style {
        start.push_attribute(("s", s));
    }
    writer
        .write_event(Event::Start(start))
        .map_err(Failure::storage)?;
    writer
        .write_event(Event::Start(BytesStart::new("is")))
        .map_err(Failure::storage)?;
    let mut text = BytesStart::new("t");
    text.push_attribute(("xml:space", "preserve"));
    writer
        .write_event(Event::Start(text))
        .map_err(Failure::storage)?;
    writer
        .write_event(Event::Text(BytesText::new(value)))
        .map_err(Failure::storage)?;
    for name in ["t", "is", "c"] {
        writer
            .write_event(Event::End(BytesEnd::new(name)))
            .map_err(Failure::storage)?;
    }
    Ok(())
}
fn fill_sheet(
    xml: &str,
    row: u32,
    fields: &BTreeMap<String, String>,
    columns: &[String],
) -> Result<Vec<u8>> {
    let values: BTreeMap<_, _> = columns
        .iter()
        .enumerate()
        .filter_map(|(i, name)| fields.get(name).map(|v| (format!("{}{row}", column(i)), v)))
        .collect();
    let mut reader = XmlReader::from_str(xml);
    let mut writer = Writer::new(Vec::new());
    let mut in_row = false;
    let mut inserted = false;
    let mut used = HashSet::new();
    let append = |w: &mut Writer<Vec<u8>>, used: &HashSet<String>| -> Result<()> {
        for (reference, value) in &values {
            if !used.contains(reference) {
                cell(w, reference, value, None)?;
            }
        }
        Ok(())
    };
    loop {
        let event = reader.read_event().map_err(Failure::storage)?;
        match event {
            Event::Start(e) if e.local_name().as_ref() == b"row" => {
                let current = attr(&e, b"r")
                    .and_then(|s| s.parse::<u32>().ok())
                    .ok_or_else(|| Failure::new("TEMPLATE_INVALID", "模板行号无效。"))?;
                if !inserted && current > row {
                    let mut e = BytesStart::new("row");
                    let number = row.to_string();
                    e.push_attribute(("r", number.as_str()));
                    writer
                        .write_event(Event::Start(e))
                        .map_err(Failure::storage)?;
                    append(&mut writer, &used)?;
                    writer
                        .write_event(Event::End(BytesEnd::new("row")))
                        .map_err(Failure::storage)?;
                    inserted = true;
                }
                in_row = current == row;
                writer
                    .write_event(Event::Start(e))
                    .map_err(Failure::storage)?;
            }
            Event::Start(e)
                if in_row
                    && e.local_name().as_ref() == b"c"
                    && values.contains_key(&attr(&e, b"r").unwrap_or_default()) =>
            {
                let reference = attr(&e, b"r").unwrap();
                let style = attr(&e, b"s");
                let mut depth = 1;
                while depth > 0 {
                    match reader.read_event().map_err(Failure::storage)? {
                        Event::Start(e) => {
                            if e.local_name().as_ref() == b"f" {
                                return Err(Failure::new(
                                    "TEMPLATE_INVALID",
                                    format!("{reference} 含公式，不能覆盖。"),
                                ));
                            }
                            depth += 1;
                        }
                        Event::Empty(e) if e.local_name().as_ref() == b"f" => {
                            return Err(Failure::new("TEMPLATE_INVALID", "不能覆盖模板公式。"))
                        }
                        Event::End(_) => depth -= 1,
                        Event::Eof => {
                            return Err(Failure::new("TEMPLATE_INVALID", "模板 XML 不完整。"))
                        }
                        _ => {}
                    }
                }
                cell(
                    &mut writer,
                    &reference,
                    values[&reference],
                    style.as_deref(),
                )?;
                used.insert(reference);
            }
            Event::Empty(e)
                if in_row
                    && e.local_name().as_ref() == b"c"
                    && values.contains_key(&attr(&e, b"r").unwrap_or_default()) =>
            {
                let reference = attr(&e, b"r").unwrap();
                cell(
                    &mut writer,
                    &reference,
                    values[&reference],
                    attr(&e, b"s").as_deref(),
                )?;
                used.insert(reference);
            }
            Event::End(e) if in_row && e.local_name().as_ref() == b"row" => {
                append(&mut writer, &used)?;
                in_row = false;
                inserted = true;
                writer
                    .write_event(Event::End(e))
                    .map_err(Failure::storage)?;
            }
            Event::End(e) if e.local_name().as_ref() == b"sheetData" && !inserted => {
                let mut start = BytesStart::new("row");
                let n = row.to_string();
                start.push_attribute(("r", n.as_str()));
                writer
                    .write_event(Event::Start(start))
                    .map_err(Failure::storage)?;
                append(&mut writer, &used)?;
                writer
                    .write_event(Event::End(BytesEnd::new("row")))
                    .map_err(Failure::storage)?;
                writer
                    .write_event(Event::End(e))
                    .map_err(Failure::storage)?;
                inserted = true;
            }
            Event::Eof => break,
            e => writer.write_event(e).map_err(Failure::storage)?,
        }
    }
    if !inserted {
        return Err(Failure::new(
            "TEMPLATE_INVALID",
            "缺少 sheetData，无法填写。",
        ));
    }
    Ok(writer.into_inner())
}
pub fn write(
    template: &Template,
    fields: &BTreeMap<String, String>,
    destination: &Path,
) -> Result<Vec<String>> {
    if fields.keys().any(|k| !template.columns.contains(k)) {
        return Err(Failure::new(
            "TEMPLATE_INVALID",
            "返回字段包含模板之外的列。",
        ));
    }
    if destination == Path::new(&template.path) {
        return Err(Failure::new("TEMPLATE_INVALID", "不能覆盖原始模板。"));
    }
    let raw = std::fs::read(&template.path)?;
    if hash(&raw) != template.fingerprint {
        return Err(Failure::new(
            "TEMPLATE_CHANGED",
            "模板内容已变化，请重新注册。",
        ));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(raw)).map_err(Failure::storage)?;
    let mut read_xml = |name: &str| -> Result<String> {
        let mut out = String::new();
        let file = archive.by_name(name).map_err(Failure::storage)?;
        if file.size() > 8 * 1024 * 1024 {
            return Err(Failure::new("TEMPLATE_INVALID", "工作表过大。"));
        }
        file.take(8 * 1024 * 1024 + 1).read_to_string(&mut out)?;
        Ok(out)
    };
    let book = read_xml("xl/workbook.xml")?;
    let rels = read_xml("xl/_rels/workbook.xml.rels")?;
    let target = sheet_path(&book, &rels, &template.sheet)?;
    let xml = read_xml(&target)?;
    let filled = fill_sheet(&xml, template.header_row + 2, fields, &template.columns)?;
    let mut result = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(Failure::storage)?;
        if file.name() == target {
            result
                .start_file(
                    &target,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .map_err(Failure::storage)?;
            result.write_all(&filled)?;
        } else {
            result.raw_copy_file(file).map_err(Failure::storage)?;
        }
    }
    let output = result.finish().map_err(Failure::storage)?.into_inner();
    std::fs::write(destination, output)?;
    Ok(template
        .required
        .iter()
        .filter(|k| fields.get(*k).map(|s| s.trim().is_empty()).unwrap_or(true))
        .cloned()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_styles_validations_and_formulas() {
        let xml = r#"<worksheet><sheetData><row r="2"><c r="A2" s="3"/><c r="B2"><f>1+1</f><v>2</v></c></row></sheetData><dataValidations count="1"/></worksheet>"#;
        let fields = BTreeMap::from([("题名".into(), "A & B".into())]);
        let result =
            String::from_utf8(fill_sheet(xml, 2, &fields, &["题名".into()]).unwrap()).unwrap();
        assert!(result.contains("s=\"3\""));
        assert!(result.contains("A &amp; B"));
        assert!(result.contains("<f>1+1</f>"));
        assert!(result.contains("dataValidations"));
    }
    #[test]
    fn rejects_formula_overwrite() {
        let xml = r#"<worksheet><sheetData><row r="2"><c r="A2"><f>1+1</f></c></row></sheetData></worksheet>"#;
        assert!(fill_sheet(
            xml,
            2,
            &BTreeMap::from([("题名".into(), "Paper".into())]),
            &["题名".into()]
        )
        .is_err());
    }
}
