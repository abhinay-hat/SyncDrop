import XCTest
@testable import SyncDropCore

final class DryRunEngineTests: XCTestCase {

    func test_classify_newFile_isAdd() {
        let line = ">f+++++++++ projects/new.txt"
        let file = DryRunEngine.classify(line: line)
        XCTAssertEqual(file?.action, .add)
        XCTAssertEqual(file?.path, "projects/new.txt")
    }

    func test_classify_changedFile_isUpdate() {
        let line = ">f.st...... projects/changed.txt"
        let file = DryRunEngine.classify(line: line)
        XCTAssertEqual(file?.action, .update)
        XCTAssertEqual(file?.path, "projects/changed.txt")
    }

    func test_classify_deletion_isDelete() {
        let line = "*deleting   projects/gone.txt"
        let file = DryRunEngine.classify(line: line)
        XCTAssertEqual(file?.action, .delete)
        XCTAssertEqual(file?.path, "projects/gone.txt")
    }

    func test_classify_nonChangeLine_isNil() {
        XCTAssertNil(DryRunEngine.classify(line: "sending incremental file list"))
        XCTAssertNil(DryRunEngine.classify(line: ""))
        XCTAssertNil(DryRunEngine.classify(line: "Number of files: 10"))
        XCTAssertNil(DryRunEngine.classify(line: "cd+++++++++ adir/"))
    }

    func test_parse_countsByAction() {
        let output = """
sending incremental file list
>f+++++++++ a.txt
>f+++++++++ b.txt
>f.st...... c.txt
*deleting   d.txt

Number of files: 4
"""
        let result = DryRunEngine.parse(output: output)
        XCTAssertEqual(result.toCopy, 2)
        XCTAssertEqual(result.toUpdate, 1)
        XCTAssertEqual(result.toDelete, 1)
        XCTAssertEqual(result.files.count, 4)
    }

    func test_parse_emptyOutput_isAllZero() {
        let result = DryRunEngine.parse(output: "sending incremental file list\n\n")
        XCTAssertEqual(result.toCopy, 0)
        XCTAssertEqual(result.toUpdate, 0)
        XCTAssertEqual(result.toDelete, 0)
        XCTAssertTrue(result.files.isEmpty)
    }
}
