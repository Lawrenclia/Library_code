//! Explicit reuse of frozen bibliographic sources; never transfers SA business state.
use crate::{
    source_files::{self, Draft, Receipt, Selection},
    *,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{io::Write, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Origin {
    pub task_id: String,
    pub input_hash: String,
    pub record_fingerprint: String,
    pub evidence_id: String,
    pub evidence_hash: String,
    pub snapshot_path: String,
    pub snapshot_hash: String,
    pub selection: Selection,
    pub title: String,
    pub source_url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Snapshot {
    schema: String,
    record: Record,
    input_hash: String,
    evidence: Evidence,
}
fn fail(message: &str) -> Failure {
    Failure::new("SOURCE_REUSE_INVALID", message)
}
/// At least one real identifier must agree and neither known identifier may conflict.
fn compatible(record: &Record, source: &Receipt) -> bool {
    let doi = normalized_doi(&record.doi);
    let wos = normalized_wos(&record.wos);
    let sd = normalized_doi(&source.doi);
    let sw = normalized_wos(&source.wos);
    let doi_pair = !doi.is_empty() && !sd.is_empty();
    let wos_pair = !wos.is_empty() && !sw.is_empty();
    (doi_pair || wos_pair) && (!doi_pair || doi == sd) && (!wos_pair || wos == sw)
}
fn eligible(store: &Store, task: &Task) -> Result<()> {
    if task.record.done
        || task.record.skipped
        || matches!(task.stage, Stage::Completed | Stage::Unknown)
        || store.pending_input(&task.id)?.is_some()
        || !store.unresolved(&task.id)?.is_empty()
    {
        return Err(fail("先处理本条名单版本或未确认操作，再复用原始来源。"));
    }
    Ok(())
}
pub fn options(store: &Store, target: &Task) -> Result<Value> {
    eligible(store, target)?;
    let mut choices = Vec::new();
    let mut problems = Vec::new();
    for donor in store.tasks()? {
        if donor.id == target.id || store.pending_input(&donor.id)?.is_some() {
            continue;
        }
        // Cheap identifier filtering first; actual bytes and selected fields are then reread.
        if !source_files::current_receipts(&donor)
            .iter()
            .any(|r| compatible(&target.record, r))
        {
            continue;
        }
        match source_files::original_exports(&store.root, &donor) {
            Ok(exports) => {
                for original in exports
                    .into_iter()
                    .filter(|e| compatible(&target.record, &e.receipt))
                {
                    choices.push(
                        json!({"source_id":donor.id,"source_input_hash":donor.input_hash,
                    "source_fingerprint":donor.record.fingerprint(),"original":original}),
                    );
                }
            }
            Err(error) => problems.push(json!({"source_id":donor.id,"error":error})),
        }
    }
    Ok(
        json!({"task_id":target.id,"task_revision":target.revision,"choices":choices,"problems":problems}),
    )
}
fn snapshot(root: &Path, origin: &Origin) -> Result<(Snapshot, Receipt)> {
    let sha = &origin.snapshot_hash;
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(fail("来源复用快照身份无效。"));
    }
    let file = root.join("source-reuse").join(format!("{sha}.json"));
    let meta = std::fs::symlink_metadata(&file)?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() as usize > source_files::MAX_BYTES
        || Path::new(&origin.snapshot_path).canonicalize()? != file.canonicalize()?
    {
        return Err(fail("来源复用快照不在归档目录中或已变化。"));
    }
    let raw = std::fs::read(file)?;
    if raw.len() > source_files::MAX_BYTES || hash(&raw) != *sha {
        return Err(fail("来源复用快照哈希变化。"));
    }
    let frozen: Snapshot = serde_json::from_slice(&raw)?;
    let receipt: Receipt = serde_json::from_str(&frozen.evidence.text)?;
    if frozen.schema != "source_reuse_v1"
        || frozen.record.sa_id != origin.task_id
        || frozen.record.fingerprint() != origin.record_fingerprint
        || frozen.input_hash != origin.input_hash
        || frozen.evidence.kind != "external_metadata"
        || frozen.evidence.id != origin.evidence_id
        || hash(frozen.evidence.text.as_bytes()) != origin.evidence_hash
        || receipt.schema != "original_source_v1"
        || receipt.task_id != origin.task_id
        || receipt.input_hash != origin.input_hash
        || receipt.record_fingerprint != origin.record_fingerprint
        || receipt.institution_verified
        || matches!(receipt.channel.as_str(), "general" | "other")
        || receipt.selection != origin.selection
        || receipt.title != origin.title
        || receipt.source_url != origin.source_url
    {
        return Err(fail("来源复用快照与原始 SA、名单及字段绑定不一致。"));
    }
    Ok((frozen, receipt))
}
pub fn verify_draft(root: &Path, target: &Task, draft: &Draft) -> Result<()> {
    let Some(origin) = &draft.origin else {
        return Ok(());
    };
    let (_, donor) = snapshot(root, origin)?;
    if origin.task_id == target.id
        || !compatible(&target.record, &donor)
        || draft.channel != donor.channel
        || draft.original_name != donor.original_name
        || draft.path != donor.archive_path
        || draft.sha256 != donor.sha256
        || draft.format != donor.format
        || json!(draft.options) != json!(donor.options)
    {
        return Err(fail("来源复用与本条标识符、原文件或读取设置不一致。"));
    }
    Ok(())
}
pub fn verify_receipt(root: &Path, target: &Task, receipt: &Receipt) -> Result<()> {
    let Some(origin) = &receipt.origin else {
        return Ok(());
    };
    let (_, donor) = snapshot(root, origin)?;
    if origin.task_id == target.id
        || !compatible(&target.record, &donor)
        || receipt.channel != donor.channel
        || receipt.original_name != donor.original_name
        || receipt.archive_path != donor.archive_path
        || receipt.sha256 != donor.sha256
        || receipt.format != donor.format
        || json!(receipt.options) != json!(donor.options)
        || receipt.actual_encoding != donor.actual_encoding
        || receipt.selection != donor.selection
        || receipt.fields != donor.fields
        || receipt.text != donor.text
        || receipt.title != donor.title
        || receipt.doi != donor.doi
        || receipt.wos != donor.wos
    {
        return Err(fail(
            "复用必须保留原已绑定记录及全部字段；更换记录请重新选择原文件。",
        ));
    }
    Ok(())
}
pub fn prepare(
    store: &Store,
    target_id: &str,
    expected_revision: i64,
    source_id: &str,
    source_input_hash: &str,
    source_fingerprint: &str,
    evidence_id: &str,
    evidence_hash: &str,
) -> Result<Draft> {
    let target = store.task(target_id)?;
    eligible(store, &target)?;
    if target.revision != expected_revision {
        return Err(fail("本条已变化，请重新读取可复用来源。"));
    }
    let donor = store.task(source_id)?;
    if donor.id == target.id
        || donor.input_hash != source_input_hash
        || donor.record.fingerprint() != source_fingerprint
        || store.pending_input(source_id)?.is_some()
    {
        return Err(fail("原来源名单版本已变化，请重新读取来源。"));
    }
    let (original, _) = source_files::read_original_export(&store.root, &donor, evidence_id)?;
    if original.evidence_hash != evidence_hash || !compatible(&target.record, &original.receipt) {
        return Err(fail("原来源绑定或标识符已变化，不能复用。"));
    }
    let evidence = donor
        .evidence
        .iter()
        .find(|e| e.id == evidence_id)
        .ok_or_else(|| fail("原来源证据已不存在。"))?
        .clone();
    let frozen = Snapshot {
        schema: "source_reuse_v1".into(),
        record: donor.record.clone(),
        input_hash: donor.input_hash.clone(),
        evidence,
    };
    let raw = serde_json::to_vec(&frozen)?;
    if raw.len() > source_files::MAX_BYTES {
        return Err(fail("来源快照超过 16 MB，未复用。"));
    }
    let sha = hash(&raw);
    let folder = store.root.join("source-reuse");
    std::fs::create_dir_all(&folder)?;
    let path = folder.join(format!("{sha}.json"));
    if path.exists() {
        let meta = std::fs::symlink_metadata(&path)?;
        if !meta.is_file() || meta.file_type().is_symlink() || std::fs::read(&path)? != raw {
            return Err(fail("已有来源快照变化，不能覆盖。"));
        }
    } else {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(&raw)?;
        file.sync_all()?;
    }
    let r = original.receipt;
    let draft = Draft {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: target.id.clone(),
        input_hash: target.input_hash.clone(),
        record_fingerprint: target.record.fingerprint(),
        task_revision: target.revision,
        channel: r.channel,
        original_name: r.original_name,
        path: r.archive_path,
        sha256: r.sha256,
        format: r.format,
        options: r.options,
        resave: r.resave,
        origin: Some(Origin {
            task_id: donor.id.clone(),
            input_hash: donor.input_hash.clone(),
            record_fingerprint: donor.record.fingerprint(),
            evidence_id: evidence_id.into(),
            evidence_hash: evidence_hash.into(),
            snapshot_path: path.to_string_lossy().into(),
            snapshot_hash: sha,
            selection: r.selection,
            title: r.title,
            source_url: r.source_url,
        }),
    };
    // Creation checks live versions; subsequent binding is against this immutable snapshot.
    let latest = store.task(source_id)?;
    let latest_export = source_files::read_original_export(&store.root, &latest, evidence_id)?.0;
    if latest.input_hash != donor.input_hash
        || latest.record.fingerprint() != donor.record.fingerprint()
        || latest_export.evidence_hash != evidence_hash
        || store.pending_input(source_id)?.is_some()
        || store.task(target_id)?.revision != target.revision
    {
        return Err(fail("准备期间名单或来源变化，请重新读取。"));
    }
    source_files::check_draft(&store.root, &target, &draft)?;
    Ok(draft)
}
/// Full donor record lives in the report/audit, excluded from factual AI input.
pub fn audit(root: &Path, target: &Task, receipt: &Receipt) -> Result<Option<Evidence>> {
    let Some(origin) = &receipt.origin else {
        return Ok(None);
    };
    verify_receipt(root, target, receipt)?;
    let (frozen, _) = snapshot(root, origin)?;
    Ok(Some(Evidence {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "source_reuse_origin".into(),
        source: origin.snapshot_path.clone(),
        text: json!({"schema":"source_reuse_audit_v1","target_id":target.id,
            "target_input_hash":target.input_hash,"origin":origin,"snapshot":frozen})
        .to_string(),
        created: now(),
    }))
}
