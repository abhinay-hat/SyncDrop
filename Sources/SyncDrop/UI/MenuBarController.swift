import AppKit
import Combine
import SyncDropCore

@MainActor
class MenuBarController {
    private var statusItem: NSStatusItem?
    private let configStore: ConfigStore
    private let syncEngine: SyncEngine
    private let volumeMonitor: VolumeMonitor
    private var cancellables = Set<AnyCancellable>()
    private var popupWindow: SyncPopupWindow?
    private var settingsWindowController: SettingsWindowController?

    private var statusMenuItem: NSMenuItem?
    private var lastSyncMenuItem: NSMenuItem?
    private var syncNowMenuItem: NSMenuItem?
    private var profileMenuItem: NSMenuItem?

    init(configStore: ConfigStore, syncEngine: SyncEngine, volumeMonitor: VolumeMonitor) {
        self.configStore = configStore
        self.syncEngine = syncEngine
        self.volumeMonitor = volumeMonitor
    }

    func setup() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem?.button?.image = menuBarImage(for: .idle)
        buildMenu()
        observeChanges()
    }

    func showSyncPopup() {
        if popupWindow == nil {
            popupWindow = SyncPopupWindow(configStore: configStore, syncEngine: syncEngine)
        }
        popupWindow?.showPopup()
    }

    func openSettings() {
        if settingsWindowController == nil {
            settingsWindowController = SettingsWindowController(
                configStore: configStore,
                syncEngine: syncEngine
            )
        }
        settingsWindowController?.showWindow()
    }

    private func buildMenu() {
        let m = NSMenu()

        let si = NSMenuItem(title: "○ \(configStore.activeProfile.ssdName) — Not connected", action: nil, keyEquivalent: "")
        si.isEnabled = false
        statusMenuItem = si
        m.addItem(si)

        let ls = NSMenuItem(title: "Last sync: never", action: nil, keyEquivalent: "")
        ls.isEnabled = false
        lastSyncMenuItem = ls
        m.addItem(ls)

        m.addItem(.separator())

        let profileItem = NSMenuItem(title: "Switch Profile", action: nil, keyEquivalent: "")
        let profileSubmenu = NSMenu()
        profileItem.submenu = profileSubmenu
        profileMenuItem = profileItem
        m.addItem(profileItem)
        rebuildProfileSubmenu()

        m.addItem(.separator())

        let sn = NSMenuItem(title: "Sync Now", action: #selector(syncNowTapped), keyEquivalent: "")
        sn.target = self
        sn.isEnabled = false
        syncNowMenuItem = sn
        m.addItem(sn)

        m.addItem(.separator())

        let settings = NSMenuItem(title: "Settings…", action: #selector(openSettingsTapped), keyEquivalent: ",")
        settings.target = self
        m.addItem(settings)

        m.addItem(.separator())

        let quit = NSMenuItem(title: "Quit SyncDrop", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        m.addItem(quit)

        statusItem?.menu = m
    }

    private func observeChanges() {
        volumeMonitor.$ssdConnected
            .receive(on: DispatchQueue.main)
            .sink { [weak self] connected in self?.updateForConnection(connected) }
            .store(in: &cancellables)

        syncEngine.$progress
            .receive(on: DispatchQueue.main)
            .sink { [weak self] p in self?.updateForProgress(p) }
            .store(in: &cancellables)

        configStore.$activeProfileId
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in
                guard let self else { return }
                self.rebuildProfileSubmenu()
                self.updateForConnection(self.volumeMonitor.ssdConnected)
            }
            .store(in: &cancellables)

        configStore.$profiles
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in self?.rebuildProfileSubmenu() }
            .store(in: &cancellables)
    }

    private func updateForConnection(_ connected: Bool) {
        statusMenuItem?.title = connected
            ? "● \(configStore.activeProfile.ssdName) — Connected"
            : "○ \(configStore.activeProfile.ssdName) — Not connected"
        syncNowMenuItem?.isEnabled = connected && syncEngine.progress.isTerminal
        updateLastSyncLabel()
    }

    private func rebuildProfileSubmenu() {
        guard let submenu = profileMenuItem?.submenu else { return }
        submenu.removeAllItems()
        for profile in configStore.profiles {
            let item = NSMenuItem(title: profile.name, action: #selector(profileSelected(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = profile.id.uuidString
            item.state = (profile.id == configStore.activeProfileId) ? .on : .off
            submenu.addItem(item)
        }
    }

    @objc private func profileSelected(_ sender: NSMenuItem) {
        guard let idString = sender.representedObject as? String,
              let id = UUID(uuidString: idString) else { return }
        configStore.activeProfileId = id
        rebuildProfileSubmenu()
        updateForConnection(volumeMonitor.ssdConnected)
    }

    private func updateForProgress(_ p: SyncProgress) {
        switch p.state {
        case .idle:
            statusItem?.button?.image = menuBarImage(for: .idle)
            syncNowMenuItem?.title = "Sync Now"
            syncNowMenuItem?.isEnabled = volumeMonitor.ssdConnected
        case .running:
            statusItem?.button?.image = menuBarImage(for: .syncing)
            syncNowMenuItem?.title = "Syncing… (\(p.filesDone)/\(max(p.filesTotal, 1)))"
            syncNowMenuItem?.isEnabled = false
        case .done:
            statusItem?.button?.image = menuBarImage(for: .done)
            syncNowMenuItem?.title = "Sync Now"
            syncNowMenuItem?.isEnabled = volumeMonitor.ssdConnected
            updateLastSyncLabel()
        case .error(let msg):
            statusItem?.button?.image = menuBarImage(for: .error)
            statusMenuItem?.title = "⚠️ \(msg)"
        case .interrupted:
            statusItem?.button?.image = menuBarImage(for: .error)
        }
    }

    private func updateLastSyncLabel() {
        lastSyncMenuItem?.title = configStore.syncHistory.first
            .map { "Last sync: \($0.formattedDate) (\($0.fileCount) files)" }
            ?? "Last sync: never"
    }

    @objc private func syncNowTapped() { syncEngine.start() }
    @objc private func openSettingsTapped() { openSettings() }

    private enum IconState { case idle, syncing, done, error }

    private func menuBarImage(for state: IconState) -> NSImage? {
        let cfg = NSImage.SymbolConfiguration(pointSize: 15, weight: .regular)
        let name: String
        switch state {
        case .idle:    name = "externaldrive"
        case .syncing: name = "arrow.triangle.2.circlepath"
        case .done:    name = "externaldrive.badge.checkmark"
        case .error:   name = "externaldrive.badge.xmark"
        }
        return NSImage(systemSymbolName: name, accessibilityDescription: nil)?
            .withSymbolConfiguration(cfg)
    }
}

private extension SyncProgress {
    var isTerminal: Bool {
        switch state {
        case .done, .error, .interrupted, .idle: return true
        case .running: return false
        }
    }
}
