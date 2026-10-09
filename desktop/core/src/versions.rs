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
            let raw = std::fs::read(&a.path)?;
            let parsed = files::parse_wos(&raw)?;
            if parsed.sha256 != a.candidate.sha256 {
                return Err(Failure::new(
                    "FILE_CHANGED",
                    "旧归档文件已变化，未切换版本。",
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
            next.artifact = Some(artifact);
            next.batch = old.batch.clone();
            if !next.record.done && old.batch.is_some() {
                next.stage = match proposal.previous_stage.as_ref().unwrap_or(&old.stage) {
                    Stage::Pushed | Stage::Claimed => Stage::Pushed,
                    Stage::Imported => Stage::Imported,
                    Stage::Uploaded => Stage::Uploaded,
                    _ => Stage::AwaitingReview,
                };
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
        assert_eq!(next.stage, Stage::Pushed);
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
