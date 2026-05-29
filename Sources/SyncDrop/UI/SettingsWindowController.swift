import AppKit
import SwiftUI
import SyncDropCore

@MainActor
class SettingsWindowController: NSWindowController {
    private let configStore: ConfigStore
    private let syncEngine: SyncEngine

    init(configStore: ConfigStore, syncEngine: SyncEngine) {
        self.configStore = configStore
        self.syncEngine = syncEngine
        super.init(window: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    func showWindow() {
        if window == nil {
            let view = SettingsView(configStore: configStore, syncEngine: syncEngine)
            let hosting = NSHostingController(rootView: view)
            let w = NSWindow(contentViewController: hosting)
            w.title = "SyncDrop Settings"
            w.styleMask = [.titled, .closable, .miniaturizable]
            w.setContentSize(NSSize(width: 520, height: 360))
            w.center()
            self.window = w
        }
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
}
