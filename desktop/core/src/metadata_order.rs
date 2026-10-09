//! Reorder complete source-backed entities; preserve IDs and affiliation links.
use crate::*;
use serde_json::Value;

fn permutation(value: &Value, count: usize) -> Result<Vec<usize>> {
    let indices = value
        .as_array()
        .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "需明确核对完整顺序。"))?;
    let order: Vec<usize> = indices
        .iter()
        .map(|v| {
            v.as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "顺序必须使用完整原列表的位置。"))
        })
        .collect::<Result<_>>()?;
    let mut sorted = order.clone();
    sorted.sort_unstable();
    if count < 2 || sorted != (0..count).collect::<Vec<_>>() {
        return Err(Failure::new(
            "REVIEW_REQUIRED",
            "必须保留全部原条目，不能重复、缺失或新增作者和单位。",
        ));
    }
    if order == (0..count).collect::<Vec<_>>() {
        return Err(Failure::new("REVIEW_REQUIRED", "顺序没有变化，无需提交。"));
    }
    Ok(order)
}
fn original(form: &Value, field: &str) -> Result<Vec<Value>> {
    let rows = form["metadata"][field]
        .as_array()
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "缺少完整作者或单位列表。"))?;
    if rows
        .iter()
        .enumerate()
        .any(|(index, row)| row["order"].as_u64() != Some(index as u64 + 1))
    {
        return Err(Failure::new(
            "PAGE_UNSUPPORTED",
            "原始顺序与列表位置不一致，请先核对实际表单。",
        ));
    }
    Ok(rows.clone())
}
pub fn reordered_form(form: &Value, operation: &str, order: &Value) -> Result<Value> {
    let field = match operation {
        "author_order" => "author",
        "institution_order" => "authorInstitution",
        _ => return Err(Failure::new("PAGE_UNSUPPORTED", "未知顺序修改方式。")),
    };
    let rows = original(form, field)?;
    let indices = permutation(order, rows.len())?;
    let mut wanted = form.clone();
    let sorted: Vec<Value> = indices
        .iter()
        .enumerate()
        .map(|(index, old)| {
            let mut row = rows[*old].clone();
            row["order"] = (index + 1).into();
            row
        })
        .collect();
    wanted["metadata"][field] = sorted.into();
    if operation == "institution_order" {
        let mut mapping = vec![0; indices.len() + 1];
        for (index, old) in indices.iter().enumerate() {
            mapping[old + 1] = index + 1;
        }
        let authors = wanted["metadata"]["author"]
            .as_array_mut()
            .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "单位调整缺少完整作者关联。"))?;
        for author in authors {
            let (tokens, array) = if let Some(s) = author["institutionOrderNums"].as_str() {
                (s.split(',').map(str::to_owned).collect::<Vec<_>>(), false)
            } else if let Some(a) = author["institutionOrderNums"].as_array() {
                (
                    a.iter()
                        .map(|v| {
                            v.as_str().map(str::to_owned).ok_or_else(|| {
                                Failure::new("PAGE_UNSUPPORTED", "原单位编号不是完整文本。")
                            })
                        })
                        .collect::<Result<Vec<_>>>()?,
                    true,
                )
            } else {
                return Err(Failure::new("PAGE_UNSUPPORTED", "原单位编号结构无法核对。"));
            };
            let mut remapped = Vec::new();
            for token in tokens {
                let n = token
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0 && *n < mapping.len() && n.to_string() == token)
                    .ok_or_else(|| {
                        Failure::new(
                            "PAGE_UNSUPPORTED",
                            "原单位编号包含空值、非法值或越界，不能自动重排。",
                        )
                    })?;
                if remapped.contains(&mapping[n].to_string()) {
                    return Err(Failure::new("PAGE_UNSUPPORTED", "原作者单位编号重复。"));
                }
                remapped.push(mapping[n].to_string());
            }
            if remapped.is_empty() {
                return Err(Failure::new("PAGE_UNSUPPORTED", "作者缺少实际单位关联。"));
            }
            author["institutionOrderNums"] = if array {
                remapped.into()
            } else {
                remapped.join(",").into()
            };
        }
    }
    Ok(wanted)
}
