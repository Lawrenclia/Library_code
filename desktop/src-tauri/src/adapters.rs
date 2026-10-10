//! Bundled page drivers. Only Browser may inject them into its scoped webviews.
use library_core::{Failure, Result};

pub struct PageAdapter {
    pub script: &'static str,
    pub handler: &'static str,
}
pub fn resolve(role: &str, action: &str) -> Result<PageAdapter> {
    let (script, handler) = match role {
        label if label.starts_with("source-") && action == "cnki_capture" => (
            include_str!("../browser/cnki-adapter.cjs"),
            "runCNKICommand",
        ),
        "wos" => (
            include_str!("../../../extension/wos-adapter.js"),
            "runWOSCommand",
        ),
        "sa" if action.starts_with("metadata_") => (
            include_str!("../browser/metadata-adapter.cjs"),
            "runMetadataCommand",
        ),
        "sa" => (
            include_str!("../../../extension/adapter.js"),
            "runSACommand",
        ),
        "import" => (
            include_str!("../../../extension/import-adapter.js"),
            "runImportCommand",
        ),
        "scholar" => (
            include_str!("../browser/scholar-adapter.cjs"),
            "runScholarCommand",
        ),
        "duplicate" => (
            include_str!("../browser/duplicate-adapter.cjs"),
            "runDuplicateCommand",
        ),
        "library" => (
            include_str!("../browser/library-adapter.cjs"),
            "runLibraryCommand",
        ),
        _ => return Err(Failure::new("INVALID_CHANNEL", "未注册的浏览器通道。")),
    };
    Ok(PageAdapter { script, handler })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_declared_browser_has_a_bundled_driver_and_metadata_is_scoped_to_sa() {
        for spec in library_core::framework::BROWSERS {
            let adapter = resolve(spec.id, "read_sa").unwrap();
            assert!(adapter.script.contains(adapter.handler));
        }
        assert_eq!(
            resolve("sa", "metadata_read").unwrap().handler,
            "runMetadataCommand"
        );
        assert_eq!(resolve("sa", "read_sa").unwrap().handler, "runSACommand");
        assert_eq!(
            resolve("wos", "metadata_read").unwrap().handler,
            "runWOSCommand"
        );
        assert!(resolve("unknown", "read_sa").is_err());
    }
}
