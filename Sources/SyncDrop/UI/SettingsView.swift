import SwiftUI
import SyncDropCore

struct SettingsView: View {
    @ObservedObject var configStore: ConfigStore
    @ObservedObject var syncEngine: SyncEngine

    var body: some View {
        TabView {
            FoldersTab(configStore: configStore)
                .tabItem { Label("Folders", systemImage: "folder") }
            BehaviorTab(configStore: configStore)
                .tabItem { Label("Behavior", systemImage: "gearshape") }
            HistoryTab(configStore: configStore)
                .tabItem { Label("History", systemImage: "clock") }
        }
        .frame(width: 500, height: 320)
        .padding(.top, 8)
    }
}

private struct FoldersTab: View {
    @ObservedObject var configStore: ConfigStore

    var body: some View {
        Form {
            Section("Source (Mac)") {
                HStack {
                    Text(configStore.sourcePath)
                        .foregroundColor(.secondary).lineLimit(1).truncationMode(.middle)
                    Spacer()
                    Button("Choose…") { pickSource() }
                }
            }
            Section("SSD Volume Name") {
                TextField("Extreme Pro", text: $configStore.ssdName)
                    .textFieldStyle(.roundedBorder)
                    .help("Must match the exact volume name shown in Finder when SSD is connected")
            }
            Section("Destination (on SSD)") {
                HStack {
                    Text(configStore.destPath.isEmpty ? "Not set — plug in SSD then choose" : configStore.destPath)
                        .foregroundColor(configStore.destPath.isEmpty ? .red : .secondary)
                        .lineLimit(1).truncationMode(.middle)
                    Spacer()
                    Button("Choose…") { pickDest() }
                }
            }
            Section("Exclude Patterns") {
                ForEach(configStore.excludes.indices, id: \.self) { index in
                    HStack {
                        TextField("pattern", text: Binding(
                            get: { configStore.excludes[index] },
                            set: { configStore.excludes[index] = $0 }
                        ))
                        .textFieldStyle(.roundedBorder)
                        Button(role: .destructive) {
                            configStore.excludes.remove(at: index)
                        } label: {
                            Image(systemName: "minus.circle.fill")
                        }
                        .buttonStyle(.borderless)
                    }
                }
                Button {
                    configStore.excludes.append("")
                } label: {
                    Label("Add Pattern", systemImage: "plus")
                }
            }
        }
        .formStyle(.grouped)
        .padding(.horizontal)
    }

    private func pickSource() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose source folder on your Mac"
        if panel.runModal() == .OK, let url = panel.url {
            configStore.sourcePath = url.path.replacingOccurrences(of: NSHomeDirectory(), with: "~")
        }
    }

    private func pickDest() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose destination folder on your SSD"
        panel.directoryURL = URL(fileURLWithPath: "/Volumes")
        if panel.runModal() == .OK, let url = panel.url {
            configStore.destPath = url.path
        }
    }
}

private struct BehaviorTab: View {
    @ObservedObject var configStore: ConfigStore

    var body: some View {
        Form {
            Section("Sync") {
                Toggle("Auto-sync when SSD connected", isOn: $configStore.autoSync)
                Toggle("Mirror mode — delete files removed from Mac", isOn: $configStore.mirrorMode)
                    .help("Adds --delete to rsync. Files deleted on Mac are also deleted from SSD.")
                Toggle("Notify when sync completes", isOn: $configStore.notifyOnComplete)
            }
            Section("System") {
                Toggle("Launch at login", isOn: Binding(
                    get: { configStore.launchAtLogin },
                    set: { on in
                        configStore.launchAtLogin = on
                        LoginItemManager.setEnabled(on)
                    }
                ))
                .help("Requires app installed in /Applications or ~/Applications")
            }
        }
        .formStyle(.grouped)
        .padding(.horizontal)
    }
}

private struct HistoryTab: View {
    @ObservedObject var configStore: ConfigStore

    var body: some View {
        VStack {
            if configStore.syncHistory.isEmpty {
                Text("No sync history yet")
                    .foregroundColor(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(configStore.syncHistory) { record in
                    HStack(spacing: 8) {
                        Image(systemName: record.succeeded ? "checkmark.circle.fill" : "xmark.circle.fill")
                            .foregroundColor(record.succeeded ? .green : .red)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(record.formattedDate).font(.subheadline)
                            Text("\(record.fileCount) files · \(record.formattedSize) · \(record.formattedDuration)")
                                .font(.caption).foregroundColor(.secondary)
                        }
                    }
                }
            }
            HStack {
                Spacer()
                Button("Clear History") { configStore.syncHistory = [] }
                    .buttonStyle(.bordered).padding([.bottom, .trailing])
            }
        }
    }
}
