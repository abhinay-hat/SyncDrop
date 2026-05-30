import AppKit
import Combine
import SyncDropCore
import UserNotifications

@MainActor
class AppDelegate: NSObject, NSApplicationDelegate {
    let configStore = ConfigStore()
    private(set) lazy var syncEngine = SyncEngine(configStore: configStore)
    private(set) lazy var volumeMonitor = VolumeMonitor(configStore: configStore)
    private(set) lazy var menuBarController = MenuBarController(
        configStore: configStore,
        syncEngine: syncEngine,
        volumeMonitor: volumeMonitor
    )
    private var cancellables = Set<AnyCancellable>()

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        requestNotificationPermission()
        menuBarController.setup()

        // Subscriptions BEFORE start() so initial mount event isn't missed
        volumeMonitor.$ssdConnected
            .filter { $0 }
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in self?.handleSSDConnected() }
            .store(in: &cancellables)

        volumeMonitor.$ssdConnected
            .filter { !$0 }
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in self?.syncEngine.cancel() }
            .store(in: &cancellables)

        volumeMonitor.start()

        NotificationCenter.default.addObserver(
            forName: .syncDidComplete, object: nil, queue: .main
        ) { [weak self] _ in
            Task { @MainActor in self?.handleSyncCompleted() }
        }
    }

    private func handleSSDConnected() {
        guard !configStore.destPath.isEmpty else {
            menuBarController.openSettings()
            return
        }
        if configStore.autoSync {
            syncEngine.start()  // start first so state = .running when popup renders
            menuBarController.showSyncPopup()
        } else {
            menuBarController.showSyncPopup()
        }
    }

    private func handleSyncCompleted() {
        guard configStore.autoEject else { return }
        let path = "/Volumes/\(configStore.ssdName.trimmingCharacters(in: .whitespaces))"
        do {
            try NSWorkspace.shared.unmountAndEjectDevice(at: URL(fileURLWithPath: path))
        } catch {
            postEjectFailureNotification()
        }
    }

    private func postEjectFailureNotification() {
        let content = UNMutableNotificationContent()
        content.title = "SyncDrop"
        content.body = "Could not eject — check Finder"
        UNUserNotificationCenter.current().add(
            UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil),
            withCompletionHandler: nil
        )
    }

    private func requestNotificationPermission() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }
}
