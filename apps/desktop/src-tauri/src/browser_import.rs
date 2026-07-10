use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FIXTURE_ENV: &str = "CMUX_UI_TEST_BROWSER_IMPORT_FIXTURE";
const DESTINATIONS_ENV: &str = "CMUX_UI_TEST_BROWSER_IMPORT_DESTINATIONS";
const CAPTURE_PATH_ENV: &str = "CMUX_UI_TEST_BROWSER_IMPORT_CAPTURE_PATH";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BrowserImportProfile {
    pub browser_id: String,
    pub browser_name: String,
    pub profile_name: String,
    pub profile_path: String,
    pub bookmarks_path: Option<String>,
    pub history_path: Option<String>,
    pub cookies_path: Option<String>,
    pub importable_items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BrowserImportDestinationProfile {
    pub id: String,
    pub display_name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BrowserImportStartRequest {
    pub browser_id: String,
    pub browser_name: String,
    pub mode: BrowserImportMode,
    pub scope: BrowserImportScope,
    pub entries: Vec<BrowserImportExecutionEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserImportMode {
    SingleDestination,
    SeparateProfiles,
    MergeIntoOne,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserImportScope {
    CookiesAndHistory,
    Everything,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BrowserImportExecutionEntry {
    pub source_profile_paths: Vec<String>,
    pub source_profile_names: Vec<String>,
    pub destination_kind: BrowserImportDestinationKind,
    pub destination_name: String,
    pub destination_profile_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserImportDestinationKind {
    Create,
    Existing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrowserImportStartReply {
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserImportFixture {
    browser_id: Option<String>,
    browser_name: String,
    profiles: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BrowserImportDestinationFixtureEntry {
    Name(String),
    Profile(BrowserImportDestinationFixtureProfile),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserImportDestinationFixtureProfile {
    id: Option<String>,
    display_name: String,
    is_default: Option<bool>,
}

#[derive(Debug, Serialize)]
struct BrowserImportCapture {
    mode: BrowserImportMode,
    scope: BrowserImportScope,
    entries: Vec<BrowserImportCaptureEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserImportCaptureEntry {
    source_profiles: Vec<String>,
    destination_kind: BrowserImportDestinationKind,
    destination_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    destination_profile_id: Option<String>,
}

#[tauri::command]
pub fn browser_import_profiles() -> Vec<BrowserImportProfile> {
    if let Ok(fixture) = std::env::var(FIXTURE_ENV) {
        match browser_import_profiles_from_fixture(&fixture) {
            Ok(profiles) => return profiles,
            Err(error) => eprintln!("[browser-import] ignored invalid fixture: {error}"),
        }
    }
    browser_import_profiles_from_roots(
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        std::env::var_os("APPDATA").map(PathBuf::from),
    )
}

#[tauri::command]
pub fn browser_import_destination_profiles() -> Vec<BrowserImportDestinationProfile> {
    if let Ok(fixture) = std::env::var(DESTINATIONS_ENV) {
        match browser_import_destination_profiles_from_fixture(&fixture) {
            Ok(profiles) => return profiles,
            Err(error) => {
                eprintln!("[browser-import] ignored invalid destinations fixture: {error}")
            }
        }
    }
    default_browser_import_destination_profiles()
}

#[tauri::command]
pub fn browser_import_start(
    request: BrowserImportStartRequest,
) -> Result<BrowserImportStartReply, String> {
    browser_import_start_inner(
        request,
        std::env::var_os(CAPTURE_PATH_ENV).map(PathBuf::from),
    )
}

fn browser_import_start_inner(
    request: BrowserImportStartRequest,
    capture_path: Option<PathBuf>,
) -> Result<BrowserImportStartReply, String> {
    validate_browser_import_request(&request)?;
    if let Some(path) = capture_path {
        let payload = serde_json::to_vec_pretty(&capture_for_request(&request))
            .map_err(|error| format!("failed to encode browser import request: {error}"))?;
        std::fs::write(&path, payload).map_err(|error| {
            format!(
                "failed to write browser import capture {}: {error}",
                path.display()
            )
        })?;
    }
    let profile_count: usize = request
        .entries
        .iter()
        .map(|entry| entry.source_profile_paths.len())
        .sum();
    Ok(BrowserImportStartReply {
        message: format!(
            "Browser import plan accepted for {profile_count} {} profile{}.",
            request.browser_name,
            if profile_count == 1 { "" } else { "s" }
        ),
    })
}

fn validate_browser_import_request(request: &BrowserImportStartRequest) -> Result<(), String> {
    if request.browser_id.trim().is_empty() {
        return Err("browser import request is missing a browser id".to_string());
    }
    if request.browser_name.trim().is_empty() {
        return Err("browser import request is missing a browser name".to_string());
    }
    if request.entries.is_empty() {
        return Err("browser import request must include at least one mapping".to_string());
    }
    for entry in &request.entries {
        if entry.source_profile_paths.is_empty() {
            return Err("browser import mapping has no source profiles".to_string());
        }
        if entry.source_profile_paths.len() != entry.source_profile_names.len() {
            return Err(
                "browser import mapping source paths/names must have the same length".to_string(),
            );
        }
        if entry.destination_name.trim().is_empty() {
            return Err("browser import mapping is missing a destination name".to_string());
        }
    }
    Ok(())
}

fn capture_for_request(request: &BrowserImportStartRequest) -> BrowserImportCapture {
    BrowserImportCapture {
        mode: request.mode.clone(),
        scope: request.scope.clone(),
        entries: request
            .entries
            .iter()
            .map(|entry| BrowserImportCaptureEntry {
                source_profiles: entry.source_profile_names.clone(),
                destination_kind: entry.destination_kind.clone(),
                destination_name: entry.destination_name.clone(),
                destination_profile_id: entry.destination_profile_id.clone(),
            })
            .collect(),
    }
}

fn browser_import_profiles_from_fixture(raw: &str) -> Result<Vec<BrowserImportProfile>, String> {
    let fixture: BrowserImportFixture =
        serde_json::from_str(raw).map_err(|error| format!("invalid fixture JSON: {error}"))?;
    let browser_name = fixture.browser_name.trim();
    if browser_name.is_empty() {
        return Err("fixture browserName must not be blank".to_string());
    }
    let browser_id = fixture
        .browser_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| browser_fixture_id(browser_name));
    Ok(fixture
        .profiles
        .into_iter()
        .filter_map(|profile_name| {
            let profile_name = profile_name.trim().to_string();
            if profile_name.is_empty() {
                return None;
            }
            let path = format!(
                "cmux-ui-test://browser-import/{}/{}",
                browser_id,
                percent_escape_fixture_segment(&profile_name)
            );
            Some(BrowserImportProfile {
                browser_id: browser_id.clone(),
                browser_name: browser_name.to_string(),
                profile_name,
                profile_path: path,
                bookmarks_path: None,
                history_path: None,
                cookies_path: None,
                importable_items: vec![
                    "bookmarks".to_string(),
                    "history".to_string(),
                    "cookies".to_string(),
                ],
            })
        })
        .collect())
}

fn browser_import_destination_profiles_from_fixture(
    raw: &str,
) -> Result<Vec<BrowserImportDestinationProfile>, String> {
    let entries: Vec<BrowserImportDestinationFixtureEntry> = serde_json::from_str(raw)
        .map_err(|error| format!("invalid browser import destinations fixture JSON: {error}"))?;
    let mut profiles: Vec<BrowserImportDestinationProfile> = entries
        .into_iter()
        .filter_map(|entry| match entry {
            BrowserImportDestinationFixtureEntry::Name(name) => {
                let display_name = name.trim();
                if display_name.is_empty() {
                    return None;
                }
                Some(BrowserImportDestinationProfile {
                    id: browser_destination_fixture_id(display_name),
                    display_name: display_name.to_string(),
                    is_default: false,
                })
            }
            BrowserImportDestinationFixtureEntry::Profile(profile) => {
                let display_name = profile.display_name.trim();
                if display_name.is_empty() {
                    return None;
                }
                Some(BrowserImportDestinationProfile {
                    id: profile
                        .id
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| browser_destination_fixture_id(display_name)),
                    display_name: display_name.to_string(),
                    is_default: profile.is_default.unwrap_or(false),
                })
            }
        })
        .collect();
    if profiles.is_empty() {
        return Err("destinations fixture must include at least one profile".to_string());
    }
    if !profiles.iter().any(|profile| profile.is_default) {
        if let Some(first) = profiles.first_mut() {
            first.is_default = true;
        }
    }
    Ok(profiles)
}

fn default_browser_import_destination_profiles() -> Vec<BrowserImportDestinationProfile> {
    vec![BrowserImportDestinationProfile {
        id: "default".to_string(),
        display_name: "Default".to_string(),
        is_default: true,
    }]
}

fn browser_fixture_id(browser_name: &str) -> String {
    let slug: String = browser_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "browser".to_string()
    } else {
        slug.to_string()
    }
}

fn browser_destination_fixture_id(display_name: &str) -> String {
    let id = browser_fixture_id(display_name);
    if id == "browser" {
        "default".to_string()
    } else {
        id
    }
}

fn percent_escape_fixture_segment(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn browser_import_profiles_from_roots(
    local_app_data: Option<PathBuf>,
    app_data: Option<PathBuf>,
) -> Vec<BrowserImportProfile> {
    let mut profiles = Vec::new();
    if let Some(local_app_data) = local_app_data.as_deref() {
        profiles.extend(chromium_profiles(
            "chrome",
            "Google Chrome",
            &local_app_data
                .join("Google")
                .join("Chrome")
                .join("User Data"),
        ));
        profiles.extend(chromium_profiles(
            "edge",
            "Microsoft Edge",
            &local_app_data
                .join("Microsoft")
                .join("Edge")
                .join("User Data"),
        ));
        profiles.extend(chromium_profiles(
            "brave",
            "Brave",
            &local_app_data
                .join("BraveSoftware")
                .join("Brave-Browser")
                .join("User Data"),
        ));
        profiles.extend(chromium_profiles(
            "chromium",
            "Chromium",
            &local_app_data.join("Chromium").join("User Data"),
        ));
    }
    if let Some(app_data) = app_data.as_deref() {
        profiles.extend(firefox_profiles(&app_data.join("Mozilla").join("Firefox")));
    }
    profiles
}

fn chromium_profiles(
    browser_id: &str,
    browser_name: &str,
    user_data_dir: &Path,
) -> Vec<BrowserImportProfile> {
    let Ok(entries) = std::fs::read_dir(user_data_dir) else {
        return Vec::new();
    };
    let mut profiles = Vec::new();
    for entry in entries.flatten() {
        let profile_path = entry.path();
        if !profile_path.is_dir() {
            continue;
        }
        let Some(file_name) = profile_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !is_chromium_profile_dir(file_name) {
            continue;
        }
        let bookmarks = existing_file(profile_path.join("Bookmarks"));
        let history = existing_file(profile_path.join("History"));
        let cookies = existing_file(profile_path.join("Network").join("Cookies"))
            .or_else(|| existing_file(profile_path.join("Cookies")));
        let importable_items = importable_items(&bookmarks, &history, &cookies);
        if importable_items.is_empty() {
            continue;
        }
        profiles.push(BrowserImportProfile {
            browser_id: browser_id.to_string(),
            browser_name: browser_name.to_string(),
            profile_name: chromium_profile_name(file_name),
            profile_path: path_string(&profile_path),
            bookmarks_path: bookmarks.map(|path| path_string(&path)),
            history_path: history.map(|path| path_string(&path)),
            cookies_path: cookies.map(|path| path_string(&path)),
            importable_items,
        });
    }
    profiles.sort_by(|a, b| a.profile_name.cmp(&b.profile_name));
    profiles
}

fn firefox_profiles(firefox_dir: &Path) -> Vec<BrowserImportProfile> {
    let profiles_ini = firefox_dir.join("profiles.ini");
    let mut profile_paths = if profiles_ini.exists() {
        firefox_profiles_from_ini(firefox_dir, &profiles_ini)
    } else {
        Vec::new()
    };
    if profile_paths.is_empty() {
        profile_paths = firefox_profiles_from_directory(&firefox_dir.join("Profiles"));
    }

    let mut profiles = Vec::new();
    for (profile_name, profile_path) in profile_paths {
        let places = existing_file(profile_path.join("places.sqlite"));
        let cookies = existing_file(profile_path.join("cookies.sqlite"));
        let mut importable_items = Vec::new();
        if places.is_some() {
            importable_items.push("bookmarks".to_string());
            importable_items.push("history".to_string());
        }
        if cookies.is_some() {
            importable_items.push("cookies".to_string());
        }
        if importable_items.is_empty() {
            continue;
        }
        profiles.push(BrowserImportProfile {
            browser_id: "firefox".to_string(),
            browser_name: "Firefox".to_string(),
            profile_name,
            profile_path: path_string(&profile_path),
            bookmarks_path: places.as_ref().map(|path| path_string(path)),
            history_path: places.map(|path| path_string(&path)),
            cookies_path: cookies.map(|path| path_string(&path)),
            importable_items,
        });
    }
    profiles.sort_by(|a, b| a.profile_name.cmp(&b.profile_name));
    profiles
}

fn firefox_profiles_from_ini(firefox_dir: &Path, profiles_ini: &Path) -> Vec<(String, PathBuf)> {
    let Ok(contents) = std::fs::read_to_string(profiles_ini) else {
        return Vec::new();
    };
    let mut profiles = Vec::new();
    let mut section_name: Option<String> = None;
    let mut path: Option<String> = None;
    let mut is_relative = true;

    for line in contents.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            push_firefox_profile(
                firefox_dir,
                &mut profiles,
                &section_name,
                &path,
                is_relative,
            );
            section_name = None;
            path = None;
            is_relative = true;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "Name" => section_name = Some(value.to_string()),
            "Path" => path = Some(value.replace('/', std::path::MAIN_SEPARATOR_STR)),
            "IsRelative" => is_relative = value != "0",
            _ => {}
        }
    }
    push_firefox_profile(
        firefox_dir,
        &mut profiles,
        &section_name,
        &path,
        is_relative,
    );
    profiles
}

fn push_firefox_profile(
    firefox_dir: &Path,
    profiles: &mut Vec<(String, PathBuf)>,
    section_name: &Option<String>,
    path: &Option<String>,
    is_relative: bool,
) {
    let Some(path) = path else {
        return;
    };
    let profile_path = if is_relative {
        firefox_dir.join(path)
    } else {
        PathBuf::from(path)
    };
    let profile_name = section_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            profile_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "Firefox Profile".to_string());
    profiles.push((profile_name, profile_path));
}

fn firefox_profiles_from_directory(profiles_dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(profiles_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_string();
            Some((name, path))
        })
        .collect()
}

fn is_chromium_profile_dir(name: &str) -> bool {
    name == "Default"
        || name == "Guest Profile"
        || name.starts_with("Profile ")
        || name.starts_with("Person ")
}

fn chromium_profile_name(name: &str) -> String {
    match name {
        "Default" => "Default".to_string(),
        "Guest Profile" => "Guest".to_string(),
        other => other.to_string(),
    }
}

fn existing_file(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

fn importable_items(
    bookmarks_path: &Option<PathBuf>,
    history_path: &Option<PathBuf>,
    cookies_path: &Option<PathBuf>,
) -> Vec<String> {
    let mut items = Vec::new();
    if bookmarks_path.is_some() {
        items.push("bookmarks".to_string());
    }
    if history_path.is_some() {
        items.push("history".to_string());
    }
    if cookies_path.is_some() {
        items.push("cookies".to_string());
    }
    items
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_chromium_profiles_with_importable_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let local = temp.path().join("Local");
        let profile = local
            .join("Google")
            .join("Chrome")
            .join("User Data")
            .join("Default");
        std::fs::create_dir_all(profile.join("Network")).expect("profile dirs");
        std::fs::write(profile.join("Bookmarks"), "{}").expect("bookmarks");
        std::fs::write(profile.join("History"), "").expect("history");
        std::fs::write(profile.join("Network").join("Cookies"), "").expect("cookies");

        let profiles = browser_import_profiles_from_roots(Some(local), None);

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].browser_id, "chrome");
        assert_eq!(profiles[0].browser_name, "Google Chrome");
        assert_eq!(profiles[0].profile_name, "Default");
        assert_eq!(
            profiles[0].importable_items,
            vec!["bookmarks", "history", "cookies"]
        );
        assert!(profiles[0]
            .bookmarks_path
            .as_deref()
            .is_some_and(|path| path.ends_with("Bookmarks")));
    }

    #[test]
    fn ignores_chromium_profile_dirs_without_importable_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let local = temp.path().join("Local");
        std::fs::create_dir_all(
            local
                .join("Microsoft")
                .join("Edge")
                .join("User Data")
                .join("Default"),
        )
        .expect("profile dir");

        assert!(browser_import_profiles_from_roots(Some(local), None).is_empty());
    }

    #[test]
    fn detects_firefox_profiles_from_profiles_ini() {
        let temp = tempfile::tempdir().expect("tempdir");
        let app_data = temp.path().join("Roaming");
        let firefox = app_data.join("Mozilla").join("Firefox");
        let profile = firefox.join("Profiles").join("abc.default-release");
        std::fs::create_dir_all(&profile).expect("profile dir");
        std::fs::write(profile.join("places.sqlite"), "").expect("places");
        std::fs::write(profile.join("cookies.sqlite"), "").expect("cookies");
        std::fs::write(
            firefox.join("profiles.ini"),
            "[Profile0]\nName=default-release\nIsRelative=1\nPath=Profiles/abc.default-release\n",
        )
        .expect("profiles.ini");

        let profiles = browser_import_profiles_from_roots(None, Some(app_data));

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].browser_id, "firefox");
        assert_eq!(profiles[0].profile_name, "default-release");
        assert_eq!(
            profiles[0].importable_items,
            vec!["bookmarks", "history", "cookies"]
        );
    }

    #[test]
    fn browser_import_profiles_from_fixture_matches_ui_test_contract() {
        let profiles = browser_import_profiles_from_fixture(
            r#"{"browserName":"Helium","profiles":["You","austin","  "]}"#,
        )
        .expect("fixture profiles");

        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].browser_id, "helium");
        assert_eq!(profiles[0].browser_name, "Helium");
        assert_eq!(profiles[0].profile_name, "You");
        assert_eq!(
            profiles[0].importable_items,
            vec!["bookmarks", "history", "cookies"]
        );
        assert_eq!(profiles[1].profile_name, "austin");
        assert!(profiles[1]
            .profile_path
            .starts_with("cmux-ui-test://browser-import/helium/"));
    }

    #[test]
    fn browser_import_destination_profiles_from_fixture_matches_ui_test_contract() {
        let profiles = browser_import_destination_profiles_from_fixture(r#"["Default"," Work "]"#)
            .expect("destination profiles");

        assert_eq!(
            profiles,
            vec![
                BrowserImportDestinationProfile {
                    id: "default".to_string(),
                    display_name: "Default".to_string(),
                    is_default: true,
                },
                BrowserImportDestinationProfile {
                    id: "work".to_string(),
                    display_name: "Work".to_string(),
                    is_default: false,
                },
            ]
        );
    }

    #[test]
    fn browser_import_destination_profiles_from_object_fixture_preserves_ids() {
        let profiles = browser_import_destination_profiles_from_fixture(
            r#"[{"id":"profile-1","displayName":"Personal","isDefault":true}]"#,
        )
        .expect("destination profiles");

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "profile-1");
        assert_eq!(profiles[0].display_name, "Personal");
        assert!(profiles[0].is_default);
    }

    #[test]
    fn browser_import_start_validates_and_captures_the_selected_plan() {
        let temp = tempfile::tempdir().expect("tempdir");
        let capture = temp.path().join("browser-import.json");
        let request = BrowserImportStartRequest {
            browser_id: "chrome".to_string(),
            browser_name: "Google Chrome".to_string(),
            mode: BrowserImportMode::SeparateProfiles,
            scope: BrowserImportScope::CookiesAndHistory,
            entries: vec![BrowserImportExecutionEntry {
                source_profile_paths: vec!["C:/Chrome/Default".to_string()],
                source_profile_names: vec!["Default".to_string()],
                destination_kind: BrowserImportDestinationKind::Create,
                destination_name: "Default".to_string(),
                destination_profile_id: None,
            }],
        };

        let reply =
            browser_import_start_inner(request, Some(capture.clone())).expect("accepted plan");

        assert_eq!(
            reply.message,
            "Browser import plan accepted for 1 Google Chrome profile."
        );
        let captured = std::fs::read_to_string(capture).expect("capture file");
        assert!(captured.contains(r#""mode": "separateProfiles""#));
        assert!(captured.contains(r#""scope": "cookiesAndHistory""#));
        assert!(captured.contains(r#""sourceProfiles": ["#));
        assert!(captured.contains(r#""destinationKind": "create""#));
        assert!(captured.contains(r#""destinationName": "Default""#));
        assert!(!captured.contains("source_profile_names"));
    }

    #[test]
    fn browser_import_start_rejects_empty_mappings() {
        let request = BrowserImportStartRequest {
            browser_id: "chrome".to_string(),
            browser_name: "Google Chrome".to_string(),
            mode: BrowserImportMode::MergeIntoOne,
            scope: BrowserImportScope::Everything,
            entries: Vec::new(),
        };

        assert!(browser_import_start_inner(request, None).is_err());
    }
}
