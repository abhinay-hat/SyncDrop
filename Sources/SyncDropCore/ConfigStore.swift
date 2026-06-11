import Foundation
import Combine
import os.log

private let configLog = Logger(subsystem: "com.syncdrop", category: "ConfigStore")

@MainActor
public final class ConfigStore: ObservableObject {
    private let defaults: UserDefaults

    @Published public var profiles: [SyncProfile] {
        didSet { persistProfiles() }
    }
    @Published public var activeProfileId: UUID {
        didSet { defaults.set(activeProfileId.uuidString, forKey: Keys.activeProfileId) }
    }

    // Global (not per-profile) settings.
    @Published public var notifyOnComplete: Bool {
        didSet { defaults.set(notifyOnComplete, forKey: Keys.notifyOnComplete) }
    }
    @Published public var launchAtLogin: Bool {
        didSet { defaults.set(launchAtLogin, forKey: Keys.launchAtLogin) }
    }
    @Published public var syncHistory: [SyncRecord] = [] {
        didSet {
            do {
                let data = try JSONEncoder().encode(syncHistory)
                defaults.set(data, forKey: Keys.syncHistory)
            } catch {
                configLog.error("Failed to persist sync history: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    private enum Keys {
        static let profiles = "profiles"
        static let activeProfileId = "activeProfileId"
        static let notifyOnComplete = "notifyOnComplete"
        static let launchAtLogin = "launchAtLogin"
        static let syncHistory = "syncHistory"
        // Legacy v1 keys (used only for one-time migration).
        static let legacySourcePath = "sourcePath"
        static let legacySsdName = "ssdName"
        static let legacyDestPath = "destPath"
        static let legacyAutoSync = "autoSync"
        static let legacyMirrorMode = "mirrorMode"
        static let legacyExcludes = "excludes"
        static let legacyAutoEject = "autoEject"
        static let legacyKeepVersions = "keepVersions"
    }

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults

        // Load globals first.
        self.notifyOnComplete = defaults.object(forKey: Keys.notifyOnComplete) as? Bool ?? true
        self.launchAtLogin = defaults.bool(forKey: Keys.launchAtLogin)

        // Load profiles, or migrate from v1, or seed a default.
        let resolvedProfiles: [SyncProfile]
        let storedProfiles = ConfigStore.decodeProfiles(from: defaults)
        if let stored = storedProfiles, !stored.isEmpty {
            resolvedProfiles = stored
        } else if let migrated = ConfigStore.migrateLegacyProfile(from: defaults) {
            resolvedProfiles = [migrated]
        } else {
            resolvedProfiles = [SyncProfile(name: "Default")]
        }
        self.profiles = resolvedProfiles

        // Resolve active profile id; fall back to first profile.
        if let idString = defaults.string(forKey: Keys.activeProfileId),
           let id = UUID(uuidString: idString),
           resolvedProfiles.contains(where: { $0.id == id }) {
            self.activeProfileId = id
        } else {
            self.activeProfileId = resolvedProfiles[0].id
        }

        // Load history.
        if let data = defaults.data(forKey: Keys.syncHistory) {
            do {
                self.syncHistory = try JSONDecoder().decode([SyncRecord].self, from: data)
            } catch {
                configLog.error("Failed to decode sync history, resetting: \(error.localizedDescription, privacy: .public)")
            }
        }

        // `didSet` does not fire during `init`. Persist the seeded profiles +
        // active id when nothing valid was stored (fresh install OR corrupted
        // data that failed to decode) so we don't regenerate a new UUID every
        // launch or leave bad data in place.
        if storedProfiles == nil || storedProfiles?.isEmpty == true {
            persistProfiles()
            defaults.set(self.activeProfileId.uuidString, forKey: Keys.activeProfileId)
        }

        // If we migrated from v1, persist profiles + clear legacy keys (once).
        finishMigrationIfNeeded()
    }

    // MARK: - Active profile

    public var activeProfile: SyncProfile {
        get {
            profiles.first(where: { $0.id == activeProfileId }) ?? profiles[0]
        }
        set {
            if let idx = profiles.firstIndex(where: { $0.id == newValue.id }) {
                profiles[idx] = newValue
            } else {
                profiles.append(newValue)
            }
        }
    }

    /// Convenience helper retained from v1 — now reads the active profile.
    public var expandedSourcePath: String {
        activeProfile.expandedSourcePath
    }

    public func appendSyncRecord(_ record: SyncRecord) {
        var history = syncHistory
        history.insert(record, at: 0)
        syncHistory = Array(history.prefix(20))
    }

    // MARK: - Profile management

    public func addProfile(name: String = "New Profile") {
        profiles.append(SyncProfile(name: name))
    }

    public func deleteProfile(id: UUID) {
        guard profiles.count > 1 else { return }
        profiles.removeAll { $0.id == id }
        if activeProfileId == id {
            activeProfileId = profiles[0].id
        }
    }

    // MARK: - Persistence & migration

    private func persistProfiles() {
        do {
            let data = try JSONEncoder().encode(profiles)
            defaults.set(data, forKey: Keys.profiles)
        } catch {
            configLog.error("Failed to persist profiles: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Decodes stored profiles, logging (rather than silently dropping) corrupt data.
    /// Returns nil when no data is stored or decoding fails.
    nonisolated private static func decodeProfiles(from defaults: UserDefaults) -> [SyncProfile]? {
        guard let data = defaults.data(forKey: Keys.profiles) else { return nil }
        do {
            return try JSONDecoder().decode([SyncProfile].self, from: data)
        } catch {
            configLog.error("Failed to decode profiles, falling back: \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    nonisolated private static func migrateLegacyProfile(from defaults: UserDefaults) -> SyncProfile? {
        guard let legacySource = defaults.string(forKey: Keys.legacySourcePath) else {
            return nil
        }
        let legacyExcludes: [String]
        if let data = defaults.data(forKey: Keys.legacyExcludes),
           let stored = try? JSONDecoder().decode([String].self, from: data) {
            legacyExcludes = stored
        } else {
            legacyExcludes = [".DS_Store", ".Spotlight-V100", ".fseventsd", ".Trashes", "node_modules"]
        }
        return SyncProfile(
            name: "Default",
            sourcePath: legacySource,
            destPath: defaults.string(forKey: Keys.legacyDestPath) ?? "",
            ssdName: defaults.string(forKey: Keys.legacySsdName) ?? "Extreme Pro",
            mirrorMode: defaults.bool(forKey: Keys.legacyMirrorMode),
            autoSync: defaults.bool(forKey: Keys.legacyAutoSync),
            autoEject: defaults.bool(forKey: Keys.legacyAutoEject),
            keepVersions: defaults.bool(forKey: Keys.legacyKeepVersions),
            excludes: legacyExcludes
        )
    }

    private func finishMigrationIfNeeded() {
        guard defaults.string(forKey: Keys.legacySourcePath) != nil else { return }
        persistProfiles()
        defaults.set(activeProfileId.uuidString, forKey: Keys.activeProfileId)
        for key in [
            Keys.legacySourcePath, Keys.legacySsdName, Keys.legacyDestPath,
            Keys.legacyAutoSync, Keys.legacyMirrorMode, Keys.legacyExcludes,
            Keys.legacyAutoEject, Keys.legacyKeepVersions
        ] {
            defaults.removeObject(forKey: key)
        }
    }
}
