import Foundation

public struct DryRunFile: Identifiable, Equatable {
    public enum Action: Equatable { case add, update, delete }
    public let id = UUID()
    public let action: Action
    public let path: String

    public init(action: Action, path: String) {
        self.action = action
        self.path = path
    }
}

public struct DryRunResult: Identifiable, Equatable {
    public let id = UUID()
    public let toCopy: Int
    public let toUpdate: Int
    public let toDelete: Int
    public let files: [DryRunFile]

    public init(toCopy: Int, toUpdate: Int, toDelete: Int, files: [DryRunFile]) {
        self.toCopy = toCopy
        self.toUpdate = toUpdate
        self.toDelete = toDelete
        self.files = files
    }
}

public enum DryRunError: Error, LocalizedError {
    case launchFailed(String)
    case rsyncFailed(Int32)

    public var errorDescription: String? {
        switch self {
        case .launchFailed(let m): return "Could not start preview: \(m)"
        case .rsyncFailed(let code): return "Preview failed (rsync exit \(code))"
        }
    }
}

public struct DryRunEngine {
    public init() {}

    /// Runs rsync in dry-run/itemize mode and parses the result.
    /// `args` is the full live rsyncArgs list; this transforms it for preview:
    /// removes --progress/--stats, adds --dry-run --itemize-changes.
    public func preview(source: String, dest: String, args: [String]) async throws -> DryRunResult {
        var previewArgs = args.filter { $0 != "--progress" && $0 != "--stats" }
        previewArgs.insert("--dry-run", at: 0)
        previewArgs.insert("--itemize-changes", at: 1)

        let output = try await runRsync(previewArgs)
        return Self.parse(output: output)
    }

    private func runRsync(_ arguments: [String]) async throws -> String {
        // Accumulate output incrementally so the pipe buffer never fills (large
        // itemize output would otherwise deadlock readDataToEndOfFile in the
        // termination handler). Box allows safe mutation from the read handler;
        // ProcBox is a Sendable holder so the non-Sendable Process can be reached
        // from the @Sendable onCancel closure.
        final class Box: @unchecked Sendable { var value = "" }
        final class ProcBox: @unchecked Sendable { let process = Process() }
        let box = ProcBox()

        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<String, Error>) in
                let process = box.process
                process.executableURL = URL(fileURLWithPath: "/usr/bin/rsync")
                process.arguments = arguments

                let pipe = Pipe()
                process.standardOutput = pipe
                process.standardError = FileHandle.nullDevice

                let output = Box()
                pipe.fileHandleForReading.readabilityHandler = { handle in
                    let data = handle.availableData
                    guard !data.isEmpty else { return }
                    output.value += String(data: data, encoding: .utf8) ?? ""
                }

                process.terminationHandler = { proc in
                    pipe.fileHandleForReading.readabilityHandler = nil
                    let text = output.value
                    let status = proc.terminationStatus
                    // rsync exit 0 = success; 24 = files vanished (benign for preview).
                    if status == 0 || status == 24 {
                        continuation.resume(returning: text)
                    } else {
                        continuation.resume(throwing: DryRunError.rsyncFailed(status))
                    }
                }

                do {
                    try process.run()
                } catch {
                    continuation.resume(throwing: DryRunError.launchFailed(error.localizedDescription))
                }
            }
        } onCancel: {
            box.process.terminate()
        }
    }

    /// Parses a full --itemize-changes dry-run output into a DryRunResult.
    nonisolated public static func parse(output: String) -> DryRunResult {
        var files: [DryRunFile] = []
        for rawLine in output.components(separatedBy: "\n") {
            guard let file = classify(line: rawLine) else { continue }
            files.append(file)
        }
        let toCopy = files.filter { $0.action == .add }.count
        let toUpdate = files.filter { $0.action == .update }.count
        let toDelete = files.filter { $0.action == .delete }.count
        return DryRunResult(toCopy: toCopy, toUpdate: toUpdate, toDelete: toDelete, files: files)
    }

    /// Classifies a single itemize line. Returns nil for non-change lines.
    nonisolated public static func classify(line: String) -> DryRunFile? {
        let trimmedTrailing = line.replacingOccurrences(of: "\r", with: "")
        if trimmedTrailing.isEmpty { return nil }

        // Deletion lines: "*deleting   path/to/file"
        if trimmedTrailing.hasPrefix("*deleting") {
            let path = trimmedTrailing
                .replacingOccurrences(of: "*deleting", with: "")
                .trimmingCharacters(in: .whitespaces)
            return path.isEmpty ? nil : DryRunFile(action: .delete, path: path)
        }

        // Itemized change lines: "<11-char flags> <path>", first char '>' = received item.
        guard let first = trimmedTrailing.first, first == ">" else { return nil }

        // Split flag token from path (separated by whitespace).
        let parts = trimmedTrailing.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: true)
        guard parts.count == 2 else { return nil }
        let flags = String(parts[0])
        let path = String(parts[1]).trimmingCharacters(in: .whitespaces)
        guard !path.isEmpty else { return nil }

        // New file => all change positions are '+', e.g. ">f+++++++++".
        let action: DryRunFile.Action = flags.contains("+++++++++") ? .add : .update
        return DryRunFile(action: action, path: path)
    }
}
