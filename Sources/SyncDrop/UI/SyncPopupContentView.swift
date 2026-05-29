import SwiftUI
import SyncDropCore

@MainActor
struct SyncPopupContentView: View {
    @ObservedObject var syncEngine: SyncEngine
    @ObservedObject var configStore: ConfigStore
    let onStart: () -> Void
    let onDismiss: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            headerRow
            Divider()
            stateContent
        }
        .padding(18)
        .frame(width: 380)
    }

    private var headerRow: some View {
        HStack(spacing: 10) {
            Image(systemName: "externaldrive.fill")
                .font(.title2)
                .foregroundColor(.accentColor)
            VStack(alignment: .leading, spacing: 2) {
                Text(configStore.ssdName).font(.headline)
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
            HStack {
                Button("Cancel", action: onDismiss).keyboardShortcut(.cancelAction)
                Spacer()
                Button("Start Sync", action: onStart)
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
            }
        }
    }

    private var pathRow: some View {
        HStack(spacing: 4) {
            Text(configStore.sourcePath)
                .font(.caption).foregroundColor(.secondary)
                .lineLimit(1).truncationMode(.middle)
            Image(systemName: "arrow.right").font(.caption2).foregroundColor(.secondary)
            Text(configStore.destPath.isEmpty ? "Not configured" : configStore.destPath)
                .font(.caption)
                .foregroundColor(configStore.destPath.isEmpty ? .red : .secondary)
                .lineLimit(1).truncationMode(.middle)
        }
    }

    private var progressView: some View {
        VStack(alignment: .leading, spacing: 8) {
            ProgressView(value: syncEngine.progress.percentComplete).progressViewStyle(.linear)
            HStack {
                Text("\(syncEngine.progress.filesDone) / \(max(syncEngine.progress.filesTotal, 1)) files")
                    .font(.caption).foregroundColor(.secondary)
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
