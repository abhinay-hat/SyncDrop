//! Native copy engine. Plans a diff, then copies. Dry-run is that plan with no writes.
//!
//! Replaces rsync: no permissions or owner (exFAT), `--modify-window=1`, contents of the
//! source directory (trailing-slash semantics), excludes, optional mirror deletes, and
//! optional versioning. History stays with the app layer — this module does not touch
//! [`crate::config::ConfigStore`].

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use filetime::FileTime;
use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

use crate::profile::SyncProfile;
use crate::progress::SyncRecord;

/// rsync `--modify-window=1`. mtimes within this window count as equal.
pub const MODIFY_WINDOW: Duration = Duration::from_secs(1);

/// FAT/exFAT timestamp quantum. The copy decision uses [`MODIFY_WINDOW`] (1s), matching
/// rsync. This is the value to switch to if a later phase detects a FAT destination.
pub const EXFAT_MODIFY_WINDOW: Duration = Duration::from_secs(2);

/// Old files land in `<dest>/.syncdrop-versions/<YYYY-MM-DD_HHMMSS>/`. Never walked or deleted.
pub const VERSIONS_DIR: &str = ".syncdrop-versions";

/// Swift-era backup folder. Mirror mode must not eat backups made by the old app.
const LEGACY_VERSIONS_DIR: &str = ".syncdrop_archive";

const TMP_SUFFIX: &str = ".syncdrop-tmp";

/// What a real sync would change. Paths are relative, `/`-separated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// In source, missing at dest.
    pub copies: Vec<String>,
    /// In both, but size differs or mtime differs by more than [`MODIFY_WINDOW`].
    pub updates: Vec<String>,
    /// In dest only. Populated when `mirror_mode` is set. Excludes are never listed.
    pub deletes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("source path is empty")]
    EmptySource,
    #[error("destination path is empty")]
    EmptyDest,
    #[error("source does not exist: {0}")]
    MissingSource(String),
    #[error("source is not a directory: {0}")]
    SourceNotDirectory(String),
    #[error("source and destination overlap")]
    Overlapping,
    #[error("google drive cloud profiles are handled by the drive engine")]
    GoogleDriveCloud,
    #[error("sync cancelled")]
    Cancelled,
    #[error("exclude pattern `{pattern}`: {message}")]
    BadExclude { pattern: String, message: String },
    #[error(transparent)]
    Io(#[from] io::Error),
}

struct Built {
    plan: Plan,
    /// Dest leftovers from an unplugged copy. Removed on a real sync, omitted from [`Plan`].
    stale_tmps: Vec<String>,
}

/// Diff `profile`'s source against `dest`. Creates nothing.
pub fn dry_run(profile: &SyncProfile, dest: &Path) -> Result<Plan, EngineError> {
    Ok(build(profile, dest, None)?.plan)
}

/// Copy `profile`'s source into `dest` (already resolved by the caller).
///
/// `progress` is `(files_done, files_total, bytes_done, bytes_total, current_path)`.
/// `current_path` is relative. `cancel` can be behind an `Arc`; check it from the callback
/// to stop between files. A finished run returns a [`SyncRecord`] with `succeeded: true`.
/// The caller appends history.
pub fn sync<F>(
    profile: &SyncProfile,
    dest: &Path,
    mut progress: F,
    cancel: &AtomicBool,
) -> Result<SyncRecord, EngineError>
where
    F: FnMut(i64, i64, i64, i64, &str),
{
    let started = Instant::now();
    let built = build(profile, dest, Some(cancel))?;
    let (files_done, bytes_done) = execute(profile, dest, &built, &mut progress, cancel)?;
    Ok(SyncRecord::new(
        chrono::Utc::now(),
        files_done,
        bytes_done,
        started.elapsed().as_secs_f64(),
        true,
    ))
}

fn build(
    profile: &SyncProfile,
    dest: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<Built, EngineError> {
    if profile.is_google_drive_cloud() {
        return Err(EngineError::GoogleDriveCloud);
    }
    let source = PathBuf::from(profile.expanded_source_path());
    if source.as_os_str().is_empty() {
        return Err(EngineError::EmptySource);
    }
    if dest.as_os_str().is_empty() {
        return Err(EngineError::EmptyDest);
    }
    if !source.exists() {
        return Err(EngineError::MissingSource(source.display().to_string()));
    }
    if !source.is_dir() {
        return Err(EngineError::SourceNotDirectory(source.display().to_string()));
    }
    if paths_overlap(&source, dest) {
        return Err(EngineError::Overlapping);
    }
    check_cancel(cancel)?;

    let excludes = exclude_set(&profile.excludes)?;
    let mut files: Vec<String> = Vec::new();
    let mut source_paths: HashSet<String> = HashSet::new();

    let mut walker = WalkDir::new(&source)
        .min_depth(1)
        .follow_links(false)
        .into_iter();
    while let Some(item) = walker.next() {
        check_cancel(cancel)?;
        let entry = item.map_err(walk_err)?;
        let rel = rel_of(&source, entry.path())?;
        if is_excluded(&excludes, &rel) {
            if entry.file_type().is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        source_paths.insert(rel.clone());
        if entry.file_type().is_dir() {
            continue;
        }
        // ponytail: symlinks and specials are skipped. Upgrade path is copy-the-link if a profile needs them.
        if entry.file_type().is_file() {
            files.push(rel);
        }
    }
    files.sort();

    let mut copies = Vec::new();
    let mut updates = Vec::new();
    for rel in &files {
        check_cancel(cancel)?;
        let src_meta = fs::metadata(source.join(rel))?;
        let dest_file = dest.join(rel);
        if dest_file.is_file() {
            let dest_meta = fs::metadata(&dest_file)?;
            if content_differs(&src_meta, &dest_meta) {
                updates.push(rel.clone());
            }
        } else {
            copies.push(rel.clone());
        }
    }

    let mut deletes = Vec::new();
    let mut stale_tmps = Vec::new();
    if dest.is_dir() {
        let mut protected: HashSet<String> = HashSet::new();
        let mut extras: Vec<String> = Vec::new();
        let mut dwalker = WalkDir::new(dest).min_depth(1).follow_links(false).into_iter();
        while let Some(item) = dwalker.next() {
            check_cancel(cancel)?;
            let entry = item.map_err(walk_err)?;
            let rel = rel_of(dest, entry.path())?;
            if rel.ends_with(TMP_SUFFIX) && entry.file_type().is_file() && !source_paths.contains(&rel)
            {
                stale_tmps.push(rel);
                continue;
            }
            if is_excluded(&excludes, &rel) {
                protect(&mut protected, &rel);
                if entry.file_type().is_dir() {
                    dwalker.skip_current_dir();
                }
                continue;
            }
            if !source_paths.contains(&rel) {
                extras.push(rel);
            }
        }
        if profile.mirror_mode {
            deletes.extend(extras.into_iter().filter(|rel| !protected.contains(rel)));
            deletes.sort_by(|a, b| depth(b).cmp(&depth(a)).then(b.cmp(a)));
        }
    }

    Ok(Built {
        plan: Plan {
            copies,
            updates,
            deletes,
        },
        stale_tmps,
    })
}

fn execute<F>(
    profile: &SyncProfile,
    dest: &Path,
    built: &Built,
    progress: &mut F,
    cancel: &AtomicBool,
) -> Result<(i64, i64), EngineError>
where
    F: FnMut(i64, i64, i64, i64, &str),
{
    check_cancel(Some(cancel))?;
    fs::create_dir_all(dest)?;
    for rel in &built.stale_tmps {
        match fs::remove_file(dest.join(rel)) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }

    let source = PathBuf::from(profile.expanded_source_path());
    let plan = &built.plan;
    let work: Vec<&str> = plan
        .copies
        .iter()
        .chain(plan.updates.iter())
        .map(String::as_str)
        .collect();
    let files_total = (work.len() + plan.deletes.len()) as i64;
    let mut bytes_total: i64 = 0;
    for rel in &work {
        bytes_total += fs::metadata(source.join(rel))?.len() as i64;
    }

    let stamp = version_stamp();
    let mut files_done: i64 = 0;
    let mut bytes_done: i64 = 0;
    progress(0, files_total, 0, bytes_total, "");

    for rel in &work {
        check_cancel(Some(cancel))?;
        progress(files_done, files_total, bytes_done, bytes_total, rel);
        let n = copy_file(
            &source.join(rel),
            &dest.join(rel),
            dest,
            rel,
            profile.keep_versions,
            &stamp,
        )?;
        bytes_done += n as i64;
        files_done += 1;
        progress(files_done, files_total, bytes_done, bytes_total, rel);
    }
    for rel in &plan.deletes {
        check_cancel(Some(cancel))?;
        progress(files_done, files_total, bytes_done, bytes_total, rel);
        delete_path(dest, rel, profile.keep_versions, &stamp)?;
        files_done += 1;
        progress(files_done, files_total, bytes_done, bytes_total, rel);
    }
    Ok((files_done, bytes_done))
}

fn copy_file(
    src: &Path,
    dest_file: &Path,
    dest_root: &Path,
    rel: &str,
    keep_versions: bool,
    stamp: &str,
) -> io::Result<u64> {
    if let Some(parent) = dest_file.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = tmp_sibling(dest_file)?;
    let bytes = write_tmp(src, &tmp)?;
    let committed = commit_copy(&tmp, dest_file, dest_root, rel, keep_versions, stamp);
    if committed.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    committed?;
    if let Ok(modified) = fs::metadata(src)?.modified() {
        filetime::set_file_mtime(dest_file, FileTime::from_system_time(modified))?;
    }
    Ok(bytes)
}

fn commit_copy(
    tmp: &Path,
    dest_file: &Path,
    dest_root: &Path,
    rel: &str,
    keep_versions: bool,
    stamp: &str,
) -> io::Result<()> {
    if dest_file.exists() && keep_versions {
        archive_file(dest_root, rel, stamp)?;
    }
    // Unix rename replaces. Windows refuses when dest exists — drop it, then rename.
    // The new bytes are already durable in `tmp`, and source still has the original.
    if dest_file.exists() {
        fs::remove_file(dest_file)?;
    }
    fs::rename(tmp, dest_file)
}

fn write_tmp(src: &Path, tmp: &Path) -> io::Result<u64> {
    let mut input = File::open(src)?;
    let mut output = File::create(tmp)?;
    let n = io::copy(&mut input, &mut output)?;
    output.flush()?;
    output.sync_all()?;
    Ok(n)
}

fn archive_file(dest_root: &Path, rel: &str, stamp: &str) -> io::Result<()> {
    let from = dest_root.join(rel);
    let meta = match fs::symlink_metadata(&from) {
        Ok(m) if m.is_file() => m,
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let _ = meta;
    let to = dest_root.join(VERSIONS_DIR).join(stamp).join(rel);
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if to.exists() {
        fs::remove_file(&to)?;
    }
    fs::rename(from, &to)
}

fn delete_path(dest_root: &Path, rel: &str, keep_versions: bool, stamp: &str) -> io::Result<()> {
    let path = dest_root.join(rel);
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.is_dir() {
        return match fs::remove_dir(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
    }
    if keep_versions {
        archive_file(dest_root, rel, stamp)?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn exclude_set(patterns: &[String]) -> Result<GlobSet, EngineError> {
    let mut builder = GlobSetBuilder::new();
    let mut add = |pattern: &str| -> Result<(), EngineError> {
        let glob = Glob::new(pattern).map_err(|e| EngineError::BadExclude {
            pattern: pattern.to_string(),
            message: e.to_string(),
        })?;
        builder.add(glob);
        Ok(())
    };
    add(VERSIONS_DIR)?;
    add(LEGACY_VERSIONS_DIR)?;
    for raw in patterns {
        let pattern = raw.trim();
        if pattern.is_empty() {
            continue;
        }
        add(pattern)?;
    }
    builder.build().map_err(|e| EngineError::BadExclude {
        pattern: String::new(),
        message: e.to_string(),
    })
}

/// File name, each ancestor name, and each relative prefix. A directory pattern
/// (`node_modules`) therefore skips its contents.
fn is_excluded(set: &GlobSet, rel: &str) -> bool {
    if set.is_empty() {
        return false;
    }
    let mut prefix = String::new();
    for (i, part) in rel.split('/').filter(|p| !p.is_empty()).enumerate() {
        if set.is_match(part) {
            return true;
        }
        if i > 0 {
            prefix.push('/');
        }
        prefix.push_str(part);
        if set.is_match(&prefix) {
            return true;
        }
    }
    false
}

fn content_differs(src: &fs::Metadata, dest: &fs::Metadata) -> bool {
    if src.len() != dest.len() {
        return true;
    }
    match (src.modified(), dest.modified()) {
        (Ok(a), Ok(b)) => mtime_differs(a, b, MODIFY_WINDOW),
        _ => true,
    }
}

fn mtime_differs(a: SystemTime, b: SystemTime, window: Duration) -> bool {
    let diff = a.duration_since(b).unwrap_or_else(|err| err.duration());
    diff > window
}

fn protect(set: &mut HashSet<String>, rel: &str) {
    set.insert(rel.to_string());
    let mut rest = rel;
    while let Some((parent, _)) = rest.rsplit_once('/') {
        set.insert(parent.to_string());
        rest = parent;
    }
}

fn rel_of(root: &Path, path: &Path) -> Result<String, EngineError> {
    let rel = path.strip_prefix(root).map_err(|e| {
        io::Error::new(io::ErrorKind::Other, e.to_string())
    })?;
    Ok(rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn tmp_sibling(dest_file: &Path) -> io::Result<PathBuf> {
    let name = dest_file.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination file has no name")
    })?;
    let mut tmp = name.to_os_string();
    tmp.push(TMP_SUFFIX);
    Ok(dest_file.with_file_name(tmp))
}

fn version_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H%M%S").to_string()
}

fn depth(path: &str) -> usize {
    path.bytes().filter(|b| *b == b'/').count()
}

fn check_cancel(cancel: Option<&AtomicBool>) -> Result<(), EngineError> {
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        Err(EngineError::Cancelled)
    } else {
        Ok(())
    }
}

fn walk_err(err: walkdir::Error) -> EngineError {
    EngineError::Io(io::Error::new(io::ErrorKind::Other, err.to_string()))
}

fn paths_overlap(source: &Path, dest: &Path) -> bool {
    let source = source.canonicalize().unwrap_or_else(|_| source.to_path_buf());
    if !dest.exists() {
        return dest.starts_with(&source);
    }
    let dest = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
    source == dest || dest.starts_with(&source) || source.starts_with(&dest)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use filetime::{set_file_mtime, FileTime};
    use tempfile::tempdir;

    use super::*;

    fn profile(source: &Path, excludes: &[&str], mirror: bool, versions: bool) -> SyncProfile {
        SyncProfile {
            source_path: source.to_string_lossy().into_owned(),
            dest_path: String::new(),
            excludes: excludes.iter().map(|s| (*s).to_string()).collect(),
            mirror_mode: mirror,
            keep_versions: versions,
            ..SyncProfile::new("t")
        }
    }

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    fn touch(path: &Path, unix: i64) {
        set_file_mtime(path, FileTime::from_unix_time(unix, 0)).unwrap();
    }

    fn noop(_: i64, _: i64, _: i64, _: i64, _: &str) {}

    #[test]
    fn dry_run_counts_copies_updates_deletes_and_writes_nothing() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        write(&src.path().join("a.txt"), b"aaa");
        write(&src.path().join("b.txt"), b"bbb");
        write(&src.path().join("c.txt"), b"new");
        write(&dst.path().join("c.txt"), b"old");
        write(&dst.path().join("d.txt"), b"gone");
        touch(&src.path().join("c.txt"), 1_700_000_100);
        touch(&dst.path().join("c.txt"), 1_700_000_000);
        let before = fs::read(dst.path().join("c.txt")).unwrap();

        let p = profile(src.path(), &[], true, false);
        let plan = dry_run(&p, dst.path()).unwrap();
        assert_eq!(plan.copies, vec!["a.txt".to_string(), "b.txt".to_string()]);
        assert_eq!(plan.updates, vec!["c.txt".to_string()]);
        assert_eq!(plan.deletes, vec!["d.txt".to_string()]);
        assert_eq!(fs::read(dst.path().join("c.txt")).unwrap(), before);
        assert!(!dst.path().join("a.txt").exists());
    }

    #[test]
    fn dry_run_empty_source_is_all_zero() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        let p = profile(src.path(), &[], true, false);
        let plan = dry_run(&p, dst.path()).unwrap();
        assert!(plan.copies.is_empty());
        assert!(plan.updates.is_empty());
        assert!(plan.deletes.is_empty());
    }

    #[test]
    fn unchanged_file_is_omitted() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        write(&src.path().join("same.txt"), b"same-bytes");
        write(&dst.path().join("same.txt"), b"same-bytes");
        touch(&src.path().join("same.txt"), 1_700_000_000);
        touch(&dst.path().join("same.txt"), 1_700_000_000);
        let p = profile(src.path(), &[], false, false);
        let plan = dry_run(&p, dst.path()).unwrap();
        assert!(plan.copies.is_empty() && plan.updates.is_empty() && plan.deletes.is_empty());
    }

    #[test]
    fn sync_copies_source_contents_and_preserves_mtime() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        write(&src.path().join("nested/note.txt"), b"hello");
        touch(&src.path().join("nested/note.txt"), 1_700_000_050);
        let p = profile(src.path(), &[], false, false);
        let cancel = AtomicBool::new(false);
        let mut seen = Vec::new();
        let record = sync(
            &p,
            dst.path(),
            |done, total, bytes_done, bytes_total, path| {
                seen.push((done, total, bytes_done, bytes_total, path.to_string()));
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(fs::read(dst.path().join("nested/note.txt")).unwrap(), b"hello");
        assert!(!dst.path().join(src.path().file_name().unwrap()).exists());
        let mtime = fs::metadata(dst.path().join("nested/note.txt"))
            .unwrap()
            .modified()
            .unwrap();
        assert!(!mtime_differs(
            mtime,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_050),
            Duration::ZERO,
        ));
        assert!(record.succeeded);
        assert_eq!(record.file_count, 1);
        assert_eq!(record.total_bytes, 5);
        assert!(seen.iter().any(|s| s.1 == 1 && s.4 == "nested/note.txt"));
        assert!(dry_run(&p, dst.path()).unwrap().copies.is_empty());
        assert!(dry_run(&p, dst.path()).unwrap().updates.is_empty());
    }

    #[test]
    fn modify_window_skips_one_second_and_copies_two() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        write(&src.path().join("w.txt"), b"AAAA");
        write(&dst.path().join("w.txt"), b"BBBB");
        touch(&src.path().join("w.txt"), 1_700_000_001);
        touch(&dst.path().join("w.txt"), 1_700_000_000);
        let p = profile(src.path(), &[], false, false);
        let cancel = AtomicBool::new(false);
        sync(&p, dst.path(), noop, &cancel).unwrap();
        assert_eq!(fs::read(dst.path().join("w.txt")).unwrap(), b"BBBB");

        touch(&src.path().join("w.txt"), 1_700_000_002);
        sync(&p, dst.path(), noop, &cancel).unwrap();
        assert_eq!(fs::read(dst.path().join("w.txt")).unwrap(), b"AAAA");
        assert_eq!(MODIFY_WINDOW, Duration::from_secs(1));
        assert_eq!(EXFAT_MODIFY_WINDOW, Duration::from_secs(2));
    }

    #[test]
    fn size_difference_copies_even_when_mtime_matches() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        write(&src.path().join("s.txt"), b"longer");
        write(&dst.path().join("s.txt"), b"short");
        touch(&src.path().join("s.txt"), 1_700_000_000);
        touch(&dst.path().join("s.txt"), 1_700_000_000);
        let p = profile(src.path(), &[], false, false);
        let plan = dry_run(&p, dst.path()).unwrap();
        assert_eq!(plan.updates, vec!["s.txt".to_string()]);
        let cancel = AtomicBool::new(false);
        sync(&p, dst.path(), noop, &cancel).unwrap();
        assert_eq!(fs::read(dst.path().join("s.txt")).unwrap(), b"longer");
    }

    #[test]
    fn excludes_match_name_and_directory_and_blank_patterns_are_skipped() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        write(&src.path().join(".DS_Store"), b"junk");
        write(&src.path().join("node_modules/pkg/index.js"), b"js");
        write(&src.path().join("keep.txt"), b"yes");
        write(&src.path().join("skipme/no.txt"), b"no");
        let p = profile(
            src.path(),
            &["", "  ", ".DS_Store", "node_modules", "skipme"],
            false,
            false,
        );
        let cancel = AtomicBool::new(false);
        sync(&p, dst.path(), noop, &cancel).unwrap();
        assert_eq!(fs::read(dst.path().join("keep.txt")).unwrap(), b"yes");
        assert!(!dst.path().join(".DS_Store").exists());
        assert!(!dst.path().join("node_modules").exists());
        assert!(!dst.path().join("skipme").exists());
    }

    #[test]
    fn mirror_deletes_extras_but_not_excluded_or_versions() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        write(&src.path().join("keep.txt"), b"k");
        write(&dst.path().join("keep.txt"), b"k");
        write(&dst.path().join("extra.txt"), b"x");
        write(&dst.path().join("nested/.DS_Store"), b"ds");
        write(&dst.path().join("nested/gone.txt"), b"g");
        touch(&src.path().join("keep.txt"), 1_700_000_000);
        touch(&dst.path().join("keep.txt"), 1_700_000_000);
        write(
            &dst.path().join(".syncdrop-versions/2020-01-01_000000/old.txt"),
            b"old",
        );

        let off = profile(src.path(), &[".DS_Store"], false, false);
        let cancel = AtomicBool::new(false);
        sync(&off, dst.path(), noop, &cancel).unwrap();
        assert!(dst.path().join("extra.txt").exists());

        let on = profile(src.path(), &[".DS_Store"], true, false);
        sync(&on, dst.path(), noop, &cancel).unwrap();
        assert!(!dst.path().join("extra.txt").exists());
        assert!(!dst.path().join("nested/gone.txt").exists());
        assert_eq!(fs::read(dst.path().join("nested/.DS_Store")).unwrap(), b"ds");
        assert!(dst.path().join("nested").exists());
        assert_eq!(
            fs::read(dst.path().join(".syncdrop-versions/2020-01-01_000000/old.txt")).unwrap(),
            b"old"
        );
    }

    #[test]
    fn keep_versions_archives_overwrite_and_delete() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        write(&src.path().join("a.txt"), b"new");
        write(&dst.path().join("a.txt"), b"old");
        write(&dst.path().join("gone.txt"), b"bye");
        touch(&src.path().join("a.txt"), 1_700_000_010);
        touch(&dst.path().join("a.txt"), 1_700_000_000);
        let p = profile(src.path(), &[], true, true);
        let cancel = AtomicBool::new(false);
        sync(&p, dst.path(), noop, &cancel).unwrap();
        assert_eq!(fs::read(dst.path().join("a.txt")).unwrap(), b"new");
        assert!(!dst.path().join("gone.txt").exists());

        let versions = dst.path().join(VERSIONS_DIR);
        let stamp = fs::read_dir(&versions)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name();
        let stamp = stamp.to_string_lossy();
        assert!(
            stamp.len() == "YYYY-MM-DD_HHMMSS".len() && stamp.as_bytes()[4] == b'-',
            "{stamp}"
        );
        assert_eq!(stamp.as_bytes()[10], b'_');
        assert_eq!(fs::read(versions.join(&*stamp).join("a.txt")).unwrap(), b"old");
        assert_eq!(fs::read(versions.join(&*stamp).join("gone.txt")).unwrap(), b"bye");
    }

    #[test]
    fn keep_versions_off_leaves_no_archive() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        write(&src.path().join("a.txt"), b"new");
        write(&dst.path().join("a.txt"), b"old");
        touch(&src.path().join("a.txt"), 1_700_000_010);
        touch(&dst.path().join("a.txt"), 1_700_000_000);
        let p = profile(src.path(), &[], false, false);
        let cancel = AtomicBool::new(false);
        sync(&p, dst.path(), noop, &cancel).unwrap();
        assert!(!dst.path().join(VERSIONS_DIR).exists());
        assert_eq!(fs::read(dst.path().join("a.txt")).unwrap(), b"new");
    }

    #[test]
    fn cancel_stops_between_files_and_leaves_no_tmp() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        write(&src.path().join("a.txt"), b"a");
        write(&src.path().join("b.txt"), b"b");
        let p = profile(src.path(), &[], false, false);
        let cancel = AtomicBool::new(false);
        let err = sync(
            &p,
            dst.path(),
            |done, _, _, _, _| {
                if done >= 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        );
        assert!(matches!(err, Err(EngineError::Cancelled)));
        assert!(dst.path().join("a.txt").exists());
        assert!(!dst.path().join("b.txt").exists());
        let leftovers: Vec<_> = WalkDir::new(dst.path())
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .ends_with(".syncdrop-tmp")
            })
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn successful_sync_leaves_no_tmp() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        write(&src.path().join("a.txt"), b"a");
        write(&dst.path().join("a.txt.syncdrop-tmp"), b"stale");
        let p = profile(src.path(), &[], false, false);
        let cancel = AtomicBool::new(false);
        sync(&p, dst.path(), noop, &cancel).unwrap();
        assert_eq!(fs::read(dst.path().join("a.txt")).unwrap(), b"a");
        assert!(!dst.path().join("a.txt.syncdrop-tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn does_not_copy_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        let file = src.path().join("x.sh");
        write(&file, b"#!/bin/sh\n");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        let p = profile(src.path(), &[], false, false);
        let cancel = AtomicBool::new(false);
        sync(&p, dst.path(), noop, &cancel).unwrap();
        let mode = fs::metadata(dst.path().join("x.sh")).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0, "mode {mode:o} copied the executable bit");
    }

    #[test]
    fn rejects_empty_missing_and_cloud_profiles() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut empty = profile(src.path(), &[], false, false);
        empty.source_path.clear();
        assert!(matches!(
            sync(&empty, dst.path(), noop, &cancel),
            Err(EngineError::EmptySource)
        ));
        assert!(matches!(
            dry_run(&profile(src.path(), &[], false, false), Path::new("")),
            Err(EngineError::EmptyDest)
        ));
        let missing = profile(Path::new("/no/such/syncdrop-source"), &[], false, false);
        assert!(matches!(
            dry_run(&missing, dst.path()),
            Err(EngineError::MissingSource(_))
        ));
        let mut cloud = profile(src.path(), &[], false, false);
        cloud.dest_path = "gdrive://Mine".to_string();
        assert!(matches!(
            dry_run(&cloud, dst.path()),
            Err(EngineError::GoogleDriveCloud)
        ));
        assert!(matches!(
            dry_run(&profile(src.path(), &["["], false, false), dst.path()),
            Err(EngineError::BadExclude { .. })
        ));
    }

    #[test]
    fn refuses_overlapping_paths() {
        let src = tempdir().unwrap();
        let p = profile(src.path(), &[], true, false);
        assert!(matches!(
            dry_run(&p, src.path()),
            Err(EngineError::Overlapping)
        ));
    }
}
