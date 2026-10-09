use super::*;

impl Engine {
    pub fn review(&self, id: &str, mut r: Review, source: String, proof: String) -> Result<Task> {
        let mut t = self.store.task(id)?;
        if self.store.pending_input(id)?.is_some()
            || t.last_error
                .as_ref()
                .map(|e| e.code == "INPUT_CHANGED")
                .unwrap_or(false)
        {
            return Err(Failure::new(
                "INPUT_CHANGED",
                "名单关键字段已变化，先核对任务版本，不能用旧名单保存核验。",
            ));
        }
        if matches!(t.stage, Stage::Unknown | Stage::Completed) {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "当前状态需先回读或已经完成。",
            ));
        }
        if source.trim().is_empty() || proof.trim().is_empty() {
            return Err(Failure::new(
                "EVIDENCE_REQUIRED",
                "填写核验来源和具体依据。",
            ));
        }
        let e = Evidence {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "human_review".into(),
            source,
            text: proof,
            created: now(),
        };
        r.evidence_id = e.id.clone();
        t.evidence.push(e);
        if matches!(r.route, Route::Missing | Route::CorrectedExisting) {
            library::review(&t, &r)?;
            r.library_checked = true;
        }
        if t.needs_issue_review() {
            r.issues_resolved = t
                .sa_snapshot
                .as_ref()
                .map(|s| issues::assert_resolved(&t, s).is_ok())
                .unwrap_or(false);
        }
        t.validate_review(&r)?;
        if let Some(a) = t.artifact.as_mut() {
            if r.identity_confirmed {
                files::verify_identity(&t.record, &a.candidate)?;
                a.identity_confirmed = true;
            }
        }
        t.route = r.route.clone();
        t.platform_id = r.platform_id.clone();
        t.review = Some(r);
        t.last_error = None;
        if !matches!(
            t.stage,
            Stage::Uploaded | Stage::Imported | Stage::Pushed | Stage::Claimed
        ) {
            t.stage = if t.route == Route::Missing && t.import_ready().is_ok() {
                Stage::Ready
            } else {
                Stage::AwaitingReview
            };
        }
        self.store.save(&mut t, "human_review")?;
        Ok(t)
    }
}
