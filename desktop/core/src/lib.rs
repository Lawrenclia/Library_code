pub mod ai_queue;
pub mod alias;
pub mod catalog;
pub mod claim;
pub mod classification;
pub mod download;
pub mod files;
pub mod framework;
pub mod issues;
pub mod legacy;
pub mod legacy_materials;
pub mod library;
pub mod material_batch;
pub mod materials;
pub mod merge;
pub mod metadata;
pub mod metadata_order;
pub mod model;
pub mod queue;
pub mod sa;
pub mod source_files;
pub mod store;
pub mod template_rules;
pub mod templates;
pub mod versions;
pub mod workflow;
pub use model::*;
pub use store::Store;
#[cfg(test)]
mod ai_queue_tests;
#[cfg(test)]
mod alias_tests;
#[cfg(test)]
mod claim_tests;
#[cfg(test)]
mod download_tests;
#[cfg(test)]
mod issue_tests;
#[cfg(test)]
mod library_tests;
#[cfg(test)]
mod materials_tests;
#[cfg(test)]
mod merge_tests;
#[cfg(test)]
mod metadata_order_tests;
#[cfg(test)]
mod metadata_tests;
#[cfg(test)]
mod queue_tests;
#[cfg(test)]
mod sa_tests;
#[cfg(test)]
mod workflow_tests;

use std::time::{SystemTime, UNIX_EPOCH};
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
