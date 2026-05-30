import SwiftUI
import SyncDropCore

struct SettingsView: View {
    @ObservedObject var configStore: ConfigStore
    @ObservedObject var syncEngine: SyncEngine

    var body: some View {
        TabView {
            ProfilesTab(configStore: configStore)
                .tabItem { Label("Profiles", systemImage: "person.2") }
            FoldersTab(configStore: configStore)
                .tabItem { Label("Folders", systemImage: "folder") }
            BehaviorTab(configStore: configStore)
                .tabItem { Label("Behavior", systemImage: "gearshape") }
            HistoryTab(configStore: configStore)
                .tabItem { Label("History", systemImage: "clock") }
        }
        .frame(width: 500, height: 360)
        .padding(.top, 8)
    }
}

private struct ProfilesTab: View {
    @ObservedObject var configStore: ConfigStore

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Selecting a profile makes it active. Edit its folders and behavior in the other tabs.")
                .font(.caption).foregroundColor(.secondary)
                .padding(.horizontal)

            List {
                ForEach(configStore.profiles) { profile in
                    HStack {
                        Image(systemName: profile.id == configStore.activeProfileId ? "checkmark.circle.fill" : "circle")
                            .foregroundColor(profile.id == configStore.activeProfileId ? .accentColor : .secondary)
                            .onTapGesture { configStore.activeProfileId = profile.id }
                        TextField("Profile name", text: Binding(
                            get: { profile.name },
                            set: { newName in
                                if let idx = configStore.profiles.firstIndex(where: { $0.id == profile.id }) {
                                    configStore.profiles[idx].name = newName
                                }
                            }
                        ))
                        .textFieldStyle(.roundedBorder)
                        Spacer()
                        Button(role: .destructive) {
                            configStore.deleteProfile(id: profile.id)
                        } label: {
                            Image(systemName: "trash")
                        }
                        .buttonStyle(.borderless)
                        .disabled(configStore.profiles.count <= 1)
                    }
                }
            }

            HStack {
                Spacer()
                Button {
                    configStore.addProfile()
                } label: {
                    Label("Add Profile", systemImage: "plus")
                }
                .buttonStyle(.bordered)
                .padding([.bottom, .trailing])
            }
        }
        .padding(.top, 4)
    }
}

private struct FoldersTab: View {
    @ObservedObject var configStore: ConfigStore

    private var profile: Binding<SyncProfile> {
        Binding(get: { configStore.activeProfile }, set: { configStore.activeProfile = $0 })
    }

    var body: some View {
        Form {
            Section("Source (Mac)") {
                HStack {
                    Text(configStore.activeProfile.sourcePath)
                        .foregroundColor(.secondary).lineLimit(1).truncationMode(.middle)
                    Spacer()
                    Button("Choose…") { pickSource() }
                }
            }
            Section("SSD Volume Name") {
                TextField("Extreme Pro", text: profile.ssdName)
                    .textFieldStyle(.roundedBorder)
                    .help("Must match the exact volume name shown in Finder when SSD is connected")
            }
            Section("Destination (on SSD)") {
                HStack {
                    Text(configStore.activeProfile.destPath.isEmpty ? "Not set — plug in SSD then choose" : configStore.activeProfile.destPath)
                        .foregroundColor(configStore.activeProfile.destPath.isEmpty ? .red : .secondary)
                        .lineLimit(1).truncationMode(.middle)
                    Spacer()
                    Button("Choose…") { pickDest() }
                }
            }
            Section("Exclude Patterns") {
                ForEach(configStore.activeProfile.excludes.indices, id: \.self) { index in
                    HStack {
                        TextField("pattern", text: Binding(
                            get: { configStore.activeProfile.excludes[index] },
                            set: { var p = configStore.activeProfile; p.excludes[index] = $0; configStore.activeProfile = p }
                        ))
                        .textFieldStyle(.roundedBorder)
                        Button(role: .destructive) {
                            var p = configStore.activeProfile
                            p.excludes.remove(at: index)
                            configStore.activeProfile = p
                        } label: {
                            Image(systemName: "minus.circle.fill")
                        }
                        .buttonStyle(.borderless)
                    }
                }
                Button {
                    var p = configStore.activeProfile
                    p.excludes.append("")
                    configStore.activeProfile = p
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
            var p = configStore.activeProfile
            p.sourcePath = url.path.replacingOccurrences(of: NSHomeDirectory(), with: "~")
            configStore.activeProfile = p
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
            var p = configStore.activeProfile
            p.destPath = url.path
            configStore.activeProfile = p
        }
    }
}

private struct BehaviorTab: View {
    @ObservedObject var configStore: ConfigStore

    private var profile: Binding<SyncProfile> {
        Binding(get: { configStore.activeProfile }, set: { configStore.activeProfile = $0 })
    }

    var body: some View {
        Form {
            Section("Sync") {
                Toggle("Auto-sync when SSD connected", isOn: profile.autoSync)
                Toggle("Mirror mode — delete files removed from Mac", isOn: profile.mirrorMode)
                    .help("Adds --delete to rsync. Files deleted on Mac are also deleted from SSD.")
                Toggle("Notify when sync completes", isOn: $configStore.notifyOnComplete)
                Toggle("Eject SSD after sync completes", isOn: profile.autoEject)
                    .help("Automatically ejects the SSD when a sync finishes successfully.")
                Toggle("Keep versions of replaced files", isOn: profile.keepVersions)
                    .help("Moves overwritten/deleted files into .syncdrop_archive/<date> on the SSD instead of discarding them.")
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
