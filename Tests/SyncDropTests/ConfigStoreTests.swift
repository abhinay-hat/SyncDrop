import XCTest
@testable import SyncDropCore

final class ConfigStoreTests: XCTestCase {
    var store: ConfigStore!
    let suite = "SyncDropTests"

    override func setUp() {
        super.setUp()
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        store = ConfigStore(defaults: defaults)
    }

    private func freshStore() -> ConfigStore {
        ConfigStore(defaults: UserDefaults(suiteName: suite)!)
    }

    func test_defaults_seedsOneDefaultProfile() {
        XCTAssertEqual(store.profiles.count, 1)
        XCTAssertEqual(store.profiles[0].name, "Default")
        XCTAssertEqual(store.activeProfileId, store.profiles[0].id)
    }

    func test_activeProfile_defaults_areCorrect() {
        let p = store.activeProfile
        XCTAssertEqual(p.sourcePath, "~/Desktop/Projects")
        XCTAssertEqual(p.ssdName, "Extreme Pro")
        XCTAssertEqual(p.destPath, "")
        XCTAssertFalse(p.autoSync)
        XCTAssertFalse(p.mirrorMode)
        XCTAssertFalse(p.autoEject)
        XCTAssertFalse(p.keepVersions)
        XCTAssertEqual(p.excludes, [".DS_Store", ".Spotlight-V100", ".fseventsd", ".Trashes", "node_modules"])
    }

    func test_globals_defaults() {
        XCTAssertTrue(store.notifyOnComplete)
        XCTAssertTrue(store.syncHistory.isEmpty)
    }

    func test_activeProfile_setter_persistsEdit() {
        var p = store.activeProfile
        p.sourcePath = "~/Documents/Work"
        store.activeProfile = p
        let reloaded = freshStore()
        XCTAssertEqual(reloaded.activeProfile.sourcePath, "~/Documents/Work")
    }

    func test_addProfile_and_switch() {
        store.addProfile(name: "Photos")
        XCTAssertEqual(store.profiles.count, 2)
        let photos = store.profiles[1]
        store.activeProfileId = photos.id
        XCTAssertEqual(store.activeProfile.name, "Photos")
    }

    func test_deleteProfile_neverDeletesLast() {
        store.deleteProfile(id: store.profiles[0].id)
        XCTAssertEqual(store.profiles.count, 1)
    }

    func test_deleteProfile_reassignsActiveWhenDeletingActive() {
        store.addProfile(name: "B")
        let a = store.profiles[0].id
        store.deleteProfile(id: a)
        XCTAssertEqual(store.profiles.count, 1)
        XCTAssertEqual(store.activeProfileId, store.profiles[0].id)
    }

    func test_expandedSourcePath_proxiesActiveProfile() {
        XCTAssertTrue(store.expandedSourcePath.hasPrefix("/Users/"))
        XCTAssertFalse(store.expandedSourcePath.contains("~"))
    }

    func test_appendSyncRecord_keepsMax20() {
        for i in 0..<25 {
            store.appendSyncRecord(SyncRecord(date: Date(), fileCount: i, totalBytes: 0, durationSeconds: 1, succeeded: true))
        }
        XCTAssertEqual(store.syncHistory.count, 20)
    }

    func test_migration_fromLegacyKeys_createsDefaultProfile_andClearsLegacy() {
        let migrationSuite = "SyncDropMigrationTests"
        let defaults = UserDefaults(suiteName: migrationSuite)!
        defaults.removePersistentDomain(forName: migrationSuite)
        defaults.set("~/OldSource", forKey: "sourcePath")
        defaults.set("MySSD", forKey: "ssdName")
        defaults.set("/Volumes/MySSD/Backup", forKey: "destPath")
        defaults.set(true, forKey: "autoSync")
        defaults.set(true, forKey: "mirrorMode")
        if let data = try? JSONEncoder().encode(["a", "b"]) {
            defaults.set(data, forKey: "excludes")
        }
        defaults.set(true, forKey: "autoEject")
        defaults.set(true, forKey: "keepVersions")

        let migrated = ConfigStore(defaults: defaults)
        XCTAssertEqual(migrated.profiles.count, 1)
        let p = migrated.activeProfile
        XCTAssertEqual(p.name, "Default")
        XCTAssertEqual(p.sourcePath, "~/OldSource")
        XCTAssertEqual(p.ssdName, "MySSD")
        XCTAssertEqual(p.destPath, "/Volumes/MySSD/Backup")
        XCTAssertTrue(p.autoSync)
        XCTAssertTrue(p.mirrorMode)
        XCTAssertTrue(p.autoEject)
        XCTAssertTrue(p.keepVersions)
        XCTAssertEqual(p.excludes, ["a", "b"])
        XCTAssertNil(defaults.string(forKey: "sourcePath"))
        XCTAssertNil(defaults.string(forKey: "ssdName"))

        let reloaded = ConfigStore(defaults: defaults)
        XCTAssertEqual(reloaded.profiles.count, 1)
        XCTAssertEqual(reloaded.activeProfile.sourcePath, "~/OldSource")
    }
}
