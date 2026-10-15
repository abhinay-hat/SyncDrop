//! Tauri 2 menu-bar shell. Sync work lives in `syncdrop-core`; this crate only
//! wires the tray, windows, and OS plugins.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use syncdrop_core::config::ConfigStore;
use syncdrop_core::engine::{self, EngineError};
use syncdrop_core::gdrive::{
    oauth_redirect_uri, GoogleDriveError, GoogleDriveSync, GoogleOAuth,
};
use syncdrop_core::nas::{self, AutoMountHandle};
use syncdrop_core::profile::{ProfileError, SyncProfile};
use syncdrop_core::progress::{State, SyncProgress};
use syncdrop_core::volumes::{self, VolumeEvent, VolumeMonitor, WatchState};
use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
#[cfg(target_os = "macos")]
use tauri::ActivationPolicy;
use tauri::{App, AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

/// Tray title: `idle`, `syncing NN%`, or `last sync <time>`.
fn tray_status_text(syncing: bool, percent: f64, last_sync: Option<&str>) -> String {
    if syncing {
        let pct = percent.round().clamp(0.0, 100.0) as i64;
        format!("syncing {pct}%")
    } else if let Some(last) = last_sync.filter(|s| !s.is_empty()) {
        format!("last sync {last}")
    } else {
        "idle".into()
    }
}

fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|home| !home.is_empty())
}

/// Store picked folders as `~/...` when they live under the home directory.
fn prefer_tilde(path: &str) -> String {
    prefer_tilde_with(path, home_dir().as_deref())
}

fn prefer_tilde_with(path: &str, home: Option<&str>) -> String {
    let Some(home) = home.filter(|home| !home.is_empty()) else {
        return path.to_string();
    };
    if path == home {
        return "~".into();
    }
    let prefix = format!("{home}/");
    if let Some(rest) = path.strip_prefix(&prefix) {
        format!("~/{rest}")
    } else {
        path.to_string()
    }
}

fn is_virtual_mount(path: &Path) -> bool {
    path.to_string_lossy().starts_with("gdrive:")
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct GoogleCache {
    signed_in: bool,
    email: String,
    folder: String,
    has_secret: bool,
}

impl Default for GoogleCache {
    fn default() -> Self {
        Self {
            signed_in: false,
            email: String::new(),
            folder: "SyncDrop".into(),
            has_secret: false,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryItem {
    id: String,
    date: String,
    profile_name: String,
    profile_id: String,
    file_count: i64,
    size: String,
    duration: String,
    succeeded: bool,
    at: i64,
    error: Option<String>,
}

#[derive(Clone)]
struct CachedConfig {
    profiles: Vec<SyncProfile>,
    active_profile_id: Uuid,
    notify_on_complete: bool,
    launch_at_login: bool,
    history: Vec<HistoryItem>,
    google_client_id: String,
}

impl CachedConfig {
    fn from_store(store: &ConfigStore) -> Self {
        Self {
            profiles: store.profiles().to_vec(),
            active_profile_id: store.active_profile_id(),
            notify_on_complete: store.notify_on_complete(),
            launch_at_login: store.launch_at_login(),
            history: store
                .sync_history()
                .iter()
                .map(|record| HistoryItem {
                    id: record.id.to_string(),
                    date: record.formatted_date(),
                    profile_name: record.profile_name.clone(),
                    profile_id: record.profile_id.clone(),
                    file_count: record.file_count,
                    size: record.formatted_size(),
                    duration: record.formatted_duration(),
                    succeeded: record.succeeded,
                    at: record.date.timestamp(),
                    error: record.error.clone(),
                })
                .collect(),
            google_client_id: store.google_client_id().to_string(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressView {
    state: String,
    detail: String,
    files_done: i64,
    files_total: i64,
    bytes_done: i64,
    bytes_total: i64,
    current_file: String,
    percent: f64,
    speed_bps: f64,
    eta_secs: Option<f64>,
    duration_secs: Option<f64>,
}

fn progress_view(progress: &SyncProgress) -> ProgressView {
    let (state, detail) = match &progress.state {
        State::Idle => ("idle", String::new()),
        State::Running => ("running", String::new()),
        State::Done => ("done", String::new()),
        State::Error(message) => ("error", message.clone()),
        State::Interrupted => ("interrupted", "Sync was interrupted".into()),
    };
    let (speed_bps, eta_secs) = progress_rates(progress);
    ProgressView {
        state: state.into(),
        detail,
        files_done: progress.files_done,
        files_total: progress.files_total,
        bytes_done: progress.bytes_done,
        bytes_total: progress.bytes_total,
        current_file: progress.current_file.clone(),
        percent: (progress.percent_complete() * 100.0).clamp(0.0, 100.0),
        speed_bps,
        eta_secs,
        duration_secs: progress.duration(),
    }
}

fn progress_rates(progress: &SyncProgress) -> (f64, Option<f64>) {
    let Some(start) = progress.start_time else {
        return (0.0, None);
    };
    let elapsed = (Utc::now() - start).num_milliseconds().max(1) as f64 / 1000.0;
    let speed = progress.bytes_done.max(0) as f64 / elapsed;
    let eta = if progress.bytes_total > progress.bytes_done && speed > 1.0 {
        Some((progress.bytes_total - progress.bytes_done) as f64 / speed)
    } else if progress.files_total > progress.files_done && progress.files_done > 0 {
        let file_rate = progress.files_done as f64 / elapsed;
        Some((progress.files_total - progress.files_done) as f64 / file_rate)
    } else {
        None
    };
    (speed, eta)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DryRunView {
    profile_id: String,
    copies: Vec<String>,
    updates: Vec<String>,
    deletes: Vec<String>,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileLive {
    id: Uuid,
    connected: bool,
    mount: String,
    free_bytes: Option<u64>,
    ejectable: bool,
}

#[derive(Clone)]
struct MountInfo {
    path: PathBuf,
    free_bytes: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UiState {
    profiles: Vec<SyncProfile>,
    active_profile_id: Uuid,
    notify_on_complete: bool,
    launch_at_login: bool,
    history: Vec<HistoryItem>,
    google_client_id: String,
    google_signed_in: bool,
    google_email: String,
    google_folder: String,
    google_has_secret: bool,
    connected: bool,
    mount: String,
    syncing: bool,
    syncing_profile_id: Option<Uuid>,
    previewing: bool,
    progress: ProgressView,
    dry_run: Option<DryRunView>,
    profiles_live: Vec<ProfileLive>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleStatusEvent {
    ok: bool,
    signed_in: bool,
    email: String,
    error: Option<String>,
}

struct AppState {
    store: Mutex<ConfigStore>,
    cache: Mutex<CachedConfig>,
    watch: Arc<Mutex<WatchState>>,
    monitor: Mutex<Option<VolumeMonitor>>,
    nas: Mutex<Option<AutoMountHandle>>,
    nas_profiles: Arc<Mutex<Vec<SyncProfile>>>,
    mounts: Mutex<HashMap<Uuid, MountInfo>>,
    sync_profile: Mutex<Option<Uuid>>,
    progress: Mutex<SyncProgress>,
    cancel: Mutex<Option<Arc<AtomicBool>>>,
    syncing: AtomicBool,
    previewing: AtomicBool,
    user_cancel: AtomicBool,
    menu_dirty: AtomicBool,
    google: Mutex<GoogleCache>,
    tray: Mutex<Option<TrayIcon>>,
    tray_title: Mutex<String>,
    dry_run: Mutex<Option<DryRunView>>,
}

impl AppState {
    fn open() -> Result<Self, String> {
        let store = ConfigStore::new().map_err(|e| e.to_string())?;
        let profiles = store.profiles().to_vec();
        let watch = Arc::new(Mutex::new(WatchState {
            profiles: profiles.clone(),
            google_signed_in: false,
        }));
        Ok(Self {
            cache: Mutex::new(CachedConfig::from_store(&store)),
            store: Mutex::new(store),
            watch,
            monitor: Mutex::new(None),
            nas: Mutex::new(None),
            nas_profiles: Arc::new(Mutex::new(profiles)),
            mounts: Mutex::new(HashMap::new()),
            sync_profile: Mutex::new(None),
            progress: Mutex::new(SyncProgress::new()),
            cancel: Mutex::new(None),
            syncing: AtomicBool::new(false),
            previewing: AtomicBool::new(false),
            user_cancel: AtomicBool::new(false),
            menu_dirty: AtomicBool::new(true),
            google: Mutex::new(GoogleCache::default()),
            tray: Mutex::new(None),
            tray_title: Mutex::new("idle".into()),
            dry_run: Mutex::new(None),
        })
    }
}

enum SyncEnd {
    Ok,
    Cancelled,
    Err(String),
}

pub fn run() {
    init_logging();
    log::info!("SyncDrop started");
    let state = match AppState::open() {
        Ok(state) => state,
        Err(err) => {
            eprintln!("SyncDrop could not open its config: {err}");
            std::process::exit(1);
        }
    };
    if let Err(err) = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .setup(|app| {
            wire(app).map_err(|err| -> Box<dyn std::error::Error> { err.into() })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            activate_profile,
            save_profile,
            add_profile,
            delete_profile,
            set_notify_on_complete,
            set_launch_at_login,
            set_nas_password,
            delete_nas_password,
            set_google_client_id,
            set_google_client_secret,
            set_google_folder,
            google_sign_in,
            google_sign_out,
            sync_now,
            dry_run,
            cancel_sync,
            eject,
            clear_history,
            open_external,
            close_window,
            prefer_home_path,
            get_recent_logs,
            open_logs_folder,
            app_version,
            google_redirect_uri
        ])
        .run(tauri::generate_context!())
    {
        eprintln!("SyncDrop failed: {err}");
        std::process::exit(1);
    }
}

fn wire(app: &mut App) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(ActivationPolicy::Accessory);

    let handle = app.handle().clone();
    let state = handle.state::<AppState>();
    refresh_google(&state);
    sync_targets(&state);
    remember(&state);

    let (monitor, rx) = VolumeMonitor::spawn(Arc::clone(&state.watch));
    *lock(&state.monitor) = Some(monitor);
    *lock(&state.nas) = Some(AutoMountHandle::spawn_profiles(Arc::clone(&state.nas_profiles)));

    spawn_volume_loop(handle.clone(), rx);
    build_tray(&handle)?;
    spawn_tray_pump(handle.clone());
    let enabled = lock(&state.store).launch_at_login();
    apply_autostart(&handle, enabled);
    Ok(())
}

fn refresh_google(state: &AppState) {
    let loaded = {
        let store = lock(&state.store);
        GoogleOAuth::load(&store)
    };
    let mut cache = lock(&state.google);
    match loaded {
        Ok(oauth) => {
            cache.signed_in = oauth.is_signed_in();
            cache.email = oauth.user_email().unwrap_or("").to_string();
            cache.folder = oauth.target_folder_name().to_string();
            cache.has_secret = !oauth.client_secret().is_empty();
        }
        Err(err) => {
            eprintln!("Google sign-in state: {err}");
            cache.signed_in = false;
        }
    }
}

fn sync_targets(state: &AppState) {
    let profiles = lock(&state.store).profiles().to_vec();
    let signed = lock(&state.google).signed_in;
    {
        let mut watch = lock(&state.watch);
        watch.profiles = profiles.clone();
        watch.google_signed_in = signed;
    }
    *lock(&state.nas_profiles) = profiles;
}

fn mount_info(state: &AppState, id: Uuid) -> Option<MountInfo> {
    lock(&state.mounts).get(&id).cloned()
}

fn profile_by_id(state: &AppState, id: Uuid) -> Result<SyncProfile, String> {
    lock(&state.store)
        .profiles()
        .iter()
        .find(|profile| profile.id == id)
        .cloned()
        .ok_or_else(|| "That profile is no longer here.".to_string())
}

fn remember(state: &AppState) -> CachedConfig {
    let cfg = CachedConfig::from_store(&lock(&state.store));
    *lock(&state.cache) = cfg.clone();
    cfg
}

fn config_snapshot(state: &AppState) -> CachedConfig {
    match state.store.try_lock() {
        Ok(store) => {
            let cfg = CachedConfig::from_store(&store);
            drop(store);
            *lock(&state.cache) = cfg.clone();
            cfg
        }
        Err(TryLockError::Poisoned(poisoned)) => {
            let cfg = CachedConfig::from_store(&poisoned.into_inner());
            *lock(&state.cache) = cfg.clone();
            cfg
        }
        Err(TryLockError::WouldBlock) => lock(&state.cache).clone(),
    }
}

fn publish_config(state: &AppState) {
    remember(state);
    sync_targets(state);
    state.menu_dirty.store(true, Ordering::SeqCst);
}

fn build_tray(app: &AppHandle) -> Result<(), String> {
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>).map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&quit]).map_err(|e| e.to_string())?;
    // Template tray is black+alpha so macOS tints it. Other desktops use the
    // colored app icon, which stays visible on light and dark taskbars.
    #[cfg(target_os = "macos")]
    let icon = tauri::include_image!("icons/tray.png");
    #[cfg(not(target_os = "macos"))]
    let icon = tauri::include_image!("icons/32x32.png");
    let builder = TrayIconBuilder::with_id("tray")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("idle")
        .on_menu_event(|app, event| handle_menu(app, &event.id().0));
    #[cfg(target_os = "macos")]
    let builder = builder.icon_as_template(true);
    let tray = builder.build(app).map_err(|e| e.to_string())?;
    *lock(&app.state::<AppState>().tray) = Some(tray);
    install_menu(app)
}

fn install_menu(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let cfg = config_snapshot(&state);
    let mount = mount_info(&state, cfg.active_profile_id);
    let connected = mount.is_some();
    let ejectable = mount.as_ref().is_some_and(|info| !is_virtual_mount(&info.path));
    let busy = state.syncing.load(Ordering::SeqCst);
    let progress = lock(&state.progress).clone();
    let running = matches!(progress.state, State::Running);
    let last = cfg.history.first().map(|item| item.date.clone());
    let title = tray_status_text(running, progress.percent_complete() * 100.0, last.as_deref());
    *lock(&state.tray_title) = title.clone();

    let mut profile_items = Vec::new();
    for profile in &cfg.profiles {
        let item = CheckMenuItem::with_id(
            app,
            format!("profile:{}", profile.id),
            &profile.name,
            true,
            profile.id == cfg.active_profile_id,
            None::<&str>,
        )
        .map_err(|e| e.to_string())?;
        profile_items.push(item);
    }
    let sync_now = MenuItem::with_id(app, "sync-now", "Sync Now", connected && !busy, None::<&str>)
        .map_err(|e| e.to_string())?;
    let dry = MenuItem::with_id(app, "dry-run", "Dry Run", connected && !busy, None::<&str>)
        .map_err(|e| e.to_string())?;
    let eject_item = MenuItem::with_id(app, "eject", "Eject", ejectable && !busy, None::<&str>)
        .map_err(|e| e.to_string())?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let launch = CheckMenuItem::with_id(
        app,
        "launch-login",
        "Launch at Login",
        true,
        cfg.launch_at_login,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>).map_err(|e| e.to_string())?;

    let mut builder = MenuBuilder::new(app);
    for item in &profile_items {
        builder = builder.item(item);
    }
    let menu = builder
        .separator()
        .item(&sync_now)
        .item(&dry)
        .item(&eject_item)
        .separator()
        .item(&settings)
        .item(&launch)
        .separator()
        .item(&quit)
        .build()
        .map_err(|e| e.to_string())?;

    {
        let tray = lock(&state.tray);
        if let Some(tray) = tray.as_ref() {
            tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
            let _ = tray.set_tooltip(Some(&title));
            #[cfg(target_os = "macos")]
            let _ = tray.set_title(Some(&title));
        }
    }
    state.menu_dirty.store(false, Ordering::SeqCst);
    Ok(())
}

fn apply_tray_title(app: &AppHandle, title: &str) {
    let state = app.state::<AppState>();
    let tray = lock(&state.tray);
    if let Some(tray) = tray.as_ref() {
        let _ = tray.set_tooltip(Some(title));
        #[cfg(target_os = "macos")]
        let _ = tray.set_title(Some(title));
    }
}

fn spawn_tray_pump(app: AppHandle) {
    std::thread::Builder::new()
        .name("syncdrop-tray".into())
        .spawn(move || {
            let mut shown = String::new();
            loop {
                std::thread::sleep(Duration::from_millis(200));
                let state = app.state::<AppState>();
                let title = lock(&state.tray_title).clone();
                let dirty = state.menu_dirty.load(Ordering::SeqCst);
                if !dirty && title == shown {
                    continue;
                }
                let app2 = app.clone();
                let title2 = title.clone();
                if app
                    .run_on_main_thread(move || {
                        if dirty {
                            let _ = install_menu(&app2);
                        } else {
                            apply_tray_title(&app2, &title2);
                        }
                    })
                    .is_err()
                {
                    break;
                }
                shown = title;
            }
        })
        .ok();
}

fn handle_menu(app: &AppHandle, id: &str) {
    match id {
        "sync-now" => {
            let profile = lock(&app.state::<AppState>().store).active_profile().clone();
            if profile.mirror_mode {
                let _ = start_dry_run(app, profile.id);
            } else {
                let _ = start_sync(app, profile.id);
            }
        }
        "dry-run" => {
            let id = lock(&app.state::<AppState>().store).active_profile_id();
            let _ = start_dry_run(app, id);
        }
        "eject" => {
            let id = lock(&app.state::<AppState>().store).active_profile_id();
            let _ = eject_mounted(app, id);
        }
        "settings" => show_settings(app),
        "launch-login" => {
            let enabled = !config_snapshot(&app.state::<AppState>()).launch_at_login;
            let _ = set_login(app, enabled);
        }
        "quit" => {
            app.exit(0);
            return;
        }
        other => {
            if let Some(id) = other.strip_prefix("profile:") {
                let _ = activate(app, id);
            }
        }
    }
    let _ = install_menu(app);
}

fn spawn_volume_loop(app: AppHandle, rx: Receiver<VolumeEvent>) {
    std::thread::Builder::new()
        .name("syncdrop-volume-ui".into())
        .spawn(move || {
            while let Ok(event) = rx.recv() {
                let app2 = app.clone();
                if app
                    .run_on_main_thread(move || on_volume(&app2, event))
                    .is_err()
                {
                    break;
                }
            }
        })
        .ok();
}

fn on_volume(app: &AppHandle, event: VolumeEvent) {
    let state = app.state::<AppState>();
    match event {
        VolumeEvent::Connected {
            profile_id,
            path,
            free_bytes,
        } => {
            lock(&state.mounts).insert(
                profile_id,
                MountInfo {
                    path: path.clone(),
                    free_bytes,
                },
            );
            let profile = lock(&state.store)
                .profiles()
                .iter()
                .find(|profile| profile.id == profile_id)
                .cloned();
            if let Some(profile) = profile {
                let needs_folder = profile.dest_path.trim().is_empty() && !profile.is_google_drive_cloud();
                if needs_folder {
                    show_settings(app);
                } else if profile.auto_sync {
                    let _ = start_sync(app, profile.id);
                    show_popup(app);
                }
            }
        }
        VolumeEvent::Disconnected { profile_id } => {
            lock(&state.mounts).remove(&profile_id);
            if *lock(&state.sync_profile) == Some(profile_id) {
                state.user_cancel.store(false, Ordering::SeqCst);
                if let Some(flag) = lock(&state.cancel).as_ref() {
                    flag.store(true, Ordering::SeqCst);
                }
            }
        }
    }
    let _ = install_menu(app);
    emit_ui(app);
}

fn show_settings(app: &AppHandle) {
    show_page(app, "settings", "index.html", "SyncDrop", 980.0, 720.0);
}

fn show_popup(app: &AppHandle) {
    show_page(app, "popup", "popup.html", "SyncDrop", 440.0, 460.0);
}

fn show_page(app: &AppHandle, label: &str, url: &str, title: &str, width: f64, height: f64) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    let (min_w, min_h) = if label == "settings" {
        (860.0, 560.0)
    } else {
        (380.0, 320.0)
    };
    let _ = WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(width, height)
        .min_inner_size(min_w, min_h)
        .resizable(true)
        .focused(true)
        .visible(true)
        .build();
}

fn notify_msg(app: &AppHandle, body: &str) {
    let _ = app.notification().builder().title("SyncDrop").body(body).show();
}

fn apply_autostart(app: &AppHandle, enabled: bool) {
    let result = if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    };
    if let Err(err) = result {
        // Expected when the app is not installed in /Applications.
        eprintln!("Launch at login: {err}");
    }
}

fn note_running(state: &AppState, progress: SyncProgress) {
    let percent = progress.percent_complete() * 100.0;
    *lock(&state.tray_title) = tray_status_text(true, percent, None);
    *lock(&state.progress) = progress;
}

fn emit_progress(app: &AppHandle, progress: &SyncProgress) {
    let _ = app.emit("sync-progress", progress_view(progress));
}

fn emit_ui(app: &AppHandle) {
    let _ = app.emit("ui-state", ());
}

fn report_files(app: &AppHandle, done: i64, total: i64, bytes_done: i64, bytes_total: i64, path: &str) {
    let state = app.state::<AppState>();
    let mut progress = lock(&state.progress).clone();
    let start = progress.start_time.unwrap_or_else(Utc::now);
    progress.state = State::Running;
    progress.files_done = done;
    progress.files_total = total;
    progress.bytes_done = bytes_done;
    progress.bytes_total = bytes_total;
    progress.current_file = path.to_string();
    progress.start_time = Some(start);
    progress.end_time = None;
    note_running(&state, progress.clone());
    emit_progress(app, &progress);
}

fn start_sync(app: &AppHandle, profile_id: Uuid) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.syncing.load(Ordering::SeqCst) {
        show_popup(app);
        return Ok(());
    }
    let profile = profile_by_id(&state, profile_id)?;
    if let Err(issues) = profile.validate() {
        let message = issues
            .first()
            .map(ToString::to_string)
            .unwrap_or_else(|| "Profile is incomplete.".into());
        notify_msg(app, &message);
        show_settings(app);
        return Err(issues_json(issues));
    }
    if state.syncing.swap(true, Ordering::SeqCst) {
        show_popup(app);
        return Ok(());
    }
    let notify = lock(&state.store).notify_on_complete();
    let mount = mount_info(&state, profile.id).map(|info| info.path);
    log::info!("sync started profile={}", profile.name);
    let cancel = Arc::new(AtomicBool::new(false));
    *lock(&state.cancel) = Some(Arc::clone(&cancel));
    *lock(&state.sync_profile) = Some(profile.id);
    state.user_cancel.store(false, Ordering::SeqCst);
    state.previewing.store(false, Ordering::SeqCst);
    *lock(&state.dry_run) = None;
    let mut progress = SyncProgress::new();
    progress.state = State::Running;
    progress.start_time = Some(Utc::now());
    note_running(&state, progress.clone());
    emit_progress(app, &progress);
    state.menu_dirty.store(true, Ordering::SeqCst);

    let app2 = app.clone();
    let cloud = profile.is_google_drive_cloud();
    let auto_eject = profile.auto_eject;
    let profile_name = profile.name.clone();
    let finished_id = profile.id;
    if std::thread::Builder::new()
        .name("syncdrop-sync".into())
        .spawn(move || {
            let end = if cloud {
                run_drive(&app2, &profile, &cancel)
            } else {
                run_local(&app2, &profile, &cancel)
            };
            finish_sync(
                &app2,
                end,
                notify,
                auto_eject,
                mount,
                cloud,
                finished_id,
                profile_name,
            );
        })
        .is_err()
    {
        state.syncing.store(false, Ordering::SeqCst);
        *lock(&state.sync_profile) = None;
        return Err("Could not start sync.".into());
    }
    show_popup(app);
    Ok(())
}

fn run_local(app: &AppHandle, profile: &SyncProfile, cancel: &AtomicBool) -> SyncEnd {
    let dest = PathBuf::from(profile.expanded_dest_path());
    if let Err(err) = std::fs::create_dir_all(&dest) {
        return SyncEnd::Err(format!(
            "Cannot create destination directory at '{}': {err}",
            dest.display()
        ));
    }
    match engine::sync(
        profile,
        &dest,
        |done, total, bytes_done, bytes_total, path| {
            report_files(app, done, total, bytes_done, bytes_total, path);
        },
        cancel,
    ) {
        Ok(mut record) => {
            record.profile_name = profile.name.clone();
            record.profile_id = profile.id.to_string();
            if let Err(err) = lock(&app.state::<AppState>().store).append_sync_record(record) {
                SyncEnd::Err(err.to_string())
            } else {
                SyncEnd::Ok
            }
        }
        Err(EngineError::Cancelled) => SyncEnd::Cancelled,
        Err(err) => SyncEnd::Err(err.to_string()),
    }
}

fn run_drive(app: &AppHandle, profile: &SyncProfile, early_cancel: &AtomicBool) -> SyncEnd {
    let state = app.state::<AppState>();
    let mut engine = {
        let store = lock(&state.store);
        let oauth = match GoogleOAuth::load(&store) {
            Ok(oauth) => oauth,
            Err(err) => return SyncEnd::Err(err.to_string()),
        };
        match GoogleDriveSync::new(oauth) {
            Ok(engine) => engine,
            Err(err) => return SyncEnd::Err(err.to_string()),
        }
    };
    let flag = engine.cancel_flag();
    if early_cancel.load(Ordering::SeqCst) {
        flag.store(true, Ordering::SeqCst);
    }
    *lock(&state.cancel) = Some(flag);
    let mut store = lock(&state.store);
    match engine.run(&mut store, profile, &|done, total, bytes_done, bytes_total, path| {
        report_files(app, done, total, bytes_done, bytes_total, path);
    }) {
        Ok(()) => SyncEnd::Ok,
        Err(GoogleDriveError::Interrupted) => SyncEnd::Cancelled,
        Err(err) => SyncEnd::Err(err.to_string()),
    }
}

fn finish_sync(
    app: &AppHandle,
    end: SyncEnd,
    notify: bool,
    auto_eject: bool,
    mount: Option<PathBuf>,
    cloud: bool,
    profile_id: Uuid,
    profile_name: String,
) {
    let state = app.state::<AppState>();
    state.syncing.store(false, Ordering::SeqCst);
    *lock(&state.sync_profile) = None;
    let files = lock(&state.progress).files_done;
    match end {
        SyncEnd::Ok => {
            {
                let mut progress = lock(&state.progress);
                progress.state = State::Done;
                progress.end_time = Some(Utc::now());
                log::info!(
                    "sync finished files={} bytes={}",
                    progress.files_done,
                    progress.bytes_done
                );
            }
            if notify {
                let body = if cloud {
                    format!("Google Drive sync complete — {files} files")
                } else {
                    format!("Sync complete — {files} files")
                };
                notify_msg(app, &body);
            }
            if auto_eject {
                if let Some(path) = mount.filter(|path| !is_virtual_mount(path)) {
                    if volumes::eject(&path).is_err() {
                        notify_msg(app, "Could not eject — check Finder");
                    } else {
                        lock(&state.mounts).remove(&profile_id);
                    }
                }
            }
        }
        SyncEnd::Cancelled => {
            {
                let mut progress = lock(&state.progress);
                progress.state = State::Interrupted;
                progress.end_time = Some(Utc::now());
            }
            log::info!("sync cancelled");
            if !state.user_cancel.load(Ordering::SeqCst) {
                notify_msg(app, "Sync was interrupted");
            }
        }
        SyncEnd::Err(message) => {
            log::error!("sync failed: {message}");
            let duration = {
                let mut progress = lock(&state.progress);
                progress.state = State::Error(message.clone());
                progress.end_time = Some(Utc::now());
                progress.duration().unwrap_or(0.0)
            };
            let bytes = lock(&state.progress).bytes_done;
            let mut record = syncdrop_core::progress::SyncRecord::new(
                Utc::now(),
                files,
                bytes,
                duration,
                false,
            );
            record.profile_name = profile_name;
            record.profile_id = profile_id.to_string();
            record.error = Some(message.clone());
            if let Err(err) = lock(&state.store).append_sync_record(record) {
                log::error!("could not store the failed sync: {err}");
            }
            notify_msg(app, &message);
        }
    }
    remember(&state);
    let last = lock(&state.cache).history.first().map(|item| item.date.clone());
    *lock(&state.tray_title) = tray_status_text(false, 0.0, last.as_deref());
    state.menu_dirty.store(true, Ordering::SeqCst);
    let progress = lock(&state.progress).clone();
    emit_progress(app, &progress);
}

fn start_dry_run(app: &AppHandle, profile_id: Uuid) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.syncing.load(Ordering::SeqCst) {
        return Err("A sync is already running.".into());
    }
    let profile = profile_by_id(&state, profile_id)?;
    if let Err(issues) = profile.validate() {
        show_settings(app);
        return Err(issues_json(issues));
    }
    let connected = mount_info(&state, profile.id).is_some();
    if !connected && !profile.is_google_drive_cloud() {
        notify_msg(app, "Connect the destination volume first.");
        return Err("Connect the destination volume first.".into());
    }
    if state.syncing.swap(true, Ordering::SeqCst) {
        return Err("A sync is already running.".into());
    }
    log::info!("dry run started profile={}", profile.name);
    state.previewing.store(true, Ordering::SeqCst);
    *lock(&state.sync_profile) = Some(profile.id);
    *lock(&state.dry_run) = None;
    show_popup(app);
    let app2 = app.clone();
    let profile_key = profile.id.to_string();
    if std::thread::Builder::new()
        .name("syncdrop-dry-run".into())
        .spawn(move || {
            let view = match dry_plan(&profile) {
                Ok(plan) => DryRunView {
                    profile_id: profile_key,
                    copies: plan.copies,
                    updates: plan.updates,
                    deletes: plan.deletes,
                    error: None,
                },
                Err(err) => DryRunView {
                    profile_id: profile_key,
                    copies: Vec::new(),
                    updates: Vec::new(),
                    deletes: Vec::new(),
                    error: Some(err),
                },
            };
            let state = app2.state::<AppState>();
            log::info!(
                "dry run finished copies={} updates={} deletes={} error={}",
                view.copies.len(),
                view.updates.len(),
                view.deletes.len(),
                view.error.is_some()
            );
            *lock(&state.dry_run) = Some(view.clone());
            state.previewing.store(false, Ordering::SeqCst);
            state.syncing.store(false, Ordering::SeqCst);
            *lock(&state.sync_profile) = None;
            state.menu_dirty.store(true, Ordering::SeqCst);
            let _ = app2.emit("dry-run", &view);
        })
        .is_err()
    {
        state.syncing.store(false, Ordering::SeqCst);
        state.previewing.store(false, Ordering::SeqCst);
        *lock(&state.sync_profile) = None;
        return Err("Could not start dry run.".into());
    }
    Ok(())
}

fn dry_plan(profile: &SyncProfile) -> Result<syncdrop_core::engine::Plan, String> {
    if profile.is_google_drive_cloud() {
        return Err(
            "Direct Google Drive sync has no local preview. Sync Now uploads changed files."
                .into(),
        );
    }
    let dest = PathBuf::from(profile.expanded_dest_path());
    engine::dry_run(profile, &dest).map_err(|err| err.to_string())
}

fn eject_mounted(app: &AppHandle, profile_id: Uuid) -> Result<(), String> {
    let state = app.state::<AppState>();
    let path = mount_info(&state, profile_id)
        .map(|info| info.path)
        .ok_or_else(|| "No volume is connected.".to_string())?;
    if is_virtual_mount(&path) {
        return Err("This destination is not a removable volume.".into());
    }
    volumes::eject(&path).map_err(|err| err.to_string())?;
    lock(&state.mounts).remove(&profile_id);
    state.menu_dirty.store(true, Ordering::SeqCst);
    Ok(())
}

fn activate(app: &AppHandle, id: &str) -> Result<(), String> {
    let id = Uuid::parse_str(id).map_err(|err| err.to_string())?;
    let state = app.state::<AppState>();
    lock(&state.store)
        .set_active_profile_id(id)
        .map_err(|err| err.to_string())?;
    publish_config(&state);
    Ok(())
}

fn set_login(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    lock(&state.store)
        .set_launch_at_login(enabled)
        .map_err(|err| err.to_string())?;
    apply_autostart(app, enabled);
    publish_config(&state);
    Ok(())
}

fn parse_id(id: String) -> Result<Uuid, String> {
    Uuid::parse_str(&id).map_err(|err| err.to_string())
}

fn ui_state(state: &AppState) -> UiState {
    let cfg = config_snapshot(state);
    let google = lock(&state.google);
    let mounts = lock(&state.mounts).clone();
    let progress = lock(&state.progress).clone();
    let active = mounts.get(&cfg.active_profile_id);
    let profiles_live = cfg
        .profiles
        .iter()
        .map(|profile| {
            let info = mounts.get(&profile.id);
            let mount = info
                .map(|info| info.path.display().to_string())
                .unwrap_or_default();
            let virtual_mount = mount.starts_with("gdrive:");
            ProfileLive {
                id: profile.id,
                connected: info.is_some(),
                mount: mount.clone(),
                free_bytes: info.and_then(|info| info.free_bytes),
                ejectable: info.is_some() && !virtual_mount,
            }
        })
        .collect();
    UiState {
        profiles: cfg.profiles,
        active_profile_id: cfg.active_profile_id,
        notify_on_complete: cfg.notify_on_complete,
        launch_at_login: cfg.launch_at_login,
        history: cfg.history,
        google_client_id: cfg.google_client_id,
        google_signed_in: google.signed_in,
        google_email: google.email.clone(),
        google_folder: google.folder.clone(),
        google_has_secret: google.has_secret,
        connected: active.is_some(),
        mount: active
            .map(|info| info.path.display().to_string())
            .unwrap_or_default(),
        syncing: state.syncing.load(Ordering::SeqCst),
        syncing_profile_id: *lock(&state.sync_profile),
        previewing: state.previewing.load(Ordering::SeqCst),
        progress: progress_view(&progress),
        dry_run: lock(&state.dry_run).clone(),
        profiles_live,
    }
}

#[tauri::command]
fn get_state(state: tauri::State<AppState>) -> UiState {
    ui_state(&state)
}

#[tauri::command]
fn activate_profile(app: AppHandle, id: String) -> Result<(), String> {
    activate(&app, &id)
}

#[tauri::command]
fn save_profile(app: AppHandle, profile: SyncProfile) -> Result<(), String> {
    if let Err(issues) = profile.validate() {
        return Err(issues_json(issues));
    }
    let state = app.state::<AppState>();
    lock(&state.store)
        .set_active_profile(profile)
        .map_err(|err| err.to_string())?;
    publish_config(&state);
    Ok(())
}

#[tauri::command]
fn add_profile(app: AppHandle, name: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    let id = {
        let mut store = lock(&state.store);
        store.add_profile(name).map_err(|err| err.to_string())?;
        store
            .profiles()
            .last()
            .map(|profile| profile.id)
            .ok_or_else(|| "profile was not added".to_string())?
    };
    publish_config(&state);
    Ok(id.to_string())
}

#[tauri::command]
fn delete_profile(app: AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    lock(&state.store)
        .delete_profile(parse_id(id)?)
        .map_err(|err| err.to_string())?;
    publish_config(&state);
    Ok(())
}

#[tauri::command]
fn set_notify_on_complete(app: AppHandle, value: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    lock(&state.store)
        .set_notify_on_complete(value)
        .map_err(|err| err.to_string())?;
    publish_config(&state);
    Ok(())
}

#[tauri::command]
fn set_launch_at_login(app: AppHandle, value: bool) -> Result<(), String> {
    set_login(&app, value)
}

#[tauri::command]
fn set_nas_password(profile_id: String, password: String) -> Result<(), String> {
    let id = parse_id(profile_id)?;
    if password.trim().is_empty() {
        nas::delete_password(id).map_err(|err| err.to_string())?;
    } else {
        nas::save_password(id, password.trim()).map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn delete_nas_password(profile_id: String) -> Result<(), String> {
    nas::delete_password(parse_id(profile_id)?).map_err(|err| err.to_string())
}

#[tauri::command]
fn set_google_client_id(app: AppHandle, value: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    lock(&state.store)
        .set_google_client_id(value.trim())
        .map_err(|err| err.to_string())?;
    publish_config(&state);
    Ok(())
}

#[tauri::command]
fn set_google_client_secret(app: AppHandle, value: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let store = lock(&state.store);
        let mut oauth = GoogleOAuth::load(&store).map_err(|err| err.to_string())?;
        drop(store);
        oauth.set_client_secret(value).map_err(|err| err.to_string())?;
        log::info!("google client secret updated");
        let mut cache = lock(&state.google);
        cache.has_secret = !oauth.client_secret().is_empty();
    }
    Ok(())
}

#[tauri::command]
fn set_google_folder(app: AppHandle, value: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let store = lock(&state.store);
    let mut oauth = GoogleOAuth::load(&store).map_err(|err| err.to_string())?;
    drop(store);
    oauth
        .set_target_folder_name(value)
        .map_err(|err| err.to_string())?;
    lock(&state.google).folder = oauth.target_folder_name().to_string();
    Ok(())
}

#[tauri::command]
fn google_sign_in(app: AppHandle) {
    std::thread::Builder::new()
        .name("syncdrop-google-auth".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            log::info!("google sign-in started");
            let outcome = (|| {
                let mut oauth = {
                    let store = lock(&state.store);
                    GoogleOAuth::load(&store).map_err(|err| err.to_string())?
                };
                oauth.sign_in().map_err(|err| err.to_string())?;
                let mut cache = lock(&state.google);
                cache.signed_in = oauth.is_signed_in();
                cache.email = oauth.user_email().unwrap_or("").to_string();
                cache.folder = oauth.target_folder_name().to_string();
                cache.has_secret = !oauth.client_secret().is_empty();
                Ok::<(), String>(())
            })();
            match &outcome {
                Ok(()) => log::info!("google sign-in finished"),
                Err(err) => log::warn!("google sign-in failed: {}", redact_secret(err)),
            }
            sync_targets(&state);
            state.menu_dirty.store(true, Ordering::SeqCst);
            let google = lock(&state.google);
            let _ = app.emit(
                "google-status",
                GoogleStatusEvent {
                    ok: outcome.is_ok(),
                    signed_in: google.signed_in,
                    email: google.email.clone(),
                    error: outcome.err(),
                },
            );
        })
        .ok();
}

#[tauri::command]
fn google_sign_out(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let store = lock(&state.store);
        let mut oauth = GoogleOAuth::load(&store).map_err(|err| err.to_string())?;
        drop(store);
        oauth.sign_out().map_err(|err| err.to_string())?;
    }
    log::info!("google signed out");
    refresh_google(&state);
    sync_targets(&state);
    state.menu_dirty.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn sync_now(app: AppHandle, profile_id: String, confirmed: bool) -> Result<(), String> {
    let id = parse_id(profile_id)?;
    let mirror = profile_by_id(&app.state::<AppState>(), id)?.mirror_mode;
    if mirror && !confirmed {
        start_dry_run(&app, id)
    } else {
        start_sync(&app, id)
    }
}

#[tauri::command]
fn dry_run(app: AppHandle, profile_id: String) -> Result<(), String> {
    start_dry_run(&app, parse_id(profile_id)?)
}

#[tauri::command]
fn cancel_sync(state: tauri::State<AppState>) {
    state.user_cancel.store(true, Ordering::SeqCst);
    if let Some(flag) = lock(&state.cancel).as_ref() {
        flag.store(true, Ordering::SeqCst);
    }
}

#[tauri::command]
fn eject(app: AppHandle, profile_id: String) -> Result<(), String> {
    eject_mounted(&app, parse_id(profile_id)?)
}

#[tauri::command]
fn clear_history(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    lock(&state.store)
        .clear_sync_history()
        .map_err(|err| err.to_string())?;
    publish_config(&state);
    let last = lock(&state.cache).history.first().map(|item| item.date.clone());
    let running = matches!(lock(&state.progress).state, State::Running);
    let percent = lock(&state.progress).percent_complete() * 100.0;
    *lock(&state.tray_title) = tray_status_text(running, percent, last.as_deref());
    Ok(())
}

#[tauri::command]
fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("only https URLs can be opened".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn close_window(window: WebviewWindow) {
    let _ = window.close();
}

/// Folder picker paths come back absolute. Collapse the home prefix so profiles
/// stay portable, matching the Swift settings panel.
#[tauri::command]
fn prefer_home_path(path: String) -> String {
    prefer_tilde(&path)
}

#[tauri::command]
fn get_recent_logs(limit: usize) -> Vec<String> {
    recent_log_lines(limit)
}

#[tauri::command]
fn open_logs_folder(app: AppHandle) -> Result<(), String> {
    let dir = log_dir();
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    app.opener()
        .open_path(dir.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
fn google_redirect_uri() -> String {
    oauth_redirect_uri()
}

fn issues_json(issues: Vec<ProfileError>) -> String {
    let rows: Vec<FieldIssue> = issues
        .iter()
        .map(|issue| FieldIssue {
            field: issue.field(),
            message: issue.to_string(),
        })
        .collect();
    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
}

#[derive(Serialize)]
struct FieldIssue {
    field: &'static str,
    message: String,
}

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

fn log_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("SyncDrop")
        .join("logs")
}

fn init_logging() {
    use tracing_subscriber::prelude::*;
    let dir = log_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let appender = match tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("syncdrop")
        .max_log_files(7)
        .build(&dir)
    {
        Ok(appender) => appender,
        Err(err) => {
            eprintln!("SyncDrop logs: {err}");
            return;
        }
    };
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let _ = LOG_GUARD.set(guard);
    let _ = tracing_log::LogTracer::init();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let subscriber = tracing_subscriber::registry().with(filter).with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(writer),
    );
    let _ = tracing::subscriber::set_global_default(subscriber);
}

fn recent_log_lines(limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let limit = limit.min(2000);
    let mut files = std::fs::read_dir(log_dir())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    files.sort();
    let mut lines = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            if !line.trim().is_empty() {
                lines.push(line.to_string());
            }
        }
    }
    if lines.len() > limit {
        lines.split_off(lines.len() - limit)
    } else {
        lines
    }
}

/// Drop token-shaped failures before they reach the log file.
fn redact_secret(err: &str) -> String {
    let lower = err.to_ascii_lowercase();
    if lower.contains("token")
        || lower.contains("secret")
        || lower.contains("bearer")
        || lower.contains("ya29.")
    {
        "authorization failed".into()
    } else {
        err.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_status_cycles_through_idle_syncing_and_last_sync() {
        assert_eq!(tray_status_text(false, 0.0, None), "idle");
        assert_eq!(tray_status_text(false, 0.0, Some("")), "idle");
        assert_eq!(tray_status_text(true, 42.2, None), "syncing 42%");
        assert_eq!(
            tray_status_text(false, 0.0, Some("9/9/01, 1:46 AM")),
            "last sync 9/9/01, 1:46 AM"
        );
        assert_eq!(tray_status_text(true, 140.0, Some("ignored")), "syncing 100%");
    }

    #[test]
    fn prefer_tilde_collapses_the_home_prefix() {
        let home = Some("/Users/example");
        assert_eq!(prefer_tilde_with("/Users/example", home), "~");
        assert_eq!(prefer_tilde_with("/Users/example/Desktop", home), "~/Desktop");
        assert_eq!(prefer_tilde_with("/Volumes/Extreme", home), "/Volumes/Extreme");
        assert_eq!(prefer_tilde_with("/tmp/x", None), "/tmp/x");
    }

    #[test]
    fn full_sync_and_dry_run_between_temp_dirs() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("note.txt"), b"one").unwrap();
        std::fs::create_dir(src.path().join("keep")).unwrap();
        std::fs::write(src.path().join("keep").join("b.txt"), b"two").unwrap();

        let mut profile = SyncProfile::new("Integration");
        profile.source_path = src.path().display().to_string();
        profile.dest_path = dst.path().display().to_string();
        profile.ssd_name = "TestDrive".into();
        profile.mirror_mode = true;
        profile.validate().expect("profile is ready to sync");

        let plan = dry_plan(&profile).unwrap();
        assert!(plan.copies.iter().any(|path| path == "note.txt"));
        assert!(plan.copies.iter().any(|path| path == "keep/b.txt"));
        assert!(plan.deletes.is_empty());

        let cancel = AtomicBool::new(false);
        let dest = PathBuf::from(profile.expanded_dest_path());
        let record = engine::sync(&profile, &dest, |_, _, _, _, _| {}, &cancel).unwrap();
        assert!(record.succeeded);
        assert_eq!(std::fs::read(dst.path().join("note.txt")).unwrap(), b"one");

        std::fs::write(src.path().join("note.txt"), b"changed").unwrap();
        let updates = dry_plan(&profile).unwrap();
        assert!(updates.updates.iter().any(|path| path == "note.txt"));
        let record = engine::sync(&profile, &dest, |_, _, _, _, _| {}, &cancel).unwrap();
        assert!(record.succeeded);
        assert_eq!(std::fs::read(dst.path().join("note.txt")).unwrap(), b"changed");

        std::fs::write(dst.path().join("gone.txt"), b"x").unwrap();
        let deletes = dry_plan(&profile).unwrap();
        assert!(deletes.deletes.iter().any(|path| path == "gone.txt"));
        let record = engine::sync(&profile, &dest, |_, _, _, _, _| {}, &cancel).unwrap();
        assert!(record.succeeded);
        assert!(!dst.path().join("gone.txt").exists());
        assert_eq!(std::fs::read(dst.path().join("keep").join("b.txt")).unwrap(), b"two");
    }
}
