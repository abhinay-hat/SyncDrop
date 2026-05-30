import XCTest
@testable import SyncDropCore

final class ConfigStoreTests: XCTestCase {
    var store: ConfigStore!

    override func setUp() {
        super.setUp()
        let defaults = UserDefaults(suiteName: "SyncDropTests")!
        defaults.removePersistentDomain(forName: "SyncDropTests")
        store = ConfigStore(defaults: defaults)
    }

    func test_defaults_areCorrect() {
        XCTAssertEqual(store.sourcePath, "~/Desktop/Projects")
        XCTAssertEqual(store.ssdName, "Extreme Pro")
        XCTAssertEqual(store.destPath, "")
        XCTAssertFalse(store.autoSync)
        XCTAssertFalse(store.mirrorMode)
        XCTAssertTrue(store.notifyOnComplete)
        XCTAssertTrue(store.syncHistory.isEmpty)
        XCTAssertEqual(store.excludes, [".DS_Store", ".Spotlight-V100", ".fseventsd", ".Trashes", "node_modules"])
    }

    func test_savingAndLoading_sourcePath() {
        store.sourcePath = "~/Documents/Work"
        XCTAssertEqual(store.sourcePath, "~/Documents/Work")
    }

    func test_appendSyncRecord_keepsMax20() {
        for i in 0..<25 {
            store.appendSyncRecord(SyncRecord(date: Date(), fileCount: i, totalBytes: 0, durationSeconds: 1, succeeded: true))
        }
        XCTAssertEqual(store.syncHistory.count, 20)
    }

    func test_expandedSourcePath_expandsTilde() {
        store.sourcePath = "~/Desktop/Projects"
        XCTAssertTrue(store.expandedSourcePath.hasPrefix("/Users/"))
        XCTAssertFalse(store.expandedSourcePath.contains("~"))
    }

    func test_excludes_saveAndLoad() {
        store.excludes = ["*.tmp", "build"]
        let defaults = UserDefaults(suiteName: "SyncDropTests")!
        let reloaded = ConfigStore(defaults: defaults)
        XCTAssertEqual(reloaded.excludes, ["*.tmp", "build"])
    }
}
