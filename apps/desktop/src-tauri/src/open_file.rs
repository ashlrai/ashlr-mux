//! Native file pickers for pane-hosted preview surfaces.
//!
//! The markdown lane uses a dedicated picker so the web shell can ask for an
//! initial document path before switching a pane into the markdown viewer.

use tauri::AppHandle;

#[tauri::command]
pub fn pick_markdown_file(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;

    app.dialog()
        .file()
        .set_title("Open Markdown File")
        .add_filter(
            "Markdown",
            &["md", "markdown", "mdown", "mkd", "mkdn", "mdx"],
        )
        .blocking_pick_file()
        .and_then(|entry| entry.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
}
