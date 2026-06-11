import Foundation
import Combine
import UserNotifications

@MainActor
public final class SyncEngine: ObservableObject {
    @Published public var progress = SyncProgress()

    private let configStore: ConfigStore
    private var process: Process?
    private var outputPipe: Pipe?
    private var cancelRequested = false

    public init(configStore: ConfigStore) {
        self.configStore = configStore
    }

    /// Convenience: build args using the current date for the backup dir.
    public var rsyncArgs: [String] { rsyncArgs(date: Date()) }

    /// Builds the rsync argument list.
    /// exFAT-safe: omits -a (which sets -p/-o/-g and breaks on exFAT with
    /// EPERM). Uses --modify-window=1 to handle exFAT's 2-second timestamp
    /// granularity and avoid re-copying unchanged files every run.
    public func rsyncArgs(date: Date) -> [String] {
        let profile = configStore.activeProfile
        var args = [
            "-rltDv",
            "--no-perms",
            "--no-owner",
            "--no-group",
            "--modify-window=1",
            "--progress",
            "--stats"
        ]
        if profile.mirrorMode { args.append("--delete") }
        for pattern in profile.excludes where !pattern.trimmingCharacters(in: .whitespaces).isEmpty {
            args.append("--exclude=\(pattern)")
        }
        if profile.keepVersions {
            args.append("--backup")
            args.append("--backup-dir=.syncdrop_archive/\(Self.backupDateString(date))")
        }
        // Trailing slash on source tells rsync to copy *contents*, not the dir itself
        args += [profile.expandedSourcePath + "/", profile.destPath]
        return args
    }

    nonisolated public static func backupDateString(_ date: Date) -> String {
        let fmt = DateFormatter()
        fmt.locale = Locale(identifier: "en_US_POSIX")
        fmt.timeZone = TimeZone.current
        fmt.dateFormat = "yyyy-MM-dd"
        return fmt.string(from: date)
    }

    public func start() {
        guard process?.isRunning != true else { return }
        let profile = configStore.activeProfile
        guard !profile.expandedSourcePath.isEmpty,
              !profile.destPath.isEmpty else {
            progress.state = .error("Source or destination path not configured")
            return
        }

        do {
            try FileManager.default.createDirectory(
                atPath: profile.destPath,
                withIntermediateDirectories: true,
                attributes: nil
            )
        } catch {
            progress.state = .error("Cannot create destination directory at '\(profile.destPath)': \(error.localizedDescription)")
            return
        }

        cancelRequested = false
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/rsync")
        let syncStartDate = Date()
        p.arguments = rsyncArgs(date: syncStartDate)

        // openrsync (macOS) writes progress to stdout. Capturing stdout+stderr on the
        // SAME pipe causes exit 1 (shared FD breaks internal multiplexing). Fix:
        // stdout = Pipe (progress), stderr = /dev/null (discard error text).
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = FileHandle.nullDevice
        outputPipe = pipe

        var initial = SyncProgress()
        initial.state = .running
        initial.startTime = Date()
        progress = initial

        // Buffer partial lines — rsync progress (to-check=N/T) can split across reads.
        // Use a Box to allow safe mutation from the readabilityHandler closure.
        final class Box { var value = "" }
        let lineBuffer = Box()
        pipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty else { return }
            lineBuffer.value += String(data: data, encoding: .utf8) ?? ""
            let parts = lineBuffer.value.components(separatedBy: CharacterSet(charactersIn: "\r\n"))
            lineBuffer.value = parts.last ?? ""
            for line in parts.dropLast() where !line.trimmingCharacters(in: .whitespaces).isEmpty {
                let captured = line
                Task { @MainActor [weak self] in self?.handleOutputLine(captured) }
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
        cancelRequested = true
        process?.terminate()
        process = nil
        if case .running = progress.state {
            progress.state = .interrupted
        }
    }

    /// Parses an rsync `--progress` output line.
    /// GNU rsync:   `to-chk=N/T`  → N = remaining, filesDone = T - N
    /// openrsync:   `to-check=N/T` → N = done,      filesDone = N
    nonisolated public static func parseProgress(from line: String) -> (filesDone: Int, filesTotal: Int)? {
        let pattern = #"to-(chk|check)=(\d+)/(\d+)"#
        guard let regex = try? NSRegularExpression(pattern: pattern),
              let match = regex.firstMatch(in: line, range: NSRange(line.startIndex..., in: line)),
              let typeRange = Range(match.range(at: 1), in: line),
              let nRange = Range(match.range(at: 2), in: line),
              let tRange = Range(match.range(at: 3), in: line),
              let n = Int(line[nRange].replacingOccurrences(of: ",", with: "")),
              let t = Int(line[tRange].replacingOccurrences(of: ",", with: ""))
        else { return nil }
        let isOpenRsync = line[typeRange] == "check"
        return (filesDone: isOpenRsync ? n : (t - n), filesTotal: t)
    }

    // MARK: - Private

    private func handleOutputLine(_ line: String) {
        if let parsed = SyncEngine.parseProgress(from: line) {
            progress.filesDone = parsed.filesDone
            progress.filesTotal = parsed.filesTotal
        } else if line.hasPrefix("Transfer starting:") {
            // "Transfer starting: N files" — set total early for better UX
            let words = line.components(separatedBy: " ")
            if let idx = words.firstIndex(of: "files"), idx > 0,
               let count = Int(words[idx - 1]) {
                progress.filesTotal = count
            }
        } else if !line.hasPrefix(" ") && !line.hasPrefix("Number") &&
                  !line.hasPrefix("Total") && !line.hasPrefix("Transfer") &&
                  !line.hasPrefix("sent") && !line.hasPrefix("Unmatched") &&
                  !line.hasPrefix("Matched") && !line.hasPrefix("File list") &&
                  !line.isEmpty {
            // Plain filename lines
            progress.currentFile = line.hasPrefix("./") ? String(line.dropFirst(2)) : line
        }
    }

    private func handleTermination(_ p: Process) {
        outputPipe?.fileHandleForReading.readabilityHandler = nil
        process = nil
        var updated = progress
        updated.endTime = Date()

        if cancelRequested {
            // User-initiated cancel: SIGTERM yields varied exit codes; force interrupted.
            updated.state = .interrupted
        } else {
            switch p.terminationStatus {
            case 0:      updated.state = .done
            case 20, 23: updated.state = .interrupted  // 20: SIGINT/SIGTERM, 23: partial transfer
            default:     updated.state = .error("rsync exited with code \(p.terminationStatus)")
            }
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
