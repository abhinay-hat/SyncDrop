import Foundation

struct SyncRecord: Codable, Identifiable {
    let id: UUID
    let date: Date
    let fileCount: Int
    let totalBytes: Int64
    let durationSeconds: TimeInterval
    let succeeded: Bool

    init(date: Date, fileCount: Int, totalBytes: Int64, durationSeconds: TimeInterval, succeeded: Bool) {
        self.id = UUID()
        self.date = date
        self.fileCount = fileCount
        self.totalBytes = totalBytes
        self.durationSeconds = durationSeconds
        self.succeeded = succeeded
    }

    var formattedDate: String {
        let f = DateFormatter()
        f.dateStyle = .short
        f.timeStyle = .short
        return f.string(from: date)
    }

    var formattedSize: String {
        ByteCountFormatter.string(fromByteCount: totalBytes, countStyle: .file)
    }

    var formattedDuration: String {
        let d = Int(durationSeconds)
        if d < 60 { return "\(d)s" }
        return "\(d / 60)m \(d % 60)s"
    }
}
