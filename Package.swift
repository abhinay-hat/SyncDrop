// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "SyncDrop",
    platforms: [.macOS(.v13)],
    targets: [
        .target(
            name: "SyncDropCore",
            path: "Sources/SyncDropCore"
        ),
        .executableTarget(
            name: "SyncDrop",
            dependencies: ["SyncDropCore"],
            path: "Sources/SyncDrop"
        ),
        .testTarget(
            name: "SyncDropTests",
            dependencies: ["SyncDropCore"],
            path: "Tests/SyncDropTests"
        )
    ]
)
