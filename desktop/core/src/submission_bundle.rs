//! A local handoff contains exact selected bytes, full provenance, and honest capability limits.
//! Exporting it is never an upload, import, push, claim, or SA completion.
use crate::*;
use serde_json::{json, Value};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::Path,
};

const MAX_ENTRY: u64 = 64 * 1024 * 1024;
fn fail(message: &str) -> Failure {
    Failure::new("SUBMISSION_BUNDLE_INVALID", message)
}
fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit {
        return Err(fail("资料包来源不是受支持的普通文件或超过大小限制。"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(fail("资料包来源在读取期间变大。"));
    }
    Ok(bytes)
}

pub(crate) struct Bundle {
    pub bytes: Vec<u8>,
    pub manifest: Value,
}
pub(crate) fn build(store: &Store, id: &str, packet_id: &str, revision: i64) -> Result<Bundle> {
    let (task, packet, prepared) = submission::bundle_scope(store, id, packet_id, revision)?;
    let material = read_file(Path::new(&packet.material.path), MAX_ENTRY)?;
    if hash(&material) != packet.material.sha256 {
        return Err(fail("当前选定材料的字节变化，未生成资料包。"));
    }
    let audit = read_file(Path::new(&packet.material.audit), MAX_ENTRY)?;
    let audit_value: Value = serde_json::from_slice(&audit)?;
    if !task.evidence.iter().any(|e| {
        e.kind == "material_validation"
            && serde_json::from_str::<Value>(&e.text).ok().as_ref() == Some(&audit_value)
    }) {
        return Err(fail("复制的材料来源记录与本条保存依据不一致。"));
    }
    let extension = Path::new(&packet.material.path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .ok_or_else(|| fail("材料缺少已登记的文件格式。"))?;
    if !["xlsx", "txt", "csv"].contains(&extension.as_str()) {
        return Err(fail("资料包仅接收已登记的 Excel、TXT 或 CSV 材料。"));
    }
    let original = materials::original_source(store, &packet.material)?;
    let mut instructions = format!(
        "本篇资料包\n\nSA ID：{}\n原表行号：{}\n负责人：{}\n工号：{}\n渠道：{}\n成果类型建议（仍需核对）：{}\n所属机构：{}\n导入说明：{}\n\n本包仅为选定资料及本地依据副本，未执行平台操作，也不代表材料已经通过业务核验。\n\n",
        task.id, task.record.row, task.record.owner, task.record.staff_id, packet.channel_label,
        packet.work_type.as_deref().unwrap_or("待判断"), packet.organisation, packet.instructions);
    if packet.material.kind == "original_pending" {
        instructions.push_str("原始导出待渠道核验：保留完整原格式文件，可能包含其他记录；不能把选定行/文本范围自动视为单篇上传文件。先检查实际导出范围、渠道格式、必要字段和文献身份。\n\n");
    }
    if let Some(export) = &original {
        instructions.push_str(&format!(
            "原始来源：{}\n选定记录位置：{} · 第 {} 至 {} 行\n来源网址：{}\n\n",
            export.receipt.original_name,
            export.receipt.selection.sheet,
            export.receipt.selection.row,
            export.receipt.selection.end_row,
            export.receipt.source_url
        ));
    }
    instructions.push_str("本地待办事项：\n");
    let issues = prepared["issues"]
        .as_array()
        .ok_or_else(|| fail("缺少当前材料核验事项。"))?;
    if issues.is_empty() {
        instructions.push_str("当前本地自动上传条件已满足；执行前仍须重新核对实际页面。\n");
    }
    for issue in issues {
        instructions.push_str(&format!(
            "- {}\n",
            issue["message"].as_str().unwrap_or("需核对完整记录")
        ));
    }
    if packet.channel == "wos_txt" {
        instructions.push_str(&format!("\nWOS TXT（PPT 第 8 页）步骤：\n1. 文献身份、交大归属和本库缺失核验通过后，选择 WOS 数据导入（Txt），核对所属机构及上述 SA 说明，再选择本篇材料.txt。\n2. 核对上传成功；上传不等于导入。\n3. 导入后核对单篇批次说明、题名、WOS ID 和原 DOI。\n4. 推送前核对：{}。\n5. 回读推送结果与实际条目 ID，再回到 SA 核对、关联和认领；推送不等于 SA 已处理。\n", PUSH_POLICY));
    } else {
        instructions.push_str("\n该渠道的自动上传驱动尚未接通。请在实际平台确认本渠道上传窗口、模板类型和导入规则；不要把 WOS 推送选项直接套用到其他渠道。上传、导入、推送各自核验后，回到原 SA 继续核对。本包不生成平台成功回执。\n");
    }
    let mut entries = vec![
        (format!("本篇材料.{extension}"), material),
        ("材料来源.json".into(), audit),
        (
            "本篇任务与来源.xlsx".into(),
            files::report_bytes(&[task.clone()])?,
        ),
        ("操作说明.txt".into(), instructions.into_bytes()),
    ];
    if entries
        .iter()
        .any(|(_, bytes)| bytes.len() as u64 > MAX_ENTRY)
    {
        return Err(fail(
            "本篇完整资料超过导出大小限制，未省略内容或生成不完整资料包。",
        ));
    }
    let manifest = json!({"schema":"submission_bundle_v1", "created":now(), "packet":packet,
        "task_revision":revision,"input_hash":task.input_hash,"record_fingerprint":task.record.fingerprint(),
        "review":task.review,"stage":task.stage,"platform_id":task.platform_id,
        "original_source_selection":original,"can_upload":prepared["can_upload"],"issues":prepared["issues"],
        "platform_verified":false,"export_only":true,
        "files":entries.iter().map(|(name,bytes)|json!({"name":name,"bytes":bytes.len(),"sha256":hash(bytes)})).collect::<Vec<_>>()});
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    if manifest_bytes.len() as u64 > MAX_ENTRY {
        return Err(fail("完整提交说明超过大小限制，未截断资料。"));
    }
    entries.push(("提交信息.json".into(), manifest_bytes));
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        archive
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(Failure::storage)?;
        archive.write_all(&bytes)?;
    }
    let bytes = archive.finish().map_err(Failure::storage)?.into_inner();
    // Recheck the original task/source after reading the full report and all files.
    submission::bundle_scope(store, id, packet_id, revision)?;
    Ok(Bundle { bytes, manifest })
}

pub fn export(
    store: &Store,
    id: &str,
    packet_id: &str,
    revision: i64,
    destination: &Path,
) -> Result<Value> {
    if destination
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .as_deref()
        != Some("zip")
    {
        return Err(fail("本篇资料包请保存为 ZIP 文件。"));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| fail("资料包保存目录无效。"))?
        .canonicalize()?;
    if parent.starts_with(store.root.canonicalize()?) {
        return Err(fail("请选择工作目录外的位置保存副本，保留应用内原始材料。"));
    }
    if destination.exists() {
        let meta = fs::symlink_metadata(destination)?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(fail("资料包保存位置不是普通文件。"));
        }
    }
    let bundle = build(store, id, packet_id, revision)?;
    let digest = hash(&bundle.bytes);
    let temporary = parent.join(format!(".submission-{}.tmp", uuid::Uuid::new_v4()));
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    output.write_all(&bundle.bytes)?;
    output.sync_all()?;
    drop(output);
    if hash(&read_file(&temporary, MAX_ENTRY * 5)?) != digest {
        return Err(fail("实际 ZIP 与生成内容不一致，未登记导出成功。"));
    }
    let (mut task, _, prepared) = submission::bundle_scope(store, id, packet_id, revision)?;
    if destination
        .parent()
        .ok_or_else(|| fail("资料包保存目录无效。"))?
        .canonicalize()?
        != parent
    {
        return Err(fail("资料包保存目录在生成后变化，未发布副本。"));
    }
    if destination.exists() {
        let meta = fs::symlink_metadata(destination)?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(fail("资料包保存位置在生成后变化，未发布副本。"));
        }
    }
    fs::rename(&temporary, destination)?;
    if hash(&read_file(destination, MAX_ENTRY * 5)?) != digest {
        return Err(fail("已发布 ZIP 与生成内容不一致，未登记导出成功。"));
    }
    task.evidence.push(Evidence {id:uuid::Uuid::new_v4().to_string(),kind:"submission_bundle".into(),
        source:destination.to_string_lossy().into(),created:now(),
        text:json!({"path":destination.to_string_lossy(),"sha256":digest,"bytes":bundle.bytes.len(),"manifest":bundle.manifest,"platform_verified":false}).to_string()});
    if let Err(error) = store.save(&mut task, "submission_bundle_exported") {
        return Err(Failure::new(
            &error.code,
            format!(
                "资料包已保存至 {}，本地登记未完成：{}；没有执行平台提交。",
                destination.display(),
                error.message
            ),
        ));
    }
    let mut prepared = prepared;
    prepared["task_revision"] = json!(task.revision);
    Ok(
        json!({"path":destination.to_string_lossy(),"sha256":digest,"bytes":bundle.bytes.len(),
        "packet_id":packet_id,"task_revision":task.revision,"prepared":prepared,"platform_verified":false}),
    )
}
