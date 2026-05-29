import Foundation

public struct SyncProgress {
    public enum State: Equatable {
        case idle
        case running
        case done
        case error(String)
        case interrupted
    }

    public var state: State = .idle
    public var filesTotal: Int = 0
    public var filesDone: Int = 0
    public var currentFile: String = ""
    public var bytesTotal: Int64 = 0
    public var bytesDone: Int64 = 0
    public var startTime: Date?
    public var endTime: Date?

    public init() {}

    public var percentComplete: Double {
        guard filesTotal > 0 else { return 0 }
        return Double(filesDone) / Double(filesTotal)
    }

    public var duration: TimeInterval? {
        guard let start = startTime else { return nil }
        return (endTime ?? Date()).timeIntervalSince(start)
    }

    public var isTerminal: Bool {
        switch state {
        case .done, .error, .interrupted: return true
        case .idle, .running: return false
        }
    }
}
