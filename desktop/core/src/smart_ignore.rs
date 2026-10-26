//! Smart ignore: gitignore/.ignore semantics plus built-in dependency and cache skips.

use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;

/// Profile knobs that control smart ignore. Kept separate from [`crate::profile::SyncProfile`]
/// so the matcher can be unit-tested without a full profile.
#[derive(Debug, Clone, Copy)]
pub struct IgnoreOptions {
    pub respect_gitignore: bool,
    pub skip_dependency_caches: bool,
    /// When true, `.git` directory contents are kept (copied). Default for profiles.
    pub keep_git: bool,
}

impl Default for IgnoreOptions {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
            skip_dependency_caches: true,
            keep_git: true,
        }
    }
}

/// Matcher for one source root. Rebuilds nested gitignore stacks as the walk descends.
pub struct SmartIgnore {
    root: PathBuf,
    options: IgnoreOptions,
    /// Stack of (directory absolute path, gitignore for that directory). Root is first.
    stack: Vec<(PathBuf, Gitignore)>,
}

impl SmartIgnore {
    pub fn new(root: &Path, options: IgnoreOptions) -> Self {
        let root = root.to_path_buf();
        let mut stack = Vec::new();
        if options.respect_gitignore {
            if let Some(gi) = load_dir_ignores(&root) {
                stack.push((root.clone(), gi));
            }
        }
        Self {
            root,
            options,
            stack,
        }
    }

    /// Rebuild the ignore stack for ancestors of `abs` (and `abs` itself when it is a dir).
    pub fn prepare_for(&mut self, abs: &Path, is_dir: bool) {
        self.stack.clear();
        if !self.options.respect_gitignore {
            return;
        }
        let mut chain = Vec::new();
        let mut cur = if is_dir {
            abs.to_path_buf()
        } else {
            abs.parent()
                .unwrap_or(abs)
                .to_path_buf()
        };
        loop {
            chain.push(cur.clone());
            if cur == self.root {
                break;
            }
            match cur.parent() {
                Some(p) if !p.as_os_str().is_empty() => cur = p.to_path_buf(),
                _ => break,
            }
        }
        chain.reverse();
        for dir in chain {
            if let Some(gi) = load_dir_ignores(&dir) {
                self.stack.push((dir, gi));
            }
        }
    }

    /// True when `rel` (source-relative, `/`-separated) should be skipped.
    /// `abs` is the absolute path; `is_dir` matches gitignore directory semantics.
    pub fn is_ignored(&mut self, abs: &Path, rel: &str, is_dir: bool) -> bool {
        if rel.is_empty() {
            return false;
        }
        if is_git_path(rel) {
            return !self.options.keep_git;
        }
        if self.options.skip_dependency_caches && is_dependency_or_cache(abs, rel, is_dir) {
            return true;
        }
        if !self.options.respect_gitignore {
            return false;
        }
        self.prepare_for(abs, is_dir);
        for (_dir, gi) in self.stack.iter().rev() {
            match gi.matched(abs, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        false
    }
}

fn load_dir_ignores(dir: &Path) -> Option<Gitignore> {
    let mut builder = GitignoreBuilder::new(dir);
    let mut any = false;
    for name in [".gitignore", ".ignore"] {
        let path = dir.join(name);
        if path.is_file() {
            let _ = builder.add(&path);
            any = true;
        }
    }
    // Root of a git work tree: honour .git/info/exclude.
    let exclude = dir.join(".git").join("info").join("exclude");
    if exclude.is_file() {
        let _ = builder.add(&exclude);
        any = true;
    }
    if !any {
        return None;
    }
    builder.build().ok()
}

fn is_git_path(rel: &str) -> bool {
    rel.split('/')
        .filter(|p| !p.is_empty())
        .any(|p| p == ".git")
}

/// Built-in dependency / cache / junk rules from the smart-sync spec.
pub fn is_dependency_or_cache(abs: &Path, rel: &str, is_dir: bool) -> bool {
    let name = rel
        .rsplit('/')
        .find(|p| !p.is_empty())
        .unwrap_or(rel);

    if name == ".DS_Store"
        || name == ".Spotlight-V100"
        || name == ".fseventsd"
        || name == ".Trashes"
    {
        return true;
    }
    if name.starts_with("~$") {
        return true;
    }
    if name.ends_with(".pyc") {
        return true;
    }

    if !is_dir {
        return false;
    }

    let parent = abs.parent().unwrap_or(abs);

    match name {
        "node_modules"
        | "bower_components"
        | ".venv"
        | "venv"
        | "__pycache__"
        | ".pytest_cache"
        | ".mypy_cache"
        | ".ruff_cache"
        | ".tox"
        | ".gradle"
        | "Pods"
        | "DerivedData"
        | ".dart_tool"
        | ".next"
        | ".nuxt"
        | ".svelte-kit"
        | ".turbo"
        | ".parcel-cache"
        | ".cache"
        | ".expo"
        | "coverage"
        | ".terraform" => true,
        "env" => parent.join("env").join("pyvenv.cfg").is_file() || abs.join("pyvenv.cfg").is_file(),
        "target" => parent.join("Cargo.toml").is_file(),
        "build" => {
            parent.join("build.gradle").is_file()
                || parent.join("build.gradle.kts").is_file()
                || parent.join("settings.gradle").is_file()
                || parent.join("settings.gradle.kts").is_file()
        }
        "dist" => parent.join("package.json").is_file(),
        _ => false,
    }
}

/// Walk-helper: whether a relative path looks like a dependency/cache folder root
/// under `root` (used by cleanup on the destination).
pub fn dependency_folder_at(root: &Path, rel: &str) -> bool {
    let abs = root.join(rel);
    if !abs.is_dir() {
        return false;
    }
    is_dependency_or_cache(&abs, rel, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn target_only_next_to_cargo_toml() {
        let dir = tempdir().unwrap();
        let rust = dir.path().join("rust");
        fs::create_dir_all(rust.join("target")).unwrap();
        fs::write(rust.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        let other = dir.path().join("other");
        fs::create_dir_all(other.join("target")).unwrap();

        assert!(is_dependency_or_cache(
            &rust.join("target"),
            "rust/target",
            true
        ));
        assert!(!is_dependency_or_cache(
            &other.join("target"),
            "other/target",
            true
        ));
    }

    #[test]
    fn dist_only_next_to_package_json() {
        let dir = tempdir().unwrap();
        let app = dir.path().join("app");
        fs::create_dir_all(app.join("dist")).unwrap();
        fs::write(app.join("package.json"), "{}").unwrap();
        let loose = dir.path().join("loose");
        fs::create_dir_all(loose.join("dist")).unwrap();

        assert!(is_dependency_or_cache(&app.join("dist"), "app/dist", true));
        assert!(!is_dependency_or_cache(
            &loose.join("dist"),
            "loose/dist",
            true
        ));
    }

    #[test]
    fn env_only_with_pyvenv_cfg() {
        let dir = tempdir().unwrap();
        let venv = dir.path().join("env");
        fs::create_dir_all(&venv).unwrap();
        assert!(!is_dependency_or_cache(&venv, "env", true));
        fs::write(venv.join("pyvenv.cfg"), "home = /usr\n").unwrap();
        assert!(is_dependency_or_cache(&venv, "env", true));
    }

    #[test]
    fn build_only_next_to_gradle() {
        let dir = tempdir().unwrap();
        let android = dir.path().join("android");
        fs::create_dir_all(android.join("build")).unwrap();
        fs::write(android.join("build.gradle"), "").unwrap();
        let plain = dir.path().join("plain");
        fs::create_dir_all(plain.join("build")).unwrap();

        assert!(is_dependency_or_cache(
            &android.join("build"),
            "android/build",
            true
        ));
        assert!(!is_dependency_or_cache(
            &plain.join("build"),
            "plain/build",
            true
        ));
    }

    #[test]
    fn office_lock_and_pyc() {
        let dir = tempdir().unwrap();
        assert!(is_dependency_or_cache(
            &dir.path().join("~$Book.xlsx"),
            "~$Book.xlsx",
            false
        ));
        assert!(is_dependency_or_cache(
            &dir.path().join("x.pyc"),
            "pkg/x.pyc",
            false
        ));
    }

    #[test]
    fn keep_git_default_does_not_skip_git() {
        let dir = tempdir().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir_all(git.join("objects")).unwrap();
        let opts = IgnoreOptions {
            keep_git: true,
            ..IgnoreOptions::default()
        };
        let mut ig = SmartIgnore::new(dir.path(), opts);
        assert!(!ig.is_ignored(&git, ".git", true));
        assert!(!ig.is_ignored(&git.join("objects"), ".git/objects", true));
    }

    #[test]
    fn drop_git_when_keep_git_false() {
        let dir = tempdir().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir_all(&git).unwrap();
        let opts = IgnoreOptions {
            keep_git: false,
            ..IgnoreOptions::default()
        };
        let mut ig = SmartIgnore::new(dir.path(), opts);
        assert!(ig.is_ignored(&git, ".git", true));
    }

    #[test]
    fn respects_nested_gitignore() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "secret.txt\n").unwrap();
        let nested = dir.path().join("sub");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join(".gitignore"), "local.bin\n").unwrap();
        fs::write(dir.path().join("secret.txt"), "x").unwrap();
        fs::write(nested.join("local.bin"), "x").unwrap();
        fs::write(nested.join("ok.txt"), "x").unwrap();

        let mut ig = SmartIgnore::new(dir.path(), IgnoreOptions::default());
        assert!(ig.is_ignored(&dir.path().join("secret.txt"), "secret.txt", false));
        assert!(ig.is_ignored(&nested.join("local.bin"), "sub/local.bin", false));
        assert!(!ig.is_ignored(&nested.join("ok.txt"), "sub/ok.txt", false));
    }

    #[test]
    fn node_modules_always_skipped_when_deps_on() {
        let dir = tempdir().unwrap();
        let nm = dir.path().join("node_modules");
        fs::create_dir_all(&nm).unwrap();
        let mut ig = SmartIgnore::new(dir.path(), IgnoreOptions::default());
        assert!(ig.is_ignored(&nm, "node_modules", true));
    }
}
