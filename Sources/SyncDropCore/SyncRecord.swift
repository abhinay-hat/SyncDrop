import Foundation

public struct SyncRecord: Codable, Identifiable {
    public let id: UUID
    public let date: Date
    public let fileCount: Int
    public let totalBytes: Int64
    public let durationSeconds: TimeInterval
    public let succeeded: Bool

    public init(date: Date, fileCount: Int, totalBytes: Int64, durationSeconds: TimeInterval, succeeded: Bool) {
        self.id = UUID()
        self.date = date
        self.fileCount = fileCount
        self.totalBytes = totalBytes
        self.durationSeconds = durationSeconds
        self.succeeded = succeeded
    }

    public var formattedDate: String {
        Self.dateFormatter.string(from: date)
    }

    private static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .short
        f.timeStyle = .short
        return f
    }()

    public var formattedSize: String {
        ByteCountFormatter.string(fromByteCount: totalBytes, countStyle: .file)
    }

    public var formattedDuration: String {
        let d = Int(durationSeconds)
        if d < 60 { return "\(d)s" }
        return "\(d / 60)m \(d % 60)s"
    }
}
