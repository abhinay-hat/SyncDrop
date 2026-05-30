import AppKit
import Combine

@MainActor
public final class VolumeMonitor: ObservableObject {
    @Published public var ssdConnected = false
    @Published public var ssdMountURL: URL?

    private let configStore: ConfigStore
    private var observers: [NSObjectProtocol] = []

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
        checkCurrentlyMountedVolumes()
    }

    public func stop() {
        observers.forEach { NSWorkspace.shared.notificationCenter.removeObserver($0) }
        observers = []
    }

    private func checkCurrentlyMountedVolumes() {
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
