use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Patterns skipped by a sync. Windows adds the usual FAT junk names.
pub fn default_excludes() -> Vec<String> {
    let base = [
        ".DS_Store",
        ".Spotlight-V100",
        ".fseventsd",
        ".Trashes",
        "node_modules",
    ];
    #[cfg(windows)]
    let extra = ["Thumbs.db", "desktop.ini", "$RECYCLE.BIN"];
    #[cfg(not(windows))]
    let extra: [&str; 0] = [];
    base.into_iter()
        .chain(extra)
        .map(str::to_string)
        .collect()
}

/// One sync pairing. The SMB password is not stored here — it belongs in the
/// OS keyring, keyed by profile id (see the NAS module).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProfile {
    pub id: Uuid,
    pub name: String,
    pub source_path: String,
    pub dest_path: String,
    pub ssd_name: String,
    pub mirror_mode: bool,
    pub auto_sync: bool,
    pub auto_eject: bool,
    pub keep_versions: bool,
    pub excludes: Vec<String>,
    /// Hostname or IP. `None` or empty means this profile has no NAS share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nas_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nas_share: Option<String>,
    /// Empty/`None` attempts a guest mount.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nas_username: Option<String>,
    /// When true, SyncDrop mounts `nas_host`/`nas_share` itself.
    /// `autoMountNAS` is the Swift CodingKey; camelCase rename emits `autoMountNas`.
    #[serde(default, alias = "autoMountNAS")]
    pub auto_mount_nas: bool,
}

impl SyncProfile {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            source_path: "~/Desktop/Projects".to_string(),
            dest_path: String::new(),
            ssd_name: "Extreme Pro".to_string(),
            mirror_mode: false,
            auto_sync: false,
            auto_eject: false,
            keep_versions: false,
            excludes: default_excludes(),
            nas_host: None,
            nas_share: None,
            nas_username: None,
            auto_mount_nas: false,
        }
    }

    pub fn expanded_source_path(&self) -> String {
        expand_tilde(&self.source_path)
    }

    pub fn expanded_dest_path(&self) -> String {
        expand_tilde(&self.dest_path)
    }

    /// Destination or volume name looks like a Google Drive folder.
    pub fn is_google_drive(&self) -> bool {
        contains_ci(&self.dest_path, "GoogleDrive")
            || contains_ci(&self.dest_path, "Google Drive")
            || contains_ci(&self.ssd_name, "Google Drive")
            || contains_ci(&self.ssd_name, "GoogleDrive")
    }

    /// Destination sits in macOS CloudStorage, or is a Google Drive folder.
    pub fn is_cloud_storage(&self) -> bool {
        self.is_google_drive() || self.dest_path.contains("CloudStorage")
    }

    /// Direct cloud sync via the Drive API (`gdrive://` or the cloud volume name).
    pub fn is_google_drive_cloud(&self) -> bool {
        self.dest_path.starts_with("gdrive://")
            || contains_ci(&self.dest_path, "Google Drive (Cloud)")
            || self.ssd_name == "Google Drive (Cloud)"
    }

    /// Checks the profile before it is saved or synced. Returns every problem,
    /// so the settings window can mark each field.
    pub fn validate(&self) -> Result<(), Vec<ProfileError>> {
        let mut issues = Vec::new();
        let source_raw = self.source_path.trim();
        let dest_raw = self.dest_path.trim();
        let source = self.expanded_source_path();
        let source_path = Path::new(&source);

        if source_raw.is_empty() {
            issues.push(ProfileError::EmptySource);
        } else if !source_path.exists() {
            issues.push(ProfileError::MissingSource);
        } else if !source_path.is_dir() {
            issues.push(ProfileError::SourceNotDirectory);
        }

        if dest_raw.is_empty() {
            issues.push(ProfileError::EmptyDest);
        } else if !self.is_google_drive_cloud() && !source_raw.is_empty() {
            match path_relation(&source, &self.expanded_dest_path()) {
                PathRelation::Same => issues.push(ProfileError::SamePath),
                PathRelation::DestInsideSource => issues.push(ProfileError::DestInsideSource),
                PathRelation::Distinct => {}
            }
        }

        if !self.is_cloud_storage()
            && !self.is_google_drive_cloud()
            && self.ssd_name.trim().is_empty()
        {
            issues.push(ProfileError::EmptyDrive);
        }

        let host = self
            .nas_host
            .as_deref()
            .map(str::trim)
            .filter(|host| !host.is_empty());
        let share = self
            .nas_share
            .as_deref()
            .map(str::trim)
            .filter(|share| !share.is_empty());
        match host {
            Some(host) if !valid_nas_host(host) => issues.push(ProfileError::BadNasHost),
            None if self.auto_mount_nas => issues.push(ProfileError::NasHostRequired),
            _ => {}
        }
        if self.auto_mount_nas && share.is_none() {
            issues.push(ProfileError::NasShareRequired);
        }

        for pattern in &self.excludes {
            let pattern = pattern.trim();
            if pattern.is_empty() {
                continue;
            }
            if globset::Glob::new(pattern).is_err() {
                issues.push(ProfileError::BadExclude(pattern.to_string()));
            }
        }

        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues)
        }
    }
}

/// A profile that cannot be saved or synced. [`ProfileError::field`] names the
/// settings control that should show [`std::fmt::Display`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    #[error("Choose a source folder.")]
    EmptySource,
    #[error("Source folder does not exist.")]
    MissingSource,
    #[error("Source is not a folder.")]
    SourceNotDirectory,
    #[error("Choose a destination.")]
    EmptyDest,
    #[error("Source and destination are the same folder.")]
    SamePath,
    #[error("Destination is inside the source folder.")]
    DestInsideSource,
    #[error("Enter the drive name.")]
    EmptyDrive,
    #[error("NAS host must be a hostname or IP address.")]
    BadNasHost,
    #[error("Enter a NAS host to auto-mount.")]
    NasHostRequired,
    #[error("Enter the share name to auto-mount.")]
    NasShareRequired,
    #[error("Exclude pattern is not a valid glob: {0}")]
    BadExclude(String),
}

impl ProfileError {
    pub fn field(&self) -> &'static str {
        match self {
            Self::EmptySource | Self::MissingSource | Self::SourceNotDirectory => "source",
            Self::EmptyDest | Self::SamePath | Self::DestInsideSource => "dest",
            Self::EmptyDrive => "ssd",
            Self::BadNasHost | Self::NasHostRequired => "nasHost",
            Self::NasShareRequired => "nasShare",
            Self::BadExclude(_) => "excludes",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathRelation {
    Same,
    DestInsideSource,
    Distinct,
}

fn path_relation(source: &str, dest: &str) -> PathRelation {
    let pairs = [
        (normalize_path(source, true), normalize_path(dest, true)),
        (normalize_path(source, false), normalize_path(dest, false)),
    ];
    if pairs.iter().any(|(src, dst)| src == dst) {
        return PathRelation::Same;
    }
    if pairs.iter().any(|(src, dst)| path_inside(src, dst)) {
        return PathRelation::DestInsideSource;
    }
    PathRelation::Distinct
}

fn normalize_path(path: &str, canonicalize: bool) -> PathBuf {
    let path = PathBuf::from(path.trim());
    if canonicalize {
        if let Ok(canon) = path.canonicalize() {
            return canon;
        }
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn path_inside(parent: &Path, child: &Path) -> bool {
    let parent: Vec<_> = parent.components().collect();
    let child: Vec<_> = child.components().collect();
    child.len() > parent.len() && child.starts_with(&parent)
}

/// Hostname or IP. A single trailing dot (FQDN) is accepted.
fn valid_nas_host(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() || host.len() > 253 || host.contains(':') && host.parse::<Ipv6Addr>().is_err()
    {
        return false;
    }
    if host.parse::<Ipv4Addr>().is_ok() || host.parse::<Ipv6Addr>().is_ok() {
        return true;
    }
    let labels: Vec<&str> = host.split('.').collect();
    !labels.is_empty()
        && labels.iter().all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

/// ponytail: expands only "~" and "~/...", not "~user". Upgrade path is the `home` crate if ~user paths show up.
fn expand_tilde(path: &str) -> String {
    let Some(home) = dirs::home_dir() else {
        return path.to_string();
    };
    if path == "~" {
        return home.to_string_lossy().into_owned();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home.join(rest).to_string_lossy().into_owned();
    }
    path.to_string()
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_leave_nas_fields_unset() {
        let p = SyncProfile::new("Default");
        assert!(p.nas_host.is_none());
        assert!(p.nas_share.is_none());
        assert!(p.nas_username.is_none());
        assert!(!p.auto_mount_nas);
    }

    #[test]
    fn encode_decode_round_trips_nas_fields() {
        let original = SyncProfile {
            nas_host: Some("nas.local".to_string()),
            nas_share: Some("Backups".to_string()),
            nas_username: Some("abhinay".to_string()),
            auto_mount_nas: true,
            ..SyncProfile::new("NAS")
        };
        let data = serde_json::to_vec(&original).unwrap();
        let decoded: SyncProfile = serde_json::from_slice(&data).unwrap();
        assert_eq!(decoded, original);
    }

    /// Profiles saved before the NAS fields existed must still decode.
    #[test]
    fn decode_legacy_json_without_nas_fields() {
        let id = Uuid::new_v4();
        let legacy = format!(
            r#"{{
                "id": "{id}",
                "name": "Legacy",
                "sourcePath": "~/Desktop/Projects",
                "destPath": "/Volumes/SSD/Backup",
                "ssdName": "Extreme Pro",
                "mirrorMode": false,
                "autoSync": true,
                "autoEject": false,
                "keepVersions": false,
                "excludes": [".DS_Store"]
            }}"#
        );
        let decoded: SyncProfile = serde_json::from_str(&legacy).unwrap();
        assert_eq!(decoded.name, "Legacy");
        assert!(decoded.auto_sync);
        assert!(decoded.nas_host.is_none());
        assert!(decoded.nas_share.is_none());
        assert!(decoded.nas_username.is_none());
        assert!(!decoded.auto_mount_nas);
    }

    #[test]
    fn expands_tilde_and_leaves_absolute_paths() {
        let mut p = SyncProfile::new("t");
        let expected = dirs::home_dir().unwrap().join("Desktop/Projects");
        assert_eq!(p.expanded_source_path(), expected.to_string_lossy());
        assert!(!p.expanded_source_path().contains('~'));
        p.dest_path = "/tmp/abs".to_string();
        assert_eq!(p.expanded_dest_path(), "/tmp/abs");
    }

    #[test]
    fn google_drive_and_cloud_helpers() {
        let mut p = SyncProfile::new("g");
        p.dest_path = "/Users/x/Library/CloudStorage/GoogleDrive-a/My Drive".to_string();
        assert!(p.is_google_drive());
        assert!(p.is_cloud_storage());

        p.dest_path = "gdrive://My Folder".to_string();
        assert!(p.is_google_drive_cloud());

        p.dest_path = "/Volumes/SSD".to_string();
        p.ssd_name = "Google Drive (Cloud)".to_string();
        assert!(p.is_google_drive_cloud());

        p.ssd_name = "Extreme Pro".to_string();
        p.dest_path = "/Users/x/Library/CloudStorage/Foo".to_string();
        assert!(p.is_cloud_storage());
        assert!(!p.is_google_drive());
        assert!(!p.is_google_drive_cloud());
    }

    #[test]
    fn default_excludes_include_windows_junk_on_windows() {
        let owned = default_excludes();
        let names: Vec<&str> = owned.iter().map(String::as_str).collect();
        assert!(names.contains(&".DS_Store"));
        assert!(names.contains(&"node_modules"));
        #[cfg(windows)]
        {
            assert!(names.contains(&"Thumbs.db"));
            assert!(names.contains(&"desktop.ini"));
            assert!(names.contains(&"$RECYCLE.BIN"));
        }
        #[cfg(not(windows))]
        {
            assert!(!names.contains(&"Thumbs.db"));
        }
    }

    fn ready(source: &std::path::Path, dest: &std::path::Path) -> SyncProfile {
        SyncProfile {
            source_path: source.display().to_string(),
            dest_path: dest.display().to_string(),
            ssd_name: "Extreme Pro".into(),
            ..SyncProfile::new("Ready")
        }
    }

    #[test]
    fn validate_accepts_a_real_pair_of_folders() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let dest = root.path().join("dest");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        let mut profile = ready(&source, &dest);
        profile.nas_host = Some("nas.local".into());
        profile.excludes = vec!["*.tmp".into(), "node_modules".into()];
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn validate_reports_every_field() {
        let mut profile = SyncProfile::new("Bad");
        profile.source_path.clear();
        profile.dest_path.clear();
        profile.ssd_name.clear();
        profile.auto_mount_nas = true;
        profile.excludes = vec!["[".into()];
        let issues = profile.validate().unwrap_err();
        let fields: Vec<_> = issues.iter().map(ProfileError::field).collect();
        assert!(fields.contains(&"source"));
        assert!(fields.contains(&"dest"));
        assert!(fields.contains(&"ssd"));
        assert!(fields.contains(&"nasHost"));
        assert!(fields.contains(&"nasShare"));
        assert!(fields.contains(&"excludes"));
        assert!(issues.iter().any(|err| matches!(err, ProfileError::EmptySource)));
        assert!(issues.iter().any(|err| matches!(err, ProfileError::BadExclude(_))));
    }

    #[test]
    fn validate_rejects_missing_source_same_path_and_nested_dest() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        let file = root.path().join("file.txt");
        std::fs::write(&file, b"x").unwrap();

        let mut missing = ready(&root.path().join("nope"), &root.path().join("dest"));
        missing.ssd_name = "Disk".into();
        assert!(missing
            .validate()
            .unwrap_err()
            .iter()
            .any(|err| matches!(err, ProfileError::MissingSource)));

        let not_dir = ready(&file, &root.path().join("dest"));
        assert!(not_dir
            .validate()
            .unwrap_err()
            .iter()
            .any(|err| matches!(err, ProfileError::SourceNotDirectory)));

        let same = ready(&source, &source);
        assert!(same
            .validate()
            .unwrap_err()
            .iter()
            .any(|err| matches!(err, ProfileError::SamePath)));

        let nested = source.join("inside");
        std::fs::create_dir_all(&nested).unwrap();
        let inside = ready(&source, &nested);
        assert!(inside
            .validate()
            .unwrap_err()
            .iter()
            .any(|err| matches!(err, ProfileError::DestInsideSource)));
    }

    #[test]
    fn validate_skips_drive_name_for_cloud_and_checks_nas_host() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        let mut cloud = SyncProfile::new("Cloud");
        cloud.source_path = source.display().to_string();
        cloud.dest_path = "gdrive://SyncDrop".into();
        cloud.ssd_name.clear();
        assert!(cloud.validate().is_ok());

        let mut bad_host = ready(&source, &root.path().join("dest"));
        std::fs::create_dir_all(root.path().join("dest")).unwrap();
        bad_host.nas_host = Some("not a host".into());
        assert!(bad_host
            .validate()
            .unwrap_err()
            .iter()
            .any(|err| matches!(err, ProfileError::BadNasHost)));
        bad_host.nas_host = Some("192.168.1.20".into());
        assert!(bad_host.validate().is_ok());
    }
}
