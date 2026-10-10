//! Accept changed roster facts only after a new, exact live SA read.
use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub record: Record,
    pub input_hash: String,
    pub source_file: String,
    pub previous_fingerprint: String,
    #[serde(default)]
    pub previous_stage: Option<Stage>,
    #[serde(default)]
    pub previous_version_id: Option<String>,
    pub created: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchRecheck {
    pub id: String,
    pub sa_id: String,
    pub input_hash: String,
    pub record: Record,
    pub original_input_hash: String,
    pub original_record: Record,
    pub original_revision: i64,
    pub original_stage: Stage,
    pub candidate: Candidate,
    pub checkpoint: Value,
    pub created: u64,
}
impl BatchRecheck {
    pub fn expects_upload(&self) -> bool {
        self.original_stage == Stage::Uploaded
    }
    pub fn requires_push(&self) -> bool {
        matches!(
            self.original_stage,
            Stage::Pushed | Stage::Claimed | Stage::Completed
        )
    }
}
pub fn validate_batch_recheck(task: &Task) -> Result<BatchRecheck> {
    let saved = task
        .batch_recheck
        .as_ref()
        .ok_or_else(|| Failure::new("REMOTE_RESULT_UNKNOWN", "缺少本版本的旧批次检查点。"))?;
    if !matches!(
        task.stage,
        Stage::AwaitingReview | Stage::Downloaded | Stage::Ready
    ) || saved.sa_id != task.id
        || saved.original_record.sa_id != task.id
        || saved.input_hash != task.input_hash
        || json!(saved.record) != json!(task.record)
        || task.batch.as_ref() != Some(&saved.checkpoint)
        || task
            .artifact
            .as_ref()
            .is_none_or(|a| json!(a.candidate) != json!(saved.candidate))
    {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "旧批次检查点与当前名单、文件或任务阶段变化，未回读或推进。",
        ));
    }
    files::read_metadata_candidate(task)?;
    Ok(saved.clone())
}
/// A fresh full readback clears the marker; old stage/JSON alone never clears it.
pub fn adopt_batch_recheck(
    task: &mut Task,
    expected: &BatchRecheck,
    result: &Value,
    payload: &Value,
) -> Result<()> {
    let current = validate_batch_recheck(task)?;
    if json!(current) != json!(expected) {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "回读期间旧批次检查点变化，未采用结果。",
        ));
    }
    let raw = files::read_import_archive(task, payload)?;
    let stage = if result["uploaded"] == true {
        if !current.expects_upload() {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "原检查点要求回读导入或推送，不能以临时上传回执代替。",
            ));
        }
        validate_upload_readback(result, payload, raw.len())?;
        if [
            "sa_id",
            "instructions",
            "sha256",
            "filename",
            "size",
            "server_name",
            "dataset_id",
            "dataset_label",
        ]
        .iter()
        .any(|key| result[*key] != current.checkpoint[*key])
        {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "回读文件、服务端上传对象或所属机构与旧检查点变化，未采用结果。",
            ));
        }
        Stage::Uploaded
    } else {
        let mut bound = payload.clone();
        if current.checkpoint["uploaded"] != true {
            bound["batch"] = current.checkpoint.clone();
            bound["batch_id"] = current.checkpoint["id"].clone();
        }
        validate_import_readback(result, &bound, current.requires_push())?
    };
    let audit = Evidence { id:uuid::Uuid::new_v4().to_string(), kind:"input_version_batch_rechecked".into(), source:"机构库 · 新名单版本的原批次只读核验".into(),
        text:json!({"schema":"input_version_batch_rechecked_v1","sa_id":task.id,"input_hash":task.input_hash,"checkpoint":current,"payload":payload,"result":result,"stage":stage,"write_sent":false}).to_string(), created:now() };
    task.stage = stage;
    task.batch = Some(if result["uploaded"] == true {
        result.clone()
    } else {
        result["batch"].clone()
    });
    task.batch_recheck = None;
    task.running = false;
    task.last_error = None;
    task.evidence.push(audit);
    Ok(())
}
pub fn validate_live(proposal: &Proposal, snapshot: &Value) -> Result<()> {
    let r = &proposal.record;
    let row = &snapshot["row"];
    let fields = [
        "saLzkId",
        "titleValue",
        "gh",
        "doiValue",
        "wosValue",
        "matchCount",
        "itemId",
        "markStatus",
        "reason",
    ];
    if !fields
        .iter()
        .all(|k| row.as_object().is_some_and(|m| m.contains_key(*k)))
    {
        return Err(Failure::new(
            "PAGE_UNSUPPORTED",
            "实时 SA 缺少版本核对所需字段。",
        ));
    }
    if ["doiValue", "wosValue", "reason"]
        .iter()
        .any(|k| !row[k].is_string() && !row[k].is_null())
    {
        return Err(Failure::new(
            "PAGE_UNSUPPORTED",
            "实时 SA 的版本字段类型无法识别。",
        ));
    }
    let mut ids = matched_ids(row)?;
    ids.sort();
    let mut expected = matched_ids(&json!({"matchCount":r.matches,"itemId":r.item_ids}))?;
    expected.sort();
    if row["saLzkId"] != r.sa_id
        || row["gh"] != r.staff_id
        || norm(row["titleValue"].as_str().unwrap_or("")) != norm(&r.title)
        || normalized_doi(row["doiValue"].as_str().unwrap_or("")) != normalized_doi(&r.doi)
        || normalized_wos(row["wosValue"].as_str().unwrap_or("")) != normalized_wos(&r.wos)
        || ids.len() != r.matches as usize
        || ids != expected
        || row["markStatus"] != r.mark
        || row["reason"].as_str().unwrap_or("").trim() != r.reason.trim()
        || !["待处理", "已处理"].contains(&r.mark.as_str())
        || (r.done && row["markStatus"] != "已处理")
    {
        return Err(Failure::new(
            "INPUT_CHANGED",
            "新名单的题名、完整工号、标识符、匹配条目、原因或状态与实时 SA 不一致；没有切换版本。",
        ));
    }
    Ok(())
}
pub fn build_next(
    old: &Task,
    proposal: &Proposal,
    snapshot: &Value,
    source: &str,
    proof: &str,
) -> Result<Task> {
    if old.running
        || old.stage == Stage::Unknown
        || proposal.record.sa_id != old.id
        || proposal.previous_fingerprint != old.record.fingerprint()
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "原任务或待确认操作已变化，不能切换版本。",
        ));
    }
    if source.trim().is_empty() || proof.trim().is_empty() {
        return Err(Failure::new(
            "EVIDENCE_REQUIRED",
            "填写新名单版本与责任人安排的来源及具体依据。",
        ));
    }
    validate_live(proposal, snapshot)?;
    let mut next = Task::new(proposal.record.clone(), proposal.input_hash.clone());
    next.revision = old.revision;
    next.record.done = snapshot["row"]["markStatus"] == "已处理";
    next.stage = if next.record.done {
        Stage::Completed
    } else {
        Stage::AwaitingReview
    };
    next.sa_snapshot = Some(snapshot.clone());
    next.evidence = old
        .evidence
        .iter()
        .filter(|e| e.kind == "input_version_history")
        .cloned()
        .collect();
    let mut archived = old.clone();
    archived
        .evidence
        .retain(|e| e.kind != "input_version_history");
    next.evidence.push(Evidence {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "input_version_history".into(),
        source: "旧任务版本 · 完整核验与操作历史".into(),
        text: serde_json::to_string(&archived)?,
        created: now(),
    });
    next.evidence.push(Evidence {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "input_version_review".into(),
        source: source.trim().into(),
        text: json!({"proposal":proposal,"proof":proof,"live_sa":snapshot}).to_string(),
        created: now(),
    });
    // Reuse a source file only after checking its actual bytes against the new facts.
    if let Some(a) = &old.artifact {
        if files::verify_identity(&next.record, &a.candidate).unwrap_or(false) {
            // A matching cached DOI/title is only a reason to inspect the source.
            // The bounded ordinary-file reader compares every parsed field and
            // derived value with the old cache, not just its claimed SHA.
            let parsed = files::read_metadata_candidate(old)?.ok_or_else(|| {
                Failure::new("FILE_INVALID", "旧版本缺少可核验的实际归档，未复用。")
            })?;
            if !files::verify_identity(&next.record, &parsed)? {
                return Err(Failure::new(
                    "IDENTITY_CONFLICT",
                    "实际归档与新名单的完整题名和标识符未确认一致，未复用。",
                ));
            }
            let mut artifact = a.clone();
            artifact.candidate = parsed;
            artifact.identity_confirmed = true;
            next.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "metadata".into(),
                source: artifact.record_url.clone(),
                text: serde_json::to_string(&artifact.candidate.fields)?,
                created: now(),
            });
            next.evidence.push(Evidence {
                id: uuid::Uuid::new_v4().to_string(),
                kind: "artifact_reuse".into(),
                source: artifact.record_url.clone(),
                text: json!({"schema":"input_version_artifact_reuse_v1","sa_id":next.id,
                    "input_hash":next.input_hash,"record_fingerprint":next.record.fingerprint(),
                    "original_input_hash":old.input_hash,"original_record":old.record,
                    "original_revision":old.revision,"original_artifact":a,
                    "archive_path":artifact.path,"sha256":artifact.candidate.sha256,
                    "candidate":artifact.candidate,"original_downloaded_at":artifact.downloaded,
                    "reused_at":now(),"new_download":false,"platform_verified":false})
                .to_string(),
                created: now(),
            });
            next.artifact = Some(artifact);
            next.batch = old.batch.clone();
            if !next.record.done && old.batch.is_some() {
                let original_stage = if old.batch_recheck.is_some() {
                    validate_batch_recheck(old)?.original_stage
                } else if matches!(
                    old.stage,
                    Stage::Uploaded
                        | Stage::Imported
                        | Stage::Pushed
                        | Stage::Claimed
                        | Stage::Completed
                ) {
                    old.stage.clone()
                } else {
                    proposal
                        .previous_stage
                        .clone()
                        .unwrap_or_else(|| old.stage.clone())
                };
                let recheck = BatchRecheck {
                    id: uuid::Uuid::new_v4().to_string(),
                    sa_id: next.id.clone(),
                    input_hash: next.input_hash.clone(),
                    record: next.record.clone(),
                    original_input_hash: old.input_hash.clone(),
                    original_record: old.record.clone(),
                    original_revision: old.revision,
                    original_stage,
                    candidate: next.artifact.as_ref().unwrap().candidate.clone(),
                    checkpoint: old.batch.clone().unwrap(),
                    created: now(),
                };
                next.evidence.push(Evidence { id:recheck.id.clone(), kind:"input_version_batch_pending".into(), source:"旧名单版本的原批次检查点，待独立平台回读".into(),
                    text:json!({"schema":"input_version_batch_pending_v1","checkpoint":recheck,"platform_verified":false,"write_sent":false}).to_string(), created:now() });
                next.batch_recheck = Some(recheck);
                next.stage = Stage::AwaitingReview;
            }
        }
    }
    // Old reviews, AI output, issue baselines, merges and author selections stay in history.
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> Record {
        serde_json::from_value(json!({"row":2,"owner":"旧负责人","sa_id":"version-sa","title":"Paper","doi":"10.1234/test","wos":"","staff_id":"001","matches":0,"item_ids":"","mark":"待处理","reason":"旧原因","done":false,"skipped":false,"source":""})).unwrap()
    }
    fn incoming() -> Record {
        let mut r = record();
        r.owner = "新负责人".into();
        r.staff_id = "002".into();
        r.reason = "新原因".into();
        r
    }
    fn live(r: &Record) -> Value {
        json!({"row":{"saLzkId":r.sa_id,"titleValue":r.title,"gh":r.staff_id,"doiValue":r.doi,"wosValue":r.wos,"matchCount":r.matches,"itemId":r.item_ids,"markStatus":r.mark,"reason":r.reason,"remark":"已核对"},"comparison":[]})
    }
    fn setup() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        store.import(vec![record()], "old-hash".into()).unwrap();
        (dir, store)
    }
    fn source_artifact(root: &std::path::Path) -> Artifact {
        let raw=b"TI\tAU\tAF\tSO\tPY\tC1\tUT\tDI\tAB\nPaper\tTest, A\tAlice Test\tJournal\t2026\tShanghai Jiao Tong Univ\tWOS:000123456789012\t10.1234/test\tFull abstract\n";
        let path = root.join("source.txt");
        std::fs::write(&path, raw).unwrap();
        Artifact {
            path: path.to_string_lossy().into(),
            source: "WOS".into(),
            record_url: "https://www.webofscience.com/wos/woscc/full-record/WOS:000123456789012"
                .into(),
            downloaded: 123456,
            candidate: files::parse_wos(raw).unwrap(),
            identity_confirmed: true,
        }
    }
    fn full_upload(task: &Task) -> Value {
        let a = task.artifact.as_ref().unwrap();
        json!({"verified":true,"uploaded":true,"sa_id":task.id,"instructions":format!("SA补充-{}",task.id),
            "sha256":a.candidate.sha256,"filename":format!("SA-WOS-{}.txt",task.id),"size":std::fs::metadata(&a.path).unwrap().len(),
            "dataset_id":"sjtu","dataset_label":"上海交通大学","server_name":"original-object",
            "response":{"success":true,"data":{"name":"original-object"}}})
    }
    fn batch_payload(task: &Task) -> Value {
        let a = task.artifact.as_ref().unwrap();
        json!({"sa_id":task.id,"instructions":format!("SA补充-{}",task.id),"candidate":a.candidate,"contentSha":a.candidate.sha256})
    }
    fn batch_result(task: &Task, status: u32) -> Value {
        let c = &task.artifact.as_ref().unwrap().candidate;
        json!({"verified":true,"batch":{"id":"batch-1","modelId":"model-1","source":"WOS","instructions":format!("SA补充-{}",task.id),"total":1,"fail":0,"actual":if status==2 {1} else {0},"status":status},
            "items":[{"metadata":{"title":[c.title],"doi":[c.doi],"wosId":[c.wos]}}]})
    }
    fn pending_batch_version(stage: Stage) -> (tempfile::TempDir, Store, Task) {
        let (dir, store) = setup();
        let mut old = store.task("version-sa").unwrap();
        old.artifact = Some(source_artifact(dir.path()));
        old.batch = Some(if stage == Stage::Uploaded {
            full_upload(&old)
        } else {
            batch_result(&old, if stage == Stage::Imported { 1 } else { 2 })["batch"].clone()
        });
        old.stage = stage;
        store.save(&mut old, "fixture_original_batch").unwrap();
        store.import(vec![incoming()], "new-hash".into()).unwrap();
        let proposal = store.pending_input(&old.id).unwrap().unwrap();
        let current = store.task(&old.id).unwrap();
        let next = store
            .accept_input(
                &current,
                &proposal,
                &live(&proposal.record),
                "source",
                "proof",
            )
            .unwrap();
        (dir, store, next)
    }
    #[test]
    fn original_remote_stages_remain_pending_after_restart_and_do_not_grant_import_writes() {
        for stage in [
            Stage::Uploaded,
            Stage::Imported,
            Stage::Pushed,
            Stage::Claimed,
            Stage::Completed,
        ] {
            let (dir, _store, next) = pending_batch_version(stage.clone());
            assert_eq!(next.stage, Stage::AwaitingReview);
            let reopened = Store::new(dir.path()).unwrap();
            reopened.recover().unwrap();
            let task = reopened.task(&next.id).unwrap();
            let saved = validate_batch_recheck(&task).unwrap();
            assert_eq!(saved.original_stage, stage);
            assert_eq!(saved.original_input_hash, "old-hash");
            assert_eq!(saved.record.staff_id, "002");
            assert_eq!(
                task.import_ready().unwrap_err().code,
                "REMOTE_RESULT_UNKNOWN"
            );
            assert!(task.review.is_none());
            assert!(task.platform_id.is_empty());
        }
    }
    #[test]
    fn thin_results_wrong_batch_or_paper_and_push_downgrade_never_clear_checkpoint() {
        let (_dir, _store, task) = pending_batch_version(Stage::Pushed);
        let saved = validate_batch_recheck(&task).unwrap();
        let mut wrong_batch = batch_result(&task, 2);
        wrong_batch["batch"]["id"] = json!("other-batch");
        let mut wrong_paper = batch_result(&task, 2);
        wrong_paper["items"][0]["metadata"]["doi"] = json!(["10.9876/other"]);
        for result in [
            json!({"verified":true}),
            batch_result(&task, 1),
            wrong_batch,
            wrong_paper,
            full_upload(&task),
        ] {
            let mut pending = task.clone();
            assert!(
                adopt_batch_recheck(&mut pending, &saved, &result, &batch_payload(&task)).is_err()
            );
            assert_eq!(json!(pending), json!(task));
        }
    }
    #[test]
    fn complete_fresh_readbacks_restore_only_remote_stage_and_preserve_independent_sa_review() {
        for stage in [Stage::Uploaded, Stage::Imported, Stage::Pushed] {
            let (dir, store, mut task) = pending_batch_version(stage.clone());
            let saved = validate_batch_recheck(&task).unwrap();
            let result = if stage == Stage::Uploaded {
                full_upload(&task)
            } else {
                batch_result(&task, if stage == Stage::Pushed { 2 } else { 1 })
            };
            let payload = batch_payload(&task);
            adopt_batch_recheck(&mut task, &saved, &result, &payload).unwrap();
            store.save(&mut task, "fixture_fresh_readback").unwrap();
            let reopened = Store::new(dir.path()).unwrap();
            let current = reopened.task(&task.id).unwrap();
            assert_eq!(current.stage, stage);
            assert!(current.batch_recheck.is_none());
            assert!(!current.record.done);
            assert!(current.review.is_none());
            assert!(current.platform_id.is_empty());
            let audit = current
                .evidence
                .iter()
                .find(|e| e.kind == "input_version_batch_rechecked")
                .unwrap();
            let full: Value = serde_json::from_str(&audit.text).unwrap();
            assert_eq!(full["checkpoint"]["original_input_hash"], "old-hash");
            assert_eq!(full["result"], result);
            assert_eq!(full["write_sent"], false);
            assert!(!classification::sources(dir.path(), &current)
                .unwrap()
                .iter()
                .any(|e| e.id == audit.id));
            let mut repeated = current.clone();
            assert!(adopt_batch_recheck(&mut repeated, &saved, &result, &payload).is_err());
            assert_eq!(json!(repeated), json!(current));
        }
    }
    #[test]
    fn changed_scope_or_upload_object_keeps_pending_batch_and_full_task_unchanged() {
        let (_dir, _store, task) = pending_batch_version(Stage::Uploaded);
        let saved = validate_batch_recheck(&task).unwrap();
        for key in ["server_name", "dataset_id"] {
            let mut result = full_upload(&task);
            result[key] = json!("other-object");
            if key == "server_name" {
                result["response"]["data"]["name"] = json!("other-object");
            }
            let mut pending = task.clone();
            assert!(
                adopt_batch_recheck(&mut pending, &saved, &result, &batch_payload(&task)).is_err()
            );
            assert_eq!(json!(pending), json!(task));
        }
        for key in ["input", "record", "batch", "candidate"] {
            let mut changed = task.clone();
            match key {
                "input" => changed.input_hash = "other-input".into(),
                "record" => changed.record.row += 1,
                "batch" => changed.batch.as_mut().unwrap()["server_name"] = json!("other-object"),
                _ => changed.artifact.as_mut().unwrap().candidate.year = "2099".into(),
            }
            assert_eq!(
                validate_batch_recheck(&changed).unwrap_err().code,
                "INPUT_CHANGED"
            );
        }
    }
    #[test]
    fn another_input_snapshot_retains_original_push_requirement_instead_of_local_pending_stage() {
        let (_dir, store, task) = pending_batch_version(Stage::Pushed);
        let mut incoming = task.record.clone();
        incoming.row = 8;
        store
            .import(vec![incoming.clone()], "third-hash".into())
            .unwrap();
        let proposal = store.pending_input(&task.id).unwrap().unwrap();
        let current = store.task(&task.id).unwrap();
        let next = store
            .accept_input(
                &current,
                &proposal,
                &live(&proposal.record),
                "source",
                "proof",
            )
            .unwrap();
        assert_eq!(next.record.row, 8);
        assert_eq!(next.input_hash, "third-hash");
        let saved = validate_batch_recheck(&next).unwrap();
        assert!(saved.requires_push());
        assert_eq!(saved.original_stage, Stage::Pushed);
        assert_eq!(saved.record.row, 8);
        assert_eq!(next.stage, Stage::AwaitingReview);
    }
    #[test]
    fn concurrent_task_revision_prevents_a_stale_readback_from_consuming_pending_checkpoint() {
        let (dir, store, mut stale) = pending_batch_version(Stage::Pushed);
        let saved = validate_batch_recheck(&stale).unwrap();
        let payload = batch_payload(&stale);
        let result = batch_result(&stale, 2);
        let mut newer = stale.clone();
        store.save(&mut newer, "fixture_concurrent_update").unwrap();
        adopt_batch_recheck(&mut stale, &saved, &result, &payload).unwrap();
        assert_eq!(
            store
                .save(&mut stale, "fixture_stale_readback")
                .unwrap_err()
                .code,
            "TASK_CHANGED"
        );
        let reopened = Store::new(dir.path()).unwrap();
        assert_eq!(json!(reopened.task(&newer.id).unwrap()), json!(newer));
        assert!(reopened.task(&newer.id).unwrap().batch_recheck.is_some());
    }
    #[test]
    fn version_reuse_rejects_wrong_cached_fields_even_with_the_correct_file_hash() {
        for mutation in ["year", "sjtu", "fields"] {
            let (dir, store) = setup();
            let mut old = store.task("version-sa").unwrap();
            let mut artifact = source_artifact(dir.path());
            match mutation {
                "year" => artifact.candidate.year = "2099".into(),
                "sjtu" => artifact.candidate.sjtu = false,
                _ => {
                    artifact
                        .candidate
                        .fields
                        .insert("AB".into(), "Forged abstract".into());
                }
            }
            old.artifact = Some(artifact);
            store.save(&mut old, "fixture_cache_mismatch").unwrap();
            store.import(vec![incoming()], "new-input".into()).unwrap();
            let current = store.task(&old.id).unwrap();
            let proposal = store.pending_input(&old.id).unwrap().unwrap();
            assert_eq!(
                store
                    .accept_input(
                        &current,
                        &proposal,
                        &live(&proposal.record),
                        "source",
                        "proof"
                    )
                    .unwrap_err()
                    .code,
                "FILE_INVALID"
            );
            assert_eq!(json!(store.task(&old.id).unwrap()), json!(current));
            assert_eq!(
                store.pending_input(&old.id).unwrap().unwrap().id,
                proposal.id
            );
        }
    }
    #[test]
    fn invalid_missing_or_oversized_archive_never_partially_accepts_the_version() {
        for mutation in ["changed", "missing", "oversized"] {
            let (dir, store) = setup();
            let mut old = store.task("version-sa").unwrap();
            let artifact = source_artifact(dir.path());
            old.artifact = Some(artifact.clone());
            store.save(&mut old, "fixture_valid_source").unwrap();
            store.import(vec![incoming()], "new-input".into()).unwrap();
            let current = store.task(&old.id).unwrap();
            let proposal = store.pending_input(&old.id).unwrap().unwrap();
            match mutation {
                "missing" => std::fs::remove_file(&artifact.path).unwrap(),
                "oversized" => std::fs::write(&artifact.path, vec![b'x'; 524289]).unwrap(),
                _ => std::fs::write(&artifact.path, b"Different file").unwrap(),
            }
            let before: i64 = store
                .connect()
                .unwrap()
                .query_row("SELECT count(*) FROM task_versions", [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                store
                    .accept_input(
                        &current,
                        &proposal,
                        &live(&proposal.record),
                        "source",
                        "proof"
                    )
                    .unwrap_err()
                    .code,
                "FILE_INVALID"
            );
            assert_eq!(json!(store.task(&old.id).unwrap()), json!(current));
            assert_eq!(
                store.pending_input(&old.id).unwrap().unwrap().id,
                proposal.id
            );
            let after: i64 = store
                .connect()
                .unwrap()
                .query_row("SELECT count(*) FROM task_versions", [], |r| r.get(0))
                .unwrap();
            assert_eq!(after, before);
        }
    }
    #[test]
    fn unrelated_new_paper_keeps_old_source_and_batch_in_history_without_adopting_them() {
        let (dir, store) = setup();
        let mut old = store.task("version-sa").unwrap();
        let artifact = source_artifact(dir.path());
        old.artifact = Some(artifact.clone());
        old.batch = Some(json!({"id":"old-batch"}));
        old.stage = Stage::Pushed;
        store.save(&mut old, "fixture_old_batch").unwrap();
        let mut changed = incoming();
        changed.title = "Other paper".into();
        changed.doi = "10.9876/other".into();
        store.import(vec![changed], "new-input".into()).unwrap();
        let current = store.task(&old.id).unwrap();
        let proposal = store.pending_input(&old.id).unwrap().unwrap();
        std::fs::remove_file(&artifact.path).unwrap();
        let next = store
            .accept_input(
                &current,
                &proposal,
                &live(&proposal.record),
                "source",
                "proof",
            )
            .unwrap();
        assert!(next.artifact.is_none());
        assert!(next.batch.is_none());
        assert_eq!(next.stage, Stage::AwaitingReview);
        assert!(next
            .evidence
            .iter()
            .any(|e| e.kind == "input_version_history" && e.text.contains("old-batch")));
        assert!(!next
            .evidence
            .iter()
            .any(|e| e.kind == "metadata" || e.kind == "artifact_reuse"));
    }
    #[test]
    fn valid_reuse_preserves_original_download_and_full_origin_without_claiming_a_new_download() {
        let (dir, store) = setup();
        let mut old = store.task("version-sa").unwrap();
        let artifact = source_artifact(dir.path());
        old.artifact = Some(artifact.clone());
        store.save(&mut old, "fixture_original_download").unwrap();
        store.import(vec![incoming()], "new-input".into()).unwrap();
        let current = store.task(&old.id).unwrap();
        let proposal = store.pending_input(&old.id).unwrap().unwrap();
        let next = store
            .accept_input(
                &current,
                &proposal,
                &live(&proposal.record),
                "source",
                "proof",
            )
            .unwrap();
        assert_eq!(
            next.artifact.as_ref().unwrap().downloaded,
            artifact.downloaded
        );
        assert_eq!(
            json!(next.artifact.as_ref().unwrap().candidate),
            json!(artifact.candidate)
        );
        let reuse = next
            .evidence
            .iter()
            .find(|e| e.kind == "artifact_reuse")
            .unwrap();
        let audit: Value = serde_json::from_str(&reuse.text).unwrap();
        assert_eq!(audit["original_input_hash"], "old-hash");
        assert_eq!(audit["original_record"], json!(old.record));
        assert_eq!(audit["input_hash"], "new-input");
        assert_eq!(audit["original_downloaded_at"], 123456);
        assert_eq!(audit["new_download"], false);
        assert_eq!(audit["platform_verified"], false);
        let reopened = Store::new(dir.path()).unwrap();
        let task = reopened.task(&old.id).unwrap();
        let sources = classification::sources(dir.path(), &task).unwrap();
        assert!(sources
            .iter()
            .any(|e| e.kind == "metadata" && e.text.contains("Full abstract")));
        assert!(!sources.iter().any(|e| e.id == reuse.id));
        let (report, _) = reopened.report_snapshot().unwrap();
        assert!(report[0]
            .evidence
            .iter()
            .any(|e| e.id == reuse.id && e.text == reuse.text));
    }
    #[test]
    fn proposals_survive_restart_and_superseding_preserves_original_stage_and_facts() {
        let (dir, store) = setup();
        let mut old = store.task("version-sa").unwrap();
        old.stage = Stage::Pushed;
        store.save(&mut old, "fixture").unwrap();
        store
            .import_from(vec![incoming()], "new-hash".into(), "inputs/new.xlsx")
            .unwrap();
        let first = store.pending_input(&old.id).unwrap().unwrap();
        let reopened = Store::new(dir.path()).unwrap();
        reopened.recover().unwrap();
        assert_eq!(
            reopened.pending_input(&old.id).unwrap().unwrap().id,
            first.id
        );
        assert_eq!(
            reopened.task(&old.id).unwrap().record.title,
            old.record.title
        );
        let mut next = incoming();
        next.title = "New title".into();
        reopened.import(vec![next], "third-hash".into()).unwrap();
        let latest = reopened.pending_input(&old.id).unwrap().unwrap();
        assert_ne!(latest.id, first.id);
        assert_eq!(latest.previous_stage, Some(Stage::Pushed));
        assert_eq!(latest.previous_version_id, first.previous_version_id);
        reopened
            .import(vec![record()], "reverted-file".into())
            .unwrap();
        assert!(
            reopened.pending_input(&old.id).unwrap().is_some(),
            "reverting the file does not silently accept a version"
        );
    }
    #[test]
    fn every_new_input_fact_must_match_live_sa_and_missing_fields_are_not_empty_values() {
        let (_dir, store) = setup();
        store.import(vec![incoming()], "new".into()).unwrap();
        let p = store.pending_input("version-sa").unwrap().unwrap();
        let s = live(&p.record);
        validate_live(&p, &s).unwrap();
        for key in [
            "saLzkId",
            "titleValue",
            "gh",
            "doiValue",
            "wosValue",
            "itemId",
            "markStatus",
            "reason",
        ] {
            let mut bad = s.clone();
            bad["row"][key] = json!("changed");
            assert!(validate_live(&p, &bad).is_err(), "{key}");
        }
        let mut bad = s.clone();
        bad["row"].as_object_mut().unwrap().remove("doiValue");
        assert_eq!(
            validate_live(&p, &bad).unwrap_err().code,
            "PAGE_UNSUPPORTED"
        );
        let mut bad = s.clone();
        bad["row"]["doiValue"] = json!(false);
        assert!(validate_live(&p, &bad).is_err());
        let mut text_count = s.clone();
        text_count["row"]["matchCount"] = json!("0");
        validate_live(&p, &text_count).unwrap();
    }
    #[test]
    fn accepted_version_keeps_full_old_history_and_batch_but_resets_business_conclusions() {
        let (dir, store) = setup();
        let mut old = store.task("version-sa").unwrap();
        let raw=b"TI\tAU\tAF\tSO\tPY\tC1\tUT\tDI\nPaper\tTest, A\tAlice Test\tJournal\t2026\tShanghai Jiao Tong Univ\tWOS:000123456789012\t10.1234/test\n";
        let path = dir.path().join("archive.txt");
        std::fs::write(&path, raw).unwrap();
        let c = files::parse_wos(raw).unwrap();
        old.artifact = Some(Artifact {
            path: path.to_string_lossy().into(),
            source: "WOS".into(),
            record_url: "fixture".into(),
            downloaded: now(),
            candidate: c.clone(),
            identity_confirmed: true,
        });
        old.stage = Stage::Pushed;
        old.batch = Some(json!({"id":"batch-1","status":2}));
        old.classification = Some(json!({"old_ai":"not reusable"}));
        old.evidence.push(Evidence {
            id: "old-source".into(),
            kind: "human_review".into(),
            source: "旧原文".into(),
            text: "完整独立依据".into(),
            created: now(),
        });
        old.review = Some(Review {
            route: Route::Missing,
            evidence_id: "old-source".into(),
            library_checked: true,
            platform_id: "".into(),
            affiliation_confirmed: true,
            identity_confirmed: true,
            issues_resolved: true,
            note: "旧结论".into(),
        });
        store.save(&mut old, "fixture").unwrap();
        store
            .set_setting("claim:version-sa", json!({"old_preparation":true}))
            .unwrap();
        store.import(vec![incoming()], "new-hash".into()).unwrap();
        let p = store.pending_input(&old.id).unwrap().unwrap();
        let current = store.task(&old.id).unwrap();
        let next = store
            .accept_input(
                &current,
                &p,
                &live(&p.record),
                "新名单与负责人安排",
                "完整核对工号与实时 SA",
            )
            .unwrap();
        assert_eq!(next.record.staff_id, "002");
        assert_eq!(next.stage, Stage::AwaitingReview);
        assert!(next.batch_recheck.as_ref().unwrap().requires_push());
        assert_eq!(next.batch.as_ref().unwrap()["id"], "batch-1");
        assert!(next.review.is_none());
        assert!(next.classification.is_none());
        assert!(next.issue_plan.is_none());
        assert!(next.issue_reviews.is_empty());
        assert!(next.merges.is_empty());
        assert!(next
            .evidence
            .iter()
            .any(|e| e.kind == "input_version_history" && e.text.contains("完整独立依据")));
        assert!(!next.evidence.iter().any(|e| e.kind == "human_review"));
        assert!(store.pending_input(&old.id).unwrap().is_none());
        assert!(store.setting("claim:version-sa").unwrap().is_none());
        assert_eq!(
            store.assert_no_prior_upload(&c).unwrap_err().code,
            "DUPLICATE_WRITE"
        );
        files::export_report(&[next], &dir.path().join("versions.xlsx")).unwrap();
    }
    #[test]
    fn unresolved_old_writes_and_superseded_proposals_never_get_discarded() {
        let (_dir, store) = setup();
        let mut old = store.task("version-sa").unwrap();
        let intent = store
            .begin_attempt_with_payload(&mut old, "link", json!({"candidate":"original"}))
            .unwrap();
        store.import(vec![incoming()], "new".into()).unwrap();
        let p = store.pending_input(&old.id).unwrap().unwrap();
        let task = store.task(&old.id).unwrap();
        assert_eq!(
            store
                .accept_input(&task, &p, &live(&p.record), "来源", "依据")
                .unwrap_err()
                .code,
            "REMOTE_RESULT_UNKNOWN"
        );
        assert_eq!(store.unresolved(&old.id).unwrap()[0]["id"], intent);
        let mut later = incoming();
        later.owner = "第三负责人".into();
        store.import(vec![later], "third".into()).unwrap();
        assert_eq!(
            store
                .accept_input(
                    &store.task(&old.id).unwrap(),
                    &p,
                    &live(&p.record),
                    "来源",
                    "依据"
                )
                .unwrap_err()
                .code,
            "INPUT_CHANGED"
        );
    }
    #[test]
    fn stale_accept_rolls_back_archive_and_keeps_latest_task_and_proposal() {
        let (dir, store) = setup();
        store.import(vec![incoming()], "new".into()).unwrap();
        let p = store.pending_input("version-sa").unwrap().unwrap();
        let stale = store.task("version-sa").unwrap();
        let mut current = stale.clone();
        store.save(&mut current, "changed").unwrap();
        let db = rusqlite::Connection::open(dir.path().join("workspace.sqlite3")).unwrap();
        let count = || {
            db.query_row("SELECT count(*) FROM task_versions", [], |r| {
                r.get::<_, u32>(0)
            })
            .unwrap()
        };
        let before = count();
        assert_eq!(
            store
                .accept_input(&stale, &p, &live(&p.record), "来源", "依据")
                .unwrap_err()
                .code,
            "TASK_CHANGED"
        );
        assert_eq!(count(), before);
        assert_eq!(store.task(&stale.id).unwrap().revision, current.revision);
        assert!(store.pending_input(&stale.id).unwrap().is_some());
    }
    #[test]
    fn original_import_receipts_block_upload_even_when_same_sa_changes_to_another_paper() {
        let (_dir, store) = setup();
        let mut t = store.task("version-sa").unwrap();
        let candidate=serde_json::from_value::<Candidate>(json!({"title":"Paper","doi":"10.1234/test","wos":"WOS:000123456789012","authors":"A","year":"2026","journal":"J","affiliation":"SJTU","sjtu":true,"sha256":"test","fields":{}})).unwrap();
        let id = store
            .begin_attempt_with_payload(&mut t, "import_upload", json!({"candidate":candidate}))
            .unwrap();
        t.running = false;
        store
            .finish_attempt(
                &mut t,
                &id,
                "verified",
                json!({"uploaded":true}),
                "uploaded",
            )
            .unwrap();
        let mut r = incoming();
        r.title = "Another paper".into();
        r.doi = "10.1234/another".into();
        store.import(vec![r], "another".into()).unwrap();
        let p = store.pending_input(&t.id).unwrap().unwrap();
        store
            .accept_input(
                &store.task(&t.id).unwrap(),
                &p,
                &live(&p.record),
                "来源",
                "已核对另一论文",
            )
            .unwrap();
        assert_eq!(
            store.assert_no_prior_upload(&candidate).unwrap_err().code,
            "DUPLICATE_WRITE"
        );
    }
    #[test]
    fn excel_done_flag_cannot_override_live_pending_and_input_archive_is_verified() {
        let (dir, store) = setup();
        let mut r = incoming();
        r.done = true;
        store.import(vec![r], "new".into()).unwrap();
        let p = store.pending_input("version-sa").unwrap().unwrap();
        assert!(validate_live(&p, &live(&p.record)).is_err());
        let path = dir.path().join("fixture.xlsx");
        std::fs::write(&path, b"synthetic input bytes").unwrap();
        let h = hash(b"synthetic input bytes");
        let archived = files::archive_roster(dir.path(), &path, &h).unwrap();
        assert_eq!(std::fs::read(archived).unwrap(), b"synthetic input bytes");
        std::fs::write(&path, b"changed").unwrap();
        assert_eq!(
            files::archive_roster(dir.path(), &path, &h)
                .unwrap_err()
                .code,
            "INPUT_CHANGED"
        );
    }
}
