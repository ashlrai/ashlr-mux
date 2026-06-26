// swift-tools-version:5.9
//
// Reference Package.swift for the macOS golden-fixture exporter (WS6).
//
// NOTE: the relative `.package(path:)` deps below assume this directory lives at
// `crates/cmux-golden/swift-exporter/` inside the cmux checkout, so the macOS SPM
// packages are reached via `../../../Packages/...`. Adjust the relative depth if
// the exporter is relocated. This file is NOT built on the Windows port machine
// (no Swift toolchain); it is the drop-in target for the macOS CI runner.
import PackageDescription

let package = Package(
    name: "CmuxGoldenExport",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "cmux-golden-export", targets: ["CmuxGoldenExport"])
    ],
    dependencies: [
        .package(path: "../../../Packages/macOS/CmuxControlSocket"),
        .package(path: "../../../Packages/Shared/CmuxAgentChat"),
        .package(path: "../../../Packages/macOS/CmuxSettings"),
    ],
    targets: [
        .executableTarget(
            name: "CmuxGoldenExport",
            dependencies: [
                .product(name: "CmuxControlSocket", package: "CmuxControlSocket"),
                .product(name: "CmuxAgentChat", package: "CmuxAgentChat"),
                .product(name: "CmuxSettings", package: "CmuxSettings"),
            ],
            path: "Sources/CmuxGoldenExport"
        )
    ]
)
