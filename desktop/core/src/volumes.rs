//! Drive detection (polling) and eject.
//!
//! ponytail: polls `sysinfo` every 2s instead of DiskArbitration / WM_DEVICECHANGE / udev.
//! Upgrade if a 2s connect delay is too slow.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use uuid::Uuid;

use crate::profile::SyncProfile;

pub const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// A gap longer than this means the machine slept (or the thread stalled). Rescan immediately.
pub const WAKE_GAP: Duration = Duration::from_secs(10);

/// How long to wait before the next poll. A gap past [`WAKE_GAP`] waits zero.
pub fn poll_delay(since_last: Duration, poll_interval: Duration, wake_gap: Duration) -> Duration {
    if since_last > wake_gap {
        Duration::ZERO
    } else {
        poll_interval
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedDisk {
    pub name: String,
    pub mount_point: PathBuf,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeEvent {
    Connected {
        profile_id: Uuid,
        path: PathBuf,
        free_bytes: Option<u64>,
    },
    Disconnected {
        profile_id: Uuid,
    },
}

/// Snapshot the monitor thread reads each poll. Every profile is watched, not only the active one.
#[derive(Debug, Clone)]
pub struct WatchState {
    pub profiles: Vec<SyncProfile>,
    /// Cloud profiles stay disconnected until this is true.
    pub google_signed_in: bool,
}

impl WatchState {
    pub fn new(profile: SyncProfile) -> Self {
        Self {
            profiles: vec![profile],
            google_signed_in: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VolumeError {
    #[error("eject failed: {0}")]
    Eject(String),
}

/// A disk matches when its volume name or its mount-point basename equals `ssd_name`.
pub fn disk_matches(ssd_name: &str, disk: &MountedDisk) -> bool {
    if ssd_name.is_empty() {
        return false;
    }
    if disk.name == ssd_name {
        return true;
    }
    mount_basename(&disk.mount_point) == Some(ssd_name)
}

/// Where a profile is "connected", if anywhere.
///
/// Order matches `VolumeMonitor.checkCurrentlyMountedVolumes`: Google Drive cloud
/// (signed-in only), then an existing cloud-storage destination directory, then a
/// mounted disk whose name or basename equals `ssd_name`.
pub fn resolve_mount(
    profile: &SyncProfile,
    google_signed_in: bool,
    dest_is_directory: bool,
    disks: &[MountedDisk],
) -> Option<PathBuf> {
    if profile.is_google_drive_cloud() {
        return google_signed_in.then(|| PathBuf::from("gdrive://root"));
    }
    let dest = profile.expanded_dest_path();
    if profile.is_cloud_storage() && !dest.is_empty() && dest_is_directory {
        return Some(PathBuf::from(dest));
    }
    disks.iter().find(|d| disk_matches(&profile.ssd_name, d)).map(|d| d.mount_point.clone())
}

/// Free space on the disk mounted at `mount`, when that disk is in the snapshot.
pub fn free_bytes_at(disks: &[MountedDisk], mount: &Path) -> Option<u64> {
    disks
        .iter()
        .find(|disk| disk.mount_point == mount)
        .map(|disk| disk.available_bytes)
}

/// Event to emit when one profile's resolved mount changes. `None` means nothing new happened.
pub fn mount_transition(
    profile_id: Uuid,
    previous: Option<&Path>,
    next: Option<&Path>,
    free_bytes: Option<u64>,
) -> Option<VolumeEvent> {
    match (previous, next) {
        (None, Some(path)) => Some(VolumeEvent::Connected {
            profile_id,
            path: path.to_path_buf(),
            free_bytes,
        }),
        (Some(_), None) => Some(VolumeEvent::Disconnected { profile_id }),
        (Some(prev), Some(next)) if prev != next => Some(VolumeEvent::Connected {
            profile_id,
            path: next.to_path_buf(),
            free_bytes,
        }),
        _ => None,
    }
}

pub fn list_mounted_disks() -> Vec<MountedDisk> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .map(|disk| MountedDisk {
            name: disk.name().to_string_lossy().into_owned(),
            mount_point: disk.mount_point().to_path_buf(),
            available_bytes: disk.available_space(),
        })
        .collect()
}

/// Background poller. Dropping it (or calling [`VolumeMonitor::stop`]) ends the thread.
pub struct VolumeMonitor {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl VolumeMonitor {
    /// Poll [`list_mounted_disks`] every [`POLL_INTERVAL`] and send transitions on the channel.
    pub fn spawn(state: Arc<Mutex<WatchState>>) -> (Self, Receiver<VolumeEvent>) {
        spawn_inner(
            state,
            Box::new(SysinfoSource),
            MonitorTiming::default(),
        )
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for VolumeMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Eject `mount`. macOS uses `diskutil`; Linux tries `udisksctl unmount -b` then `umount`;
/// Windows uses the Shell.Application eject verb.
pub fn eject(mount: &Path) -> Result<(), VolumeError> {
    let device = block_device_for(mount);
    let commands = eject_commands(host_os(), mount, device.as_deref());
    let mut last = String::from("no eject command for this platform");
    for (program, args) in commands {
        match Command::new(program)
            .args(&args)
            .stdin(Stdio::null())
            .output()
        {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) => {
                last = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if last.is_empty() {
                    last = format!("{program} exited with {}", output.status);
                }
            }
            Err(err) => last = err.to_string(),
        }
    }
    Err(VolumeError::Eject(last))
}

fn mount_basename(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}

fn block_device_for(mount: &Path) -> Option<String> {
    // ponytail: sysinfo's disk name is a /dev node on Linux and a volume label elsewhere.
    // If it isn't a device path, eject falls straight through to `umount`.
    list_mounted_disks().into_iter().find_map(|disk| {
        if disk.mount_point == mount && disk.name.starts_with("/dev/") {
            Some(disk.name)
        } else {
            None
        }
    })
}

#[derive(Clone, Copy)]
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

fn eject_commands(os: OsKind, mount: &Path, block_device: Option<&str>) -> Vec<(&'static str, Vec<String>)> {
    let mount_s = mount.display().to_string();
    match os {
        OsKind::Macos => vec![("diskutil", vec!["eject".to_string(), mount_s])],
        OsKind::Linux => {
            let mut commands = Vec::new();
            if let Some(device) = block_device {
                commands.push((
                    "udisksctl",
                    vec!["unmount".to_string(), "-b".to_string(), device.to_string()],
                ));
            }
            commands.push(("umount", vec![mount_s]));
            commands
        }
        OsKind::Windows => {
            let spec = windows_drive_spec(mount);
            let escaped = spec.replace('\'', "''");
            let script = format!(
                "(New-Object -comObject Shell.Application).Namespace(17).ParseName('{escaped}').InvokeVerb('Eject')"
            );
            vec![(
                "powershell",
                vec!["-NoProfile".to_string(), "-Command".to_string(), script],
            )]
        }
    }
}

fn windows_drive_spec(mount: &Path) -> String {
    let raw = mount.display().to_string();
    let trimmed = raw.trim_start_matches(r"\\?\");
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return trimmed[..2].to_string();
    }
    trimmed.to_string()
}

struct MonitorTiming {
    poll: Duration,
    wake_gap: Duration,
}

impl Default for MonitorTiming {
    fn default() -> Self {
        Self {
            poll: POLL_INTERVAL,
            wake_gap: WAKE_GAP,
        }
    }
}

trait DiskSource: Send {
    fn list(&mut self) -> Vec<MountedDisk>;
}

struct SysinfoSource;

impl DiskSource for SysinfoSource {
    fn list(&mut self) -> Vec<MountedDisk> {
        list_mounted_disks()
    }
}

fn spawn_inner(
    state: Arc<Mutex<WatchState>>,
    mut disks: Box<dyn DiskSource>,
    timing: MonitorTiming,
) -> (VolumeMonitor, Receiver<VolumeEvent>) {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let thread = thread::Builder::new()
        .name("syncdrop-volumes".into())
        .spawn(move || {
            let mut previous: HashMap<Uuid, PathBuf> = HashMap::new();
            // First iteration is already "late", so startup scans immediately (same as Swift's start).
            let mut last_poll = std::time::Instant::now()
                .checked_sub(timing.wake_gap + Duration::from_secs(1))
                .unwrap_or_else(std::time::Instant::now);
            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                let delay = poll_delay(last_poll.elapsed(), timing.poll, timing.wake_gap);
                if !interruptible_sleep(delay, &stop_flag) {
                    break;
                }
                last_poll = std::time::Instant::now();
                let snapshot = lock(&state).clone();
                let mounted = disks.list();
                let mut seen = HashSet::new();
                let mut stopped = false;
                for profile in &snapshot.profiles {
                    seen.insert(profile.id);
                    let dest = profile.expanded_dest_path();
                    let dest_is_directory = !dest.is_empty() && Path::new(&dest).is_dir();
                    let next = resolve_mount(
                        profile,
                        snapshot.google_signed_in,
                        dest_is_directory,
                        &mounted,
                    );
                    let free = next.as_deref().and_then(|path| free_bytes_at(&mounted, path));
                    if let Some(event) = mount_transition(
                        profile.id,
                        previous.get(&profile.id).map(PathBuf::as_path),
                        next.as_deref(),
                        free,
                    ) {
                        match &event {
                            VolumeEvent::Connected { path, .. } => {
                                log::info!("volume connected at {}", path.display());
                            }
                            VolumeEvent::Disconnected { profile_id } => {
                                log::info!("volume disconnected profile={profile_id}");
                            }
                        }
                        if tx.send(event).is_err() {
                            stopped = true;
                            break;
                        }
                    }
                    match next {
                        Some(path) => {
                            previous.insert(profile.id, path);
                        }
                        None => {
                            previous.remove(&profile.id);
                        }
                    }
                }
                if stopped {
                    break;
                }
                let stale: Vec<Uuid> = previous
                    .keys()
                    .copied()
                    .filter(|id| !seen.contains(id))
                    .collect();
                for profile_id in stale {
                    previous.remove(&profile_id);
                    log::info!("volume disconnected profile={profile_id}");
                    if tx
                        .send(VolumeEvent::Disconnected { profile_id })
                        .is_err()
                    {
                        stopped = true;
                        break;
                    }
                }
                if stopped {
                    break;
                }
            }
        })
        .expect("spawn volume monitor thread");
    (VolumeMonitor { stop, thread: Some(thread) }, rx)
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
    use std::fs;
    use std::time::Duration;

    fn disk(name: &str, mount: &str) -> MountedDisk {
        MountedDisk {
            name: name.to_string(),
            mount_point: PathBuf::from(mount),
            available_bytes: 0,
        }
    }

    fn profile_named(ssd: &str) -> SyncProfile {
        let mut profile = SyncProfile::new("test");
        profile.ssd_name = ssd.to_string();
        profile.dest_path.clear();
        profile
    }

    #[test]
    fn matches_volume_name_or_mount_basename() {
        let by_name = disk("Extreme Pro", "/mnt/disk1");
        let by_base = disk("disk1s2", "/Volumes/Extreme Pro");
        let other = disk("Macintosh HD", "/Volumes/Macintosh HD");
        assert!(disk_matches("Extreme Pro", &by_name));
        assert!(disk_matches("Extreme Pro", &by_base));
        assert!(!disk_matches("Extreme Pro", &other));
        assert!(!disk_matches("", &by_base));
    }

    #[test]
    fn resolve_mount_prefers_cloud_then_disk() {
        let disks = vec![disk("Extreme Pro", "/Volumes/Extreme Pro")];
        let mut profile = profile_named("Extreme Pro");
        assert_eq!(
            resolve_mount(&profile, false, false, &disks),
            Some(PathBuf::from("/Volumes/Extreme Pro"))
        );

        profile.dest_path = "gdrive://My Folder".into();
        assert!(resolve_mount(&profile, false, false, &disks).is_none());
        assert_eq!(
            resolve_mount(&profile, true, false, &disks),
            Some(PathBuf::from("gdrive://root"))
        );
    }

    #[test]
    fn cloud_storage_uses_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("CloudStorage");
        fs::create_dir(&dest).unwrap();
        let mut profile = profile_named("nope");
        profile.dest_path = dest.to_string_lossy().into_owned();
        assert!(profile.is_cloud_storage());
        assert_eq!(
            resolve_mount(&profile, false, true, &[]),
            Some(dest.clone())
        );
        assert!(resolve_mount(&profile, false, false, &[]).is_none());
    }

    #[test]
    fn basename_match_ignores_parent_directory() {
        let disks = vec![disk("other", "/some/other/parent/Backups")];
        let profile = profile_named("Backups");
        assert_eq!(
            resolve_mount(&profile, false, false, &disks),
            Some(PathBuf::from("/some/other/parent/Backups"))
        );
    }

    #[test]
    fn mount_transition_is_per_profile_and_only_on_change() {
        let mount = Path::new("/Volumes/Extreme Pro");
        let alpha = Uuid::new_v4();
        let beta = Uuid::new_v4();
        assert_eq!(
            mount_transition(alpha, None, Some(mount), Some(10)),
            Some(VolumeEvent::Connected {
                profile_id: alpha,
                path: mount.to_path_buf(),
                free_bytes: Some(10),
            })
        );
        assert_eq!(
            mount_transition(beta, Some(mount), None, None),
            Some(VolumeEvent::Disconnected { profile_id: beta })
        );
        assert_eq!(mount_transition(alpha, Some(mount), Some(mount), Some(10)), None);
        assert_eq!(mount_transition(alpha, None, None, None), None);
        let other = Path::new("/Volumes/Other");
        assert_eq!(
            mount_transition(alpha, Some(mount), Some(other), None),
            Some(VolumeEvent::Connected {
                profile_id: alpha,
                path: other.to_path_buf(),
                free_bytes: None,
            })
        );
        assert_ne!(alpha, beta);
    }

    #[test]
    fn free_bytes_follow_the_matched_mount() {
        let mut extreme = disk("Extreme Pro", "/Volumes/Extreme Pro");
        extreme.available_bytes = 42;
        let disks = vec![extreme, disk("Other", "/Volumes/Other")];
        assert_eq!(
            free_bytes_at(&disks, Path::new("/Volumes/Extreme Pro")),
            Some(42)
        );
        assert_eq!(free_bytes_at(&disks, Path::new("/missing")), None);
    }

    #[test]
    fn wake_gap_skips_the_wait() {
        assert_eq!(
            poll_delay(Duration::from_secs(1), POLL_INTERVAL, WAKE_GAP),
            POLL_INTERVAL
        );
        assert_eq!(
            poll_delay(Duration::from_secs(11), POLL_INTERVAL, WAKE_GAP),
            Duration::ZERO
        );
        assert_eq!(
            poll_delay(WAKE_GAP, POLL_INTERVAL, WAKE_GAP),
            POLL_INTERVAL
        );
    }

    #[test]
    fn eject_commands_match_each_os() {
        let mount = Path::new("/Volumes/Extreme Pro");
        let mac = eject_commands(OsKind::Macos, mount, None);
        assert_eq!(mac[0].0, "diskutil");
        assert_eq!(mac[0].1, vec!["eject", "/Volumes/Extreme Pro"]);

        let linux = eject_commands(OsKind::Linux, mount, Some("/dev/sdb1"));
        assert_eq!(linux[0].0, "udisksctl");
        assert_eq!(linux[0].1, vec!["unmount", "-b", "/dev/sdb1"]);
        assert_eq!(linux[1].0, "umount");

        let linux_fallback = eject_commands(OsKind::Linux, Path::new("/mnt/share"), None);
        assert_eq!(linux_fallback.len(), 1);
        assert_eq!(linux_fallback[0].0, "umount");

        let win = eject_commands(OsKind::Windows, Path::new(r"E:\"), None);
        assert_eq!(win[0].0, "powershell");
        assert!(win[0].1[2].contains("ParseName('E:')"));
        assert!(win[0].1[2].contains("InvokeVerb('Eject')"));
    }

    struct ScriptedDisks {
        steps: Mutex<Vec<Vec<MountedDisk>>>,
    }

    impl DiskSource for ScriptedDisks {
        fn list(&mut self) -> Vec<MountedDisk> {
            let mut steps = lock(&self.steps);
            if steps.len() > 1 {
                steps.remove(0)
            } else {
                steps.first().cloned().unwrap_or_default()
            }
        }
    }

    #[test]
    fn monitor_emits_connect_then_disconnect() {
        let profile = profile_named("Extreme Pro");
        let id = profile.id;
        let state = Arc::new(Mutex::new(WatchState::new(profile)));
        let scripted = ScriptedDisks {
            steps: Mutex::new(vec![
                vec![],
                vec![disk("Extreme Pro", "/Volumes/Extreme Pro")],
                vec![],
            ]),
        };
        let timing = MonitorTiming {
            poll: Duration::from_millis(40),
            wake_gap: Duration::from_secs(10),
        };
        let (monitor, rx) = spawn_inner(state, Box::new(scripted), timing);
        let first = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            first,
            VolumeEvent::Connected {
                profile_id: id,
                path: PathBuf::from("/Volumes/Extreme Pro"),
                free_bytes: Some(0),
            }
        );
        let second = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(second, VolumeEvent::Disconnected { profile_id: id });
        drop(monitor);
    }

    #[test]
    fn monitor_tracks_each_profile_on_its_own() {
        let alpha = profile_named("Alpha");
        let beta = profile_named("Beta");
        let id_a = alpha.id;
        let id_b = beta.id;
        let state = Arc::new(Mutex::new(WatchState {
            profiles: vec![alpha, beta],
            google_signed_in: false,
        }));
        let scripted = ScriptedDisks {
            steps: Mutex::new(vec![
                vec![],
                vec![disk("Alpha", "/Volumes/Alpha")],
                vec![
                    disk("Alpha", "/Volumes/Alpha"),
                    disk("Beta", "/Volumes/Beta"),
                ],
                vec![],
            ]),
        };
        let timing = MonitorTiming {
            poll: Duration::from_millis(40),
            wake_gap: Duration::from_secs(10),
        };
        let (monitor, rx) = spawn_inner(state, Box::new(scripted), timing);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            VolumeEvent::Connected {
                profile_id: id_a,
                path: PathBuf::from("/Volumes/Alpha"),
                free_bytes: Some(0),
            }
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            VolumeEvent::Connected {
                profile_id: id_b,
                path: PathBuf::from("/Volumes/Beta"),
                free_bytes: Some(0),
            }
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            VolumeEvent::Disconnected { profile_id: id_a }
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            VolumeEvent::Disconnected { profile_id: id_b }
        );
        drop(monitor);
    }
}
