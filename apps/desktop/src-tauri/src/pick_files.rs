//! `app.pickFiles` — the agent composer's "Add photos & files" action.
//!
//! Faithful port of the macOS `AgentSessionWebRendererCoordinator`
//! `pickLocalFiles` / `pickedLocalFileDictionary`: each selected path becomes a
//! `{ label, path, fsPath, mimeType, isImage }` object, and an image within BOTH
//! the per-file (512KB) and the shared per-pick (2MB) caps additionally carries a
//! base64 `data:` URL for inline preview. The 2MB budget is spent across the whole
//! selection, in order, exactly like the Swift `remainingImagePreviewBytes` fold.
//!
//! The native dialog invocation lives in `agent_session.rs` (it needs the Tauri
//! `AppHandle`); everything here is pure + unit-tested. Divergence from macOS: the
//! macOS panel also allows choosing directories; the Tauri file picker selects
//! files only.

use std::path::Path;

use base64::Engine;
use serde_json::{json, Value};
use tauri::AppHandle;

/// Per-image cap: an image larger than this is attached by path only (no preview).
/// Swift `imagePreviewMaxBytes`.
const IMAGE_PREVIEW_MAX_BYTES: u64 = 512 * 1024;
/// Shared cap across one pick: once spent, later images get no preview. Swift
/// `imagePreviewTotalMaxBytes`.
const IMAGE_PREVIEW_TOTAL_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// The MIME type for a path by extension, mirroring `UTType.preferredMIMEType`
/// with the same `application/octet-stream` fallback.
fn mime_for(path: &Path) -> String {
    mime_guess::from_path(path)
        .first_raw()
        .unwrap_or("application/octet-stream")
        .to_string()
}

/// Build the picked-file object for one path (Swift `pickedLocalFileDictionary`).
///
/// Reads the bytes into a `data:` URL only when the file is an image within both
/// caps, decrementing `remaining_image_bytes` by the bytes actually embedded.
pub fn picked_local_file(path: &Path, remaining_image_bytes: &mut u64) -> Value {
    let label = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path_str = path.to_string_lossy().into_owned();
    let mime = mime_for(path);
    let is_image = mime.starts_with("image/");

    let mut file = json!({
        "label": label,
        "path": path_str,
        "fsPath": path_str,
        "mimeType": mime,
        "isImage": is_image,
    });

    if is_image {
        if let Ok(metadata) = std::fs::metadata(path) {
            let byte_count = metadata.len();
            if byte_count <= IMAGE_PREVIEW_MAX_BYTES && byte_count <= *remaining_image_bytes {
                if let Ok(data) = std::fs::read(path) {
                    // Guard against a file that grew between stat and read (Swift's
                    // `data.count <= byteCount`), so the shared budget can't be
                    // overspent by a race.
                    if data.len() as u64 <= byte_count {
                        *remaining_image_bytes -= data.len() as u64;
                        let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
                        file["dataUrl"] = Value::String(format!("data:{mime};base64,{encoded}"));
                    }
                }
            }
        }
    }

    file
}

/// Map the selected paths to the `{ "files": [...] }` reply, spending a single
/// shared 2MB image-preview budget across the selection in order.
pub fn picked_files_value<I, P>(paths: I) -> Value
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut remaining = IMAGE_PREVIEW_TOTAL_MAX_BYTES;
    let files: Vec<Value> = paths
        .into_iter()
        .map(|path| picked_local_file(path.as_ref(), &mut remaining))
        .collect();
    json!({ "files": files })
}

#[tauri::command]
pub fn pick_textbox_files(app: AppHandle) -> Value {
    use tauri_plugin_dialog::DialogExt;

    match app
        .dialog()
        .file()
        .set_title("Attach files to TextBox Input")
        .blocking_pick_files()
    {
        Some(entries) => {
            let paths = entries
                .into_iter()
                .filter_map(|entry| entry.into_path().ok());
            picked_files_value(paths)
        }
        None => json!({ "files": [] }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("cmux-pickfiles-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(bytes).unwrap();
        path
    }

    #[test]
    fn non_image_has_no_data_url() {
        let path = write_temp("notes.txt", b"hello");
        let mut remaining = IMAGE_PREVIEW_TOTAL_MAX_BYTES;
        let file = picked_local_file(&path, &mut remaining);
        assert_eq!(file["isImage"], json!(false));
        assert_eq!(file["label"], json!("notes.txt"));
        assert_eq!(file["path"], file["fsPath"]);
        assert!(file.get("dataUrl").is_none());
        assert_eq!(remaining, IMAGE_PREVIEW_TOTAL_MAX_BYTES, "budget untouched");
    }

    #[test]
    fn small_image_gets_data_url_and_spends_budget() {
        let bytes = b"\x89PNG\r\n\x1a\nfake-png-body";
        let path = write_temp("pic.png", bytes);
        let mut remaining = IMAGE_PREVIEW_TOTAL_MAX_BYTES;
        let file = picked_local_file(&path, &mut remaining);
        assert_eq!(file["isImage"], json!(true));
        assert_eq!(file["mimeType"], json!("image/png"));
        let data_url = file["dataUrl"].as_str().expect("data url present");
        let expected = base64::engine::general_purpose::STANDARD.encode(bytes);
        assert_eq!(data_url, format!("data:image/png;base64,{expected}"));
        assert_eq!(
            remaining,
            IMAGE_PREVIEW_TOTAL_MAX_BYTES - bytes.len() as u64
        );
    }

    #[test]
    fn image_over_per_file_cap_is_path_only() {
        let big = vec![0u8; (IMAGE_PREVIEW_MAX_BYTES + 1) as usize];
        let path = write_temp("big.png", &big);
        let mut remaining = IMAGE_PREVIEW_TOTAL_MAX_BYTES;
        let file = picked_local_file(&path, &mut remaining);
        assert_eq!(file["isImage"], json!(true));
        assert!(
            file.get("dataUrl").is_none(),
            "over per-file cap → no preview"
        );
        assert_eq!(remaining, IMAGE_PREVIEW_TOTAL_MAX_BYTES, "budget untouched");
    }

    #[test]
    fn shared_budget_is_exhausted_across_the_selection() {
        // Three ~400KB images: the first two fit the 2MB total, the third would
        // exceed it and so is path-only.
        let body = vec![7u8; 400 * 1024];
        let a = write_temp("a.png", &body);
        let b = write_temp("b.png", &body);
        let c = write_temp("c.png", &body);
        // Force the running budget down so only two more fit is not the point —
        // here 3×400KB = 1.2MB all fit; instead shrink via a pre-spent budget by
        // picking five to cross 2MB.
        let paths = vec![a.clone(), b.clone(), c.clone(), a.clone(), b.clone(), c];
        let value = picked_files_value(paths);
        let files = value["files"].as_array().unwrap();
        let with_preview = files.iter().filter(|f| f.get("dataUrl").is_some()).count();
        // floor(2MB / 400KB) = 5 previews; the 6th is path-only.
        assert_eq!(with_preview, 5);
        assert!(files[5].get("dataUrl").is_none());
    }

    #[test]
    fn empty_selection_is_empty_files() {
        let value = picked_files_value(Vec::<std::path::PathBuf>::new());
        assert_eq!(value, json!({ "files": [] }));
    }
}
