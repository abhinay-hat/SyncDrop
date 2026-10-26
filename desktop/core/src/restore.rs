//! Detect project lockfiles during sync and write `<dest>/.syncdrop/RESTORE.md`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::sync_index::SYNCDROP_META_DIR;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreProject {
    pub relative_path: String,
    pub lockfile: String,
    pub command: String,
}

/// Lockfile name -> reinstall command. Lockfiles themselves are always copied.
pub fn command_for_lockfile(name: &str) -> Option<&'static str> {
    match name {
        "package-lock.json" => Some("npm ci"),
        "pnpm-lock.yaml" => Some("pnpm i --frozen-lockfile"),
        "yarn.lock" => Some("yarn install --frozen-lockfile"),
        "bun.lockb" | "bun.lock" => Some("bun install --frozen-lockfile"),
        "uv.lock" => Some("uv sync"),
        "poetry.lock" => Some("poetry install --no-root"),
        "Pipfile.lock" => Some("pipenv sync"),
        "Cargo.lock" => Some("cargo fetch"),
        "Gemfile.lock" => Some("bundle install"),
        "go.sum" => Some("go mod download"),
        "pubspec.lock" => Some("dart pub get"),
        "Podfile.lock" => Some("pod install"),
        other if other.starts_with("requirements") && other.ends_with(".txt") => {
            Some("pip install -r")
        }
        _ => None,
    }
}

/// Scan relative file paths (source tree) for lockfiles and build restore entries.
/// Prefers one command per project directory (first lockfile wins by sort order).
pub fn detect_projects(relative_files: &[String]) -> Vec<RestoreProject> {
    let mut found: Vec<(String, String, String)> = Vec::new();
    for rel in relative_files {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let Some(cmd_base) = command_for_lockfile(name) else {
            continue;
        };
        let dir = match rel.rfind('/') {
            Some(i) => rel[..i].to_string(),
            None => ".".to_string(),
        };
        let command = if cmd_base == "pip install -r" {
            format!("pip install -r {name}")
        } else {
            cmd_base.to_string()
        };
        found.push((dir, name.to_string(), command));
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    let mut out = Vec::new();
    let mut seen_dirs = std::collections::HashSet::new();
    for (dir, lockfile, command) in found {
        if !seen_dirs.insert(dir.clone()) {
            continue;
        }
        out.push(RestoreProject {
            relative_path: dir,
            lockfile,
            command,
        });
    }
    out
}

pub fn restore_md_path(dest: &Path) -> PathBuf {
    dest.join(SYNCDROP_META_DIR).join("RESTORE.md")
}

pub fn render_restore_md(projects: &[RestoreProject]) -> String {
    let mut out = String::from(
        "# SyncDrop dependency restore\n\n\
         Dependency and cache folders were skipped during backup. \
         Reinstall them from these lockfiles:\n\n",
    );
    if projects.is_empty() {
        out.push_str("_No lockfiles detected._\n");
        return out;
    }
    for project in projects {
        let cd = if project.relative_path == "." {
            "`<backup root>`".to_string()
        } else {
            format!("`{}`", project.relative_path)
        };
        out.push_str(&format!(
            "- {cd} ({}) → `{}`\n",
            project.lockfile, project.command
        ));
    }
    out
}

pub fn write_restore_md(dest: &Path, relative_files: &[String]) -> io::Result<Vec<RestoreProject>> {
    let projects = detect_projects(relative_files);
    let path = restore_md_path(dest);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, render_restore_md(&projects))?;
    Ok(projects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_npm_and_cargo() {
        let files = vec![
            "app/package-lock.json".into(),
            "app/src/index.js".into(),
            "rust/Cargo.lock".into(),
            "rust/src/lib.rs".into(),
        ];
        let projects = detect_projects(&files);
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].relative_path, "app");
        assert_eq!(projects[0].command, "npm ci");
        assert_eq!(projects[1].relative_path, "rust");
        assert_eq!(projects[1].command, "cargo fetch");
    }

    #[test]
    fn requirements_txt_command_includes_filename() {
        let files = vec!["svc/requirements-dev.txt".into()];
        let projects = detect_projects(&files);
        assert_eq!(projects[0].command, "pip install -r requirements-dev.txt");
    }

    #[test]
    fn write_restore_round_trip() {
        let dir = tempdir().unwrap();
        let files = vec!["web/pnpm-lock.yaml".into()];
        write_restore_md(dir.path(), &files).unwrap();
        let text = fs::read_to_string(restore_md_path(dir.path())).unwrap();
        assert!(text.contains("pnpm i --frozen-lockfile"));
        assert!(text.contains("`web`"));
    }
}
