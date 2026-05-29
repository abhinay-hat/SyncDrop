import Foundation

struct SyncProgress {
    enum State: Equatable {
        case idle
        case running
        case done
        case error(String)
        case interrupted
    }

    var state: State = .idle
    var filesTotal: Int = 0
    var filesDone: Int = 0
    var currentFile: String = ""
    var bytesTotal: Int64 = 0
    var bytesDone: Int64 = 0
    var startTime: Date?
    var endTime: Date?

    var percentComplete: Double {
        guard filesTotal > 0 else { return 0 }
        return Double(filesDone) / Double(filesTotal)
    }

    var duration: TimeInterval? {
        guard let start = startTime else { return nil }
        return (endTime ?? Date()).timeIntervalSince(start)
    }

    var isTerminal: Bool {
        switch state {
        case .done, .error, .interrupted: return true
        case .idle, .running: return false
        }
    }
}
