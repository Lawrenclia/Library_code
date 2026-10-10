use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Failure {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub submitted: Option<bool>,
}
pub type Result<T> = std::result::Result<T, Failure>;
impl Failure {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            submitted: None,
        }
    }
    pub fn storage(e: impl std::fmt::Display) -> Self {
        Self::new("STORAGE_ERROR", e.to_string())
    }
    pub fn channel(&self) -> bool {
        matches!(
            self.code.as_str(),
            "AUTH_REQUIRED" | "BROWSER_DISCONNECTED" | "PAGE_UNSUPPORTED"
        )
    }
}
impl From<rusqlite::Error> for Failure {
    fn from(e: rusqlite::Error) -> Self {
        Self::storage(e)
    }
}
impl From<serde_json::Error> for Failure {
    fn from(e: serde_json::Error) -> Self {
        Self::storage(e)
    }
}
impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Self::storage(e)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Pending,
    Searching,
    Downloading,
    Downloaded,
    Ready,
    Uploaded,
    Imported,
    Pushed,
    Claimed,
    Completed,
    AwaitingReview,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Existing,
    Duplicate,
    ZeroReview,
    NonSjtu,
    CorrectedExisting,
    Missing,
    NotFound,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub row: u32,
    pub owner: String,
    pub sa_id: String,
    pub title: String,
    pub doi: String,
    pub wos: String,
    pub staff_id: String,
    pub matches: u32,
    pub item_ids: String,
    pub mark: String,
    pub reason: String,
    pub skipped: bool,
    pub done: bool,
    pub source: String,
}
impl Record {
    pub fn fingerprint(&self) -> String {
        crate::hash(
            json!([
                self.owner,
                self.sa_id,
                self.title,
                self.doi,
                self.wos,
                self.staff_id,
                self.matches,
                self.item_ids,
                self.reason
            ])
            .to_string()
            .as_bytes(),
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub text: String,
    pub created: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub title: String,
    pub doi: String,
    pub wos: String,
    pub authors: String,
    pub year: String,
    pub journal: String,
    pub affiliation: String,
    pub sjtu: bool,
    pub sha256: String,
    pub fields: std::collections::BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub path: String,
    pub source: String,
    pub record_url: String,
    pub downloaded: u64,
    pub candidate: Candidate,
    pub identity_confirmed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paper {
    pub id: String,
    pub artifact: Artifact,
    pub updated: u64,
}
pub fn validate_alias_source(task: &Task, alias: &str, evidence_id: &str) -> Result<()> {
    if alias.trim().is_empty()
        || alias.chars().count() > 200
        || alias.chars().any(char::is_control)
        || !task.evidence.iter().any(|e| {
            e.id == evidence_id
                && matches!(e.kind.as_str(), "metadata" | "human_review")
                && norm(&e.text).contains(&norm(alias))
        })
    {
        return Err(Failure::new(
            "EVIDENCE_REQUIRED",
            "选择包含此真实文献署名的来源，不能凭相似姓名或平台别名列表新增。",
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub route: Route,
    pub evidence_id: String,
    pub library_checked: bool,
    pub platform_id: String,
    pub affiliation_confirmed: bool,
    pub identity_confirmed: bool,
    pub issues_resolved: bool,
    pub note: String,
}
pub fn verified_import_stage(result: &Value, require_push: bool) -> Result<Stage> {
    let status = result["batch"]["status"].as_u64().or_else(|| {
        result["batch"]["status"]
            .as_str()
            .and_then(|s| s.parse().ok())
    });
    if result["verified"] == true {
        match status {
            Some(2) => return Ok(Stage::Pushed),
            Some(1) if !require_push => return Ok(Stage::Imported),
            _ => {}
        }
    }
    Err(Failure::new(
        "REMOTE_RESULT_UNKNOWN",
        "尚未回读到本次导入/推送的准确完成状态，不能重发。",
    ))
}
pub fn validate_upload_readback(result: &Value, payload: &Value, size: usize) -> Result<()> {
    let sa_id = payload["sa_id"].as_str().unwrap_or("");
    let sha = payload["candidate"]["sha256"].as_str().unwrap_or("");
    if sa_id.is_empty()
        || sha.len() != 64
        || !sha.bytes().all(|b| b.is_ascii_hexdigit())
        || payload["contentSha"] != sha
        || payload["instructions"] != format!("SA补充-{sa_id}")
        || result["verified"] != true
        || result["uploaded"] != true
        || result["sa_id"] != sa_id
        || result["instructions"] != payload["instructions"]
        || result["sha256"] != sha
        || result["filename"] != format!("SA-WOS-{sa_id}.txt")
        || result["size"].as_u64() != Some(size as u64)
        || (payload.get("content_size").is_some()
            && payload["content_size"].as_u64() != Some(size as u64))
        || size == 0
        || size > 524288
        || result["dataset_label"] != "上海交通大学"
        || result["dataset_id"].as_str().is_none_or(|s| s.is_empty())
        || result["server_name"].as_str().is_none_or(|s| s.is_empty())
        || result["response"]["data"]["name"] != result["server_name"]
        || result["response"]["success"] == false
        || (result["response"]["success"] != true && result["response"]["code"] != 200)
    {
        return Err(Failure::new(
            "REMOTE_RESULT_UNKNOWN",
            "原上传的任务、文件、机构与成功回执尚未完整核验，不能重新上传。",
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRecord {
    pub group_id: String,
    pub source_id: String,
    pub target_id: String,
    pub evidence_id: String,
    pub created: u64,
}
pub fn matched_ids(row: &Value) -> Result<Vec<String>> {
    let count = row["matchCount"]
        .as_u64()
        .or_else(|| row["matchCount"].as_str().and_then(|s| s.parse().ok()))
        .ok_or_else(|| Failure::new("PAGE_UNSUPPORTED", "实时匹配数格式未知。"))?;
    let raw = row["itemId"]
        .as_str()
        .ok_or_else(|| Failure::new("IDENTITY_CONFLICT", "平台唯一号不是完整文本。"))?;
    let ids: Vec<String> = raw
        .trim_start_matches(',')
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_string())
        .collect();
    if count != ids.len() as u64
        || ids
            .iter()
            .any(|s| s.is_empty() || s.chars().any(char::is_whitespace))
        || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
    {
        return Err(Failure::new(
            "IDENTITY_CONFLICT",
            "实时匹配数与条目编号不一致。",
        ));
    }
    Ok(ids)
}
pub fn validate_merge_review(
    task: &Task,
    source: &str,
    target: &str,
    ids: &[String],
    evidence_id: &str,
    confirmed: bool,
    retained: &str,
) -> Result<()> {
    if task.route != Route::Duplicate
        || matches!(task.stage, Stage::Unknown | Stage::Completed)
        || source == target
        || !ids.iter().any(|s| s == source)
        || !ids.iter().any(|s| s == target)
    {
        return Err(Failure::new(
            "INVALID_TRANSITION",
            "只合并当前 SA 匹配的两个不同条目，且需先核验上次结果。",
        ));
    }
    if !confirmed
        || retained.trim().is_empty()
        || !task
            .evidence
            .iter()
            .any(|e| e.id == evidence_id && matches!(e.kind.as_str(), "metadata" | "human_review"))
    {
        return Err(Failure::new(
            "EVIDENCE_REQUIRED",
            "需提供同一论文的来源依据、保留字段说明并确认本条合并。",
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub paper_id: String,
    pub record: Record,
    pub input_hash: String,
    pub revision: i64,
    pub route: Route,
    pub stage: Stage,
    pub running: bool,
    pub last_error: Option<Failure>,
    pub evidence: Vec<Evidence>,
    pub artifact: Option<Artifact>,
    pub review: Option<Review>,
    pub classification: Option<Value>,
    pub platform_id: String,
    pub batch: Option<Value>,
    pub sa_snapshot: Option<Value>,
    #[serde(default)]
    pub merges: Vec<MergeRecord>,
    #[serde(default)]
    pub issue_plan: Option<crate::issues::IssuePlan>,
    #[serde(default)]
    pub issue_reviews: Vec<crate::issues::IssueReview>,
    pub updated: u64,
}
impl Task {
    pub fn needs_issue_review(&self) -> bool {
        self.record.matches == 1
            || self.issue_plan.is_some()
            || self
                .sa_snapshot
                .as_ref()
                .map(|s| {
                    matched_ids(&s["row"])
                        .map(|ids| ids.len() == 1)
                        .unwrap_or(false)
                })
                .unwrap_or(false)
    }
    pub fn new(record: Record, input_hash: String) -> Self {
        let paper_id =
            crate::hash(format!("{}\n{}", norm(&record.title), norm(&record.doi)).as_bytes());
        let route = match record.matches {
            0 => Route::ZeroReview,
            1 => Route::Existing,
            _ => Route::Duplicate,
        };
        let stage = if record.done {
            Stage::Completed
        } else {
            Stage::Pending
        };
        Self {
            id: record.sa_id.clone(),
            paper_id,
            record,
            input_hash,
            revision: 0,
            route,
            stage,
            running: false,
            last_error: None,
            evidence: vec![],
            artifact: None,
            review: None,
            classification: None,
            platform_id: String::new(),
            batch: None,
            sa_snapshot: None,
            merges: vec![],
            issue_plan: None,
            issue_reviews: vec![],
            updated: crate::now(),
        }
    }
    pub fn import_ready(&self) -> Result<()> {
        let review = self
            .review
            .as_ref()
            .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "请先核验文献归属与机构库检索结果。"))?;
        if self.record.matches != 0
            || self.route != Route::Missing
            || !review.library_checked
            || !review.affiliation_confirmed
            || !review.identity_confirmed
        {
            return Err(Failure::new(
                "REVIEW_REQUIRED",
                "只有身份和交大归属已核实、且机构库确实缺失的零匹配成果可以导入。",
            ));
        }
        if !self.evidence.iter().any(|e| e.id == review.evidence_id) {
            return Err(Failure::new("EVIDENCE_REQUIRED", "核验缺少原始依据。"));
        }
        crate::library::assert_absent(self)?;
        let a = self
            .artifact
            .as_ref()
            .ok_or_else(|| Failure::new("FILE_INVALID", "缺少核验后的原始导出文件。"))?;
        if !a.identity_confirmed || !a.candidate.sjtu {
            return Err(Failure::new(
                "IDENTITY_CONFLICT",
                "文件身份或交大署名尚未通过核验。",
            ));
        }
        Ok(())
    }
    pub fn validate_review(&self, r: &Review) -> Result<()> {
        if !self.evidence.iter().any(|e| e.id == r.evidence_id) || r.note.trim().is_empty() {
            return Err(Failure::new(
                "EVIDENCE_REQUIRED",
                "选择来源依据并填写核验结论。",
            ));
        }
        match (&self.record.matches, &r.route) {
            (0, Route::Missing) => {
                if !(r.library_checked && r.affiliation_confirmed && r.identity_confirmed) {
                    return Err(Failure::new(
                        "REVIEW_REQUIRED",
                        "需确认身份、交大归属及机构库缺失。",
                    ));
                }
            }
            (0, Route::NonSjtu) if r.note.contains("非交大") => {}
            (0, Route::NotFound) => {}
            (0, Route::CorrectedExisting)
                if r.library_checked && !r.platform_id.trim().is_empty() => {}
            (1, Route::Existing) => {}
            (n, Route::Duplicate) if *n >= 2 => {}
            _ => {
                return Err(Failure::new(
                    "INVALID_ROUTE",
                    "核验分支与匹配数量或平台唯一号不符。",
                ))
            }
        }
        crate::library::review(self, r)?;
        Ok(())
    }
    pub fn assert_complete(&self) -> Result<()> {
        let r = self
            .review
            .as_ref()
            .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "请先完成逐项核验。"))?;
        if self.route == Route::NotFound || self.route == Route::ZeroReview {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "未查询到文献不能设置平台已处理。",
            ));
        }
        if !r.issues_resolved {
            return Err(Failure::new("REVIEW_REQUIRED", "待处理原因尚未逐项解决。"));
        }
        if matches!(self.route, Route::Missing | Route::CorrectedExisting) {
            crate::library::review(self, r)?;
        }
        if self.route == Route::Existing {
            if !r.identity_confirmed {
                return Err(Failure::new(
                    "REVIEW_REQUIRED",
                    "先核实为目标文献，再完成现有条目核对。",
                ));
            }
        }
        if self.needs_issue_review() {
            let snapshot = self
                .sa_snapshot
                .as_ref()
                .ok_or_else(|| Failure::new("REVIEW_REQUIRED", "先读取实时 SA 逐项核对清单。"))?;
            crate::issues::assert_resolved(self, snapshot)?;
        }
        if self.route == Route::Missing && !matches!(self.stage, Stage::Pushed | Stage::Claimed) {
            return Err(Failure::new(
                "INVALID_TRANSITION",
                "补充成果需先核验推送结果。",
            ));
        }
        if self.route == Route::Duplicate && !self.has_verified_merges() {
            return Err(Failure::new(
                "REVIEW_REQUIRED",
                "需保留主条目、被合并编号及实际合并回读，再核验 SA。",
            ));
        }
        Ok(())
    }
    pub fn has_verified_merges(&self) -> bool {
        if self.platform_id.is_empty() || self.merges.is_empty() {
            return false;
        }
        let mut sources = std::collections::HashSet::new();
        self.merges.iter().enumerate().all(|(index, m)| {
            if m.source_id.is_empty()
                || m.source_id == m.target_id
                || !sources.insert(&m.source_id)
                || !self
                    .evidence
                    .iter()
                    .any(|e| e.id == m.evidence_id && e.kind == "duplicate_merge_verified")
            {
                return false;
            }
            let mut target = m.target_id.as_str();
            for later in &self.merges[index + 1..] {
                if later.source_id == target {
                    target = &later.target_id;
                }
            }
            target == self.platform_id
        })
    }
}
pub fn norm(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfkc()
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn normalized_doi(s: &str) -> String {
    let s = norm(s);
    s.trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .trim_start_matches("doi:")
        .trim()
        .to_string()
}
pub fn normalized_wos(s: &str) -> String {
    let s = s.trim().to_uppercase();
    if s.is_empty() {
        s
    } else if s.starts_with("WOS:") {
        s
    } else {
        format!("WOS:{s}")
    }
}
pub fn is_write(action: &str) -> bool {
    matches!(
        action,
        "import_upload"
            | "import_submit"
            | "import_push"
            | "submit_claim"
            | "add_alias"
            | "alias_add"
            | "merge_duplicate"
            | "duplicate_merge"
            | "save_metadata"
            | "metadata_save"
            | "link"
            | "complete"
    )
}
pub const PUSH_POLICY:&str="查重：是；唯一标识 +（期刊 & 发表时间 & 页码 & 卷 & 期 & 题名相似度）+ 题名相似度；重复元数据：根据优先级合并；不存在的条目：新增；本校成果：是";
