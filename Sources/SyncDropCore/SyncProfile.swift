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
