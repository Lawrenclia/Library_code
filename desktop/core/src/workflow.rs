//! The closed set of public workflow operations. Page-level commands are separate.
use crate::{Failure, Result};
use serde::{Deserialize, Serialize};

macro_rules! actions {
    ($($variant:ident => ($id:literal, $service:literal, $kind:ident)),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum StepAction { $(#[serde(rename = $id)] $variant),+ }
        impl StepAction {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn parse(id: &str) -> Result<Self> {
                match id { $($id => Ok(Self::$variant),)+ _ => Err(Failure::new("INVALID_ACTION", "未知业务操作。")) }
            }
            pub fn id(self) -> &'static str { match self { $(Self::$variant => $id),+ } }
            pub fn service(self) -> &'static str { match self { $(Self::$variant => $service),+ } }
            pub fn kind(self) -> OperationKind { match self { $(Self::$variant => OperationKind::$kind),+ } }
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Read,
    Prepare,
    LocalReview,
    Recover,
    Write,
}

actions! {
    ReadSa => ("read_sa", "sa", Read),
    LibrarySearch => ("library_search", "library", Read),
    SearchWos => ("search_wos", "downloads", Read),
    PrepareIssues => ("prepare_issues", "sa", Prepare),
    PrepareNote => ("prepare_note", "sa", Prepare),
    VerifyNote => ("verify_note", "sa", Recover),
    ReviewIssue => ("review_issue", "sa", LocalReview),
    PrepareMetadata => ("prepare_metadata", "metadata", Prepare),
    OpenMetadata => ("open_metadata", "authors", Read),
    OpenClaim => ("open_claim", "authors", Read),
    PrepareClaim => ("prepare_claim", "authors", Prepare),
    ScanDuplicates => ("scan_duplicates", "duplicates", Read),
    PrepareDuplicate => ("prepare_duplicate", "duplicates", Prepare),
    PrepareAlias => ("prepare_alias", "authors", Prepare),
    VerifyMetadata => ("verify_metadata", "metadata", Recover),
    VerifyDuplicate => ("verify_duplicate", "duplicates", Recover),
    VerifyAlias => ("verify_alias", "authors", Recover),
    VerifySa => ("verify_sa", "sa", Recover),
    VerifyLegacySa => ("verify_legacy_sa", "sa", Recover),
    VerifyLegacyClaim => ("verify_legacy_claim", "authors", Recover),
    VerifyImport => ("verify_import", "submission", Recover),
    SaveMetadata => ("save_metadata", "metadata", Write),
    MergeDuplicate => ("merge_duplicate", "duplicates", Write),
    AddAlias => ("add_alias", "authors", Write),
    ImportUpload => ("import_upload", "submission", Write),
    ImportSubmit => ("import_submit", "submission", Write),
    ImportPush => ("import_push", "submission", Write),
    Link => ("link", "sa", Write),
    Complete => ("complete", "sa", Write),
    SubmitClaim => ("submit_claim", "authors", Write),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_operation_round_trips_and_agrees_with_write_protection() {
        let mut ids = std::collections::HashSet::new();
        for action in StepAction::ALL {
            assert!(ids.insert(action.id()));
            assert_eq!(StepAction::parse(action.id()).unwrap(), *action);
            assert_eq!(
                crate::is_write(action.id()),
                action.kind() == OperationKind::Write
            );
            assert_eq!(serde_json::to_value(action).unwrap(), action.id());
        }
    }
    #[test]
    fn page_commands_and_unknown_operations_are_not_public_workflow_steps() {
        for id in [
            "metadata_save",
            "duplicate_merge",
            "alias_add",
            "eval",
            "",
            "import_submit ",
        ] {
            assert_eq!(StepAction::parse(id).unwrap_err().code, "INVALID_ACTION");
        }
    }
}
