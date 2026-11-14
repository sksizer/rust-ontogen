// swift-tools-version: 6.0
// The native macOS iron-log: SwiftUI over the Rust core, called in-process
// through ontogen's generated UniFFI export. Run ../scripts/build-xcframework.sh
// first; it produces IronLogFFI.xcframework and Sources/IronLogCore/Generated.
import PackageDescription

let package = Package(
    name: "IronLogMac",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "IronLogMac", targets: ["IronLogMac"]),
        .library(name: "IronLogCore", targets: ["IronLogCore"]),
    ],
    targets: [
        // The Rust static library and its C header.
        .binaryTarget(name: "IronLogFFI", path: "IronLogFFI.xcframework"),
        // UniFFI's Swift bindings plus a little hand-written sugar.
        .target(name: "IronLogCore", dependencies: ["IronLogFFI"]),
        .executableTarget(name: "IronLogMac", dependencies: ["IronLogCore"]),
        .testTarget(name: "IronLogCoreTests", dependencies: ["IronLogCore"]),
    ]
)
