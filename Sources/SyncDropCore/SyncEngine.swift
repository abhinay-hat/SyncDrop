import Foundation
import Combine
import UserNotifications

@MainActor
public final class SyncEngine: ObservableObject {
    @Published public var progress = SyncProgress()

    private let configStore: ConfigStore
    private var process: Process?
    private var outputPipe: Pipe?

    public init(configStore: ConfigStore) {
        self.configStore = configStore
    }

    /// Builds the rsync argument list.
    /// exFAT-safe: omits -a (which sets -p/-o/-g and breaks on exFAT with
    /// EPERM). Uses --modify-window=1 to handle exFAT's 2-second timestamp
    /// granularity and avoid re-copying unchanged files every run.
    public var rsyncArgs: [String] {
        var args = [
            "-rltDv",
            "--no-perms",
            "--no-owner",
            "--no-group",
            "--modify-window=1",
            "--progress",
            "--stats"
        ]
        if configStore.mirrorMode { args.append("--delete") }
        // Trailing slash on source tells rsync to copy *contents*, not the dir itself
        args += [configStore.expandedSourcePath + "/", configStore.destPath]
        return args
    }

    public func start() {
        guard process?.isRunning != true else { return }
        guard !configStore.expandedSourcePath.isEmpty,
              !configStore.destPath.isEmpty else {
            progress.state = .error("Source or destination path not configured")
            return
        }

        try? FileManager.default.createDirectory(
            atPath: configStore.destPath,
            withIntermediateDirectories: true,
            attributes: nil
        )

        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/rsync")
        p.arguments = rsyncArgs

        // macOS openrsync uses stdout for its local client↔server protocol.
        // Capturing stdout with a Pipe breaks it (exit 1, io_read errors).
        // Send stdout to /dev/null explicitly; capture stderr for progress/stats.
        let pipe = Pipe()
        p.standardOutput = FileHandle.nullDevice
        p.standardError = pipe
        outputPipe = pipe

        var initial = SyncProgress()
        initial.state = .running
        initial.startTime = Date()
        progress = initial

        pipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty else { return }
            let text = String(data: data, encoding: .utf8) ?? ""
            for line in text.components(separatedBy: CharacterSet(charactersIn: "\r\n")) {
                guard !line.isEmpty else { continue }
                Task { @MainActor [weak self] in self?.handleOutputLine(line) }
            }
        }

        p.terminationHandler = { [weak self] proc in
            Task { @MainActor [weak self] in self?.handleTermination(proc) }
        }

        process = p
        do {
            try p.run()
        } catch {
            progress.state = .error("Failed to launch rsync: \(error.localizedDescription)")
        }
    }

    public func cancel() {
        process?.terminate()
        process = nil
        if case .running = progress.state {
            progress.state = .interrupted
        }
    }

    /// Parses an rsync `--progress` output line.
    /// Returns `(filesDone, filesTotal)` when the line contains `to-chk=R/T`,
    /// where filesDone = T - R (remaining).
    nonisolated public static func parseProgress(from line: String) -> (filesDone: Int, filesTotal: Int)? {
        let pattern = #"to-chk=(\d+)/(\d+)"#
        guard let regex = try? NSRegularExpression(pattern: pattern),
              let match = regex.firstMatch(
                in: line,
                range: NSRange(line.startIndex..., in: line)
              ),
              let remainingRange = Range(match.range(at: 1), in: line),
              let totalRange = Range(match.range(at: 2), in: line),
              let remaining = Int(line[remainingRange]),
              let total = Int(line[totalRange])
        else { return nil }
        return (filesDone: total - remaining, filesTotal: total)
    }

    // MARK: - Private

    private func handleOutputLine(_ line: String) {
        if let parsed = SyncEngine.parseProgress(from: line) {
            progress.filesDone = parsed.filesDone
            progress.filesTotal = parsed.filesTotal
        }
    }

    private func handleTermination(_ p: Process) {
        outputPipe?.fileHandleForReading.readabilityHandler = nil
        process = nil
        var updated = progress
        updated.endTime = Date()

        switch p.terminationStatus {
        case 0:  updated.state = .done
        case 20: updated.state = .interrupted   // rsync: received SIGINT/SIGTERM
        default: updated.state = .error("rsync exited with code \(p.terminationStatus)")
        }
        progress = updated

        if case .done = updated.state {
            saveRecord(updated)
            NotificationCenter.default.post(name: .syncDidComplete, object: nil)
            if configStore.notifyOnComplete { sendSystemNotification(updated) }
        }
    }

    private func saveRecord(_ p: SyncProgress) {
        configStore.appendSyncRecord(SyncRecord(
            date: Date(),
            fileCount: p.filesDone,
            totalBytes: p.bytesDone,
            durationSeconds: p.duration ?? 0,
            succeeded: true
        ))
    }

    private func sendSystemNotification(_ p: SyncProgress) {
        let content = UNMutableNotificationContent()
        content.title = "SyncDrop"
        content.body = "Sync complete — \(p.filesDone) files"
        UNUserNotificationCenter.current().add(
            UNNotificationRequest(
                identifier: UUID().uuidString,
                content: content,
                trigger: nil
            ),
            withCompletionHandler: nil
        )
    }
}

public extension Notification.Name {
    static let syncDidComplete = Notification.Name("com.syncdrop.syncDidComplete")
}
