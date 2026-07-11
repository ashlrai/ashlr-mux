use std::fs;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

const SHA256: &str = "f77b12a53ece5f6b7050800bbdbf8cc5ebe87f1b1387cf739f243e43e2ce886b";

#[test]
fn executable_reports_and_verifies_injected_remote_daemon_manifest() {
    let home = TempDir::new().unwrap();
    let cache = home
        .path()
        .join(".local/state/cmux/remote-daemons/9.8.7/linux-arm64/cmuxd-remote");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(&cache, b"daemon").unwrap();
    let manifest = format!(
        r#"{{"schemaVersion":1,"appVersion":"9.8.7","releaseTag":"v9.8.7","releaseURL":"https://example.test/release","checksumsAssetName":"checksums.txt","checksumsURL":"https://example.test/checksums","entries":[{{"goOS":"linux","goArch":"arm64","assetName":"cmuxd-remote-linux-arm64","downloadURL":"https://example.test/daemon","sha256":"{SHA256}"}}]}}"#
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "remote-daemon-status",
            "--os",
            "linux",
            "--arch=arm64",
            "--json",
        ])
        .env("USERPROFILE", home.path())
        .env("HOME", home.path())
        .env("CMUX_REMOTE_DAEMON_MANIFEST_JSON", manifest)
        .env("CMUX_REMOTE_DAEMON_ALLOW_LOCAL_BUILD", "1")
        .env_remove("CMUX_REMOTE_DAEMON_MANIFEST_PATH")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["manifest_present"], true);
    assert_eq!(value["target_goos"], "linux");
    assert_eq!(value["target_goarch"], "arm64");
    assert_eq!(value["asset_name"], "cmuxd-remote-linux-arm64");
    assert_eq!(value["cache_exists"], true);
    assert_eq!(value["cache_sha256"], SHA256);
    assert_eq!(value["cache_verified"], true);
    assert_eq!(value["dev_local_build_fallback"], true);
    assert!(value["attestation_verify_command"]
        .as_str()
        .unwrap()
        .contains("release.yml"));
}

#[test]
fn executable_missing_manifest_still_reports_complete_text() {
    let home = TempDir::new().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["remote-daemon-status", "--os=darwin", "--arch", "amd64"])
        .env("USERPROFILE", home.path())
        .env("HOME", home.path())
        .env_remove("CMUX_REMOTE_DAEMON_MANIFEST_JSON")
        .env(
            "CMUX_REMOTE_DAEMON_MANIFEST_PATH",
            home.path().join("missing.json"),
        )
        .env_remove("CMUX_REMOTE_DAEMON_ALLOW_LOCAL_BUILD")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("manifest: missing\nplatform: darwin/amd64\nrelease: unknown"));
    assert!(text.contains("cache exists: no\ncache verified: no"));
    assert!(text.contains("this build has no embedded remote daemon manifest"));
}
