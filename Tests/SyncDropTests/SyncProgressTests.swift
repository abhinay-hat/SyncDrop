// Tests/SyncDropTests/SyncProgressTests.swift
import XCTest
@testable import SyncDropCore

final class SyncProgressTests: XCTestCase {
    func test_syncRecord_encodesAndDecodes() throws {
        let record = SyncRecord(
            date: Date(timeIntervalSince1970: 1_000_000),
            fileCount: 42,
            totalBytes: 1_024,
            durationSeconds: 3.5,
            succeeded: true
        )
        let data = try JSONEncoder().encode(record)
        let decoded = try JSONDecoder().decode(SyncRecord.self, from: data)
        XCTAssertEqual(decoded.fileCount, 42)
        XCTAssertEqual(decoded.totalBytes, 1_024)
        XCTAssertEqual(decoded.durationSeconds, 3.5)
        XCTAssertTrue(decoded.succeeded)
    }

    func test_syncProgress_defaultState_isIdle() {
        let p = SyncProgress()
        if case .idle = p.state { } else {
            XCTFail("Expected idle state, got \(p.state)")
        }
    }

    func test_syncProgress_percentComplete_zeroWhenNoFiles() {
        let p = SyncProgress()
        XCTAssertEqual(p.percentComplete, 0.0)
    }

    func test_syncProgress_percentComplete_calculatesCorrectly() {
        var p = SyncProgress()
        p.filesTotal = 100
        p.filesDone = 25
        XCTAssertEqual(p.percentComplete, 0.25, accuracy: 0.001)
    }
}
