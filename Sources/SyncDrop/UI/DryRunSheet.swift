import SwiftUI
import SyncDropCore

struct DryRunSheet: View {
    let result: DryRunResult
    let onConfirm: () -> Void
    let onCancel: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Preview of Changes").font(.headline)

            HStack(spacing: 16) {
                countLabel(systemImage: "plus.circle.fill", color: .green,
                           count: result.toCopy, label: "to add")
                countLabel(systemImage: "arrow.triangle.2.circlepath.circle.fill", color: .blue,
                           count: result.toUpdate, label: "to update")
                countLabel(systemImage: "minus.circle.fill", color: .red,
                           count: result.toDelete, label: "to delete")
            }

            Divider()

            if result.files.isEmpty {
                Text("Everything is already up to date.")
                    .foregroundColor(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 120)
            } else {
                List(result.files) { file in
                    HStack(spacing: 8) {
                        Image(systemName: icon(for: file.action))
                            .foregroundColor(color(for: file.action))
                        Text(file.path)
                            .font(.caption)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                }
                .frame(minHeight: 180)
            }

            HStack {
                Button("Cancel", action: onCancel).keyboardShortcut(.cancelAction)
                Spacer()
                Button("Sync Now", action: onConfirm)
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(result.files.isEmpty)
            }
        }
        .padding(18)
        .frame(width: 420, height: 360)
    }

    private func countLabel(systemImage: String, color: Color, count: Int, label: String) -> some View {
        HStack(spacing: 4) {
            Image(systemName: systemImage).foregroundColor(color)
            Text("\(count) \(label)").font(.subheadline)
        }
    }

    private func icon(for action: DryRunFile.Action) -> String {
        switch action {
        case .add:    return "plus.circle.fill"
        case .update: return "arrow.triangle.2.circlepath.circle.fill"
        case .delete: return "minus.circle.fill"
        }
    }

    private func color(for action: DryRunFile.Action) -> Color {
        switch action {
        case .add:    return .green
        case .update: return .blue
        case .delete: return .red
        }
    }
}
