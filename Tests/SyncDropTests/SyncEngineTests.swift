import XCTest
@testable import SyncDropCore

final class SyncEngineTests: XCTestCase {
    var configStore: ConfigStore!
    var engine: SyncEngine!

    override func setUp() {
        super.setUp()
        let defaults = UserDefaults(suiteName: "SyncEngineTests")!
        defaults.removePersistentDomain(forName: "SyncEngineTests")
        configStore = ConfigStore(defaults: defaults)
        configStore.sourcePath = "~/Desktop/Projects"
        configStore.destPath = "/Volumes/Extreme Pro/Projects"
        engine = SyncEngine(configStore: configStore)
    }

    func test_rsyncArgs_noMirrorMode_noDelete() {
        configStore.mirrorMode = false
        let args = engine.rsyncArgs
        XCTAssertFalse(args.contains("--delete"))
        XCTAssertTrue(args.contains("-rltDv"))
        XCTAssertTrue(args.contains("--no-perms"))
        XCTAssertTrue(args.contains("--no-owner"))
        XCTAssertTrue(args.contains("--no-group"))
        XCTAssertTrue(args.contains("--modify-window=1"))
    }

    func test_rsyncArgs_mirrorMode_addsDelete() {
        configStore.mirrorMode = true
        XCTAssertTrue(engine.rsyncArgs.contains("--delete"))
    }

    func test_rsyncArgs_sourceEndsWithSlash() {
        let args = engine.rsyncArgs
        let sourceArg = args.first(where: { $0.contains("Desktop/Projects") })
        XCTAssertNotNil(sourceArg)
        XCTAssertTrue(sourceArg!.hasSuffix("/"), "Source must end with / for rsync")
    }

    func test_rsyncArgs_noMinusA() {
        XCTAssertFalse(engine.rsyncArgs.contains("-a"))
        XCTAssertFalse(engine.rsyncArgs.contains { $0.hasPrefix("-") && $0.contains("a") })
    }

    func test_parseProgress_extractsFileCount() {
        let line = "     524,288 100%  500.00kB/s    0:00:01 (xfr#5, to-chk=145/150)"
        let result = SyncEngine.parseProgress(from: line)
        XCTAssertNotNil(result)
        XCTAssertEqual(result?.filesDone, 5)
        XCTAssertEqual(result?.filesTotal, 150)
    }

    func test_parseProgress_returnsNilForNonProgressLine() {
        XCTAssertNil(SyncEngine.parseProgress(from: "sending incremental file list"))
        XCTAssertNil(SyncEngine.parseProgress(from: ""))
        XCTAssertNil(SyncEngine.parseProgress(from: "Number of files: 150"))
    }

    func test_initialState_isIdle() {
        guard case .idle = engine.progress.state else {
            XCTFail("Expected idle, got \(engine.progress.state)")
            return
        }
    }
}
