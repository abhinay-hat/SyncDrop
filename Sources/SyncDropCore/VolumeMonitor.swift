import AppKit
import Combine

@MainActor
public final class VolumeMonitor: ObservableObject {
    @Published public var ssdConnected = false
    @Published public var ssdMountURL: URL?

    private let configStore: ConfigStore
    private var observers: [NSObjectProtocol] = []
    private var cancellables: Set<AnyCancellable> = []

    public init(configStore: ConfigStore) {
        self.configStore = configStore
    }

    public func start() {
        let center = NSWorkspace.shared.notificationCenter

        let mount = center.addObserver(
            forName: NSWorkspace.didMountNotification,
            object: nil,
            queue: .main
        ) { [weak self] n in
            Task { @MainActor in self?.handleMount(n) }
        }

        let unmount = center.addObserver(
            forName: NSWorkspace.didUnmountNotification,
            object: nil,
            queue: .main
        ) { [weak self] n in
            Task { @MainActor in self?.handleUnmount(n) }
        }

        observers = [mount, unmount]

        // Re-evaluate connection state when the active profile (and thus the
        // configured SSD name) changes, otherwise ssdConnected goes stale.
        configStore.$activeProfileId
            .sink { [weak self] _ in self?.checkCurrentlyMountedVolumes() }
            .store(in: &cancellables)

        checkCurrentlyMountedVolumes()
    }

    public func stop() {
        observers.forEach { NSWorkspace.shared.notificationCenter.removeObserver($0) }
        observers = []
        cancellables.removeAll()
    }

    private func checkCurrentlyMountedVolumes() {
        // Reset first: if no mounted volume matches, state must fall back to
        // disconnected instead of retaining a previous (stale) match.
        ssdMountURL = nil
        ssdConnected = false

        let urls = FileManager.default.mountedVolumeURLs(
            includingResourceValuesForKeys: nil,
            options: .skipHiddenVolumes
        ) ?? []
        for url in urls where url.lastPathComponent == configStore.activeProfile.ssdName {
            ssdMountURL = url
            ssdConnected = true
            return
        }
    }

    private func handleMount(_ notification: Notification) {
        guard let url = notification.userInfo?[NSWorkspace.volumeURLUserInfoKey] as? URL else { return }
        if url.lastPathComponent == configStore.activeProfile.ssdName {
            ssdMountURL = url
            ssdConnected = true
        }
    }

    private func handleUnmount(_ notification: Notification) {
        guard let url = notification.userInfo?[NSWorkspace.volumeURLUserInfoKey] as? URL else { return }
        if url.lastPathComponent == configStore.activeProfile.ssdName {
            ssdMountURL = nil
            ssdConnected = false
        }
    }
}
