//! RIS source reading only: never converts a citation file into a platform export.
use crate::*;
use std::collections::BTreeMap;

const MAX_FIELDS: usize = 200_000;
const MAX_RECORD_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub struct Record {
    pub fields: Vec<(String, String)>,
    pub raw: String,
    pub title: String,
    pub doi: String,
}

fn invalid(message: &str) -> Failure {
    Failure::new("RIS_INVALID", message)
}

fn tagged(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    if bytes.len() < 5
        || !bytes[0].is_ascii_uppercase()
        || !(bytes[1].is_ascii_uppercase() || bytes[1].is_ascii_digit())
        || bytes[2] != b' '
    {
        return None;
    }
    let rest = line[2..].trim_start_matches(' ');
    let value = rest.strip_prefix('-')?;
    // Only a standalone separator is a field; prose containing a dash isn't.
    if !value.is_empty() && !value.starts_with([' ', '\t']) {
        return None;
    }
    Some((&line[..2], value.strip_prefix(' ').unwrap_or(value)))
}

pub fn recognizes(text: &str) -> bool {
    text.trim_start_matches('\u{feff}')
        .lines()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| tagged(line.trim_end_matches('\r')))
        .is_some_and(|(key, _)| key == "TY")
}

fn finish(mut record: Record) -> Result<Record> {
    let titles: Vec<_> = record
        .fields
        .iter()
        .filter(|(key, value)| {
            matches!(key.as_str(), "TI" | "T1" | "CT") && !value.trim().is_empty()
        })
        .map(|(_, value)| value.trim())
        .collect();
    let Some(title) = titles.first() else {
        return Err(invalid("RIS 记录缺少 TI、T1 或 CT 题名，不能采纳。"));
    };
    if titles.iter().any(|value| norm(value) != norm(title)) {
        return Err(invalid("RIS 同一记录有多个不同题名，请核对原始导出。"));
    }
    record.title = title.to_string();
    let dois: Vec<_> = record
        .fields
        .iter()
        .filter(|(key, value)| key == "DO" && !value.trim().is_empty())
        .map(|(_, value)| value.trim())
        .collect();
    if let Some(doi) = dois.first() {
        if dois
            .iter()
            .any(|value| normalized_doi(value) != normalized_doi(doi))
        {
            return Err(invalid("RIS 同一记录有多个不同 DOI，请核对原始导出。"));
        }
        record.doi = doi.to_string();
    }
    Ok(record)
}

/// Require complete TY ... ER records. No partial record or first-result adoption.
pub fn parse(text: &str) -> Result<Vec<Record>> {
    let mut records = Vec::new();
    let mut current: Option<Record> = None;
    let mut field_count = 0usize;
    for original in text.trim_start_matches('\u{feff}').split_inclusive('\n') {
        let line = original.trim_end_matches(['\r', '\n']);
        if let Some((key, value)) = tagged(line) {
            if key == "TY" {
                if current.is_some() || value.trim().is_empty() {
                    return Err(invalid("RIS 缺少上一条 ER 结束标记，或 TY 类型为空。"));
                }
                current = Some(Record {
                    fields: vec![],
                    raw: String::new(),
                    title: String::new(),
                    doi: String::new(),
                });
            }
            let record = current
                .as_mut()
                .ok_or_else(|| invalid("RIS 字段不在 TY 与 ER 记录边界内。"))?;
            field_count += 1;
            if field_count > MAX_FIELDS {
                return Err(invalid("RIS 字段超过 200000 个，请缩小导出范围。"));
            }
            record.raw.push_str(original);
            record.fields.push((key.into(), value.into()));
            if record.raw.len() > MAX_RECORD_BYTES {
                return Err(invalid("单条 RIS 记录超过 1 MB，不会截断字段。"));
            }
            if key == "ER" {
                if !value.trim().is_empty() {
                    return Err(invalid("RIS 的 ER 结束标记包含额外内容。"));
                }
                records.push(finish(current.take().unwrap())?);
            }
        } else if let Some(record) = current.as_mut() {
            record.raw.push_str(original);
            if record.raw.len() > MAX_RECORD_BYTES {
                return Err(invalid("单条 RIS 记录超过 1 MB，不会截断字段。"));
            }
            // Preserve multiline values verbatim, including blank continuation lines.
            let (key, value) = record.fields.last_mut().unwrap();
            if key == "TY" && !line.trim().is_empty() {
                return Err(invalid("RIS 类型后出现没有字段标记的内容。"));
            }
            value.push('\n');
            value.push_str(line);
        } else if !line.trim().is_empty() {
            return Err(invalid("RIS 记录外存在非空内容，不能作为完整题录读取。"));
        }
    }
    if current.is_some() {
        return Err(invalid(
            "RIS 最后一条缺少 ER 结束标记，请重新取得完整文件。",
        ));
    }
    if records.is_empty() {
        return Err(invalid("RIS 没有完整记录。"));
    }
    Ok(records)
}

pub fn rows(text: &str) -> Result<Vec<Vec<String>>> {
    let records = parse(text)?;
    let mut tags = Vec::new();
    for record in &records {
        for (tag, _) in &record.fields {
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
    }
    if (records.len() + 1).saturating_mul(tags.len() + 3) > MAX_FIELDS {
        return Err(invalid("RIS 预览超过 200000 个单元格，请缩小导出范围。"));
    }
    let mut header = vec!["题名（RIS）".into(), "DOI（RIS）".into()];
    header.extend(tags.iter().cloned());
    header.push("原始完整记录".into());
    let mut rows = vec![header];
    for record in records {
        let mut fields: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (tag, value) in record.fields {
            fields.entry(tag).or_default().push(value);
        }
        let mut row = vec![record.title, record.doi];
        row.extend(tags.iter().map(|tag| {
            fields
                .get(tag)
                .map(|values| values.join("\n"))
                .unwrap_or_default()
        }));
        row.push(record.raw);
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retains_full_records_repeated_fields_and_unknown_multiline_content() {
        let text = "\u{feff}TY  - JOUR\r\nTI  - Paper\r\nAU  - 张三\r\nAU  - 李四\r\nDO  - https://doi.org/10.1234/test\r\nAB  - first line\r\nsecond line\r\nZZ  - unknown\r\nER  - \r\n\r\nTY  - BOOK\r\nT1  - Other\r\nER  -";
        let records = parse(text).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].raw,
            text.trim_start_matches('\u{feff}')
                .split("\r\n\r\n")
                .next()
                .unwrap()
                .to_string()
                + "\r\n"
        );
        let table = rows(text).unwrap();
        let authors = table[0].iter().position(|field| field == "AU").unwrap();
        let abstract_column = table[0].iter().position(|field| field == "AB").unwrap();
        assert_eq!(table[1][authors], "张三\n李四");
        assert_eq!(table[1][abstract_column], "first line\nsecond line");
        assert_eq!(table[1].last().unwrap(), &records[0].raw);
    }
    #[test]
    fn rejects_incomplete_boundaries_and_conflicting_identity_fields() {
        for text in [
            "TY  - JOUR\nTI  - Paper\n",
            "TY  - JOUR\nTI  - Paper\nTY  - BOOK\nT1  - Other\nER  -",
            "TI  - Paper\nER  -",
            "TY  - JOUR\nTI  - Paper\nT1  - Other\nER  -",
            "TY  - JOUR\nTI  - Paper\nDO  - 10.1234/a\nDO  - 10.1234/b\nER  -",
            "TY  - JOUR\nAU  - Name\nER  -",
            "TY  - JOUR\nTI  - Paper\nER  - extra",
            "TY  - JOUR\nTI  - Paper\nER  -\ntrailing content",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
        assert!(parse("TY  - JOUR\nTI  - Paper\nT1  - Paper\nER  -").is_ok());
        assert!(!recognizes("A paper mentioning TY  - JOUR"));
    }
}
