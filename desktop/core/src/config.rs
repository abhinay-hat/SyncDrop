use std::fs::{self, File};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::profile::SyncProfile;
use crate::progress::SyncRecord;

/// Swift `ConfigStore.appendSyncRecord` keeps the newest 20.
/// ponytail: phase 2's plan says 100 — raise this constant there if the product wants the longer log.
const HISTORY_CAP: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not resolve a config directory")]
    NoConfigDir,
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// `dirs::config_dir()/SyncDrop/config.json`.
pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    let dir = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
    Ok(dir.join("SyncDrop").join("config.json"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFile {
    profiles: Vec<SyncProfile>,
    active_profile_id: Uuid,
    #[serde(default = "default_true")]
    notify_on_complete: bool,
    #[serde(default)]
    launch_at_login: bool,
    #[serde(default)]
    sync_history: Vec<SyncRecord>,
    #[serde(default)]
    google_client_id: String,
}

fn default_true() -> bool {
    true
}

/// Permissive read of `config.json`, including the v1 flat keys Swift kept in UserDefaults.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadedConfig {
    #[serde(default)]
    profiles: Vec<SyncProfile>,
    #[serde(default)]
    active_profile_id: Option<String>,
    #[serde(default = "default_true")]
    notify_on_complete: bool,
    #[serde(default)]
    launch_at_login: bool,
    #[serde(default)]
    sync_history: Vec<SyncRecord>,
    #[serde(default)]
    google_client_id: String,
    #[serde(default)]
    source_path: Option<String>,
    #[serde(default)]
    ssd_name: Option<String>,
    #[serde(default)]
    dest_path: Option<String>,
    #[serde(default)]
    auto_sync: bool,
    #[serde(default)]
    mirror_mode: bool,
    #[serde(default)]
    excludes: Option<Vec<String>>,
    #[serde(default)]
    auto_eject: bool,
    #[serde(default)]
    keep_versions: bool,
}

/// Profiles, the active id, globals, and history in one JSON file.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
    profiles: Vec<SyncProfile>,
    active_profile_id: Uuid,
    notify_on_complete: bool,
    launch_at_login: bool,
    sync_history: Vec<SyncRecord>,
    google_client_id: String,
}

impl ConfigStore {
    /// Opens `dirs::config_dir()/SyncDrop/config.json`, creating it if needed.
    pub fn new() -> Result<Self, ConfigError> {
        Self::with_path(default_config_path()?)
    }

    pub fn with_path(path: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        let path = path.into();
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<LoadedConfig>(&bytes) {
                Ok(loaded) => {
                    log::info!("config loaded");
                    Self::from_loaded(path, loaded)
                }
                Err(_) => {
                    log::warn!("config unreadable; renamed and replaced");
                    quarantine(&path)?;
                    Self::fresh(path)
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Only the real config path: unit tests open missing temp files and must not shell out.
                #[cfg(target_os = "macos")]
                if let Some(imported) = import_macos_defaults(&path) {
                    return Ok(imported);
                }
                Self::fresh(path)
            },
            Err(e) => Err(e.into()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn profiles(&self) -> &[SyncProfile] {
        &self.profiles
    }

    pub fn active_profile_id(&self) -> Uuid {
        self.active_profile_id
    }

    pub fn notify_on_complete(&self) -> bool {
        self.notify_on_complete
    }

    pub fn launch_at_login(&self) -> bool {
        self.launch_at_login
    }

    pub fn sync_history(&self) -> &[SyncRecord] {
        &self.sync_history
    }

    pub fn google_client_id(&self) -> &str {
        &self.google_client_id
    }

    pub fn active_profile(&self) -> &SyncProfile {
        let fallback = self
            .profiles
            .first()
            .expect("ConfigStore always holds at least one profile");
        self.profiles
            .iter()
            .find(|p| p.id == self.active_profile_id)
            .unwrap_or(fallback)
    }

    pub fn expanded_source_path(&self) -> String {
        self.active_profile().expanded_source_path()
    }

    pub fn set_active_profile_id(&mut self, id: Uuid) -> Result<(), ConfigError> {
        self.active_profile_id = id;
        self.save()
    }

    pub fn set_active_profile(&mut self, profile: SyncProfile) -> Result<(), ConfigError> {
        if let Some(slot) = self.profiles.iter_mut().find(|p| p.id == profile.id) {
            *slot = profile;
        } else {
            self.profiles.push(profile);
        }
        self.save()
    }

    pub fn set_notify_on_complete(&mut self, value: bool) -> Result<(), ConfigError> {
        self.notify_on_complete = value;
        self.save()
    }

    pub fn set_launch_at_login(&mut self, value: bool) -> Result<(), ConfigError> {
        self.launch_at_login = value;
        self.save()
    }

    pub fn set_google_client_id(&mut self, value: impl Into<String>) -> Result<(), ConfigError> {
        self.google_client_id = value.into();
        self.save()
    }

    pub fn add_profile(&mut self, name: impl Into<String>) -> Result<(), ConfigError> {
        self.profiles.push(SyncProfile::new(name));
        self.save()
    }

    /// No-op when this is the only profile. Deleting the active one selects the first remaining.
    pub fn delete_profile(&mut self, id: Uuid) -> Result<(), ConfigError> {
        if self.profiles.len() <= 1 {
            return Ok(());
        }
        self.profiles.retain(|p| p.id != id);
        if self.active_profile_id == id {
            self.active_profile_id = self.profiles[0].id;
        }
        self.save()
    }

    pub fn append_sync_record(&mut self, record: SyncRecord) -> Result<(), ConfigError> {
        self.sync_history.insert(0, record);
        if self.sync_history.len() > HISTORY_CAP {
            self.sync_history.truncate(HISTORY_CAP);
        }
        self.save()
    }

    pub fn clear_sync_history(&mut self) -> Result<(), ConfigError> {
        self.sync_history.clear();
        self.save()
    }

    fn fresh(path: PathBuf) -> Result<Self, ConfigError> {
        let profile = SyncProfile::new("Default");
        let store = Self {
            path,
            active_profile_id: profile.id,
            profiles: vec![profile],
            notify_on_complete: true,
            launch_at_login: false,
            sync_history: Vec::new(),
            google_client_id: String::new(),
        };
        store.save()?;
        Ok(store)
    }

    fn from_loaded(path: PathBuf, loaded: LoadedConfig) -> Result<Self, ConfigError> {
        let had_profiles = !loaded.profiles.is_empty();
        let migrated = loaded.source_path.is_some();
        let profiles = if had_profiles {
            loaded.profiles
        } else if migrated {
            vec![legacy_profile(&loaded)]
        } else {
            vec![SyncProfile::new("Default")]
        };
        let active_profile_id = loaded
            .active_profile_id
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok())
            .filter(|id| profiles.iter().any(|p| p.id == *id))
            .unwrap_or(profiles[0].id);
        let store = Self {
            path,
            profiles,
            active_profile_id,
            notify_on_complete: loaded.notify_on_complete,
            launch_at_login: loaded.launch_at_login,
            sync_history: loaded.sync_history,
            google_client_id: loaded.google_client_id,
        };
        if !had_profiles || migrated {
            store.save()?;
        }
        Ok(store)
    }

    fn save(&self) -> Result<(), ConfigError> {
        let file = ConfigFile {
            profiles: self.profiles.clone(),
            active_profile_id: self.active_profile_id,
            notify_on_complete: self.notify_on_complete,
            launch_at_login: self.launch_at_login,
            sync_history: self.sync_history.clone(),
            google_client_id: self.google_client_id.clone(),
        };
        let data = serde_json::to_vec_pretty(&file)?;
        atomic_write(&self.path, &data)?;
        log::info!("config saved ({} profiles)", self.profiles.len());
        Ok(())
    }
}

/// `Info.plist` `CFBundleIdentifier` of the Swift menu-bar app.
#[cfg(target_os = "macos")]
const SWIFT_DEFAULTS_DOMAIN: &str = "com.syncdrop.app";

/// Seconds between the Unix epoch and the Apple reference date (2001-01-01 UTC).
const APPLE_EPOCH_OFFSET: f64 = 978_307_200.0;

/// Read the Swift app's UserDefaults once, when `config.json` is not there yet.
#[cfg(target_os = "macos")]
fn import_macos_defaults(path: &Path) -> Option<ConfigStore> {
    let default_path = default_config_path().ok()?;
    if path != default_path {
        return None;
    }
    let output = std::process::Command::new("defaults")
        .args(["export", SWIFT_DEFAULTS_DOMAIN, "-"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let loaded = config_from_user_defaults_plist(&output.stdout)?;
    let store = ConfigStore::from_loaded(path.to_path_buf(), loaded).ok()?;
    store.save().ok()?;
    Some(store)
}

/// Convert a `defaults export` XML plist into the JSON config shape.
/// Profiles and history are JSON inside `<data>` blobs. Returns `None` when the
/// plist has none of the Swift keys.
fn config_from_user_defaults_plist(bytes: &[u8]) -> Option<LoadedConfig> {
    let value = plist::Value::from_reader(Cursor::new(bytes)).ok()?;
    let dict = value.into_dictionary()?;
    if !dict_has_swift_keys(&dict) {
        return None;
    }
    let profiles = json_blob(&dict, "profiles")
        .and_then(|v| serde_json::from_value::<Vec<SyncProfile>>(v).ok())
        .unwrap_or_default();
    let mut history_json = json_blob(&dict, "syncHistory").unwrap_or(serde_json::Value::Array(vec![]));
    normalize_apple_dates(&mut history_json);
    let sync_history = serde_json::from_value(history_json).unwrap_or_default();
    let excludes = json_blob(&dict, "excludes").and_then(|v| serde_json::from_value(v).ok());
    Some(LoadedConfig {
        profiles,
        active_profile_id: plist_string(&dict, "activeProfileId"),
        notify_on_complete: plist_bool(&dict, "notifyOnComplete").unwrap_or(true),
        launch_at_login: plist_bool(&dict, "launchAtLogin").unwrap_or(false),
        sync_history,
        google_client_id: plist_string(&dict, "google_oauth_client_id").unwrap_or_default(),
        source_path: plist_string(&dict, "sourcePath"),
        ssd_name: plist_string(&dict, "ssdName"),
        dest_path: plist_string(&dict, "destPath"),
        auto_sync: plist_bool(&dict, "autoSync").unwrap_or(false),
        mirror_mode: plist_bool(&dict, "mirrorMode").unwrap_or(false),
        excludes,
        auto_eject: plist_bool(&dict, "autoEject").unwrap_or(false),
        keep_versions: plist_bool(&dict, "keepVersions").unwrap_or(false),
    })
}

fn dict_has_swift_keys(dict: &plist::Dictionary) -> bool {
    const KEYS: &[&str] = &[
        "profiles",
        "activeProfileId",
        "notifyOnComplete",
        "launchAtLogin",
        "syncHistory",
        "google_oauth_client_id",
        "sourcePath",
        "ssdName",
        "destPath",
        "autoSync",
        "mirrorMode",
        "excludes",
        "autoEject",
        "keepVersions",
    ];
    KEYS.iter().any(|key| dict.contains_key(*key))
}

fn json_blob(dict: &plist::Dictionary, key: &str) -> Option<serde_json::Value> {
    let data = dict.get(key)?.as_data()?;
    serde_json::from_slice(data).ok()
}

fn plist_string(dict: &plist::Dictionary, key: &str) -> Option<String> {
    dict.get(key)
        .and_then(|v| v.as_string())
        .map(str::to_string)
}

fn plist_bool(dict: &plist::Dictionary, key: &str) -> Option<bool> {
    dict.get(key).and_then(|v| v.as_boolean())
}

/// Swift `JSONEncoder` writes `Date` as seconds since 2001-01-01. Chrono expects RFC3339.
fn normalize_apple_dates(value: &mut serde_json::Value) {
    let Some(records) = value.as_array_mut() else {
        return;
    };
    for record in records {
        let Some(obj) = record.as_object_mut() else {
            continue;
        };
        let Some(date) = obj.get("date").and_then(|d| d.as_f64()) else {
            continue;
        };
        let Some(rfc3339) = apple_date_to_rfc3339(date) else {
            continue;
        };
        obj.insert("date".to_string(), serde_json::Value::String(rfc3339));
    }
}

fn apple_date_to_rfc3339(apple_seconds: f64) -> Option<String> {
    let millis = ((apple_seconds + APPLE_EPOCH_OFFSET) * 1000.0).round() as i64;
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis).map(|dt| dt.to_rfc3339())
}

fn legacy_profile(loaded: &LoadedConfig) -> SyncProfile {
    let mut profile = SyncProfile::new("Default");
    if let Some(source) = &loaded.source_path {
        profile.source_path = source.clone();
    }
    profile.dest_path = loaded.dest_path.clone().unwrap_or_default();
    if let Some(ssd) = &loaded.ssd_name {
        profile.ssd_name = ssd.clone();
    }
    profile.mirror_mode = loaded.mirror_mode;
    profile.auto_sync = loaded.auto_sync;
    profile.auto_eject = loaded.auto_eject;
    profile.keep_versions = loaded.keep_versions;
    if let Some(excludes) = &loaded.excludes {
        profile.excludes = excludes.clone();
    }
    profile
}

fn quarantine(path: &Path) -> io::Result<()> {
    let bak = backup_path(path);
    if bak.exists() {
        fs::remove_file(&bak)?;
    }
    fs::rename(path, bak)
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    PathBuf::from(name)
}

fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
            }
        }
    }
    let tmp = tmp_path(path);
    {
        let mut file = File::create(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
        }
        file.write_all(data)?;
        file.sync_all()?;
    }
    if let Err(e) = fs::rename(&tmp, path) {
        #[cfg(windows)]
        {
            fs::remove_file(path)?;
            return fs::rename(&tmp, path);
        }
        #[cfg(not(windows))]
        {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::progress::SyncRecord;

    fn open_store() -> (tempfile::TempDir, ConfigStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::with_path(dir.path().join("config.json")).unwrap();
        (dir, store)
    }

    #[test]
    fn default_config_path_is_under_syncdrop() {
        let path = default_config_path().unwrap();
        assert!(path.ends_with(Path::new("SyncDrop").join("config.json")));
    }

    #[test]
    fn defaults_seed_one_default_profile() {
        let (_dir, store) = open_store();
        assert_eq!(store.profiles().len(), 1);
        assert_eq!(store.profiles()[0].name, "Default");
        assert_eq!(store.active_profile_id(), store.profiles()[0].id);
    }

    #[test]
    fn active_profile_defaults_are_correct() {
        let (_dir, store) = open_store();
        let p = store.active_profile();
        assert_eq!(p.source_path, "");
        assert_eq!(p.ssd_name, "");
        assert_eq!(p.dest_path, "");
        assert!(!p.auto_sync);
        assert!(!p.mirror_mode);
        assert!(!p.auto_eject);
        assert!(!p.keep_versions);
        assert_eq!(
            p.excludes,
            [
                ".DS_Store",
                ".Spotlight-V100",
                ".fseventsd",
                ".Trashes",
                "node_modules",
                #[cfg(windows)]
                "Thumbs.db",
                #[cfg(windows)]
                "desktop.ini",
                #[cfg(windows)]
                "$RECYCLE.BIN",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn globals_default() {
        let (_dir, store) = open_store();
        assert!(store.notify_on_complete());
        assert!(!store.launch_at_login());
        assert!(store.sync_history().is_empty());
        assert_eq!(store.google_client_id(), "");
    }

    #[test]
    fn active_profile_setter_persists_edit() {
        let (_dir, mut store) = open_store();
        let mut p = store.active_profile().clone();
        p.source_path = "~/Documents/Work".to_string();
        store.set_active_profile(p).unwrap();
        let reloaded = ConfigStore::with_path(store.path()).unwrap();
        assert_eq!(reloaded.active_profile().source_path, "~/Documents/Work");
    }

    #[test]
    fn add_profile_and_switch() {
        let (_dir, mut store) = open_store();
        store.add_profile("Photos").unwrap();
        assert_eq!(store.profiles().len(), 2);
        let photos = store.profiles()[1].id;
        store.set_active_profile_id(photos).unwrap();
        assert_eq!(store.active_profile().name, "Photos");
    }

    #[test]
    fn delete_profile_never_deletes_last() {
        let (_dir, mut store) = open_store();
        store.delete_profile(store.profiles()[0].id).unwrap();
        assert_eq!(store.profiles().len(), 1);
    }

    #[test]
    fn delete_profile_reassigns_active_when_deleting_active() {
        let (_dir, mut store) = open_store();
        store.add_profile("B").unwrap();
        let a = store.profiles()[0].id;
        store.delete_profile(a).unwrap();
        assert_eq!(store.profiles().len(), 1);
        assert_eq!(store.active_profile_id(), store.profiles()[0].id);
        assert_eq!(store.profiles()[0].name, "B");
    }

    #[test]
    fn expanded_source_path_proxies_active_profile() {
        let (_dir, mut store) = open_store();
        let mut p = store.active_profile().clone();
        p.source_path = "~/Desktop/Projects".to_string();
        store.set_active_profile(p).unwrap();
        let home = dirs::home_dir().unwrap();
        assert!(store.expanded_source_path().starts_with(&*home.to_string_lossy()));
        assert!(!store.expanded_source_path().contains('~'));
    }

    #[test]
    fn append_sync_record_keeps_max_20() {
        let (_dir, mut store) = open_store();
        for i in 0..25 {
            store
                .append_sync_record(SyncRecord::new(Utc::now(), i, 0, 1.0, true))
                .unwrap();
        }
        assert_eq!(store.sync_history().len(), 20);
        assert_eq!(store.sync_history()[0].file_count, 24);
        let reloaded = ConfigStore::with_path(store.path()).unwrap();
        assert_eq!(reloaded.sync_history().len(), 20);
        assert_eq!(reloaded.sync_history()[0].file_count, 24);
    }

    #[test]
    fn google_client_id_round_trips_and_missing_field_defaults_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let id = Uuid::new_v4();
        let body = json!({
            "profiles": [{
                "id": id,
                "name": "Default",
                "sourcePath": "~/Desktop/Projects",
                "destPath": "",
                "ssdName": "Extreme Pro",
                "mirrorMode": false,
                "autoSync": false,
                "autoEject": false,
                "keepVersions": false,
                "excludes": [".DS_Store"]
            }],
            "activeProfileId": id
        });
        fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
        let mut store = ConfigStore::with_path(&path).unwrap();
        assert_eq!(store.google_client_id(), "");
        assert!(store.notify_on_complete());
        store.set_google_client_id("client-id").unwrap();
        let reloaded = ConfigStore::with_path(&path).unwrap();
        assert_eq!(reloaded.google_client_id(), "client-id");
    }

    #[test]
    fn migration_from_legacy_keys_creates_default_profile_and_clears_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let legacy = json!({
            "sourcePath": "~/OldSource",
            "ssdName": "MySSD",
            "destPath": "/Volumes/MySSD/Backup",
            "autoSync": true,
            "mirrorMode": true,
            "excludes": ["a", "b"],
            "autoEject": true,
            "keepVersions": true
        });
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();

        let migrated = ConfigStore::with_path(&path).unwrap();
        assert_eq!(migrated.profiles().len(), 1);
        let p = migrated.active_profile();
        assert_eq!(p.name, "Default");
        assert_eq!(p.source_path, "~/OldSource");
        assert_eq!(p.ssd_name, "MySSD");
        assert_eq!(p.dest_path, "/Volumes/MySSD/Backup");
        assert!(p.auto_sync);
        assert!(p.mirror_mode);
        assert!(p.auto_eject);
        assert!(p.keep_versions);
        assert_eq!(p.excludes, ["a".to_string(), "b".to_string()]);

        let raw: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(raw.get("sourcePath").is_none());
        assert!(raw.get("ssdName").is_none());

        let reloaded = ConfigStore::with_path(&path).unwrap();
        assert_eq!(reloaded.profiles().len(), 1);
        assert_eq!(reloaded.active_profile().source_path, "~/OldSource");
    }

    #[test]
    fn corrupt_config_is_backed_up_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, b"this is not json").unwrap();
        let store = ConfigStore::with_path(&path).unwrap();
        assert_eq!(store.profiles().len(), 1);
        assert_eq!(store.profiles()[0].name, "Default");
        let bak: PathBuf = backup_path(&path);
        assert_eq!(fs::read(&bak).unwrap(), b"this is not json");
        let fresh = fs::read_to_string(&path).unwrap();
        assert!(fresh.contains("Default"));
    }

    #[test]
    fn user_defaults_plist_converts_swift_json_blobs() {
        let id = "11111111-1111-1111-1111-111111111111";
        let profiles = serde_json::to_vec(&json!([{
            "id": id,
            "name": "Laptop",
            "sourcePath": "~/Work",
            "destPath": "/Volumes/Extreme Pro/Backup",
            "ssdName": "Extreme Pro",
            "mirrorMode": true,
            "autoSync": true,
            "autoEject": false,
            "keepVersions": true,
            "excludes": [".DS_Store"],
            "nasHost": "nas.local",
            "nasShare": "share",
            "nasUsername": "me",
            "autoMountNAS": true
        }]))
        .unwrap();
        // Swift JSONEncoder Date: seconds since 2001-01-01. 0 → that instant.
        let history = serde_json::to_vec(&json!([{
            "id": "22222222-2222-2222-2222-222222222222",
            "date": 0.0,
            "fileCount": 4,
            "totalBytes": 99,
            "durationSeconds": 1.5,
            "succeeded": true
        }]))
        .unwrap();
        let xml = defaults_plist(
            &[
                ("profiles", profiles.as_slice()),
                ("syncHistory", history.as_slice()),
            ],
            &[
                ("activeProfileId", id),
                ("google_oauth_client_id", "abc.apps.googleusercontent.com"),
                ("google_oauth_client_secret", "super-secret"),
            ],
            &[("notifyOnComplete", false), ("launchAtLogin", true)],
        );

        let loaded = config_from_user_defaults_plist(xml.as_bytes()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let store = ConfigStore::from_loaded(path.clone(), loaded).unwrap();
        store.save().unwrap();

        assert_eq!(store.profiles().len(), 1);
        let p = store.active_profile();
        assert_eq!(p.name, "Laptop");
        assert_eq!(p.source_path, "~/Work");
        assert_eq!(p.dest_path, "/Volumes/Extreme Pro/Backup");
        assert!(p.auto_sync);
        assert!(p.mirror_mode);
        assert!(p.keep_versions);
        assert!(p.auto_mount_nas);
        assert_eq!(p.nas_host.as_deref(), Some("nas.local"));
        assert_eq!(store.active_profile_id(), p.id);
        assert!(!store.notify_on_complete());
        assert!(store.launch_at_login());
        assert_eq!(store.google_client_id(), "abc.apps.googleusercontent.com");
        assert_eq!(store.sync_history().len(), 1);
        assert_eq!(store.sync_history()[0].file_count, 4);
        assert_eq!(store.sync_history()[0].total_bytes, 99);
        assert_eq!(
            store.sync_history()[0].date,
            chrono::DateTime::parse_from_rfc3339("2001-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc)
        );
        let saved = fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("super-secret"));
    }

    #[test]
    fn user_defaults_plist_migrates_legacy_v1_keys() {
        let excludes = serde_json::to_vec(&json!(["*.tmp", "build"])).unwrap();
        let xml = defaults_plist(
            &[("excludes", excludes.as_slice())],
            &[
                ("sourcePath", "~/OldSource"),
                ("ssdName", "MySSD"),
                ("destPath", "/Volumes/MySSD/Backup"),
            ],
            &[
                ("autoSync", true),
                ("mirrorMode", true),
                ("autoEject", true),
                ("keepVersions", true),
            ],
        );
        let loaded = config_from_user_defaults_plist(xml.as_bytes()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let store = ConfigStore::from_loaded(path, loaded).unwrap();
        let p = store.active_profile();
        assert_eq!(p.name, "Default");
        assert_eq!(p.source_path, "~/OldSource");
        assert_eq!(p.ssd_name, "MySSD");
        assert_eq!(p.dest_path, "/Volumes/MySSD/Backup");
        assert!(p.auto_sync && p.mirror_mode && p.auto_eject && p.keep_versions);
        assert_eq!(p.excludes, ["*.tmp".to_string(), "build".to_string()]);
    }

    #[test]
    fn user_defaults_plist_without_swift_keys_is_skipped() {
        let xml = defaults_plist(&[], &[("unrelated", "x")], &[]);
        assert!(config_from_user_defaults_plist(xml.as_bytes()).is_none());
    }

    fn defaults_plist(
        blobs: &[(&str, &[u8])],
        strings: &[(&str, &str)],
        bools: &[(&str, bool)],
    ) -> String {
        let mut dict = plist::Dictionary::new();
        for (key, bytes) in blobs {
            dict.insert((*key).to_string(), plist::Value::Data(bytes.to_vec()));
        }
        for (key, value) in strings {
            dict.insert((*key).to_string(), plist::Value::String((*value).to_string()));
        }
        for (key, value) in bools {
            dict.insert((*key).to_string(), plist::Value::Boolean(*value));
        }
        let mut buf = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_xml(&mut buf)
            .unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn clear_sync_history_empties_the_list() {
        let (_dir, mut store) = open_store();
        store
            .append_sync_record(SyncRecord::new(Utc::now(), 1, 2, 3.0, true))
            .unwrap();
        store.clear_sync_history().unwrap();
        assert!(store.sync_history().is_empty());
        let reloaded = ConfigStore::with_path(store.path()).unwrap();
        assert!(reloaded.sync_history().is_empty());
    }
}
