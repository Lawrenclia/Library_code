pub mod catalog;
pub mod files;
pub mod issues;
pub mod legacy;
pub mod library;
pub mod metadata;
pub mod model;
pub mod queue;
pub mod sa;
pub mod store;
pub mod templates;
pub mod versions;
pub use model::*;
pub use store::Store;
#[cfg(test)]
mod issue_tests;
#[cfg(test)]
mod library_tests;
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
