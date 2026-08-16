//! Deterministic environment lockfile engine (`naner.lock`).
//! Captures resolved vendor versions, exact download URLs, and SHA-256 digests.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub const LOCKFILE_NAME: &str = "naner.lock";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NanerLockfile {
    pub version: String,
    pub vendors: BTreeMap<String, LockedVendor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LockedVendor {
    pub version: String,
    pub url: String,
    pub sha256: Option<String>,
}

impl NanerLockfile {
    pub fn load(naner_root: &Path) -> Option<Self> {
        let lock_path = naner_root.join(LOCKFILE_NAME);
        if !lock_path.is_file() {
            return None;
        }
        let content = fs::read_to_string(lock_path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn save(&self, naner_root: &Path) -> bool {
        let lock_path = naner_root.join(LOCKFILE_NAME);
        if let Ok(json) = serde_json::to_string_pretty(self) {
            fs::write(lock_path, json).is_ok()
        } else {
            false
        }
    }
}
