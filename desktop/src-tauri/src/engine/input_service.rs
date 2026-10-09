use super::*;

impl Engine {
    pub async fn prepare_input_version(&self, app: &AppHandle, id: &str) -> Result<Value> {
        let proposal = self
            .store
            .pending_input(id)?
            .ok_or_else(|| Failure::new("INPUT_UNCHANGED", "没有待核验的新名单版本。"))?;
        let mut task = self.store.task(id)?;
        if !self.store.unresolved(id)?.is_empty() || task.stage == Stage::Unknown {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "先回读旧版本的待确认操作，不丢弃原执行记录。",
            ));
        }
        let snapshot = self.read_sa(app, &mut task).await?;
        library_core::versions::validate_live(&proposal, &snapshot)?;
        let result = json!({"proposal_id":proposal.id,"previous_fingerprint":task.record.fingerprint(),"snapshot":snapshot});
        self.store
            .set_setting(&format!("input-version:{id}"), result.clone())?;
        self.changed(app);
        Ok(result)
    }
    pub async fn accept_input_version(
        &self,
        app: &AppHandle,
        id: &str,
        proposal_id: &str,
        source: &str,
        proof: &str,
    ) -> Result<Task> {
        let proposal = self
            .store
            .pending_input(id)?
            .ok_or_else(|| Failure::new("INPUT_UNCHANGED", "没有待核验的新名单版本。"))?;
        let prepared = self
            .store
            .setting(&format!("input-version:{id}"))?
            .ok_or_else(|| {
                Failure::new("REVIEW_REQUIRED", "先读取并核对新名单与实时 SA 的差异。")
            })?;
        let mut task = self.store.task(id)?;
        if proposal.id != proposal_id
            || prepared["proposal_id"] != proposal_id
            || prepared["previous_fingerprint"] != task.record.fingerprint()
        {
            return Err(Failure::new(
                "INPUT_CHANGED",
                "名单版本在准备后变化，请重新读取差异。",
            ));
        }
        if !self.store.unresolved(id)?.is_empty() || task.stage == Stage::Unknown {
            return Err(Failure::new(
                "REMOTE_RESULT_UNKNOWN",
                "旧版本仍有待确认操作，不能切换。",
            ));
        }
        let snapshot = self.read_sa(app, &mut task).await?;
        if snapshot != prepared["snapshot"] {
            return Err(Failure::new(
                "TASK_CHANGED",
                "平台字段在核对后变化，请重新读取新版本。",
            ));
        }
        let next = self
            .store
            .accept_input(&task, &proposal, &snapshot, source, proof)?;
        self.changed(app);
        Ok(next)
    }
}
