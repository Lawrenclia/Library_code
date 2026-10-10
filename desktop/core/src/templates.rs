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
    #[serde(default)]
    pub headers: Vec<String>,
    pub required: Vec<String>,
    pub notes: String,
    #[serde(default)]
    pub field_rules: Vec<crate::template_rules::FieldRule>,
}
pub fn inspect(
    path: &Path,
    sheet_name: &str,
    header_row: u32,
    required: Vec<String>,
    notes: String,
) -> Result<Template> {
    let raw = std::fs::read(path)?;
    let fingerprint = hash(&raw);
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
    let headers: Vec<_> = range
        .rows()
        .nth(relative as usize)
        .ok_or_else(|| Failure::new("TEMPLATE_INVALID", "表头行不存在。"))?
        .iter()
        .map(|v| v.to_string().trim().to_string())
        .collect();
    if headers.is_empty() || headers.iter().any(|s| s.is_empty()) {
        return Err(Failure::new("TEMPLATE_INVALID", "表头有空列。"));
    }
    let columns: Vec<_> = headers
        .iter()
        .enumerate()
        .map(|(i, name)| {
            if headers.iter().filter(|s| *s == name).count() > 1 {
                format!("{name} [{}列]", column(i))
            } else {
                name.clone()
            }
        })
        .collect();
    if columns.iter().collect::<HashSet<_>>().len() != columns.len() {
        return Err(Failure::new(
            "TEMPLATE_INVALID",
            "列位置标记与原表头冲突，请核对模板。",
        ));
    }
    if required.iter().any(|s| !columns.contains(s)) {
        return Err(Failure::new(
            "TEMPLATE_INVALID",
            format!(
                "必填列不在模板表头内或有重名，重名列须带列位置：{}",
                columns.join("、")
            ),
        ));
    }
    let mut field_rules = vec![];
    for (row_index, row) in range.rows().take(relative as usize).enumerate() {
        for (i, cell) in row.iter().enumerate() {
            if let Some(name) = columns.get(i) {
                field_rules.extend(crate::template_rules::instruction_rules(
                    name,
                    &format!(
                        "{sheet}!{}{}",
                        column(i),
                        range.start().unwrap_or((0, 0)).0 + row_index as u32 + 1
                    ),
                    &cell.to_string(),
                ));
            }
        }
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(&raw)).map_err(Failure::storage)?;
    let book_xml = read_xml(&mut archive, "xl/workbook.xml")?;
    let rels = read_xml(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let target = sheet_path(&book_xml, &rels, sheet)?;
    let xml = read_xml(&mut archive, &target)?;
    field_rules.extend(crate::template_rules::worksheet_rules(
        &xml,
        path,
        sheet,
        header_row + 1,
        &columns,
    )?);
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
    if fingerprint != hash(&std::fs::read(path)?) {
        return Err(Failure::new(
            "TEMPLATE_CHANGED",
            "读取期间模板发生变化，请重新注册。",
        ));
    }
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
        headers,
        required,
        notes,
        field_rules,
    })
}
fn read_xml<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<String> {
    let file = archive.by_name(name).map_err(Failure::storage)?;
    if file.size() > 8 * 1024 * 1024 {
        return Err(Failure::new("TEMPLATE_INVALID", "工作表过大。"));
    }
    let mut out = String::new();
    file.take(8 * 1024 * 1024 + 1).read_to_string(&mut out)?;
    Ok(out)
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
    let report = inspect_fields(template, fields)?;
    if !report.invalid.is_empty() || !report.requires_review.is_empty() {
        return Err(Failure::new(
            "INCOMPLETE_METADATA",
            "模板格式或枚举未通过校验，请使用材料检查查看逐字段原因。",
        ));
    }
    write_checked(template, fields, destination).map(|r| r.missing)
}
pub fn inspect_fields(
    template: &Template,
    fields: &BTreeMap<String, String>,
) -> Result<crate::template_rules::Validation> {
    let actual = inspect(
        Path::new(&template.path),
        &template.sheet,
        template.header_row,
        template.required.clone(),
        String::new(),
    )?;
    if actual.fingerprint != template.fingerprint || actual.columns != template.columns {
        return Err(Failure::new(
            "TEMPLATE_CHANGED",
            "模板内容已变化，请重新注册。",
        ));
    }
    if fields.keys().any(|k| !actual.columns.contains(k)) {
        return Err(Failure::new(
            "TEMPLATE_INVALID",
            "返回字段包含模板之外的列。",
        ));
    }
    let mut report = crate::template_rules::validate(&actual.required, &actual.field_rules, fields);
    let supplemental = template
        .notes
        .split("\n模板原文格式要求：\n")
        .next()
        .unwrap_or("")
        .trim();
    if !supplemental.is_empty() {
        report
            .requires_review
            .push(crate::template_rules::FieldProblem {
                column: "模板补充要求".into(),
                reason: "注册时的补充说明尚未自动验证，请逐项核对。".into(),
                rule_source: supplemental.into(),
            });
        report.ready = false;
    }
    if actual.required.is_empty() {
        report
            .requires_review
            .push(crate::template_rules::FieldProblem {
                column: "必填要求".into(),
                reason: "尚未配置实际模板的必填列，不能判断材料完整。".into(),
                rule_source: "模板注册要求".into(),
            });
        report.ready = false;
    }
    Ok(report)
}
pub fn write_checked(
    template: &Template,
    fields: &BTreeMap<String, String>,
    destination: &Path,
) -> Result<crate::template_rules::Validation> {
    let report = inspect_fields(template, fields)?;
    if fields.keys().any(|k| !template.columns.contains(k)) {
        return Err(Failure::new(
            "TEMPLATE_INVALID",
            "返回字段包含模板之外的列。",
        ));
    }
    if destination == Path::new(&template.path) {
        return Err(Failure::new("TEMPLATE_INVALID", "不能覆盖原始模板。"));
    }
    if destination.exists()
        && destination.canonicalize()? == Path::new(&template.path).canonicalize()?
    {
        return Err(Failure::new(
            "TEMPLATE_INVALID",
            "不能通过另一路径覆盖原始模板。",
        ));
    }
    let raw = std::fs::read(&template.path)?;
    if hash(&raw) != template.fingerprint {
        return Err(Failure::new(
            "TEMPLATE_CHANGED",
            "模板内容已变化，请重新注册。",
        ));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(raw)).map_err(Failure::storage)?;
    let book = read_xml(&mut archive, "xl/workbook.xml")?;
    let rels = read_xml(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let target = sheet_path(&book, &rels, &template.sheet)?;
    let xml = read_xml(&mut archive, &target)?;
    // Invalid values stay in the provenance, but cannot become upload cells.
    // Clear every template example cell, including optional columns without a source.
    let mut accepted: BTreeMap<String, String> = template
        .columns
        .iter()
        .map(|column| (column.clone(), String::new()))
        .collect();
    accepted.extend(fields.clone());
    for name in report
        .invalid
        .iter()
        .map(|p| &p.column)
        .chain(report.missing.iter())
    {
        accepted.insert(name.clone(), String::new());
    }
    let filled = fill_sheet(&xml, template.header_row + 2, &accepted, &template.columns)?;
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
    Ok(report)
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
    #[test]
    fn all_four_original_templates_validate_then_reopen_actual_outputs() {
        for name in ["期刊论文", "会议论文", "科技论文", "著作章节"] {
            let path =
                Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../templates/{name}.xlsx"));
            let required = vec![
                "题名".into(),
                "作者".into(),
                "作者单位".into(),
                "发表日期".into(),
            ];
            let template = inspect(&path, "", 1, required, String::new()).unwrap();
            assert!(template
                .field_rules
                .iter()
                .any(|r| matches!(r.check, crate::template_rules::Check::PublicationDate)));
            let fields = BTreeMap::from([
                ("题名".into(), "测试来源题名".into()),
                ("作者".into(), "张三(1,2);李四(1)".into()),
                ("作者单位".into(), "(1)交大;(2)其他大学".into()),
                ("发表日期".into(), "2024-02-29".into()),
            ]);
            let dir = tempfile::tempdir().unwrap();
            let output = dir.path().join("valid.xlsx");
            assert!(write_checked(&template, &fields, &output).unwrap().ready);
            let mut reopened = open_workbook_auto(&output).unwrap();
            let sheet = reopened.worksheet_range(&template.sheet).unwrap();
            for (k, v) in &fields {
                assert_eq!(
                    sheet
                        .get_value((
                            2,
                            template.columns.iter().position(|c| c == k).unwrap() as u32
                        ))
                        .unwrap()
                        .to_string(),
                    *v
                );
            }
            let mut bad = fields.clone();
            bad.insert("发表日期".into(), "2023-02-29".into());
            bad.remove("题名");
            // Legacy schemas with no cached rules still read the actual worksheet.
            let mut legacy = template.clone();
            legacy.field_rules.clear();
            let output = dir.path().join("draft.xlsx");
            let report = write_checked(&legacy, &bad, &output).unwrap();
            assert!(!report.ready);
            assert!(report.missing.contains(&"题名".into()));
            assert!(report.invalid.iter().any(|p| p.column == "发表日期"));
            let mut reopened = open_workbook_auto(&output).unwrap();
            let sheet = reopened.worksheet_range(&template.sheet).unwrap();
            assert_eq!(
                sheet
                    .get_value((
                        2,
                        template
                            .columns
                            .iter()
                            .position(|c| c == "发表日期")
                            .unwrap() as u32
                    ))
                    .unwrap()
                    .to_string(),
                ""
            );
            assert_eq!(hash(&std::fs::read(&path).unwrap()), template.fingerprint);
            let mut before =
                zip::ZipArchive::new(Cursor::new(std::fs::read(&path).unwrap())).unwrap();
            let mut after =
                zip::ZipArchive::new(Cursor::new(std::fs::read(&output).unwrap())).unwrap();
            assert_eq!(
                read_xml(&mut before, "xl/styles.xml").unwrap(),
                read_xml(&mut after, "xl/styles.xml").unwrap()
            );
        }
    }
    #[test]
    fn invalid_or_missing_cells_clear_old_sample_values_and_changed_template_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.xlsx");
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet
            .write_string(
                0,
                0,
                "格式：yyyy || yyyy-MM || yyyy-MM-dd || yyyy-MM-dd HH:mm:ss",
            )
            .unwrap();
        sheet.write_string(1, 0, "发表日期").unwrap();
        sheet.write_string(1, 1, "题名").unwrap();
        sheet.write_string(2, 0, "2000-01-01").unwrap();
        sheet.write_string(2, 1, "示例题名").unwrap();
        book.save(&path).unwrap();
        let template = inspect(&path, "", 1, vec!["题名".into()], String::new()).unwrap();
        let output = dir.path().join("draft.xlsx");
        let report = write_checked(
            &template,
            &BTreeMap::from([("发表日期".into(), "not-a-date".into())]),
            &output,
        )
        .unwrap();
        assert!(!report.ready);
        let mut book = open_workbook_auto(&output).unwrap();
        let data = book.worksheet_range(&template.sheet).unwrap();
        assert_eq!(data.get_value((2, 0)).unwrap().to_string(), "");
        assert_eq!(data.get_value((2, 1)).unwrap().to_string(), "");
        let mut supplemental = template.clone();
        supplemental.notes = "编码必须符合平台自定义规则".into();
        assert!(inspect_fields(&supplemental, &BTreeMap::new())
            .unwrap()
            .requires_review
            .iter()
            .any(|p| p.column == "模板补充要求"));
        std::fs::write(&path, b"changed").unwrap();
        assert!(write_checked(&template, &BTreeMap::new(), &output).is_err());
    }
    #[test]
    fn duplicated_headers_keep_both_columns_and_do_not_accept_ambiguous_required_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("same.xlsx");
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(1, 0, "页数").unwrap();
        sheet.write_string(1, 1, "页数").unwrap();
        book.save(&path).unwrap();
        assert!(inspect(&path, "", 1, vec!["页数".into()], String::new()).is_err());
        let template = inspect(
            &path,
            "",
            1,
            vec!["页数 [A列]".into(), "页数 [B列]".into()],
            String::new(),
        )
        .unwrap();
        assert_eq!(template.headers, vec!["页数", "页数"]);
        let output = dir.path().join("both.xlsx");
        assert!(
            write_checked(
                &template,
                &BTreeMap::from([
                    ("页数 [A列]".into(), "11".into()),
                    ("页数 [B列]".into(), "22".into())
                ]),
                &output
            )
            .unwrap()
            .ready
        );
        let mut reopened = open_workbook_auto(&output).unwrap();
        let data = reopened.worksheet_range(&template.sheet).unwrap();
        assert_eq!(data.get_value((1, 0)).unwrap().to_string(), "页数");
        assert_eq!(data.get_value((1, 1)).unwrap().to_string(), "页数");
        assert_eq!(data.get_value((2, 0)).unwrap().to_string(), "11");
        assert_eq!(data.get_value((2, 1)).unwrap().to_string(), "22");
    }
}
