//! Checks derived from the actual worksheet; they never invent a publication fact.
use crate::*;
use calamine::{open_workbook_auto, Reader};
use quick_xml::{events::Event, Reader as XmlReader};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldRule {
    pub column: String,
    pub source: String,
    pub check: Check,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Check {
    PublicationDate,
    EnglishSemicolons,
    AuthorAffiliations,
    NumberedAffiliations,
    GrantNumber,
    Enumeration { values: Vec<String> },
    Manual,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldProblem {
    pub column: String,
    pub reason: String,
    pub rule_source: String,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Validation {
    pub missing: Vec<String>,
    pub invalid: Vec<FieldProblem>,
    pub requires_review: Vec<FieldProblem>,
    pub ready: bool,
}

pub fn instruction_rules(column: &str, source: &str, text: &str) -> Vec<FieldRule> {
    let mut checks = vec![];
    if text.contains("使用英文分号分隔") {
        checks.push(Check::EnglishSemicolons);
    }
    if text.contains("yyyy || yyyy-MM || yyyy-MM-dd || yyyy-MM-dd HH:mm:ss") {
        checks.push(Check::PublicationDate);
    } else if text.contains("张三(1,2);李四(1)") {
        checks.push(Check::AuthorAffiliations);
    } else if text.contains("(1)XX大学;(2)XX科技大学") {
        checks.push(Check::NumberedAffiliations);
    } else if text.contains("项目名称#项目编号") {
        checks.push(Check::GrantNumber);
    } else if text.contains("格式") || text.contains("枚举") || text.contains("必须") {
        checks.push(Check::Manual);
    }
    checks
        .into_iter()
        .map(|check| FieldRule {
            column: column.into(),
            source: format!("{source}：{text}"),
            check,
        })
        .collect()
}
fn attr(e: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| {
            std::str::from_utf8(a.value.as_ref())
                .ok()
                .and_then(|s| quick_xml::escape::unescape(s).ok().map(|s| s.into_owned()))
        })
}
fn coordinate(value: &str) -> Option<(u32, u32)> {
    let v = value.replace('$', "");
    let pos = v.find(|c: char| c.is_ascii_digit())?;
    let mut col: u32 = 0;
    for c in v[..pos].bytes() {
        if !c.is_ascii_uppercase() {
            return None;
        }
        col = col.checked_mul(26)?.checked_add((c - b'A' + 1) as u32)?;
    }
    Some((
        v[pos..].parse::<u32>().ok()?.checked_sub(1)?,
        col.checked_sub(1)?,
    ))
}
fn contains_cell(range: &str, row: u32, col: u32) -> bool {
    range.split_whitespace().any(|part| {
        let (start, end) = part.split_once(':').unwrap_or((part, part));
        match (coordinate(start), coordinate(end)) {
            (Some((r1, c1)), Some((r2, c2))) => {
                (r1..=r2).contains(&row) && (c1..=c2).contains(&col)
            }
            _ => false,
        }
    })
}
fn enum_values(formula: &str, path: &Path, sheet: &str) -> Option<Vec<String>> {
    let f = formula.trim().trim_start_matches('=');
    if f.starts_with('"') && f.ends_with('"') && f.len() >= 2 {
        return Some(f[1..f.len() - 1].split(',').map(str::to_string).collect());
    }
    // Only fixed, local ranges. Named/dynamic/external formulas stay explicit review items.
    let (sheet, range) = f.rsplit_once('!').unwrap_or((sheet, f));
    let sheet = sheet.trim_matches('\'').replace("''", "'");
    if sheet.contains('[') {
        return None;
    }
    let (start, end) = range.split_once(':').unwrap_or((range, range));
    let ((r1, c1), (r2, c2)) = (coordinate(start)?, coordinate(end)?);
    if r2 < r1 || c2 < c1 || (r2 - r1 + 1) as u64 * (c2 - c1 + 1) as u64 > 1000 {
        return None;
    }
    let mut book = open_workbook_auto(path).ok()?;
    let data = book.worksheet_range(&sheet).ok()?;
    let formulas = book.worksheet_formula(&sheet).ok()?;
    let mut values = vec![];
    for r in r1..=r2 {
        for c in c1..=c2 {
            if formulas.get_value((r, c)).is_some_and(|s| !s.is_empty()) {
                return None;
            }
            if let Some(cell) = data.get_value((r, c)) {
                let value = cell.to_string();
                if !value.is_empty() {
                    values.push(value);
                }
            }
        }
    }
    (!values.is_empty()).then_some(values)
}
pub fn worksheet_rules(
    xml: &str,
    path: &Path,
    sheet: &str,
    row: u32,
    columns: &[String],
) -> Result<Vec<FieldRule>> {
    let mut reader = XmlReader::from_str(xml);
    let mut rules = vec![];
    let mut current: Option<(String, String, String)> = None;
    let mut formula = String::new();
    let mut in_formula = false;
    let mut in_range = false;
    loop {
        match reader.read_event().map_err(Failure::storage)? {
            Event::Empty(e) if e.local_name().as_ref() == b"dataValidation" => {
                let kind = attr(&e, b"type").unwrap_or_default();
                let range = attr(&e, b"sqref").unwrap_or_default();
                if kind != "none" {
                    for (col, name) in columns.iter().enumerate() {
                        if contains_cell(&range, row, col as u32) {
                            rules.push(FieldRule {
                                column: name.clone(),
                                source: format!("{sheet}!{range} 数据有效性 ({kind}) 无可解析公式"),
                                check: Check::Manual,
                            });
                        }
                    }
                }
            }
            Event::Start(e) if e.local_name().as_ref() == b"dataValidation" => {
                current = Some((
                    attr(&e, b"type").unwrap_or_default(),
                    attr(&e, b"sqref").unwrap_or_default(),
                    attr(&e, b"error").unwrap_or_default(),
                ));
                formula.clear();
            }
            Event::Start(e) if current.is_some() && e.local_name().as_ref() == b"formula1" => {
                in_formula = true
            }
            Event::Start(e) if current.is_some() && e.local_name().as_ref() == b"sqref" => {
                in_range = true;
            }
            Event::Text(e) if in_range => {
                if let Some((_, range, _)) = current.as_mut() {
                    range.push_str(&e.decode().map_err(Failure::storage)?);
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"sqref" => {
                in_range = false;
            }
            Event::Text(e) if in_formula => formula.push_str(
                &quick_xml::escape::unescape(&e.decode().map_err(Failure::storage)?)
                    .map_err(Failure::storage)?,
            ),
            Event::End(e) if e.local_name().as_ref() == b"formula1" => in_formula = false,
            Event::End(e) if e.local_name().as_ref() == b"dataValidation" => {
                if let Some((kind, range, note)) = current.take() {
                    if kind == "none" {
                        continue;
                    }
                    for (col, name) in columns.iter().enumerate() {
                        if contains_cell(&range, row, col as u32) {
                            let check = if kind == "list" {
                                enum_values(&formula, path, sheet)
                                    .map(|values| Check::Enumeration { values })
                                    .unwrap_or(Check::Manual)
                            } else {
                                Check::Manual
                            };
                            rules.push(FieldRule {
                                column: name.clone(),
                                source: format!(
                                    "{sheet}!{range} 数据有效性 ({kind}) {note} {formula}"
                                ),
                                check,
                            });
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(rules)
}

fn date_valid(value: &str) -> bool {
    let re = regex::Regex::new(
        r"^([0-9]{4})(?:-([0-9]{2})(?:-([0-9]{2})(?: ([0-9]{2}):([0-9]{2}):([0-9]{2}))?)?)?$",
    )
    .unwrap();
    let Some(m) = re.captures(value) else {
        return false;
    };
    let year: u32 = m[1].parse().unwrap_or(0);
    if year == 0 {
        return false;
    }
    let number = |i| m.get(i).and_then(|v| v.as_str().parse::<u32>().ok());
    if let Some(month) = number(2) {
        if !(1..=12).contains(&month) {
            return false;
        }
        if let Some(day) = number(3) {
            let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
            let max = [
                31,
                if leap { 29 } else { 28 },
                31,
                30,
                31,
                30,
                31,
                31,
                30,
                31,
                30,
                31,
            ][month as usize - 1];
            if day == 0 || day > max {
                return false;
            }
        }
    }
    number(4).is_none_or(|v| v < 24)
        && number(5).is_none_or(|v| v < 60)
        && number(6).is_none_or(|v| v < 60)
}
fn author_ids(value: &str) -> Option<Vec<String>> {
    let re = regex::Regex::new(r"^.+\(([1-9][0-9]*(?:,[1-9][0-9]*)*)\)$").unwrap();
    let mut ids = vec![];
    for entry in value.split(';') {
        ids.extend(
            re.captures(entry.trim())?
                .get(1)?
                .as_str()
                .split(',')
                .map(str::to_string),
        );
    }
    Some(ids)
}
fn unit_ids(value: &str) -> Option<HashSet<String>> {
    let re = regex::Regex::new(r"^\(([1-9][0-9]*)\).+$").unwrap();
    let mut ids = HashSet::new();
    for entry in value.split(';') {
        if !ids.insert(re.captures(entry.trim())?.get(1)?.as_str().to_string()) {
            return None;
        }
    }
    Some(ids)
}
pub fn validate(
    required: &[String],
    rules: &[FieldRule],
    fields: &BTreeMap<String, String>,
) -> Validation {
    let mut result = Validation {
        missing: required
            .iter()
            .filter(|k| fields.get(*k).is_none_or(|v| v.trim().is_empty()))
            .cloned()
            .collect(),
        ..Default::default()
    };
    for rule in rules {
        let Some(value) = fields.get(&rule.column).filter(|v| !v.trim().is_empty()) else {
            continue;
        };
        let error = match &rule.check {
            Check::PublicationDate => {
                (!date_valid(value)).then_some("日期格式或实际日期无效，需使用模板列出的格式。")
            }
            Check::EnglishSemicolons => (value.contains('；')
                || value.split(';').any(|s| s.trim().is_empty()))
            .then_some("多值须用英文分号分隔，不能含空项。"),
            Check::AuthorAffiliations => author_ids(value)
                .is_none()
                .then_some("作者须按姓名(单位编号)填写，多个编号使用英文逗号。"),
            Check::NumberedAffiliations => unit_ids(value)
                .is_none()
                .then_some("单位须按(编号)单位名填写，编号不能重复。"),
            Check::GrantNumber => value
                .split(';')
                .any(|s| s.matches('#').count() != 1 || s.trim() == "#")
                .then_some("每个资助项目须保留项目名称#项目编号分隔符。"),
            Check::Enumeration { values } => {
                (!values.iter().any(|v| v == value)).then_some("值不在模板的实际可选枚举中。")
            }
            Check::Manual => {
                result.requires_review.push(FieldProblem {
                    column: rule.column.clone(),
                    reason: "此模板规则需要人工核对，尚未自动验证。".into(),
                    rule_source: rule.source.clone(),
                });
                None
            }
        };
        if let Some(reason) = error {
            result.invalid.push(FieldProblem {
                column: rule.column.clone(),
                reason: reason.into(),
                rule_source: rule.source.clone(),
            });
        }
    }
    if rules
        .iter()
        .any(|r| r.column == "作者" && matches!(r.check, Check::AuthorAffiliations))
        && rules
            .iter()
            .any(|r| r.column == "作者单位" && matches!(r.check, Check::NumberedAffiliations))
    {
        if let Some(authors) = fields.get("作者").and_then(|v| author_ids(v)) {
            let units = fields
                .get("作者单位")
                .and_then(|v| unit_ids(v))
                .unwrap_or_default();
            if authors.iter().any(|id| !units.contains(id)) {
                result.invalid.push(FieldProblem {
                    column: "作者".into(),
                    reason: "作者引用了尚未提供或无效的单位编号。".into(),
                    rule_source: "模板作者/作者单位编号对应关系".into(),
                });
            }
        }
    }
    result.ready =
        result.missing.is_empty() && result.invalid.is_empty() && result.requires_review.is_empty();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_calendar_and_precision_are_preserved() {
        for s in ["2026", "2026-10", "2024-02-29", "2026-10-10 23:59:59"] {
            assert!(date_valid(s), "{s}");
        }
        for s in [
            "0000",
            "2023-02-29",
            "1900-02-29",
            "2026-13",
            "2026-04-31",
            "2026-10-10 24:00:00",
            "2026/10/10",
            "2026-1-1",
        ] {
            assert!(!date_valid(s), "{s}");
        }
    }
    fn rules() -> Vec<FieldRule> {
        [
            (
                "作者",
                "多值字段(使用英文分号分隔)。格式：张三(1,2);李四(1)",
            ),
            (
                "作者单位",
                "多值字段(使用英文分号分隔)。格式：(1)XX大学;(2)XX科技大学",
            ),
            (
                "资助项目",
                "多值字段(使用英文分号分隔)。格式：项目名称#项目编号",
            ),
        ]
        .iter()
        .flat_map(|(k, v)| instruction_rules(k, "原模板", v))
        .collect()
    }
    #[test]
    fn author_units_must_match_without_inventing_missing_units() {
        let mut fields = BTreeMap::from([
            ("作者".into(), "张三(1,2);李四(1)".into()),
            ("作者单位".into(), "(1)交大;(2)外单位".into()),
        ]);
        assert!(validate(&[], &rules(), &fields).ready);
        fields.insert("作者单位".into(), "(1)交大".into());
        assert!(validate(&[], &rules(), &fields)
            .invalid
            .iter()
            .any(|e| e.column == "作者"));
        fields.insert("作者单位".into(), "(1)交大;(1)外单位".into());
        assert!(!validate(&[], &rules(), &fields).ready);
        fields.insert("作者单位".into(), "(1)交大；(2)外单位".into());
        assert!(!validate(&[], &rules(), &fields).ready);
    }
    #[test]
    fn grants_keep_separators_and_do_not_make_up_unknown_numbers() {
        for s in ["项目#123", "项目#;#123"] {
            assert!(
                validate(
                    &[],
                    &rules(),
                    &BTreeMap::from([("资助项目".into(), s.into())])
                )
                .ready
            );
        }
        for s in ["项目123", "项目#123;", "项目#1#2", "#"] {
            assert!(
                !validate(
                    &[],
                    &rules(),
                    &BTreeMap::from([("资助项目".into(), s.into())])
                )
                .ready
            );
        }
    }
    #[test]
    fn unknown_instruction_and_dynamic_validation_require_review() {
        let mut rule = instruction_rules("代码", "模板", "格式：AA-999");
        rule.extend(worksheet_rules(r#"<worksheet><dataValidation type="list" sqref="A3"><formula1>INDIRECT(B3)</formula1></dataValidation><dataValidation type="custom" sqref="A3"/></worksheet>"#,Path::new("nonexistent"),"Sheet1",2,&["代码".into()]).unwrap());
        let result = validate(
            &[],
            &rule,
            &BTreeMap::from([("代码".into(), "AA-123".into())]),
        );
        assert_eq!(result.requires_review.len(), 3);
        assert!(!result.ready);
    }
    #[test]
    fn actual_lists_ranges_and_target_row_own_the_enum() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.xlsx");
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        sheet.set_name("选项").unwrap();
        sheet.write_string(0, 0, "期刊").unwrap();
        sheet.write_string(1, 0, "会议").unwrap();
        book.save(&path).unwrap();
        let xml = r#"<worksheet><dataValidations><dataValidation type="list" sqref="A3 A5:A9"><formula1>'选项'!$A$1:$A$2</formula1></dataValidation><dataValidation type="list" sqref="B3"><formula1>"yes,no"</formula1></dataValidation><dataValidation type="list" sqref="A4"><formula1>"wrong,row"</formula1></dataValidation></dataValidations></worksheet>"#;
        let rules =
            worksheet_rules(xml, &path, "Sheet1", 2, &["类型".into(), "开关".into()]).unwrap();
        assert_eq!(rules.len(), 2);
        assert!(
            validate(
                &[],
                &rules,
                &BTreeMap::from([
                    ("类型".into(), "期刊".into()),
                    ("开关".into(), "yes".into())
                ])
            )
            .ready
        );
        assert!(
            !validate(
                &[],
                &rules,
                &BTreeMap::from([("类型".into(), "臆造类型".into())])
            )
            .ready
        );
    }
}
