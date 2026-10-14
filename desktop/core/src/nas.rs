//! SMB mount per OS and NAS password in the OS keyring.
//!
//! The password is never written to config. Keyring I/O goes through [`SecretStore`]
//! so tests can use an in-memory store and never touch the OS keyring.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use uuid::Uuid;

use crate::profile::SyncProfile;
use crate::volumes::list_mounted_disks;

/// `keyring` service name. Account is the profile id. Never the Swift keychain service.
pub const KEYRING_SERVICE: &str = "SyncDrop";

pub const NAS_REMOUNT_INTERVAL: Duration = Duration::from_secs(60);

const PASSWORD_MARK: &str = "****";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NasError {
    #[error("NAS host or share name is not configured.")]
    MissingHostOrShare,
    #[error("Could not authenticate with the NAS — check username/password.")]
    AuthenticationFailed,
    #[error("Could not reach the NAS host — check the network/hostname.")]
    HostUnreachable,
    #[error("Mounting the NAS share failed (status {status}).")]
    MountFailed { status: i32 },
    #[error("keyring error")]
    Secret,
}

pub fn keyring_account(profile_id: Uuid) -> String {
    profile_id.to_string()
}

pub trait SecretStore {
    fn save(&self, account: &str, password: &str) -> Result<(), NasError>;
    fn load(&self, account: &str) -> Result<Option<String>, NasError>;
    fn delete(&self, account: &str) -> Result<(), NasError>;
}

/// OS keyring (service [`KEYRING_SERVICE`]). Not used by unit tests.
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn save(&self, account: &str, password: &str) -> Result<(), NasError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account).map_err(|_| NasError::Secret)?;
        entry.set_password(password).map_err(|_| NasError::Secret)
    }

    fn load(&self, account: &str) -> Result<Option<String>, NasError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account).map_err(|_| NasError::Secret)?;
        match entry.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(NasError::Secret),
        }
    }

    fn delete(&self, account: &str) -> Result<(), NasError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account).map_err(|_| NasError::Secret)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(NasError::Secret),
        }
    }
}

pub fn save_password(profile_id: Uuid, password: &str) -> Result<(), NasError> {
    KeyringStore.save(&keyring_account(profile_id), password)
}

pub fn load_password(profile_id: Uuid) -> Result<Option<String>, NasError> {
    KeyringStore.load(&keyring_account(profile_id))
}

pub fn delete_password(profile_id: Uuid) -> Result<(), NasError> {
    KeyringStore.delete(&keyring_account(profile_id))
}

/// True when some mount's basename equals `share_name`. Empty name never matches.
pub fn is_already_mounted<P: AsRef<Path>>(share_name: &str, mounted: &[P]) -> bool {
    if share_name.is_empty() {
        return false;
    }
    mounted.iter().any(|path| {
        path.as_ref().file_name().and_then(|name| name.to_str()) == Some(share_name)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountRequest {
    pub host: String,
    pub share: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

pub trait Mounter {
    fn mount(&self, request: &MountRequest) -> Result<(), NasError>;
}

pub struct SystemMounter;

impl Mounter for SystemMounter {
    fn mount(&self, request: &MountRequest) -> Result<(), NasError> {
        let mut plan = plan_mount(host_os(), request);
        if let Some(dir) = &plan.mount_point {
            if let Err(err) = std::fs::create_dir_all(dir) {
                log::warn!("nas mount dir missing ({err}); trying open smb://");
                return match plan.fallback.take() {
                    Some(fallback) => run_plan(&fallback),
                    None => Err(NasError::MountFailed { status: -1 }),
                };
            }
        }
        match run_plan(&plan) {
            Ok(()) => Ok(()),
            Err(NasError::AuthenticationFailed) => Err(NasError::AuthenticationFailed),
            Err(err) => match plan.fallback.take() {
                Some(fallback) => run_plan(&fallback).or(Err(err)),
                None => Err(err),
            },
        }
    }
}

/// Mount `profile`'s share if it is not already present. Idempotent.
/// Guest (no username) does not read the keyring.
pub fn mount(profile: &SyncProfile) -> Result<(), NasError> {
    let mounted = current_mount_points();
    mount_with(profile, &KeyringStore, &SystemMounter, &mounted)
}

pub fn mount_with<P>(
    profile: &SyncProfile,
    secrets: &impl SecretStore,
    mounter: &impl Mounter,
    mounted: &[P],
) -> Result<(), NasError>
where
    P: AsRef<Path>,
{
    let (host, share) = host_and_share(profile)?;
    let has_user = profile
        .nas_username
        .as_deref()
        .is_some_and(|user| !user.trim().is_empty());
    log::info!(
        "nas mount attempt host={host} share={share} user={}",
        if has_user { "set" } else { "guest" }
    );
    if is_already_mounted(share, mounted) {
        log::info!("nas share already mounted share={share}");
        return Ok(());
    }
    let username = profile
        .nas_username
        .as_deref()
        .filter(|user| !user.is_empty())
        .map(str::to_string);
    let password = match &username {
        Some(_) => secrets.load(&keyring_account(profile.id))?,
        None => None,
    };
    mounter.mount(&MountRequest {
        host: host.to_string(),
        share: share.to_string(),
        username,
        password,
    })
}

/// Retries [`mount`] while `auto_mount_nas` is set and `ssd_name` is not already a mount basename.
///
/// `ssd_name` has to match the share's mount basename or a connect event never fires.
/// Drop the handle (or call [`AutoMountHandle::stop`]) to end the loop.
pub struct AutoMountHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AutoMountHandle {
    pub fn spawn(profile: Arc<Mutex<SyncProfile>>) -> Self {
        spawn_auto_mount_with(
            profile,
            KeyringStore,
            SystemMounter,
            NAS_REMOUNT_INTERVAL,
            current_mount_points,
        )
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AutoMountHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn host_and_share(profile: &SyncProfile) -> Result<(&str, &str), NasError> {
    let host = profile.nas_host.as_deref().unwrap_or("");
    let share = profile.nas_share.as_deref().unwrap_or("");
    if host.is_empty()
        || share.is_empty()
        || share == "."
        || share == ".."
        || host.contains(['/', '\\'])
        || share.contains(['/', '\\'])
    {
        return Err(NasError::MissingHostOrShare);
    }
    Ok((host, share))
}

fn current_mount_points() -> Vec<PathBuf> {
    list_mounted_disks()
        .into_iter()
        .map(|disk| disk.mount_point)
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OsKind {
    Macos,
    Linux,
    Windows,
}

fn host_os() -> OsKind {
    if cfg!(target_os = "macos") {
        OsKind::Macos
    } else if cfg!(target_os = "windows") {
        OsKind::Windows
    } else {
        OsKind::Linux
    }
}

struct MountPlan {
    program: &'static str,
    /// Arguments with the password already substituted. Never log this.
    args: Vec<String>,
    /// Same arguments with the password replaced by [`PASSWORD_MARK`].
    redacted: Vec<String>,
    mount_point: Option<PathBuf>,
    fallback: Option<Box<MountPlan>>,
}

fn plan_mount(os: OsKind, request: &MountRequest) -> MountPlan {
    match os {
        OsKind::Macos => {
            let mount_point = macos_mount_point(&request.share);
            let spec = smbfs_spec(request, false);
            let redacted_spec = smbfs_spec(request, true);
            // `open` has no silent password channel. Omit the secret so it is not on the command line.
            let open_url = guest_smb_url(request);
            MountPlan {
                program: "mount_smbfs",
                args: vec![spec, mount_point.display().to_string()],
                redacted: vec![redacted_spec, mount_point.display().to_string()],
                mount_point: Some(mount_point),
                fallback: Some(Box::new(MountPlan {
                    program: "open",
                    args: vec![open_url.clone()],
                    redacted: vec![open_url],
                    mount_point: None,
                    fallback: None,
                })),
            }
        }
        OsKind::Linux => {
            let url = smb_url(request, false);
            let redacted = smb_url(request, true);
            MountPlan {
                program: "gio",
                args: vec!["mount".to_string(), url],
                redacted: vec!["mount".to_string(), redacted],
                mount_point: None,
                fallback: None,
            }
        }
        OsKind::Windows => {
            let unc = format!(r"\\{}\{}", request.host, request.share);
            let mut args = vec!["use".to_string(), unc];
            let mut redacted = args.clone();
            if let Some(user) = request.username.as_deref().filter(|user| !user.is_empty()) {
                args.push(format!("/user:{user}"));
                redacted.push(format!("/user:{user}"));
                if let Some(password) = request.password.as_deref().filter(|pw| !pw.is_empty()) {
                    args.push(password.to_string());
                    redacted.push(PASSWORD_MARK.to_string());
                }
            }
            MountPlan {
                program: "net",
                args,
                redacted,
                mount_point: None,
                fallback: None,
            }
        }
    }
}

fn macos_mount_point(share: &str) -> PathBuf {
    // ponytail: a user-writable dir, not /Volumes (that mkdir needs root).
    // The basename is the share name so volume matching can see it.
    let root = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir);
    root.join("SyncDrop").join("mnt").join(share)
}

fn smbfs_spec(request: &MountRequest, redact: bool) -> String {
    match (
        request.username.as_deref().filter(|user| !user.is_empty()),
        request.password.as_deref().filter(|pw| !pw.is_empty()),
    ) {
        (Some(user), Some(password)) => {
            let pw = if redact {
                PASSWORD_MARK.to_string()
            } else {
                encode_userinfo(password)
            };
            format!(
                "//{}:{}@{}/{}",
                encode_userinfo(user),
                pw,
                request.host,
                encode_userinfo(&request.share)
            )
        }
        (Some(user), None) => format!(
            "//{}@{}/{}",
            encode_userinfo(user),
            request.host,
            encode_userinfo(&request.share)
        ),
        _ => format!("//{}/{}", request.host, encode_userinfo(&request.share)),
    }
}

fn smb_url(request: &MountRequest, redact: bool) -> String {
    let spec = smbfs_spec(request, redact);
    format!("smb:{spec}")
}

fn guest_smb_url(request: &MountRequest) -> String {
    match request.username.as_deref().filter(|user| !user.is_empty()) {
        Some(user) => format!(
            "smb://{}@{}/{}",
            encode_userinfo(user),
            request.host,
            encode_userinfo(&request.share)
        ),
        None => format!("smb://{}/{}", request.host, encode_userinfo(&request.share)),
    }
}

fn encode_userinfo(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn run_plan(plan: &MountPlan) -> Result<(), NasError> {
    // ponytail: no extra timeout thread. mount_smbfs / gio / net use fail on their own
    // connect timeouts. Stdin is closed so a credential prompt cannot hang the loop.
    let output = Command::new(plan.program)
        .args(&plan.args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| {
            log::warn!(
                "nas mount failed to start {} ({err})",
                plan.redacted.join(" ")
            );
            NasError::MountFailed { status: -1 }
        })?;
    if output.status.success() {
        log::info!("nas mount succeeded");
        return Ok(());
    }
    let status = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let err = classify_failure(status, &stderr);
    log::warn!(
        "nas mount failed {} {} ({err})",
        plan.program,
        plan.redacted.join(" ")
    );
    Err(err)
}

fn classify_failure(status: i32, stderr: &str) -> NasError {
    let text = stderr.to_ascii_lowercase();
    let auth = text.contains("auth")
        || text.contains("permission denied")
        || text.contains("access denied")
        || text.contains("logon failure")
        || text.contains("wrong password")
        || status == 13
        || status == 86
        || status == 1326;
    if auth {
        return NasError::AuthenticationFailed;
    }
    let unreachable = text.contains("unreachable")
        || text.contains("timed out")
        || text.contains("timeout")
        || text.contains("no route")
        || text.contains("connection refused")
        || text.contains("unknown host")
        || text.contains("name or service not known")
        || text.contains("network name cannot be found")
        || status == 53
        || status == 67;
    if unreachable {
        return NasError::HostUnreachable;
    }
    NasError::MountFailed { status }
}

fn spawn_auto_mount_with<S, M, F>(
    profile: Arc<Mutex<SyncProfile>>,
    secrets: S,
    mounter: M,
    interval: Duration,
    list_mounts: F,
) -> AutoMountHandle
where
    S: SecretStore + Send + 'static,
    M: Mounter + Send + 'static,
    F: Fn() -> Vec<PathBuf> + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let thread = thread::Builder::new()
        .name("syncdrop-nas".into())
        .spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                attempt_auto_mount(&profile, &secrets, &mounter, &list_mounts);
                if !interruptible_sleep(interval, &stop_flag) {
                    break;
                }
            }
        })
        .expect("spawn nas remount thread");
    AutoMountHandle {
        stop,
        thread: Some(thread),
    }
}

fn attempt_auto_mount<S, M, F>(profile: &Mutex<SyncProfile>, secrets: &S, mounter: &M, list_mounts: &F)
where
    S: SecretStore,
    M: Mounter,
    F: Fn() -> Vec<PathBuf>,
{
    let snapshot = lock(profile).clone();
    if !snapshot.auto_mount_nas {
        return;
    }
    let mounted = list_mounts();
    if is_already_mounted(&snapshot.ssd_name, &mounted) {
        return;
    }
    if let Err(err) = mount_with(&snapshot, secrets, mounter, &mounted) {
        log::warn!("nas auto-mount failed: {err}");
    }
}

fn interruptible_sleep(total: Duration, stop: &AtomicBool) -> bool {
    let step = Duration::from_millis(20);
    let mut left = total;
    while !left.is_zero() {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let slice = step.min(left);
        thread::sleep(slice);
        left = left.saturating_sub(slice);
    }
    !stop.load(Ordering::Relaxed)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct MemoryStore {
        values: Mutex<std::collections::HashMap<String, String>>,
    }

    impl MemoryStore {
        fn new() -> Self {
            Self {
                values: Mutex::new(std::collections::HashMap::new()),
            }
        }
    }

    impl SecretStore for MemoryStore {
        fn save(&self, account: &str, password: &str) -> Result<(), NasError> {
            lock(&self.values).insert(account.to_string(), password.to_string());
            Ok(())
        }
        fn load(&self, account: &str) -> Result<Option<String>, NasError> {
            Ok(lock(&self.values).get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<(), NasError> {
            lock(&self.values).remove(account);
            Ok(())
        }
    }

    struct PanicStore;

    impl SecretStore for PanicStore {
        fn save(&self, _: &str, _: &str) -> Result<(), NasError> {
            panic!("keyring must not be used");
        }
        fn load(&self, _: &str) -> Result<Option<String>, NasError> {
            panic!("keyring must not be used");
        }
        fn delete(&self, _: &str) -> Result<(), NasError> {
            panic!("keyring must not be used");
        }
    }

    struct RecordingMounter {
        calls: Mutex<Vec<MountRequest>>,
    }

    impl Mounter for RecordingMounter {
        fn mount(&self, request: &MountRequest) -> Result<(), NasError> {
            lock(&self.calls).push(request.clone());
            Ok(())
        }
    }

    struct PanicMounter;

    impl Mounter for PanicMounter {
        fn mount(&self, _: &MountRequest) -> Result<(), NasError> {
            panic!("mount should not run");
        }
    }

    fn nas_profile(host: Option<&str>, share: Option<&str>) -> SyncProfile {
        let mut profile = SyncProfile::new("Test");
        profile.nas_host = host.map(str::to_string);
        profile.nas_share = share.map(str::to_string);
        profile.auto_mount_nas = true;
        profile
    }

    #[test]
    fn already_mounted_matches_basename_only() {
        let urls = [
            PathBuf::from("/Volumes/Macintosh HD"),
            PathBuf::from("/Volumes/Backups"),
        ];
        assert!(is_already_mounted("Backups", &urls));
        assert!(!is_already_mounted("Backups", &[PathBuf::from("/Volumes/Macintosh HD")]));
        assert!(!is_already_mounted("", &[PathBuf::from("/Volumes/")]));
        assert!(!is_already_mounted("Backups", &[] as &[PathBuf]));
        assert!(is_already_mounted(
            "Backups",
            &[PathBuf::from("/some/other/parent/Backups")]
        ));
    }

    #[test]
    fn mount_rejects_missing_host_or_share() {
        for profile in [
            nas_profile(None, Some("Backups")),
            nas_profile(Some("nas.local"), None),
            nas_profile(Some(""), Some("")),
        ] {
            let err = mount_with(&profile, &PanicStore, &PanicMounter, &[] as &[PathBuf]).unwrap_err();
            assert_eq!(err, NasError::MissingHostOrShare);
        }
    }

    #[test]
    fn mount_is_idempotent_when_share_is_present() {
        let profile = nas_profile(Some("nas.local"), Some("Backups"));
        let mounted = [PathBuf::from("/Volumes/Backups")];
        mount_with(&profile, &PanicStore, &PanicMounter, &mounted).unwrap();
    }

    #[test]
    fn guest_mount_does_not_touch_secrets() {
        let profile = nas_profile(Some("nas.local"), Some("Backups"));
        let mounter = RecordingMounter {
            calls: Mutex::new(Vec::new()),
        };
        mount_with(&profile, &PanicStore, &mounter, &[] as &[PathBuf]).unwrap();
        let calls = lock(&mounter.calls);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].host, "nas.local");
        assert!(calls[0].password.is_none());
        assert!(calls[0].username.is_none());
    }

    #[test]
    fn password_round_trip_stays_in_memory() {
        let store = MemoryStore::new();
        let id = Uuid::new_v4();
        let account = keyring_account(id);
        store.save(&account, "s3cret").unwrap();
        assert_eq!(store.load(&account).unwrap().as_deref(), Some("s3cret"));
        store.delete(&account).unwrap();
        assert_eq!(store.load(&account).unwrap(), None);

        let mut profile = nas_profile(Some("nas.local"), Some("Backups"));
        profile.id = id;
        profile.nas_username = Some("abhinay".into());
        store.save(&account, "s3cret").unwrap();
        let mounter = RecordingMounter {
            calls: Mutex::new(Vec::new()),
        };
        mount_with(&profile, &store, &mounter, &[] as &[PathBuf]).unwrap();
        assert_eq!(
            lock(&mounter.calls)[0].password.as_deref(),
            Some("s3cret")
        );
    }

    #[test]
    fn command_lines_hide_the_password() {
        let request = MountRequest {
            host: "nas.local".into(),
            share: "Backups".into(),
            username: Some("abhinay".into()),
            password: Some("s3cret".into()),
        };
        for os in [OsKind::Macos, OsKind::Linux, OsKind::Windows] {
            let plan = plan_mount(os, &request);
            let shown = plan.redacted.join(" ");
            assert!(!shown.contains("s3cret"), "{shown}");
            assert!(shown.contains(PASSWORD_MARK), "{shown}");
            let real = plan.args.join(" ");
            assert!(real.contains("s3cret"), "{real}");
        }
        let mac = plan_mount(OsKind::Macos, &request);
        assert_eq!(mac.program, "mount_smbfs");
        assert_eq!(mac.fallback.unwrap().program, "open");
        assert_eq!(plan_mount(OsKind::Linux, &request).program, "gio");
        let win = plan_mount(OsKind::Windows, &request);
        assert_eq!(win.program, "net");
        assert_eq!(win.args[1], r"\\nas.local\Backups");
        assert!(win.args.iter().any(|arg| arg == "/user:abhinay"));
    }

    #[test]
    fn guest_command_has_no_password_slot() {
        let request = MountRequest {
            host: "nas.local".into(),
            share: "Backups".into(),
            username: None,
            password: None,
        };
        let mac = plan_mount(OsKind::Macos, &request);
        assert_eq!(mac.args[0], "//nas.local/Backups");
        assert!(!mac.redacted[0].contains(PASSWORD_MARK));
    }

    #[test]
    fn classify_auth_and_unreachable() {
        assert_eq!(
            classify_failure(13, "Permission denied"),
            NasError::AuthenticationFailed
        );
        assert_eq!(
            classify_failure(53, "network name cannot be found"),
            NasError::HostUnreachable
        );
        assert_eq!(classify_failure(1, "boom"), NasError::MountFailed { status: 1 });
    }

    #[test]
    fn auto_mount_skips_when_disabled_or_already_present() {
        let calls = Arc::new(AtomicUsize::new(0));
        struct CountMounter(Arc<AtomicUsize>);
        impl Mounter for CountMounter {
            fn mount(&self, _: &MountRequest) -> Result<(), NasError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let mut profile = nas_profile(Some("nas.local"), Some("Backups"));
        profile.auto_mount_nas = false;
        profile.ssd_name = "Backups".into();
        let slot = Arc::new(Mutex::new(profile));
        let handle = spawn_auto_mount_with(
            Arc::clone(&slot),
            MemoryStore::new(),
            CountMounter(Arc::clone(&calls)),
            Duration::from_millis(30),
            Vec::new,
        );
        thread::sleep(Duration::from_millis(80));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(handle);

        let mut profile = nas_profile(Some("nas.local"), Some("Backups"));
        profile.ssd_name = "Backups".into();
        let slot = Arc::new(Mutex::new(profile));
        let calls = Arc::new(AtomicUsize::new(0));
        let handle = spawn_auto_mount_with(
            slot,
            MemoryStore::new(),
            CountMounter(Arc::clone(&calls)),
            Duration::from_millis(40),
            || vec![PathBuf::from("/Volumes/Backups")],
        );
        thread::sleep(Duration::from_millis(100));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(handle);
    }

    #[test]
    fn auto_mount_calls_until_the_share_appears() {
        struct CountMounter(Arc<AtomicUsize>);
        impl Mounter for CountMounter {
            fn mount(&self, _: &MountRequest) -> Result<(), NasError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
        let mut profile = nas_profile(Some("nas.local"), Some("Backups"));
        profile.ssd_name = "Backups".into();
        profile.nas_username = None;
        let slot = Arc::new(Mutex::new(profile));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(AtomicBool::new(false));
        let seen_flag = Arc::clone(&seen);
        let handle = spawn_auto_mount_with(
            slot,
            PanicStore,
            CountMounter(Arc::clone(&calls)),
            Duration::from_millis(40),
            move || {
                if seen_flag.load(Ordering::SeqCst) {
                    vec![PathBuf::from("/Volumes/Backups")]
                } else {
                    Vec::new()
                }
            },
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while calls.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(calls.load(Ordering::SeqCst) >= 1);
        seen.store(true, Ordering::SeqCst);
        let mut settled = calls.load(Ordering::SeqCst);
        let mut stable_passes = 0;
        let stable_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < stable_deadline {
            thread::sleep(Duration::from_millis(50));
            let now = calls.load(Ordering::SeqCst);
            if now == settled {
                stable_passes += 1;
                if stable_passes >= 3 {
                    break;
                }
            } else {
                settled = now;
                stable_passes = 0;
            }
        }
        assert!(stable_passes >= 3, "mount kept firing after the share appeared");
        drop(handle);
    }
}
