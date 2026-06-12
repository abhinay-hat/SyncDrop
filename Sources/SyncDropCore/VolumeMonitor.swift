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

        // queue: .main guarantees these run on the main thread, so we extract the
        // Sendable URL and hop via assumeIsolated rather than capturing the
        // non-Sendable Notification across a Task (which newer Swift rejects).
        let mount = center.addObserver(
            forName: NSWorkspace.didMountNotification,
            object: nil,
            queue: .main
        ) { [weak self] note in
            let url = note.userInfo?[NSWorkspace.volumeURLUserInfoKey] as? URL
            MainActor.assumeIsolated { self?.handleMount(url) }
        }

        let unmount = center.addObserver(
            forName: NSWorkspace.didUnmountNotification,
            object: nil,
            queue: .main
        ) { [weak self] note in
            let url = note.userInfo?[NSWorkspace.volumeURLUserInfoKey] as? URL
            MainActor.assumeIsolated { self?.handleUnmount(url) }
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

    private func handleMount(_ url: URL?) {
        guard let url, url.lastPathComponent == configStore.activeProfile.ssdName else { return }
        ssdMountURL = url
        ssdConnected = true
    }

    private func handleUnmount(_ url: URL?) {
        guard let url, url.lastPathComponent == configStore.activeProfile.ssdName else { return }
        ssdMountURL = nil
        ssdConnected = false
    }
}
