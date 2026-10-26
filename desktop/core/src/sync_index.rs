//! Destination sync index for fast plan scans.
//!
//! Lives at `<dest>/.syncdrop/index-<profileId>.json`. Compare source size+mtime to the
//! index so the plan can skip statting the destination when the last sync is known.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Metadata folder on the destination (`index-*.json`, `RESTORE.md`).
pub const SYNCDROP_META_DIR: &str = ".syncdrop";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub size: u64,
    /// Modified time as unix seconds.
    pub mtime_secs: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncIndex {
    pub profile_id: String,
    pub files: BTreeMap<String, IndexEntry>,
}

impl SyncIndex {
    pub fn new(profile_id: Uuid) -> Self {
        Self {
            profile_id: profile_id.to_string(),
            files: BTreeMap::new(),
        }
    }

    pub fn path_for(dest: &Path, profile_id: Uuid) -> PathBuf {
        dest.join(SYNCDROP_META_DIR)
            .join(format!("index-{profile_id}.json"))
    }

    pub fn load(dest: &Path, profile_id: Uuid) -> Option<Self> {
        let path = Self::path_for(dest, profile_id);
        let file = File::open(&path).ok()?;
        let loaded: SyncIndex = serde_json::from_reader(BufReader::new(file)).ok()?;
        if loaded.profile_id != profile_id.to_string() {
            return None;
        }
        Some(loaded)
    }

    pub fn save(&self, dest: &Path) -> io::Result<()> {
        let path = Self::path_for(dest, {
            Uuid::parse_str(&self.profile_id).map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, e.to_string())
            })?
        });
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        {
            let file = File::create(&tmp)?;
            serde_json::to_writer_pretty(BufWriter::new(file), self)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        }
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn matches(&self, rel: &str, size: u64, modified: SystemTime) -> bool {
        match self.files.get(rel) {
            Some(entry) => entry.size == size && entry.mtime_secs == system_time_secs(modified),
            None => false,
        }
    }

    pub fn insert_meta(&mut self, rel: impl Into<String>, size: u64, modified: SystemTime) {
        self.files.insert(
            rel.into(),
            IndexEntry {
                size,
                mtime_secs: system_time_secs(modified),
            },
        );
    }
}

pub fn system_time_secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn round_trip_index() {
        let dir = tempdir().unwrap();
        let id = Uuid::new_v4();
        let mut index = SyncIndex::new(id);
        let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        index.insert_meta("a/b.txt", 42, t);
        index.save(dir.path()).unwrap();

        let loaded = SyncIndex::load(dir.path(), id).unwrap();
        assert_eq!(loaded.files.len(), 1);
        assert!(loaded.matches("a/b.txt", 42, t));
        assert!(!loaded.matches("a/b.txt", 41, t));
    }

    #[test]
    fn wrong_profile_id_rejected() {
        let dir = tempdir().unwrap();
        let id = Uuid::new_v4();
        let index = SyncIndex::new(id);
        index.save(dir.path()).unwrap();
        assert!(SyncIndex::load(dir.path(), Uuid::new_v4()).is_none());
    }
}
