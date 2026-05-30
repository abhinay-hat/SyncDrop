import SwiftUI
import SyncDropCore

@MainActor
struct SyncPopupContentView: View {
    @ObservedObject var syncEngine: SyncEngine
    @ObservedObject var configStore: ConfigStore
    let onStart: () -> Void
    let onDismiss: () -> Void

    @State private var isPreviewing = false
    @State private var dryRunResult: DryRunResult?
    @State private var previewError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            headerRow
            Divider()
            stateContent
        }
        .padding(18)
        .frame(width: 380)
        .sheet(item: $dryRunResult) { result in
            DryRunSheet(
                result: result,
                onConfirm: {
                    dryRunResult = nil
                    onStart()
                },
                onCancel: { dryRunResult = nil }
            )
        }
    }

    private var headerRow: some View {
        HStack(spacing: 10) {
            Image(systemName: "externaldrive.fill")
                .font(.title2)
                .foregroundColor(.accentColor)
            VStack(alignment: .leading, spacing: 2) {
                Text(configStore.activeProfile.ssdName).font(.headline)
                Text("External SSD detected").font(.caption).foregroundColor(.secondary)
            }
            Spacer()
        }
    }

    @ViewBuilder
    private var stateContent: some View {
        switch syncEngine.progress.state {
        case .idle:         confirmView
        case .running:      progressView
        case .done:         doneView
        case .interrupted:  errorView("Sync was interrupted")
        case .error(let m): errorView(m)
        }
    }

    private var confirmView: some View {
        VStack(alignment: .leading, spacing: 10) {
            pathRow
            if let previewError {
                Text(previewError)
                    .font(.caption2).foregroundColor(.red).lineLimit(2)
            }
            HStack {
                Button("Cancel", action: onDismiss).keyboardShortcut(.cancelAction)
                Spacer()
                Button {
                    runPreview()
                } label: {
                    if isPreviewing {
                        ProgressView().controlSize(.small)
                    } else {
                        Text("Preview…")
                    }
                }
                .disabled(isPreviewing || configStore.activeProfile.destPath.isEmpty)
                Button("Start Sync", action: onStart)
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(configStore.activeProfile.destPath.isEmpty)
            }
        }
    }

    private func runPreview() {
        previewError = nil
        isPreviewing = true
        let source = configStore.expandedSourcePath
        let dest = configStore.activeProfile.destPath
        let args = syncEngine.rsyncArgs
        Task {
            do {
                let result = try await DryRunEngine().preview(source: source, dest: dest, args: args)
                await MainActor.run {
                    isPreviewing = false
                    dryRunResult = result
                }
            } catch {
                await MainActor.run {
                    isPreviewing = false
                    previewError = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
                }
            }
        }
    }

    private var pathRow: some View {
        HStack(spacing: 4) {
            Text(configStore.activeProfile.sourcePath)
                .font(.caption).foregroundColor(.secondary)
                .lineLimit(1).truncationMode(.middle)
            Image(systemName: "arrow.right").font(.caption2).foregroundColor(.secondary)
            Text(configStore.activeProfile.destPath.isEmpty ? "Not configured" : configStore.activeProfile.destPath)
                .font(.caption)
                .foregroundColor(configStore.activeProfile.destPath.isEmpty ? .red : .secondary)
                .lineLimit(1).truncationMode(.middle)
        }
    }

    private var progressView: some View {
        VStack(alignment: .leading, spacing: 6) {
            if syncEngine.progress.filesTotal > 0 {
                ProgressView(value: syncEngine.progress.percentComplete)
                    .progressViewStyle(.linear)
            } else {
                ProgressView()
                    .progressViewStyle(.linear)
            }
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    if syncEngine.progress.filesTotal > 0 {
                        Text("\(syncEngine.progress.filesDone) / \(syncEngine.progress.filesTotal) files")
                            .font(.caption).foregroundColor(.secondary)
                    } else {
                        Text("Preparing…")
                            .font(.caption).foregroundColor(.secondary)
                    }
                    if !syncEngine.progress.currentFile.isEmpty {
                        Text(syncEngine.progress.currentFile)
                            .font(.caption2).foregroundColor(.secondary)
                            .lineLimit(1).truncationMode(.middle)
                    }
                }
                Spacer()
                Button("Cancel") { syncEngine.cancel(); onDismiss() }.font(.caption)
            }
        }
    }

    private var doneView: some View {
        HStack {
            Image(systemName: "checkmark.circle.fill").foregroundColor(.green)
            Text("Sync complete — \(syncEngine.progress.filesDone) files").font(.subheadline)
            Spacer()
            Button("Dismiss", action: onDismiss).font(.caption)
        }
    }

    private func errorView(_ message: String) -> some View {
        HStack(alignment: .top) {
            Image(systemName: "exclamationmark.triangle.fill").foregroundColor(.orange)
            Text(message).font(.caption).lineLimit(2)
            Spacer()
            VStack(spacing: 4) {
                Button("Retry", action: onStart).font(.caption)
                Button("Dismiss", action: onDismiss).font(.caption)
            }
        }
    }
}
