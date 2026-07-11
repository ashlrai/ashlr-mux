//! Canonical no-socket `remote-daemon-status` diagnostics.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::invocation::CliError;

pub const REMOTE_DAEMON_STATUS_USAGE: &str = "Usage: cmux remote-daemon-status [--os <darwin|linux>] [--arch <arm64|amd64>]\n\nShow the embedded cmuxd-remote release manifest, local cache status, checksum verification state,\nand the GitHub attestation verification command for a target platform.\n\nExample:\n  cmux remote-daemon-status\n  cmux remote-daemon-status --os linux --arch arm64";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteDaemonManifest {
    #[serde(rename = "schemaVersion")]
    _schema_version: i64,
    app_version: String,
    release_tag: String,
    #[serde(rename = "releaseURL")]
    release_url: String,
    checksums_asset_name: String,
    #[serde(rename = "checksumsURL")]
    checksums_url: String,
    entries: Vec<RemoteDaemonEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteDaemonEntry {
    #[serde(rename = "goOS")]
    go_os: String,
    #[serde(rename = "goArch")]
    go_arch: String,
    asset_name: String,
    #[serde(rename = "downloadURL")]
    download_url: String,
    sha256: String,
}

struct StatusContext<'a> {
    home: &'a Path,
    app_version: &'a str,
    build: Option<&'a str>,
    commit: Option<&'a str>,
    manifest_json: Option<&'a str>,
    local_build_fallback: bool,
    host_go_os: &'a str,
    host_go_arch: &'a str,
}

pub fn run_remote_daemon_status(args: &[String], json_output: bool) -> Result<String, CliError> {
    let home = home_directory()
        .ok_or_else(|| CliError::new("Could not resolve the user home directory"))?;
    let manifest_json = discover_manifest_json();
    let build = normalized_environment_value("CMUX_BUILD");
    let commit = normalized_environment_value("CMUX_COMMIT");
    render_status(
        args,
        json_output,
        &StatusContext {
            home: &home,
            app_version: env!("CARGO_PKG_VERSION"),
            build: build.as_deref(),
            commit: commit.as_deref(),
            manifest_json: manifest_json.as_deref(),
            local_build_fallback: std::env::var("CMUX_REMOTE_DAEMON_ALLOW_LOCAL_BUILD")
                .is_ok_and(|value| value == "1"),
            host_go_os: host_go_os(),
            host_go_arch: host_go_arch(),
        },
    )
}

fn render_status(
    args: &[String],
    json_output: bool,
    context: &StatusContext<'_>,
) -> Result<String, CliError> {
    let go_os = normalized_target(option_value(args, "--os").as_deref(), context.host_go_os);
    let go_arch = normalized_target(
        option_value(args, "--arch").as_deref(),
        context.host_go_arch,
    );
    let manifest = context
        .manifest_json
        .and_then(|raw| serde_json::from_str::<RemoteDaemonManifest>(raw.trim()).ok());
    let entry = manifest.as_ref().and_then(|manifest| {
        manifest
            .entries
            .iter()
            .find(|entry| entry.go_os == go_os && entry.go_arch == go_arch)
    });
    let cache_version = manifest.as_ref().map_or(context.app_version, |manifest| {
        manifest.app_version.as_str()
    });
    let cache_path = context
        .home
        .join(".local/state/cmux/remote-daemons")
        .join(cache_version)
        .join(format!("{go_os}-{go_arch}"))
        .join("cmuxd-remote");
    let cache_exists = cache_path.exists();
    let cache_sha256 = if cache_exists {
        sha256_hex(&cache_path).ok()
    } else {
        None
    };
    let cache_verified = entry.is_some_and(|entry| {
        cache_sha256
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(&entry.sha256))
    });

    let release_tag = manifest
        .as_ref()
        .map_or("unknown", |manifest| manifest.release_tag.as_str());
    let asset_name = entry.map_or("unknown", |entry| entry.asset_name.as_str());
    let download_url = entry.map_or("unknown", |entry| entry.download_url.as_str());
    let checksums_asset_name = manifest
        .as_ref()
        .map_or("unknown", |manifest| manifest.checksums_asset_name.as_str());
    let checksums_url = manifest
        .as_ref()
        .map_or("unknown", |manifest| manifest.checksums_url.as_str());
    let download_command =
        format!("gh release download {release_tag} --repo manaflow-ai/cmux --pattern {asset_name}");
    let download_checksums_command = format!(
        "gh release download {release_tag} --repo manaflow-ai/cmux --pattern {checksums_asset_name}"
    );
    let checksum_verify_command =
        format!("shasum -a 256 -c {checksums_asset_name} --ignore-missing");
    let signer_workflow = if release_tag == "nightly" {
        "manaflow-ai/cmux/.github/workflows/nightly.yml"
    } else {
        "manaflow-ai/cmux/.github/workflows/release.yml"
    };
    let attestation_verify_command = format!(
        "gh attestation verify ./{asset_name} --repo manaflow-ai/cmux --signer-workflow {signer_workflow}"
    );
    let payload = json!({
        "app_version": context.app_version,
        "build": context.build,
        "commit": context.commit,
        "manifest_present": manifest.is_some(),
        "release_tag": release_tag,
        "release_url": manifest.as_ref().map(|manifest| manifest.release_url.as_str()),
        "target_goos": go_os,
        "target_goarch": go_arch,
        "asset_name": asset_name,
        "download_url": download_url,
        "checksums_asset_name": checksums_asset_name,
        "checksums_url": checksums_url,
        "expected_sha256": entry.map(|entry| entry.sha256.as_str()),
        "cache_path": cache_path.to_string_lossy(),
        "cache_exists": cache_exists,
        "cache_sha256": cache_sha256,
        "cache_verified": cache_verified,
        "dev_local_build_fallback": context.local_build_fallback,
        "download_command": download_command,
        "download_checksums_command": download_checksums_command,
        "checksum_verify_command": checksum_verify_command,
        "attestation_verify_command": attestation_verify_command,
    });
    if json_output {
        serde_json::to_string_pretty(&payload)
            .map_err(|error| CliError::new(format!("Failed to encode daemon status: {error}")))
    } else {
        Ok(render_text(&payload))
    }
}

fn render_text(payload: &Value) -> String {
    let string = |key: &str| payload.get(key).and_then(Value::as_str);
    let yes_no = |key: &str| {
        if payload.get(key).and_then(Value::as_bool) == Some(true) {
            "yes"
        } else {
            "no"
        }
    };
    let mut lines = vec![format!(
        "app version: {}",
        string("app_version").unwrap_or("unknown")
    )];
    if let Some(build) = string("build") {
        lines.push(format!("build: {build}"));
    }
    if let Some(commit) = string("commit") {
        lines.push(format!("commit: {commit}"));
    }
    lines.extend([
        format!(
            "manifest: {}",
            if payload["manifest_present"].as_bool() == Some(true) {
                "present"
            } else {
                "missing"
            }
        ),
        format!(
            "platform: {}/{}",
            string("target_goos").unwrap_or("unknown"),
            string("target_goarch").unwrap_or("unknown")
        ),
        format!("release: {}", string("release_tag").unwrap_or("unknown")),
        format!("asset: {}", string("asset_name").unwrap_or("unknown")),
        format!(
            "download url: {}",
            string("download_url").unwrap_or("unknown")
        ),
        format!(
            "checksums asset: {}",
            string("checksums_asset_name").unwrap_or("unknown")
        ),
        format!(
            "checksums: {}",
            string("checksums_url").unwrap_or("unknown")
        ),
    ]);
    if let Some(expected) = string("expected_sha256") {
        lines.push(format!("expected sha256: {expected}"));
    }
    lines.push(format!("cache: {}", string("cache_path").unwrap_or("")));
    lines.push(format!("cache exists: {}", yes_no("cache_exists")));
    if let Some(actual) = string("cache_sha256") {
        lines.push(format!("cache sha256: {actual}"));
    }
    lines.extend([
        format!("cache verified: {}", yes_no("cache_verified")),
        format!(
            "download command: {}",
            string("download_command").unwrap_or("")
        ),
        format!(
            "download checksums: {}",
            string("download_checksums_command").unwrap_or("")
        ),
        format!(
            "verify checksum: {}",
            string("checksum_verify_command").unwrap_or("")
        ),
        format!(
            "attestation verify: {}",
            string("attestation_verify_command").unwrap_or("")
        ),
    ]);
    if payload["manifest_present"].as_bool() != Some(true) {
        lines.push("note: this build has no embedded remote daemon manifest. Set CMUX_REMOTE_DAEMON_ALLOW_LOCAL_BUILD=1 only for dev builds.".to_string());
    }
    lines.join("\n")
}

fn option_value(args: &[String], name: &str) -> Option<String> {
    let equals_prefix = format!("{name}=");
    for (index, argument) in args.iter().enumerate() {
        if argument == "--" {
            return None;
        }
        if argument == name {
            return args.get(index + 1).cloned();
        }
        if let Some(value) = argument.strip_prefix(&equals_prefix) {
            return Some(value.to_string());
        }
    }
    None
}

fn normalized_target(requested: Option<&str>, fallback: &str) -> String {
    let requested = requested.unwrap_or_default().trim().to_lowercase();
    if requested.is_empty() {
        fallback.to_string()
    } else {
        requested
    }
}

fn sha256_hex(path: &Path) -> Result<String, std::io::Error> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn discover_manifest_json() -> Option<String> {
    if let Ok(raw) = std::env::var("CMUX_REMOTE_DAEMON_MANIFEST_JSON") {
        if let Some(raw) = valid_manifest_json(raw) {
            return Some(raw);
        }
    }
    if let Some(path) = std::env::var_os("CMUX_REMOTE_DAEMON_MANIFEST_PATH") {
        if let Some(raw) = read_manifest_json(Path::new(&path)) {
            return Some(raw);
        }
    }
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    for candidate in [
        directory.join("cmuxd-remote-manifest.json"),
        directory.join("resources/cmuxd-remote-manifest.json"),
    ] {
        if let Some(raw) = read_manifest_json(&candidate) {
            return Some(raw);
        }
    }
    for directory in directory.ancestors() {
        let candidate = directory.join("remote-daemon-assets/cmuxd-remote-manifest.json");
        if let Some(raw) = read_manifest_json(&candidate) {
            return Some(raw);
        }
    }
    None
}

fn read_manifest_json(path: &Path) -> Option<String> {
    valid_manifest_json(fs::read_to_string(path).ok()?)
}

fn valid_manifest_json(raw: String) -> Option<String> {
    serde_json::from_str::<RemoteDaemonManifest>(raw.trim())
        .ok()
        .map(|_| raw)
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("HOME").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

fn normalized_environment_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn host_go_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}

fn host_go_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "amd64"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const MANIFEST: &str = r#"{
      "schemaVersion": 1,
      "appVersion": "0.62.0-test",
      "releaseTag": "nightly",
      "releaseURL": "https://github.com/manaflow-ai/cmux/releases/tag/nightly",
      "checksumsAssetName": "cmuxd-remote-checksums.txt",
      "checksumsURL": "https://example.test/checksums",
      "entries": [{
        "goOS": "linux",
        "goArch": "arm64",
        "assetName": "cmuxd-remote-linux-arm64",
        "downloadURL": "https://example.test/cmuxd-remote-linux-arm64",
        "sha256": "f77b12a53ece5f6b7050800bbdbf8cc5ebe87f1b1387cf739f243e43e2ce886b"
      }]
    }"#;

    fn context<'a>(home: &'a Path, manifest_json: Option<&'a str>) -> StatusContext<'a> {
        StatusContext {
            home,
            app_version: "0.1.0",
            build: Some("123"),
            commit: Some("abcdef123456"),
            manifest_json,
            local_build_fallback: true,
            host_go_os: "windows",
            host_go_arch: "amd64",
        }
    }

    #[test]
    fn manifest_entry_and_matching_cache_render_canonical_json() {
        let home = TempDir::new().unwrap();
        let cache = home
            .path()
            .join(".local/state/cmux/remote-daemons/0.62.0-test/linux-arm64/cmuxd-remote");
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, b"daemon").unwrap();
        let output = render_status(
            &["--os= LINUX ".into(), "--arch".into(), "ARM64".into()],
            true,
            &context(home.path(), Some(MANIFEST)),
        )
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["app_version"], "0.1.0");
        assert_eq!(value["build"], "123");
        assert_eq!(value["commit"], "abcdef123456");
        assert_eq!(value["manifest_present"], true);
        assert_eq!(value["target_goos"], "linux");
        assert_eq!(value["target_goarch"], "arm64");
        assert_eq!(value["asset_name"], "cmuxd-remote-linux-arm64");
        assert_eq!(value["cache_exists"], true);
        assert_eq!(value["cache_verified"], true);
        assert_eq!(
            value["cache_sha256"],
            "f77b12a53ece5f6b7050800bbdbf8cc5ebe87f1b1387cf739f243e43e2ce886b"
        );
        assert!(value["attestation_verify_command"]
            .as_str()
            .unwrap()
            .contains("nightly.yml"));
        assert!(output.find("\"app_version\"").unwrap() < output.find("\"asset_name\"").unwrap());
    }

    #[test]
    fn missing_manifest_text_preserves_defaults_and_note() {
        let home = TempDir::new().unwrap();
        let output = render_status(&[], false, &context(home.path(), None)).unwrap();
        assert!(output.starts_with("app version: 0.1.0\nbuild: 123\ncommit: abcdef123456\n"));
        assert!(output.contains("manifest: missing"));
        assert!(output.contains("platform: windows/amd64"));
        assert!(output.contains("release: unknown"));
        assert!(output.contains("cache exists: no"));
        assert!(output.contains("cache verified: no"));
        assert!(output.ends_with("Set CMUX_REMOTE_DAEMON_ALLOW_LOCAL_BUILD=1 only for dev builds."));
    }

    #[test]
    fn option_parsing_matches_first_value_and_terminator_behavior() {
        let args = vec![
            "--os=linux".into(),
            "--os".into(),
            "darwin".into(),
            "--".into(),
            "--arch=arm64".into(),
        ];
        assert_eq!(option_value(&args, "--os").as_deref(), Some("linux"));
        assert_eq!(option_value(&args, "--arch"), None);
        assert_eq!(normalized_target(Some("  DARWIN "), "windows"), "darwin");
        assert_eq!(normalized_target(Some("  "), "windows"), "windows");
        assert!(valid_manifest_json("not json".to_string()).is_none());
        assert!(valid_manifest_json(MANIFEST.to_string()).is_some());
    }
}
