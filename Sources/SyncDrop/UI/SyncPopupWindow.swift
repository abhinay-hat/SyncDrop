import AppKit
import SwiftUI
import SyncDropCore

@MainActor
class SyncPopupWindow: NSObject {
    private var panel: NSPanel?
    private let configStore: ConfigStore
    private let syncEngine: SyncEngine
    private var autoDismissTimer: Timer?

    init(configStore: ConfigStore, syncEngine: SyncEngine) {
        self.configStore = configStore
        self.syncEngine = syncEngine
        super.init()
        setupPanel()
        NotificationCenter.default.addObserver(
            forName: .syncDidComplete, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.scheduleAutoDismiss() }
        }
    }

    func showPopup() {
        autoDismissTimer?.invalidate()
        autoDismissTimer = nil
        guard let panel = panel else { return }
        positionPanel(panel)
        panel.orderFrontRegardless()
    }

    private func setupPanel() {
        let content = SyncPopupContentView(
            syncEngine: syncEngine,
            configStore: configStore,
            onStart: { [weak self] in self?.syncEngine.start() },
            onDismiss: { [weak self] in self?.panel?.orderOut(nil) }
        )
        let hosting = NSHostingView(rootView: content)

        let p = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 380, height: 150),
            styleMask: [.hudWindow, .utilityWindow, .titled, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        p.title = "SyncDrop"
        p.isFloatingPanel = true
        p.level = .floating
        p.contentView = hosting
        p.isReleasedWhenClosed = false
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel = p
    }

    private func positionPanel(_ panel: NSPanel) {
        guard let screen = NSScreen.main else { return }
        let margin: CGFloat = 16
        let menuBarH = NSStatusBar.system.thickness
        let size = panel.frame.size
        let x = screen.frame.maxX - size.width - margin
        let y = screen.frame.maxY - menuBarH - size.height - margin
        panel.setFrameOrigin(NSPoint(x: x, y: y))
    }

    deinit {
        autoDismissTimer?.invalidate()
        NotificationCenter.default.removeObserver(self)
    }

    private func scheduleAutoDismiss() {
        // Cancel any in-flight timer so repeated completions don't stack timers.
        autoDismissTimer?.invalidate()
        autoDismissTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.panel?.orderOut(nil) }
        }
    }
}
