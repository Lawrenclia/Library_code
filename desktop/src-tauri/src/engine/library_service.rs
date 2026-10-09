use super::*;

impl Engine {
    pub(super) async fn search_library(
        &self,
        app: &AppHandle,
        t: &mut Task,
        mut target: Value,
    ) -> Result<Value> {
        let expected = target.clone();
        target["sa_id"] = t.id.clone().into();
        let result = self
            .browser
            .execute(app, "library", "library_search", target, 150)
            .await?;
        #[cfg(feature = "smoke-test")]
        let result = {
            let mut result = result;
            if crate::browser::fixture_origin()
                .as_ref()
                .map(|origin| {
                    result["source"]
                        .as_str()
                        .map(|s| {
                            s.starts_with(&format!(
                                "{}/advancedSearch",
                                origin.origin().ascii_serialization()
                            ))
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(false)
            {
                result["fixture_source"] = result["source"].clone();
                result["source"] = "http://www.ir.lib.sjtu.edu.cn/advancedSearch".into();
            }
            result
        };
        library::record(t, &expected, &result)?;
        self.store.save(t, "library_search")?;
        Ok(result)
    }
    pub(super) async fn handle_library_search(
        &self,
        app: &AppHandle,
        _id: &str,
        _action: &str,
        mut t: Task,
        extra: Value,
    ) -> Result<Value> {
        if t.stage == Stage::Completed {
            return Err(Failure::new("INVALID_TRANSITION", "任务已经完成。"));
        }
        let target = library::target(&t, extra["title"].as_str().unwrap_or(""))?;
        let result = self.search_library(app, &mut t, target).await?;
        self.changed(app);
        return Ok(result);
    }
}
