import Foundation
import Combine

@MainActor
public final class ConfigStore: ObservableObject {
    private let defaults: UserDefaults

    @Published public var sourcePath: String {
        didSet { defaults.set(sourcePath, forKey: Keys.sourcePath) }
    }
    @Published public var ssdName: String {
        didSet { defaults.set(ssdName, forKey: Keys.ssdName) }
    }
    @Published public var destPath: String {
        didSet { defaults.set(destPath, forKey: Keys.destPath) }
    }
    @Published public var autoSync: Bool {
        didSet { defaults.set(autoSync, forKey: Keys.autoSync) }
    }
    @Published public var mirrorMode: Bool {
        didSet { defaults.set(mirrorMode, forKey: Keys.mirrorMode) }
    }
    @Published public var notifyOnComplete: Bool {
        didSet { defaults.set(notifyOnComplete, forKey: Keys.notifyOnComplete) }
    }
    @Published public var launchAtLogin: Bool {
        didSet { defaults.set(launchAtLogin, forKey: Keys.launchAtLogin) }
    }
    @Published public var autoEject: Bool {
        didSet { defaults.set(autoEject, forKey: Keys.autoEject) }
    }
    @Published public var keepVersions: Bool {
        didSet { defaults.set(keepVersions, forKey: Keys.keepVersions) }
    }
    @Published public var excludes: [String] {
        didSet {
            if let data = try? JSONEncoder().encode(excludes) {
                defaults.set(data, forKey: Keys.excludes)
            }
        }
    }
    @Published public var syncHistory: [SyncRecord] = [] {
        didSet {
            if let data = try? JSONEncoder().encode(syncHistory) {
                defaults.set(data, forKey: Keys.syncHistory)
            }
        }
    }

    private enum Keys {
        static let sourcePath = "sourcePath"
        static let ssdName = "ssdName"
        static let destPath = "destPath"
        static let autoSync = "autoSync"
        static let mirrorMode = "mirrorMode"
        static let notifyOnComplete = "notifyOnComplete"
        static let launchAtLogin = "launchAtLogin"
        static let autoEject = "autoEject"
        static let keepVersions = "keepVersions"
        static let excludes = "excludes"
        static let syncHistory = "syncHistory"
    }

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        self.sourcePath = defaults.string(forKey: Keys.sourcePath) ?? "~/Desktop/Projects"
        self.ssdName = defaults.string(forKey: Keys.ssdName) ?? "Extreme Pro"
        self.destPath = defaults.string(forKey: Keys.destPath) ?? ""
        self.autoSync = defaults.bool(forKey: Keys.autoSync)
        self.mirrorMode = defaults.bool(forKey: Keys.mirrorMode)
        self.notifyOnComplete = defaults.object(forKey: Keys.notifyOnComplete) as? Bool ?? true
        self.launchAtLogin = defaults.bool(forKey: Keys.launchAtLogin)
        self.autoEject = defaults.bool(forKey: Keys.autoEject)
        self.keepVersions = defaults.bool(forKey: Keys.keepVersions)
        if let data = defaults.data(forKey: Keys.excludes),
           let stored = try? JSONDecoder().decode([String].self, from: data) {
            self.excludes = stored
        } else {
            self.excludes = [".DS_Store", ".Spotlight-V100", ".fseventsd", ".Trashes", "node_modules"]
        }
        if let data = defaults.data(forKey: Keys.syncHistory),
           let records = try? JSONDecoder().decode([SyncRecord].self, from: data) {
            self.syncHistory = records
        }
    }

    public var expandedSourcePath: String {
        (sourcePath as NSString).expandingTildeInPath
    }

    public func appendSyncRecord(_ record: SyncRecord) {
        var history = syncHistory
        history.insert(record, at: 0)
        syncHistory = Array(history.prefix(20))
    }
}
