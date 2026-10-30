// swift-tools-version:5.9
// mdview のモバイル版の、端末の書類（Files）・出力・共有の部品（v3 R-02）。dx が .app を組むときに取り込む（src/native.rs）
import PackageDescription

let package = Package(
    name: "DocumentsPlugin",
    platforms: [
        .iOS(.v16),
        .macOS(.v13),
    ],
    products: [
        .library(name: "DocumentsPlugin", type: .static, targets: ["DocumentsPlugin"])
    ],
    targets: [
        .target(
            name: "DocumentsPlugin",
            path: "Sources",
            linkerSettings: [
                .linkedFramework("UIKit", .when(platforms: [.iOS])),
                .linkedFramework("UniformTypeIdentifiers"),
                .linkedFramework("Foundation"),
            ]
        )
    ]
)
