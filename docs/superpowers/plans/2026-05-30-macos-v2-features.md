# macOS SyncDrop v2 Features — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add exclude rules, auto-eject, versioned backups, dry-run preview, and multi-profile support to the SyncDrop macOS app.

**Architecture:** Five independent features layered onto the existing ConfigStore/SyncEngine/UI stack. Features 1–3 are pure additions (new config keys + rsync args + UI toggles). Feature 4 adds a new DryRunEngine component. Feature 5 is the largest: a data-model refactor replacing single-config with a profiles array, plus UI to manage profiles.

**Tech Stack:** Swift 5.9, SwiftUI, AppKit, NSWorkspace, XCTest

---

## Conventions for every task

- **Build command:** `cd ~/SyncDrop && swift build 2>&1 | tail -5` — success when the output contains `Build complete!`
- **Test command:** `cd ~/SyncDrop && swift test 2>&1 | tail -20` — success when the output contains `Test Suite 'All tests' passed`
  - If `swift test` reports `no such module 'XCTest'`, your toolchain's bare `swift` can't find the macOS SDK's XCTest. Run via the Xcode toolchain instead: `cd ~/SyncDrop && xcodebuild test -scheme SyncDrop -destination 'platform=macOS' 2>&1 | tail -30`, or ensure `xcode-select -p` points at `Xcode.app` (not the standalone CLT) and retry `swift test`. `swift build` is unaffected.
- **Install (manual verification only):** `cd ~/SyncDrop && make install`
- Each task ends with a `git commit`. The repo currently has no `.git`; **Task 0** initializes it.
- Do not move on to the next task until the build and tests for the current task pass.

### Important ordering note

Features 1–3 add flat fields directly to `ConfigStore` (`excludes`, `autoEject`, `keepVersions`). Feature 5 then folds **all** per-sync fields (including those three plus the original `sourcePath`/`ssdName`/`destPath`/`mirrorMode`/`autoSync`) into a `SyncProfile` struct and rewrites every call site in a single commit. This churn is inherent to the requested feature order and is intentional — do not attempt to build profiles first. `notifyOnComplete` and `launchAtLogin` remain **global** (not per-profile).

---

## Task 0: Initialize git repository and fix a pre-existing red test

- [ ] Initialize the repo, fix one inherited test assertion that is already failing, then make a baseline commit so subsequent tasks have a clean, green history.

```bash
cd ~/SyncDrop
git init
cat > .gitignore <<'EOF'
.build/
*.app/
.DS_Store
*.xcuserstate
EOF
git add -A
```

### 0a. Fix the inherited `test_rsyncArgs_noMinusA` assertion (already red)

The existing test in `Tests/SyncDropTests/SyncEngineTests.swift` is broken against the current code:

```swift
    func test_rsyncArgs_noMinusA() {
        XCTAssertFalse(engine.rsyncArgs.contains("-a"))
        XCTAssertFalse(engine.rsyncArgs.contains { $0.hasPrefix("-") && $0.contains("a") })  // BUG
    }
```

The second assertion was meant to ensure no short-flag group bundles `a`, but it also matches `--stats` (`"--stats".hasPrefix("-")` is true and `"--stats".contains("a")` is true — the "a" in st**a**ts), so it fails. Verified empirically: `args.contains { $0.hasPrefix("-") && $0.contains("a") }` returns `true` because of `--stats`.

Fix it to only inspect the short-flag token (the bundled `-rltDv...` group), excluding `--`-prefixed long options:

```swift
    func test_rsyncArgs_noMinusA() {
        XCTAssertFalse(engine.rsyncArgs.contains("-a"))
        // Only the bundled short-flag group (e.g. "-rltDv") must not contain 'a';
        // long options like "--stats" are exempt.
        let shortFlagGroups = engine.rsyncArgs.filter { $0.hasPrefix("-") && !$0.hasPrefix("--") }
        XCTAssertFalse(shortFlagGroups.contains { $0.contains("a") })
    }
```

> The Task 5 rewrite of `SyncEngineTests` already incorporates this corrected assertion — do not reintroduce the buggy form.

**Verify:**

```bash
cd ~/SyncDrop && swift build 2>&1 | tail -5
cd ~/SyncDrop && swift test 2>&1 | tail -20
```

Both must pass before committing the baseline. If `swift test` cannot find XCTest, use the `xcodebuild test` fallback noted in Conventions.

**Commit:**

```bash
cd ~/SyncDrop && git commit -m "chore: initialize git repo with baseline v1 source"
```

---

## Task 1: Exclude rules

**Files:**
- `Sources/SyncDropCore/ConfigStore.swift` (edit)
- `Sources/SyncDropCore/SyncEngine.swift` (edit)
- `Sources/SyncDrop/UI/SettingsView.swift` (edit)
- `Tests/SyncDropTests/ConfigStoreTests.swift` (edit)
- `Tests/SyncDropTests/SyncEngineTests.swift` (edit)

### 1a. Add `excludes` to ConfigStore

In `Sources/SyncDropCore/ConfigStore.swift`, add a published `excludes` property after the `launchAtLogin` property (before `syncHistory`):

```swift
    @Published public var excludes: [String] {
        didSet {
            if let data = try? JSONEncoder().encode(excludes) {
                defaults.set(data, forKey: Keys.excludes)
            }
        }
    }
```

Add the key to the `Keys` enum (after `launchAtLogin`):

```swift
        static let excludes = "excludes"
```

In `init`, load it (place after the `launchAtLogin` line, before the `syncHistory` decode block). Use the default list when no value is stored:

```swift
        if let data = defaults.data(forKey: Keys.excludes),
           let stored = try? JSONDecoder().decode([String].self, from: data) {
            self.excludes = stored
        } else {
            self.excludes = [".DS_Store", ".Spotlight-V100", ".fseventsd", ".Trashes", "node_modules"]
        }
```

> Note: `excludes` must be initialized in `init` before any other stored property that the compiler considers, but since Swift requires all stored properties be set before `init` returns and these are independent, simply placing the block as shown is correct. Set it before the `syncHistory` block.

### 1b. Emit `--exclude=` args in SyncEngine

In `Sources/SyncDropCore/SyncEngine.swift`, update the `rsyncArgs` computed property so excludes are appended **after** `--delete` but **before** the source/dest paths:

```swift
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
        for pattern in configStore.excludes where !pattern.trimmingCharacters(in: .whitespaces).isEmpty {
            args.append("--exclude=\(pattern)")
        }
        // Trailing slash on source tells rsync to copy *contents*, not the dir itself
        args += [configStore.expandedSourcePath + "/", configStore.destPath]
        return args
    }
```

### 1c. Settings → Folders tab editable exclude list

In `Sources/SyncDrop/UI/SettingsView.swift`, add a new `Section` to `FoldersTab`'s `Form`, after the "Destination (on SSD)" section:

```swift
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
```

> Editing `configStore.excludes[index]` directly works because mutating an element of a `@Published var` array triggers the `didSet`, which re-persists the array.

### 1d. Tests

In `Tests/SyncDropTests/ConfigStoreTests.swift`, add to `test_defaults_areCorrect`:

```swift
        XCTAssertEqual(store.excludes, [".DS_Store", ".Spotlight-V100", ".fseventsd", ".Trashes", "node_modules"])
```

And add a new test:

```swift
    func test_excludes_saveAndLoad() {
        store.excludes = ["*.tmp", "build"]
        let defaults = UserDefaults(suiteName: "SyncDropTests")!
        let reloaded = ConfigStore(defaults: defaults)
        XCTAssertEqual(reloaded.excludes, ["*.tmp", "build"])
    }
```

In `Tests/SyncDropTests/SyncEngineTests.swift`, add:

```swift
    func test_rsyncArgs_includesExcludePatterns() {
        configStore.excludes = [".DS_Store", "node_modules"]
        let args = engine.rsyncArgs
        XCTAssertTrue(args.contains("--exclude=.DS_Store"))
        XCTAssertTrue(args.contains("--exclude=node_modules"))
    }

    func test_rsyncArgs_excludesBeforeSourceDest() {
        configStore.excludes = ["node_modules"]
        let args = engine.rsyncArgs
        let excludeIdx = args.firstIndex(of: "--exclude=node_modules")!
        let sourceIdx = args.firstIndex(where: { $0.contains("Desktop/Projects") })!
        XCTAssertLessThan(excludeIdx, sourceIdx)
    }

    func test_rsyncArgs_skipsBlankExcludes() {
        configStore.excludes = ["", "  ", "real"]
        let args = engine.rsyncArgs
        XCTAssertEqual(args.filter { $0.hasPrefix("--exclude=") }, ["--exclude=real"])
    }
```

> Note: `setUp` in `SyncEngineTests` builds a fresh `ConfigStore` from the `SyncEngineTests` suite, so `configStore.excludes` starts at the default list. Tests that assert exact exclude args set `configStore.excludes` explicitly first (as shown).

**Verify:**

```bash
cd ~/SyncDrop && swift build 2>&1 | tail -5
cd ~/SyncDrop && swift test 2>&1 | tail -20
```

**Commit:**

```bash
cd ~/SyncDrop && git add -A && git commit -m "feat: add configurable rsync exclude patterns"
```

---

## Task 2: Auto-eject after sync

**Files:**
- `Sources/SyncDropCore/ConfigStore.swift` (edit)
- `Sources/SyncDrop/AppDelegate.swift` (edit)
- `Sources/SyncDrop/UI/SettingsView.swift` (edit)
- `Tests/SyncDropTests/ConfigStoreTests.swift` (edit)

> **Design decision:** `SyncEngine` lives in `SyncDropCore`, which imports only `Foundation`/`Combine`/`UserNotifications` — it must not depend on AppKit. `NSWorkspace.unmountAndEjectDevice` and Finder eject belong in the app layer. `AppDelegate` already owns SSD lifecycle and imports AppKit, so the eject logic goes there, triggered by the existing `.syncDidComplete` notification that `SyncEngine` already posts on success.

### 2a. Add `autoEject` to ConfigStore

In `Sources/SyncDropCore/ConfigStore.swift`, add after `excludes` (or after `launchAtLogin`):

```swift
    @Published public var autoEject: Bool {
        didSet { defaults.set(autoEject, forKey: Keys.autoEject) }
    }
```

Add the key:

```swift
        static let autoEject = "autoEject"
```

In `init`, after the `launchAtLogin` line:

```swift
        self.autoEject = defaults.bool(forKey: Keys.autoEject)
```

### 2b. Eject in AppDelegate on sync completion

In `Sources/SyncDrop/AppDelegate.swift`, add a `.syncDidComplete` observer. Inside `applicationDidFinishLaunching`, after `volumeMonitor.start()`, register the observer:

```swift
        NotificationCenter.default.addObserver(
            forName: .syncDidComplete, object: nil, queue: .main
        ) { [weak self] _ in
            Task { @MainActor in self?.handleSyncCompleted() }
        }
```

Add the handler methods to the class:

```swift
    private func handleSyncCompleted() {
        guard configStore.autoEject else { return }
        let path = "/Volumes/\(configStore.ssdName)"
        do {
            try NSWorkspace.shared.unmountAndEjectDevice(at: URL(fileURLWithPath: path))
        } catch {
            postEjectFailureNotification()
        }
    }

    private func postEjectFailureNotification() {
        let content = UNMutableNotificationContent()
        content.title = "SyncDrop"
        content.body = "Could not eject — check Finder"
        UNUserNotificationCenter.current().add(
            UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil),
            withCompletionHandler: nil
        )
    }
```

> `NSWorkspace.shared.unmountAndEjectDevice(at:)` is the throwing URL-based variant (available on macOS 13+). On failure it throws, which we map to the failure notification.

### 2c. Settings → Behavior tab toggle

In `Sources/SyncDrop/UI/SettingsView.swift`, add to `BehaviorTab`'s `Sync` section after the `notifyOnComplete` toggle:

```swift
                Toggle("Eject SSD after sync completes", isOn: $configStore.autoEject)
                    .help("Automatically ejects the SSD when a sync finishes successfully.")
```

### 2d. Test

In `Tests/SyncDropTests/ConfigStoreTests.swift`, add to `test_defaults_areCorrect`:

```swift
        XCTAssertFalse(store.autoEject)
```

Add a new test:

```swift
    func test_autoEject_saveAndLoad() {
        store.autoEject = true
        let reloaded = ConfigStore(defaults: UserDefaults(suiteName: "SyncDropTests")!)
        XCTAssertTrue(reloaded.autoEject)
    }
```

> The eject side-effect itself is not unit-testable (it requires a mounted volume); verify it manually below.

### 2e. Manual verification

1. `cd ~/SyncDrop && make install`
2. Launch SyncDrop, open Settings → Behavior, enable "Eject SSD after sync completes".
3. Plug in the SSD, run a sync.
4. On completion the SSD should disappear from Finder. If it cannot eject (e.g. a file is open on it), a "Could not eject — check Finder" notification appears.

**Verify:**

```bash
cd ~/SyncDrop && swift build 2>&1 | tail -5
cd ~/SyncDrop && swift test 2>&1 | tail -20
```

**Commit:**

```bash
cd ~/SyncDrop && git add -A && git commit -m "feat: optionally eject SSD after a successful sync"
```

---

## Task 3: Keep versions (versioned backups)

**Files:**
- `Sources/SyncDropCore/ConfigStore.swift` (edit)
- `Sources/SyncDropCore/SyncEngine.swift` (edit)
- `Sources/SyncDrop/UI/SettingsView.swift` (edit)
- `Tests/SyncDropTests/ConfigStoreTests.swift` (edit)
- `Tests/SyncDropTests/SyncEngineTests.swift` (edit)

> **Design decision:** The backup-dir date must be computed once at sync start so it is stable for the whole run. `rsyncArgs` is a computed property re-evaluated on access; embedding `Date()` in it would make tests non-deterministic and could theoretically straddle midnight. We therefore make `rsyncArgs` take a date parameter (defaulting to "now") and have `start()` compute the date once and pass it in.

### 3a. Add `keepVersions` to ConfigStore

In `Sources/SyncDropCore/ConfigStore.swift`, add after `autoEject`:

```swift
    @Published public var keepVersions: Bool {
        didSet { defaults.set(keepVersions, forKey: Keys.keepVersions) }
    }
```

Add the key:

```swift
        static let keepVersions = "keepVersions"
```

In `init`, after the `autoEject` line:

```swift
        self.keepVersions = defaults.bool(forKey: Keys.keepVersions)
```

### 3b. Emit `--backup` args in SyncEngine

In `Sources/SyncDropCore/SyncEngine.swift`, replace the existing `rsyncArgs` computed property with a date-parameterized version, keeping a zero-arg computed property for existing callers/tests:

```swift
    /// Convenience: build args using the current date for the backup dir.
    public var rsyncArgs: [String] { rsyncArgs(date: Date()) }

    /// Builds the rsync argument list.
    /// exFAT-safe: omits -a (which sets -p/-o/-g and breaks on exFAT with
    /// EPERM). Uses --modify-window=1 to handle exFAT's 2-second timestamp
    /// granularity and avoid re-copying unchanged files every run.
    public func rsyncArgs(date: Date) -> [String] {
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
        if configStore.keepVersions {
            args.append("--backup")
            args.append("--backup-dir=.syncdrop_archive/\(Self.backupDateString(date))")
        }
        for pattern in configStore.excludes where !pattern.trimmingCharacters(in: .whitespaces).isEmpty {
            args.append("--exclude=\(pattern)")
        }
        // Trailing slash on source tells rsync to copy *contents*, not the dir itself
        args += [configStore.expandedSourcePath + "/", configStore.destPath]
        return args
    }

    nonisolated public static func backupDateString(_ date: Date) -> String {
        let fmt = DateFormatter()
        fmt.locale = Locale(identifier: "en_US_POSIX")
        fmt.timeZone = TimeZone.current
        fmt.dateFormat = "yyyy-MM-dd"
        return fmt.string(from: date)
    }
```

In `start()`, compute the date once and use it. Replace the line `p.arguments = rsyncArgs` with:

```swift
        let syncStartDate = Date()
        p.arguments = rsyncArgs(date: syncStartDate)
```

> `--backup-dir` is relative; rsync resolves it relative to the destination root, producing `<dest>/.syncdrop_archive/<YYYY-MM-DD>/` holding the pre-overwrite versions of changed/deleted files. The `--exclude` patterns still apply normally.

### 3c. Settings → Behavior tab toggle

In `Sources/SyncDrop/UI/SettingsView.swift`, add to `BehaviorTab`'s `Sync` section after the `autoEject` toggle:

```swift
                Toggle("Keep versions of replaced files", isOn: $configStore.keepVersions)
                    .help("Moves overwritten/deleted files into .syncdrop_archive/<date> on the SSD instead of discarding them.")
```

### 3d. Tests

In `Tests/SyncDropTests/ConfigStoreTests.swift`, add to `test_defaults_areCorrect`:

```swift
        XCTAssertFalse(store.keepVersions)
```

Add:

```swift
    func test_keepVersions_saveAndLoad() {
        store.keepVersions = true
        let reloaded = ConfigStore(defaults: UserDefaults(suiteName: "SyncDropTests")!)
        XCTAssertTrue(reloaded.keepVersions)
    }
```

In `Tests/SyncDropTests/SyncEngineTests.swift`, add:

```swift
    func test_rsyncArgs_keepVersionsOff_noBackup() {
        configStore.keepVersions = false
        let args = engine.rsyncArgs
        XCTAssertFalse(args.contains("--backup"))
        XCTAssertFalse(args.contains { $0.hasPrefix("--backup-dir=") })
    }

    func test_rsyncArgs_keepVersionsOn_addsBackupDir() {
        configStore.keepVersions = true
        let date = ISO8601DateFormatter().date(from: "2026-05-30T12:00:00Z")!
        let args = engine.rsyncArgs(date: date)
        XCTAssertTrue(args.contains("--backup"))
        XCTAssertTrue(args.contains("--backup-dir=.syncdrop_archive/2026-05-30"))
    }

    func test_backupDateString_formatsYYYYMMDD() {
        let date = ISO8601DateFormatter().date(from: "2026-01-09T00:00:00Z")!
        // Format in the same local calendar day; assert shape rather than exact value
        // to stay timezone-robust.
        let s = SyncEngine.backupDateString(date)
        XCTAssertTrue(s.range(of: #"^\d{4}-\d{2}-\d{2}$"#, options: .regularExpression) != nil)
    }
```

> The `2026-05-30` assertion uses noon UTC, which falls on 2026-05-30 in all real timezones, so it is deterministic. The shape test guards the formatter pattern independently of timezone.

**Verify:**

```bash
cd ~/SyncDrop && swift build 2>&1 | tail -5
cd ~/SyncDrop && swift test 2>&1 | tail -20
```

**Commit:**

```bash
cd ~/SyncDrop && git add -A && git commit -m "feat: add versioned backups via rsync --backup-dir"
```

---

## Task 4: Dry-run preview

**Files:**
- `Sources/SyncDropCore/DryRunEngine.swift` (new)
- `Sources/SyncDrop/UI/DryRunSheet.swift` (new)
- `Sources/SyncDrop/UI/SyncPopupContentView.swift` (edit)
- `Tests/SyncDropTests/DryRunEngineTests.swift` (new)

> **Design decision (add vs. update):** `DryRunResult` separates `toCopy` and `toUpdate`, but the spec's "`>` = copy/update" rule cannot distinguish them on its own. rsync's `--itemize-changes` emits a flag string per file; a newly created file looks like `>f+++++++++` (all `+` in the change positions), whereas an update has digits/letters (e.g. `>f.st......`). So: a line whose itemize token starts with `>` and contains `+++++++++` is an **add**; a line starting with `>` otherwise is an **update**; a line starting with `*deleting` is a **delete**. The line classifier is a `nonisolated static func` mirroring `parseProgress`, so it is unit-testable without spawning rsync.

### 4a. DryRunEngine

Create `Sources/SyncDropCore/DryRunEngine.swift`:

```swift
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

public struct DryRunResult: Equatable {
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
        try await withCheckedThrowingContinuation { continuation in
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/rsync")
            process.arguments = arguments

            let pipe = Pipe()
            process.standardOutput = pipe
            process.standardError = FileHandle.nullDevice

            process.terminationHandler = { proc in
                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                let text = String(data: data, encoding: .utf8) ?? ""
                // rsync exit 0 = success; 24 = files vanished (benign for preview).
                if proc.terminationStatus == 0 || proc.terminationStatus == 24 {
                    continuation.resume(returning: text)
                } else {
                    continuation.resume(throwing: DryRunError.rsyncFailed(proc.terminationStatus))
                }
            }

            do {
                try process.run()
            } catch {
                continuation.resume(throwing: DryRunError.launchFailed(error.localizedDescription))
            }
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

    /// Classifies a single itemize line. Returns nil for non-change lines
    /// (e.g. "sending incremental file list", "", summary stats).
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
```

### 4b. DryRunSheet

Create `Sources/SyncDrop/UI/DryRunSheet.swift`:

```swift
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
```

### 4c. Wire "Preview…" into SyncPopupContentView

In `Sources/SyncDrop/UI/SyncPopupContentView.swift`:

Add state to the struct (after the `onDismiss` property):

```swift
    @State private var isPreviewing = false
    @State private var dryRunResult: DryRunResult?
    @State private var previewError: String?
```

Replace `confirmView` with a version that has a "Preview…" button, presents the sheet, and shows loading/error state:

```swift
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
                .disabled(isPreviewing || configStore.destPath.isEmpty)
                Button("Start Sync", action: onStart)
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(configStore.destPath.isEmpty)
            }
        }
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
```

Add the preview runner method to the struct:

```swift
    private func runPreview() {
        previewError = nil
        isPreviewing = true
        let source = configStore.expandedSourcePath
        let dest = configStore.destPath
        let args = SyncEngine(configStore: configStore).rsyncArgs
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
```

> `DryRunResult` must be `Identifiable` for `.sheet(item:)`. Add `Identifiable` conformance and an `id` to `DryRunResult` in `DryRunEngine.swift`:
>
> ```swift
> public struct DryRunResult: Identifiable, Equatable {
>     public let id = UUID()
> ```
> (Insert `public let id = UUID()` as the first stored property and add `Identifiable` to the conformance list. Keep the existing `init` — `id` gets its default.)

> **Note on the 380×150 panel:** `DryRunSheet` declares its own `.frame(width: 420, height: 360)`, so the sheet sizes itself independently of the small host panel and is not clipped.

### 4d. Tests

Create `Tests/SyncDropTests/DryRunEngineTests.swift`:

```swift
import XCTest
@testable import SyncDropCore

final class DryRunEngineTests: XCTestCase {

    func test_classify_newFile_isAdd() {
        let line = ">f+++++++++ projects/new.txt"
        let file = DryRunEngine.classify(line: line)
        XCTAssertEqual(file?.action, .add)
        XCTAssertEqual(file?.path, "projects/new.txt")
    }

    func test_classify_changedFile_isUpdate() {
        let line = ">f.st...... projects/changed.txt"
        let file = DryRunEngine.classify(line: line)
        XCTAssertEqual(file?.action, .update)
        XCTAssertEqual(file?.path, "projects/changed.txt")
    }

    func test_classify_deletion_isDelete() {
        let line = "*deleting   projects/gone.txt"
        let file = DryRunEngine.classify(line: line)
        XCTAssertEqual(file?.action, .delete)
        XCTAssertEqual(file?.path, "projects/gone.txt")
    }

    func test_classify_nonChangeLine_isNil() {
        XCTAssertNil(DryRunEngine.classify(line: "sending incremental file list"))
        XCTAssertNil(DryRunEngine.classify(line: ""))
        XCTAssertNil(DryRunEngine.classify(line: "Number of files: 10"))
        XCTAssertNil(DryRunEngine.classify(line: "cd+++++++++ adir/")) // not a '>' received item
    }

    func test_parse_countsByAction() {
        let output = """
        sending incremental file list
        >f+++++++++ a.txt
        >f+++++++++ b.txt
        >f.st...... c.txt
        *deleting   d.txt

        Number of files: 4
        """
        let result = DryRunEngine.parse(output: output)
        XCTAssertEqual(result.toCopy, 2)
        XCTAssertEqual(result.toUpdate, 1)
        XCTAssertEqual(result.toDelete, 1)
        XCTAssertEqual(result.files.count, 4)
    }

    func test_parse_emptyOutput_isAllZero() {
        let result = DryRunEngine.parse(output: "sending incremental file list\n\n")
        XCTAssertEqual(result.toCopy, 0)
        XCTAssertEqual(result.toUpdate, 0)
        XCTAssertEqual(result.toDelete, 0)
        XCTAssertTrue(result.files.isEmpty)
    }
}
```

> The directory-line guard test (`cd+++++++++ adir/`) confirms only `>`-prefixed (received) items are counted, not directory-create flags (`c`).

### 4e. Manual verification

> **Verify the real rsync output format first.** Recent macOS ships **openrsync** as `/usr/bin/rsync` (the existing `SyncEngine` already accounts for its `to-check=` progress format). The unit tests validate `classify` against hand-written strings, so they pass regardless of what the binary actually emits — that masks any format mismatch. Before trusting the feature, run the dry-run by hand and confirm the lines match the classifier's assumptions (received items start with `>`, new files show `+++++++++`, deletions start with `*deleting`):
>
> ```bash
> /usr/bin/rsync --dry-run --itemize-changes -rltDv ~/Desktop/Projects/ "/Volumes/Extreme Pro/Projects"
> ```
>
> If openrsync rejects `--itemize-changes` or emits a different format, adjust `DryRunEngine.classify` accordingly (and update its unit tests to match the real format).

1. `cd ~/SyncDrop && make install`
2. Plug in SSD, click the menu bar icon to show the popup.
3. Click "Preview…". A spinner appears, then a sheet listing adds/updates/deletes with counts.
4. Click "Sync Now" in the sheet — it dismisses and the real sync starts (popup transitions to the running state).

**Verify:**

```bash
cd ~/SyncDrop && swift build 2>&1 | tail -5
cd ~/SyncDrop && swift test 2>&1 | tail -20
```

**Commit:**

```bash
cd ~/SyncDrop && git add -A && git commit -m "feat: add dry-run preview of sync changes"
```

---

## Task 5: Multiple sync profiles

This is the largest task: it introduces `SyncProfile`, refactors `ConfigStore` to hold a profiles array, migrates v1 settings, and rewrites **every** call site that read the old flat fields — all in **one commit** so the build is never red.

**Files:**
- `Sources/SyncDropCore/SyncProfile.swift` (new)
- `Sources/SyncDropCore/ConfigStore.swift` (rewrite)
- `Sources/SyncDropCore/SyncEngine.swift` (edit — read from `activeProfile`)
- `Sources/SyncDropCore/VolumeMonitor.swift` (edit — read `activeProfile.ssdName`)
- `Sources/SyncDrop/AppDelegate.swift` (edit — read `activeProfile`)
- `Sources/SyncDrop/UI/MenuBarController.swift` (edit — profile switcher submenu + read `activeProfile.ssdName`)
- `Sources/SyncDrop/UI/SettingsView.swift` (edit — bind tabs to `activeProfile`, add Profiles tab)
- `Sources/SyncDrop/UI/SyncPopupContentView.swift` (edit — read `activeProfile`)
- `Tests/SyncDropTests/ConfigStoreTests.swift` (rewrite to use `activeProfile`, add migration test)
- `Tests/SyncDropTests/SyncEngineTests.swift` (rewrite to set fields via `activeProfile`)

### 5a. SyncProfile

Create `Sources/SyncDropCore/SyncProfile.swift`:

```swift
import Foundation

public struct SyncProfile: Identifiable, Codable, Equatable {
    public var id: UUID
    public var name: String
    public var sourcePath: String
    public var destPath: String
    public var ssdName: String
    public var mirrorMode: Bool
    public var autoSync: Bool
    public var autoEject: Bool
    public var keepVersions: Bool
    public var excludes: [String]

    public init(
        id: UUID = UUID(),
        name: String,
        sourcePath: String = "~/Desktop/Projects",
        destPath: String = "",
        ssdName: String = "Extreme Pro",
        mirrorMode: Bool = false,
        autoSync: Bool = false,
        autoEject: Bool = false,
        keepVersions: Bool = false,
        excludes: [String] = [".DS_Store", ".Spotlight-V100", ".fseventsd", ".Trashes", "node_modules"]
    ) {
        self.id = id
        self.name = name
        self.sourcePath = sourcePath
        self.destPath = destPath
        self.ssdName = ssdName
        self.mirrorMode = mirrorMode
        self.autoSync = autoSync
        self.autoEject = autoEject
        self.keepVersions = keepVersions
        self.excludes = excludes
    }

    public var expandedSourcePath: String {
        (sourcePath as NSString).expandingTildeInPath
    }
}
```

### 5b. Rewrite ConfigStore

Replace the entire contents of `Sources/SyncDropCore/ConfigStore.swift`:

```swift
import Foundation
import Combine

@MainActor
public final class ConfigStore: ObservableObject {
    private let defaults: UserDefaults

    @Published public var profiles: [SyncProfile] {
        didSet { persistProfiles() }
    }
    @Published public var activeProfileId: UUID {
        didSet { defaults.set(activeProfileId.uuidString, forKey: Keys.activeProfileId) }
    }

    // Global (not per-profile) settings.
    @Published public var notifyOnComplete: Bool {
        didSet { defaults.set(notifyOnComplete, forKey: Keys.notifyOnComplete) }
    }
    @Published public var launchAtLogin: Bool {
        didSet { defaults.set(launchAtLogin, forKey: Keys.launchAtLogin) }
    }
    @Published public var syncHistory: [SyncRecord] = [] {
        didSet {
            if let data = try? JSONEncoder().encode(syncHistory) {
                defaults.set(data, forKey: Keys.syncHistory)
            }
        }
    }

    private enum Keys {
        static let profiles = "profiles"
        static let activeProfileId = "activeProfileId"
        static let notifyOnComplete = "notifyOnComplete"
        static let launchAtLogin = "launchAtLogin"
        static let syncHistory = "syncHistory"
        // Legacy v1 keys (used only for one-time migration).
        static let legacySourcePath = "sourcePath"
        static let legacySsdName = "ssdName"
        static let legacyDestPath = "destPath"
        static let legacyAutoSync = "autoSync"
        static let legacyMirrorMode = "mirrorMode"
        static let legacyExcludes = "excludes"
        static let legacyAutoEject = "autoEject"
        static let legacyKeepVersions = "keepVersions"
    }

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults

        // Load globals first.
        self.notifyOnComplete = defaults.object(forKey: Keys.notifyOnComplete) as? Bool ?? true
        self.launchAtLogin = defaults.bool(forKey: Keys.launchAtLogin)

        // Load profiles, or migrate from v1, or seed a default.
        if let data = defaults.data(forKey: Keys.profiles),
           let stored = try? JSONDecoder().decode([SyncProfile].self, from: data),
           !stored.isEmpty {
            self.profiles = stored
        } else if let migrated = ConfigStore.migrateLegacyProfile(from: defaults) {
            self.profiles = [migrated]
        } else {
            self.profiles = [SyncProfile(name: "Default")]
        }

        // Resolve active profile id; fall back to first profile.
        let resolvedProfiles = self.profiles
        if let idString = defaults.string(forKey: Keys.activeProfileId),
           let id = UUID(uuidString: idString),
           resolvedProfiles.contains(where: { $0.id == id }) {
            self.activeProfileId = id
        } else {
            self.activeProfileId = resolvedProfiles[0].id
        }

        // Load history.
        if let data = defaults.data(forKey: Keys.syncHistory),
           let records = try? JSONDecoder().decode([SyncRecord].self, from: data) {
            self.syncHistory = records
        }

        // `didSet` does not fire during `init`. On a fresh install (no stored
        // profiles) persist the seeded profiles + active id once, so we don't
        // regenerate a new Default UUID on every launch until the first edit.
        if defaults.data(forKey: Keys.profiles) == nil {
            if let data = try? JSONEncoder().encode(self.profiles) {
                defaults.set(data, forKey: Keys.profiles)
            }
            defaults.set(self.activeProfileId.uuidString, forKey: Keys.activeProfileId)
        }

        // If we migrated from v1, persist profiles + clear legacy keys (once).
        finishMigrationIfNeeded()
    }

    // MARK: - Active profile

    /// The currently-active profile. Setter writes the modified copy back into
    /// the profiles array (matched by id), so per-field edits persist.
    public var activeProfile: SyncProfile {
        get {
            profiles.first(where: { $0.id == activeProfileId }) ?? profiles[0]
        }
        set {
            if let idx = profiles.firstIndex(where: { $0.id == newValue.id }) {
                profiles[idx] = newValue
            } else {
                profiles.append(newValue)
            }
        }
    }

    /// Convenience helper retained from v1 — now reads the active profile.
    public var expandedSourcePath: String {
        activeProfile.expandedSourcePath
    }

    public func appendSyncRecord(_ record: SyncRecord) {
        var history = syncHistory
        history.insert(record, at: 0)
        syncHistory = Array(history.prefix(20))
    }

    // MARK: - Profile management

    public func addProfile(name: String = "New Profile") {
        profiles.append(SyncProfile(name: name))
    }

    public func deleteProfile(id: UUID) {
        guard profiles.count > 1 else { return } // never delete the last profile
        profiles.removeAll { $0.id == id }
        if activeProfileId == id {
            activeProfileId = profiles[0].id
        }
    }

    // MARK: - Persistence & migration

    private func persistProfiles() {
        if let data = try? JSONEncoder().encode(profiles) {
            defaults.set(data, forKey: Keys.profiles)
        }
    }

    /// Builds a "Default" profile from legacy v1 keys if any exist. Pure read.
    nonisolated private static func migrateLegacyProfile(from defaults: UserDefaults) -> SyncProfile? {
        guard let legacySource = defaults.string(forKey: Keys.legacySourcePath) else {
            return nil
        }
        let legacyExcludes: [String]
        if let data = defaults.data(forKey: Keys.legacyExcludes),
           let stored = try? JSONDecoder().decode([String].self, from: data) {
            legacyExcludes = stored
        } else {
            legacyExcludes = [".DS_Store", ".Spotlight-V100", ".fseventsd", ".Trashes", "node_modules"]
        }
        return SyncProfile(
            name: "Default",
            sourcePath: legacySource,
            destPath: defaults.string(forKey: Keys.legacyDestPath) ?? "",
            ssdName: defaults.string(forKey: Keys.legacySsdName) ?? "Extreme Pro",
            mirrorMode: defaults.bool(forKey: Keys.legacyMirrorMode),
            autoSync: defaults.bool(forKey: Keys.legacyAutoSync),
            autoEject: defaults.bool(forKey: Keys.legacyAutoEject),
            keepVersions: defaults.bool(forKey: Keys.legacyKeepVersions),
            excludes: legacyExcludes
        )
    }

    /// If we just migrated (legacy keys still present), write profiles and
    /// clear the legacy keys so migration runs exactly once.
    private func finishMigrationIfNeeded() {
        guard defaults.string(forKey: Keys.legacySourcePath) != nil else { return }
        persistProfiles()
        defaults.set(activeProfileId.uuidString, forKey: Keys.activeProfileId)
        for key in [
            Keys.legacySourcePath, Keys.legacySsdName, Keys.legacyDestPath,
            Keys.legacyAutoSync, Keys.legacyMirrorMode, Keys.legacyExcludes,
            Keys.legacyAutoEject, Keys.legacyKeepVersions
        ] {
            defaults.removeObject(forKey: key)
        }
    }
}
```

> The migration runs only when the legacy `sourcePath` key exists. After it runs once, the legacy keys are removed and the profiles JSON is the source of truth.

### 5c. SyncEngine — read from activeProfile

In `Sources/SyncDropCore/SyncEngine.swift`, update `rsyncArgs(date:)` and `start()` to read from `configStore.activeProfile`. Replace the body of `rsyncArgs(date:)`:

```swift
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
        if profile.keepVersions {
            args.append("--backup")
            args.append("--backup-dir=.syncdrop_archive/\(Self.backupDateString(date))")
        }
        for pattern in profile.excludes where !pattern.trimmingCharacters(in: .whitespaces).isEmpty {
            args.append("--exclude=\(pattern)")
        }
        args += [profile.expandedSourcePath + "/", profile.destPath]
        return args
    }
```

In `start()`, replace the guard and `createDirectory` references that used `configStore.expandedSourcePath` / `configStore.destPath` with the active profile:

```swift
    public func start() {
        guard process?.isRunning != true else { return }
        let profile = configStore.activeProfile
        guard !profile.expandedSourcePath.isEmpty,
              !profile.destPath.isEmpty else {
            progress.state = .error("Source or destination path not configured")
            return
        }

        try? FileManager.default.createDirectory(
            atPath: profile.destPath,
            withIntermediateDirectories: true,
            attributes: nil
        )

        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/rsync")
        let syncStartDate = Date()
        p.arguments = rsyncArgs(date: syncStartDate)
        // ... (rest of start() unchanged) ...
```

> Leave the rest of `start()` (pipe setup, handlers) unchanged. `notifyOnComplete` in `handleTermination` stays as `configStore.notifyOnComplete` (global) — do **not** move it to the profile.

### 5d. VolumeMonitor — read activeProfile.ssdName

In `Sources/SyncDropCore/VolumeMonitor.swift`, replace all three `configStore.ssdName` references with `configStore.activeProfile.ssdName`:

- In `checkCurrentlyMountedVolumes`: `for url in urls where url.lastPathComponent == configStore.activeProfile.ssdName {`
- In `handleMount`: `if url.lastPathComponent == configStore.activeProfile.ssdName {`
- In `handleUnmount`: `if url.lastPathComponent == configStore.activeProfile.ssdName {`

### 5e. AppDelegate — read activeProfile

In `Sources/SyncDrop/AppDelegate.swift`:

In `handleSSDConnected`, replace `configStore.destPath`/`configStore.autoSync`:

```swift
    private func handleSSDConnected() {
        guard !configStore.activeProfile.destPath.isEmpty else {
            menuBarController.openSettings()
            return
        }
        if configStore.activeProfile.autoSync {
            syncEngine.start()
            menuBarController.showSyncPopup()
        } else {
            menuBarController.showSyncPopup()
        }
    }
```

In `handleSyncCompleted` (from Task 2), replace `configStore.autoEject` / `configStore.ssdName`:

```swift
    private func handleSyncCompleted() {
        let profile = configStore.activeProfile
        guard profile.autoEject else { return }
        let path = "/Volumes/\(profile.ssdName)"
        do {
            try NSWorkspace.shared.unmountAndEjectDevice(at: URL(fileURLWithPath: path))
        } catch {
            postEjectFailureNotification()
        }
    }
```

### 5f. MenuBarController — profile switcher + activeProfile.ssdName

In `Sources/SyncDrop/UI/MenuBarController.swift`:

Add a stored property for the switcher menu near the other menu item properties:

```swift
    private var profileMenuItem: NSMenuItem?
```

In `buildMenu()`, add a "Switch Profile" submenu **above** "Sync Now" (after the `lastSync` item and its separator). Replace the block from the `lastSync` separator through the "Sync Now" item with:

```swift
        m.addItem(.separator())

        let profileItem = NSMenuItem(title: "Switch Profile", action: nil, keyEquivalent: "")
        let profileSubmenu = NSMenu()
        profileItem.submenu = profileSubmenu
        profileMenuItem = profileItem
        m.addItem(profileItem)
        rebuildProfileSubmenu()

        m.addItem(.separator())

        let sn = NSMenuItem(title: "Sync Now", action: #selector(syncNowTapped), keyEquivalent: "")
        sn.target = self
        sn.isEnabled = false
        syncNowMenuItem = sn
        m.addItem(sn)
```

Add the submenu builder and selector:

```swift
    private func rebuildProfileSubmenu() {
        guard let submenu = profileMenuItem?.submenu else { return }
        submenu.removeAllItems()
        for profile in configStore.profiles {
            let item = NSMenuItem(title: profile.name, action: #selector(profileSelected(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = profile.id.uuidString
            item.state = (profile.id == configStore.activeProfileId) ? .on : .off
            submenu.addItem(item)
        }
    }

    @objc private func profileSelected(_ sender: NSMenuItem) {
        guard let idString = sender.representedObject as? String,
              let id = UUID(uuidString: idString) else { return }
        configStore.activeProfileId = id
        rebuildProfileSubmenu()
        updateForConnection(volumeMonitor.ssdConnected)
    }
```

Replace `configStore.ssdName` references in `updateForConnection` with `configStore.activeProfile.ssdName`:

```swift
    private func updateForConnection(_ connected: Bool) {
        statusMenuItem?.title = connected
            ? "● \(configStore.activeProfile.ssdName) — Connected"
            : "○ \(configStore.activeProfile.ssdName) — Not connected"
        syncNowMenuItem?.isEnabled = connected && syncEngine.progress.isTerminal
        updateLastSyncLabel()
    }
```

And in `buildMenu()`, the initial status item title:

```swift
        let si = NSMenuItem(title: "○ \(configStore.activeProfile.ssdName) — Not connected", action: nil, keyEquivalent: "")
```

In `observeChanges()`, also observe profile changes so the submenu stays current:

```swift
        configStore.$activeProfileId
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in
                guard let self else { return }
                self.rebuildProfileSubmenu()
                self.updateForConnection(self.volumeMonitor.ssdConnected)
            }
            .store(in: &cancellables)

        configStore.$profiles
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in self?.rebuildProfileSubmenu() }
            .store(in: &cancellables)
```

### 5g. SettingsView — bind tabs to activeProfile + add Profiles tab

In `Sources/SyncDrop/UI/SettingsView.swift`:

Add a binding helper to `ConfigStore` access by introducing a computed `Binding<SyncProfile>` in each tab. The cleanest approach: each tab takes the `ConfigStore` and binds to `configStore.activeProfile`. Replace `FoldersTab` and `BehaviorTab` bodies to read/write through `activeProfile`.

Replace `FoldersTab` with:

```swift
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
```

Replace `BehaviorTab` with (per-profile toggles bound through `activeProfile`, globals unchanged):

```swift
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
```

Add a new `ProfilesTab` and wire it into the `TabView`. Update the `TabView` in `SettingsView`:

```swift
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
```

Add `ProfilesTab` (a profile list with select/rename/delete + Add):

```swift
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
```

> Note: editing `configStore.profiles[idx].name` mutates an element of the `@Published` array, triggering `persistProfiles()`. The active-profile checkmark updates because `activeProfileId` is also `@Published`.

### 5h. SyncPopupContentView — read activeProfile

In `Sources/SyncDrop/UI/SyncPopupContentView.swift`, replace the three `configStore.ssdName` / `configStore.sourcePath` / `configStore.destPath` references with `activeProfile`:

- In `headerRow`: `Text(configStore.activeProfile.ssdName).font(.headline)`
- In `pathRow`: `Text(configStore.activeProfile.sourcePath)` and `Text(configStore.activeProfile.destPath.isEmpty ? "Not configured" : configStore.activeProfile.destPath)` with `.foregroundColor(configStore.activeProfile.destPath.isEmpty ? .red : .secondary)`
- In `confirmView` (from Task 4): the `.disabled(configStore.destPath.isEmpty)` checks become `.disabled(configStore.activeProfile.destPath.isEmpty)`
- In `runPreview()` (from Task 4): `let dest = configStore.activeProfile.destPath` (note: `configStore.expandedSourcePath` already proxies to `activeProfile`, so the `source` line can stay as `configStore.expandedSourcePath`)

### 5i. Rewrite tests

Replace `Tests/SyncDropTests/ConfigStoreTests.swift` entirely:

```swift
import XCTest
@testable import SyncDropCore

final class ConfigStoreTests: XCTestCase {
    var store: ConfigStore!
    let suite = "SyncDropTests"

    override func setUp() {
        super.setUp()
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        store = ConfigStore(defaults: defaults)
    }

    private func freshStore() -> ConfigStore {
        ConfigStore(defaults: UserDefaults(suiteName: suite)!)
    }

    func test_defaults_seedsOneDefaultProfile() {
        XCTAssertEqual(store.profiles.count, 1)
        XCTAssertEqual(store.profiles[0].name, "Default")
        XCTAssertEqual(store.activeProfileId, store.profiles[0].id)
    }

    func test_activeProfile_defaults_areCorrect() {
        let p = store.activeProfile
        XCTAssertEqual(p.sourcePath, "~/Desktop/Projects")
        XCTAssertEqual(p.ssdName, "Extreme Pro")
        XCTAssertEqual(p.destPath, "")
        XCTAssertFalse(p.autoSync)
        XCTAssertFalse(p.mirrorMode)
        XCTAssertFalse(p.autoEject)
        XCTAssertFalse(p.keepVersions)
        XCTAssertEqual(p.excludes, [".DS_Store", ".Spotlight-V100", ".fseventsd", ".Trashes", "node_modules"])
    }

    func test_globals_defaults() {
        XCTAssertTrue(store.notifyOnComplete)
        XCTAssertTrue(store.syncHistory.isEmpty)
    }

    func test_activeProfile_setter_persistsEdit() {
        var p = store.activeProfile
        p.sourcePath = "~/Documents/Work"
        store.activeProfile = p
        let reloaded = freshStore()
        XCTAssertEqual(reloaded.activeProfile.sourcePath, "~/Documents/Work")
    }

    func test_addProfile_and_switch() {
        store.addProfile(name: "Photos")
        XCTAssertEqual(store.profiles.count, 2)
        let photos = store.profiles[1]
        store.activeProfileId = photos.id
        XCTAssertEqual(store.activeProfile.name, "Photos")
    }

    func test_deleteProfile_neverDeletesLast() {
        store.deleteProfile(id: store.profiles[0].id)
        XCTAssertEqual(store.profiles.count, 1) // refused
    }

    func test_deleteProfile_reassignsActiveWhenDeletingActive() {
        store.addProfile(name: "B")
        let a = store.profiles[0].id
        store.deleteProfile(id: a)
        XCTAssertEqual(store.profiles.count, 1)
        XCTAssertEqual(store.activeProfileId, store.profiles[0].id)
    }

    func test_expandedSourcePath_proxiesActiveProfile() {
        XCTAssertTrue(store.expandedSourcePath.hasPrefix("/Users/"))
        XCTAssertFalse(store.expandedSourcePath.contains("~"))
    }

    func test_appendSyncRecord_keepsMax20() {
        for i in 0..<25 {
            store.appendSyncRecord(SyncRecord(date: Date(), fileCount: i, totalBytes: 0, durationSeconds: 1, succeeded: true))
        }
        XCTAssertEqual(store.syncHistory.count, 20)
    }

    func test_migration_fromLegacyKeys_createsDefaultProfile_andClearsLegacy() {
        let migrationSuite = "SyncDropMigrationTests"
        let defaults = UserDefaults(suiteName: migrationSuite)!
        defaults.removePersistentDomain(forName: migrationSuite)
        // Seed v1 keys.
        defaults.set("~/OldSource", forKey: "sourcePath")
        defaults.set("MySSD", forKey: "ssdName")
        defaults.set("/Volumes/MySSD/Backup", forKey: "destPath")
        defaults.set(true, forKey: "autoSync")
        defaults.set(true, forKey: "mirrorMode")
        if let data = try? JSONEncoder().encode(["a", "b"]) {
            defaults.set(data, forKey: "excludes")
        }
        defaults.set(true, forKey: "autoEject")
        defaults.set(true, forKey: "keepVersions")

        let migrated = ConfigStore(defaults: defaults)
        XCTAssertEqual(migrated.profiles.count, 1)
        let p = migrated.activeProfile
        XCTAssertEqual(p.name, "Default")
        XCTAssertEqual(p.sourcePath, "~/OldSource")
        XCTAssertEqual(p.ssdName, "MySSD")
        XCTAssertEqual(p.destPath, "/Volumes/MySSD/Backup")
        XCTAssertTrue(p.autoSync)
        XCTAssertTrue(p.mirrorMode)
        XCTAssertTrue(p.autoEject)
        XCTAssertTrue(p.keepVersions)
        XCTAssertEqual(p.excludes, ["a", "b"])

        // Legacy keys cleared.
        XCTAssertNil(defaults.string(forKey: "sourcePath"))
        XCTAssertNil(defaults.string(forKey: "ssdName"))

        // Reloading does not re-migrate; profiles persist.
        let reloaded = ConfigStore(defaults: defaults)
        XCTAssertEqual(reloaded.profiles.count, 1)
        XCTAssertEqual(reloaded.activeProfile.sourcePath, "~/OldSource")
    }
}
```

Replace `Tests/SyncDropTests/SyncEngineTests.swift`'s `setUp` and any tests that set flat fields to go through `activeProfile`. Replace the file entirely:

```swift
import XCTest
@testable import SyncDropCore

final class SyncEngineTests: XCTestCase {
    var configStore: ConfigStore!
    var engine: SyncEngine!

    override func setUp() {
        super.setUp()
        let defaults = UserDefaults(suiteName: "SyncEngineTests")!
        defaults.removePersistentDomain(forName: "SyncEngineTests")
        configStore = ConfigStore(defaults: defaults)
        var p = configStore.activeProfile
        p.sourcePath = "~/Desktop/Projects"
        p.destPath = "/Volumes/Extreme Pro/Projects"
        p.excludes = [] // start clean; tests opt in explicitly
        configStore.activeProfile = p
        engine = SyncEngine(configStore: configStore)
    }

    private func setProfile(_ mutate: (inout SyncProfile) -> Void) {
        var p = configStore.activeProfile
        mutate(&p)
        configStore.activeProfile = p
    }

    func test_rsyncArgs_noMirrorMode_noDelete() {
        setProfile { $0.mirrorMode = false }
        let args = engine.rsyncArgs
        XCTAssertFalse(args.contains("--delete"))
        XCTAssertTrue(args.contains("-rltDv"))
        XCTAssertTrue(args.contains("--no-perms"))
        XCTAssertTrue(args.contains("--no-owner"))
        XCTAssertTrue(args.contains("--no-group"))
        XCTAssertTrue(args.contains("--modify-window=1"))
    }

    func test_rsyncArgs_mirrorMode_addsDelete() {
        setProfile { $0.mirrorMode = true }
        XCTAssertTrue(engine.rsyncArgs.contains("--delete"))
    }

    func test_rsyncArgs_sourceEndsWithSlash() {
        let args = engine.rsyncArgs
        let sourceArg = args.first(where: { $0.contains("Desktop/Projects") })
        XCTAssertNotNil(sourceArg)
        XCTAssertTrue(sourceArg!.hasSuffix("/"), "Source must end with / for rsync")
    }

    func test_rsyncArgs_noMinusA() {
        XCTAssertFalse(engine.rsyncArgs.contains("-a"))
        // Only the bundled short-flag group must not contain 'a'; long options exempt.
        let shortFlagGroups = engine.rsyncArgs.filter { $0.hasPrefix("-") && !$0.hasPrefix("--") }
        XCTAssertFalse(shortFlagGroups.contains { $0.contains("a") })
    }

    func test_rsyncArgs_includesExcludePatterns() {
        setProfile { $0.excludes = [".DS_Store", "node_modules"] }
        let args = engine.rsyncArgs
        XCTAssertTrue(args.contains("--exclude=.DS_Store"))
        XCTAssertTrue(args.contains("--exclude=node_modules"))
    }

    func test_rsyncArgs_excludesBeforeSourceDest() {
        setProfile { $0.excludes = ["node_modules"] }
        let args = engine.rsyncArgs
        let excludeIdx = args.firstIndex(of: "--exclude=node_modules")!
        let sourceIdx = args.firstIndex(where: { $0.contains("Desktop/Projects") })!
        XCTAssertLessThan(excludeIdx, sourceIdx)
    }

    func test_rsyncArgs_skipsBlankExcludes() {
        setProfile { $0.excludes = ["", "  ", "real"] }
        let args = engine.rsyncArgs
        XCTAssertEqual(args.filter { $0.hasPrefix("--exclude=") }, ["--exclude=real"])
    }

    func test_rsyncArgs_keepVersionsOff_noBackup() {
        setProfile { $0.keepVersions = false }
        let args = engine.rsyncArgs
        XCTAssertFalse(args.contains("--backup"))
        XCTAssertFalse(args.contains { $0.hasPrefix("--backup-dir=") })
    }

    func test_rsyncArgs_keepVersionsOn_addsBackupDir() {
        setProfile { $0.keepVersions = true }
        let date = ISO8601DateFormatter().date(from: "2026-05-30T12:00:00Z")!
        let args = engine.rsyncArgs(date: date)
        XCTAssertTrue(args.contains("--backup"))
        XCTAssertTrue(args.contains("--backup-dir=.syncdrop_archive/2026-05-30"))
    }

    func test_backupDateString_formatsYYYYMMDD() {
        let date = ISO8601DateFormatter().date(from: "2026-01-09T00:00:00Z")!
        let s = SyncEngine.backupDateString(date)
        XCTAssertTrue(s.range(of: #"^\d{4}-\d{2}-\d{2}$"#, options: .regularExpression) != nil)
    }

    func test_parseProgress_extractsFileCount() {
        let line = "     524,288 100%  500.00kB/s    0:00:01 (xfr#5, to-chk=145/150)"
        let result = SyncEngine.parseProgress(from: line)
        XCTAssertNotNil(result)
        XCTAssertEqual(result?.filesDone, 5)
        XCTAssertEqual(result?.filesTotal, 150)
    }

    func test_parseProgress_returnsNilForNonProgressLine() {
        XCTAssertNil(SyncEngine.parseProgress(from: "sending incremental file list"))
        XCTAssertNil(SyncEngine.parseProgress(from: ""))
        XCTAssertNil(SyncEngine.parseProgress(from: "Number of files: 150"))
    }

    func test_initialState_isIdle() {
        guard case .idle = engine.progress.state else {
            XCTFail("Expected idle, got \(engine.progress.state)")
            return
        }
    }
}
```

> Note: this `SyncEngineTests` rewrite supersedes the additions made in Tasks 1 and 3 (those tests are folded in here using `setProfile`). The Task 1/3 versions referenced the now-removed flat fields, so they must be replaced as part of this commit.

### 5j. Manual verification

1. To exercise migration: install the v1 app first (or run with existing v1 UserDefaults present), then `make install` the v2 build. On launch, Settings → Profiles shows one "Default" profile carrying your old source/dest/ssd settings.
2. Profiles tab: add a profile, rename it, select it (checkmark moves). The menu bar "Switch Profile" submenu lists both with a checkmark on the active one.
3. Switch profiles from the menu bar; the status line and popup header update to the active profile's SSD name.
4. Delete a profile (the last one cannot be deleted; trash button is disabled).

**Verify:**

```bash
cd ~/SyncDrop && swift build 2>&1 | tail -5
cd ~/SyncDrop && swift test 2>&1 | tail -20
```

**Commit:**

```bash
cd ~/SyncDrop && git add -A && git commit -m "feat: multi-profile support with v1 migration and profile switcher"
```

---

## Final verification checklist

- [ ] `swift build 2>&1 | tail -5` → `Build complete!`
- [ ] `swift test 2>&1 | tail -20` → `Test Suite 'All tests' passed`
- [ ] `make install` succeeds; app launches; menu bar icon appears.
- [ ] Excludes editable in Settings → Folders and honored by rsync.
- [ ] Auto-eject toggle ejects SSD after a sync (or shows the failure notification).
- [ ] Keep-versions writes `.syncdrop_archive/<date>` on the SSD.
- [ ] "Preview…" shows the dry-run sheet; "Sync Now" in the sheet starts the real sync.
- [ ] Profiles tab + menu bar switcher work; v1 settings migrate into a "Default" profile on first v2 launch.
