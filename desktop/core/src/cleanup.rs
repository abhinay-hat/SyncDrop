//! Find and remove dependency/cache folders already present on a profile destination.

use std::fs;
use std::io;
use std::path::Path;

use walkdir::WalkDir;

use crate::engine::VERSIONS_DIR;
use crate::smart_ignore;
use crate::sync_index::SYNCDROP_META_DIR;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupTarget {
    pub relative_path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CleanupPlan {
    pub targets: Vec<CleanupTarget>,
    pub total_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum CleanupError {
    #[error("destination path is empty")]
    EmptyDest,
    #[error("destination is outside the profile path")]
    OutsideDest,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Scan `dest` for dependency/cache folders (same rules as smart ignore).
/// Never reports anything outside `dest`.
pub fn plan_cleanup(dest: &Path) -> Result<CleanupPlan, CleanupError> {
    if dest.as_os_str().is_empty() {
        return Err(CleanupError::EmptyDest);
    }
    if !dest.is_dir() {
        return Ok(CleanupPlan::default());
    }
    let dest_canon = fs::canonicalize(dest).unwrap_or_else(|_| dest.to_path_buf());
    let mut targets = Vec::new();
    let mut walker = WalkDir::new(dest).min_depth(1).follow_links(false).into_iter();
    while let Some(item) = walker.next() {
        let entry = item.map_err(|e| {
            io::Error::new(io::ErrorKind::Other, e.to_string())
        })?;
        let abs = entry.path();
        ensure_under(&dest_canon, abs)?;
        let rel = rel_of(dest, abs).map_err(CleanupError::Io)?;
        if is_meta(&rel) {
            if entry.file_type().is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        if entry.file_type().is_dir() && smart_ignore::dependency_folder_at(dest, &rel) {
            let bytes = dir_size(abs)?;
            targets.push(CleanupTarget {
                relative_path: rel,
                bytes,
            });
            walker.skip_current_dir();
        }
    }
    targets.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    let total_bytes = targets.iter().map(|t| t.bytes).sum();
    Ok(CleanupPlan {
        targets,
        total_bytes,
    })
}

/// Delete planned targets. Refuses any path that escapes `dest`.
pub fn apply_cleanup(dest: &Path, plan: &CleanupPlan) -> Result<(), CleanupError> {
    if dest.as_os_str().is_empty() {
        return Err(CleanupError::EmptyDest);
    }
    let dest_canon = fs::canonicalize(dest).unwrap_or_else(|_| dest.to_path_buf());
    for target in &plan.targets {
        if target.relative_path.contains("..") {
            return Err(CleanupError::OutsideDest);
        }
        let abs = dest.join(&target.relative_path);
        ensure_under(&dest_canon, &abs)?;
        if abs.is_dir() {
            fs::remove_dir_all(&abs)?;
        }
    }
    Ok(())
}

fn is_meta(rel: &str) -> bool {
    let first = rel.split('/').next().unwrap_or(rel);
    first == VERSIONS_DIR
        || first == SYNCDROP_META_DIR
        || first == ".syncdrop-versions"
        || first == ".syncdrop_archive"
}

fn ensure_under(root_canon: &Path, candidate: &Path) -> Result<(), CleanupError> {
    let cand = if candidate.exists() {
        fs::canonicalize(candidate).unwrap_or_else(|_| candidate.to_path_buf())
    } else {
        candidate.to_path_buf()
    };
    if cand == *root_canon || cand.starts_with(root_canon) {
        Ok(())
    } else {
        Err(CleanupError::OutsideDest)
    }
}

fn rel_of(root: &Path, path: &Path) -> io::Result<String> {
    let rel = path.strip_prefix(root).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "path not under root")
    })?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

fn dir_size(path: &Path) -> io::Result<u64> {
    let mut total = 0u64;
    for item in WalkDir::new(path).follow_links(false) {
        let entry = item.map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        if entry.file_type().is_file() {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

/// Convenience: plan + optional apply. Used by the CLI.
pub fn cleanup(dest: &Path, dry_run: bool) -> Result<CleanupPlan, CleanupError> {
    let plan = plan_cleanup(dest)?;
    if !dry_run && !plan.targets.is_empty() {
        apply_cleanup(dest, &plan)?;
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn finds_node_modules_and_reports_size() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("app").join("node_modules").join("x");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("index.js"), "hello").unwrap();
        fs::write(dir.path().join("app").join("index.js"), "keep").unwrap();

        let plan = plan_cleanup(dir.path()).unwrap();
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].relative_path, "app/node_modules");
        assert_eq!(plan.targets[0].bytes, 5);
    }

    #[test]
    fn apply_deletes_only_targets_leaves_dest() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules");
        fs::create_dir_all(nm.join("pkg")).unwrap();
        fs::write(nm.join("pkg").join("a"), "x").unwrap();
        fs::write(dir.path().join("readme.txt"), "keep").unwrap();

        let plan = plan_cleanup(dir.path()).unwrap();
        apply_cleanup(dir.path(), &plan).unwrap();
        assert!(!nm.exists());
        assert!(dir.path().join("readme.txt").is_file());
        assert!(dir.path().exists());
    }

    #[test]
    fn never_touches_outside_dest() {
        let dir = tempdir().unwrap();
        let plan = CleanupPlan {
            targets: vec![CleanupTarget {
                relative_path: "../escape".into(),
                bytes: 0,
            }],
            total_bytes: 0,
        };
        let err = apply_cleanup(dir.path(), &plan).unwrap_err();
        assert!(matches!(err, CleanupError::OutsideDest));
    }
}
