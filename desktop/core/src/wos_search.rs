//! Append-only execution observations. They never establish metadata or library absence.
use crate::*;
use rusqlite::params;
use serde_json::{json, Value};

pub struct Journal {
    store: Store,
    id: String,
    task_id: String,
    record: Record,
    input_hash: String,
    revision: i64,
}
pub fn begin(store: &Store, task: &Task) -> Result<Journal> {
    let current = store.task(&task.id)?;
    if current.revision != task.revision
        || current.input_hash != task.input_hash
        || current.record.fingerprint() != task.record.fingerprint()
        || current.stage != Stage::Searching
        || !current.running
        || store.pending_input(&task.id)?.is_some()
        || !store.unresolved(&task.id)?.is_empty()
    {
        return Err(Failure::new(
            "TASK_CHANGED",
            "开始检索记录前原任务已变化，未派发查询。",
        ));
    }
    let journal = Journal {
        store: store.clone(),
        id: uuid::Uuid::new_v4().to_string(),
        task_id: task.id.clone(),
        record: task.record.clone(),
        input_hash: task.input_hash.clone(),
        revision: task.revision,
    };
    journal.record("started", None, json!({"query_sent":false}))?;
    Ok(journal)
}
impl Journal {
    pub fn record(&self, phase: &str, source: Option<&str>, data: Value) -> Result<()> {
        if ![
            "started",
            "prepared",
            "dispatch_requested",
            "observed_result",
            "finished",
        ]
        .contains(&phase)
            || serde_json::to_vec(&data)?.len() > 128 * 1024
        {
            return Err(Failure::new(
                "SEARCH_SCOPE_INVALID",
                "检索记录阶段无效或超过 128 KB，未丢弃原查询记录。",
            ));
        }
        let source = source
            .and_then(|value| url::Url::parse(value).ok())
            .filter(|u| {
                u.scheme() == "https"
                    && u.username().is_empty()
                    && u.password().is_none()
                    && matches!(
                        u.host_str(),
                        Some("webofscience.clarivate.cn" | "www.webofscience.com")
                    )
                    && u.path().starts_with("/wos/")
            })
            .map(|u| format!("{}{}", u.origin().ascii_serialization(), u.path()));
        let current = self.store.task(&self.task_id)?;
        let snapshot_matches = current.input_hash == self.input_hash
            && current.record.fingerprint() == self.record.fingerprint()
            && current.revision == self.revision;
        let receipt = json!({"schema":"wos_search_trace_v1","trace_id":self.id,"phase":phase,
            "task_id":self.task_id,"input_hash":self.input_hash,"record_fingerprint":self.record.fingerprint(),
            "original_record":self.record,"original_revision":self.revision,"current_revision":current.revision,
            "task_snapshot_matches":snapshot_matches,"observed_at":now(),"source_url":source,
            "data":data,"factual_metadata":false,"platform_verified":false,"absence_proof":false});
        self.store.connect()?.execute(
            "INSERT INTO events(task_id,created,kind,data) VALUES(?,?,?,?)",
            params![
                self.task_id,
                now() as i64,
                "wos_search_trace",
                receipt.to_string()
            ],
        )?;
        Ok(())
    }
    pub fn finish(&self, result: &Result<Value>) -> Result<()> {
        match result {
            Ok(value) => self.record("finished", value["record_url"].as_str(), json!({"result":value,"outcome":"record_located","download_completed":false})),
            Err(error) => self.record("finished", None, json!({"error":error,"outcome":"search_failed","zero_results_reported":error.code=="NO_RESULT","download_completed":false})),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, Store, Task) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path()).unwrap();
        let record: Record = serde_json::from_value(json!({"row":2,"owner":"owner","sa_id":"trace-task",
            "title":"Original paper title","doi":"10.1234/test","wos":"","staff_id":"001","matches":0,
            "item_ids":"","mark":"待处理","reason":"missing","skipped":false,"done":false,"source":"list.xlsx"})).unwrap();
        store.import(vec![record], "original-input".into()).unwrap();
        let mut task = store.task("trace-task").unwrap();
        task.stage = Stage::Searching;
        task.running = true;
        store.save(&mut task, "search_started").unwrap();
        (dir, store, task)
    }
    const PAGE: &str = "https://webofscience.clarivate.cn/wos/woscc/basic-search";
    #[test]
    fn actual_doi_query_zero_page_and_full_origin_survive_restart_and_excel_without_metadata_claims(
    ) {
        let (dir, store, task) = setup();
        let journal = begin(&store, &task).unwrap();
        let prepared = json!({"ready":true,"search_context":{"schema":"wos_search_context_v1",
            "field":"doi","field_label":"DOI","query":"10.1234/test","scope_controls":["Editions: All"],
            "scope_controls_exhaustive":false,"submission_confirmed":false}});
        journal.record("prepared", Some(PAGE), prepared).unwrap();
        journal
            .record(
                "dispatch_requested",
                Some(PAGE),
                json!({"submission_confirmed":false}),
            )
            .unwrap();
        journal
            .record(
                "observed_result",
                Some(PAGE),
                json!({"state":"zero","diagnostic":{"result_total":0}}),
            )
            .unwrap();
        journal
            .finish(&Err(Failure::new("NO_RESULT", "observed zero result")))
            .unwrap();
        assert_eq!(store.task(&task.id).unwrap().revision, task.revision);
        assert!(store.task(&task.id).unwrap().artifact.is_none());
        assert!(store.task(&task.id).unwrap().evidence.is_empty());
        let reopened = Store::new(dir.path()).unwrap();
        let traces = reopened.wos_search_traces(&task.id).unwrap();
        assert_eq!(traces.len(), 5);
        assert_eq!(traces[1]["data"]["search_context"]["query"], "10.1234/test");
        assert_eq!(
            traces[1]["original_record"]["title"],
            "Original paper title"
        );
        assert_eq!(traces[1]["input_hash"], "original-input");
        assert!(traces.iter().all(|e| e["factual_metadata"] == false
            && e["absence_proof"] == false
            && e["platform_verified"] == false));
        let (report, queues) = reopened.report_snapshot().unwrap();
        assert_eq!(report[0].evidence.len(), 5);
        assert!(classification::sources(dir.path(), &report[0])
            .unwrap()
            .iter()
            .all(|e| e.kind != "database_search_trace"));
        assert!(search_scopes::assert_not_found(&report[0]).is_err());
        let path = dir.path().join("trace.xlsx");
        files::export_report_with_queues(&report, &queues, &path).unwrap();
        use calamine::{open_workbook_auto, Reader};
        let mut book = open_workbook_auto(path).unwrap();
        let sheet = book.worksheet_range("原始来源依据").unwrap();
        let preserved = sheet
            .rows()
            .skip(1)
            .flat_map(|row| row.iter().map(ToString::to_string))
            .collect::<String>();
        assert!(preserved.contains("10.1234/test"));
        assert!(preserved.contains("wos_search_context_v1"));
    }
    #[test]
    fn original_query_audit_never_overwrites_an_advanced_or_changed_business_task() {
        let (_dir, store, task) = setup();
        let journal = begin(&store, &task).unwrap();
        let mut advanced = task.clone();
        advanced.stage = Stage::Imported;
        advanced.running = false;
        advanced.platform_id = "new-platform-id".into();
        store
            .save(&mut advanced, "fixture_business_advanced")
            .unwrap();
        journal
            .finish(&Err(Failure::new("PAGE_TIMEOUT", "late error")))
            .unwrap();
        assert_eq!(
            serde_json::to_value(store.task(&task.id).unwrap()).unwrap(),
            serde_json::to_value(&advanced).unwrap()
        );
        let last = store.wos_search_traces(&task.id).unwrap().pop().unwrap();
        assert_eq!(last["task_snapshot_matches"], false);
        assert_eq!(last["original_revision"], task.revision);
        assert_eq!(last["current_revision"], advanced.revision);
        assert_eq!(last["data"]["error"]["code"], "PAGE_TIMEOUT");
    }
    #[test]
    fn interrupted_trace_stays_unconfirmed_and_invalid_or_oversized_records_do_not_replace_it() {
        let (dir, store, task) = setup();
        let journal = begin(&store, &task).unwrap();
        journal
            .record("prepared", Some(PAGE), json!({"ready":true}))
            .unwrap();
        journal
            .record(
                "dispatch_requested",
                Some(PAGE),
                json!({"submission_confirmed":false}),
            )
            .unwrap();
        assert!(journal.record("downloaded", Some(PAGE), json!({})).is_err());
        assert!(journal
            .record(
                "observed_result",
                Some(PAGE),
                json!({"data":"😀".repeat(40000)})
            )
            .is_err());
        drop(journal);
        let reopened = Store::new(dir.path()).unwrap();
        reopened.recover().unwrap();
        let traces = reopened.wos_search_traces(&task.id).unwrap();
        assert_eq!(traces.len(), 3);
        assert_eq!(traces[2]["phase"], "dispatch_requested");
        assert_eq!(traces[2]["data"]["submission_confirmed"], false);
        assert!(reopened.task(&task.id).unwrap().artifact.is_none());
        assert!(traces
            .iter()
            .all(|e| e["data"]["outcome"] != "record_located"));
    }
}
impl Store {
    pub fn wos_search_traces(&self, id: &str) -> Result<Vec<Value>> {
        let db = self.connect()?;
        let mut stmt = db.prepare(
            "SELECT seq,data FROM events WHERE task_id=? AND kind='wos_search_trace' ORDER BY seq",
        )?;
        let mut traces = Vec::new();
        for row in stmt.query_map([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (seq, raw) = row?;
            let mut value: Value = serde_json::from_str(&raw)?;
            value["event_id"] = json!(seq);
            traces.push(value);
        }
        Ok(traces)
    }
}
