use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

const MAX_TEXT_FILE_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileExplorerListRequest {
    pub root_path: String,
    pub relative_path: Option<String>,
    pub show_hidden: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileExplorerEntry {
    pub name: String,
    pub path: String,
    pub relative_path: String,
    pub kind: FileExplorerEntryKind,
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileExplorerOpenRequest {
    pub path: String,
    pub preferred_editor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileExplorerOpenReply {
    pub opened: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileExplorerReadFileRequest {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileExplorerReadFileReply {
    pub path: String,
    pub content: String,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileExplorerWriteFileRequest {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileExplorerWriteFileReply {
    pub saved: bool,
    pub message: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileExplorerEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn validate_relative_path(relative_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative_path);
    if path.is_absolute() {
        return Err("File explorer path must be relative to the workspace.".to_string());
    }

    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => safe.push(value),
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err("File explorer path cannot escape the workspace.".to_string());
            }
        }
    }
    Ok(safe)
}

fn resolve_directory(
    root_path: &str,
    relative: Option<&str>,
) -> Result<(PathBuf, PathBuf), String> {
    let root = PathBuf::from(root_path.trim());
    let root =
        fs::canonicalize(&root).map_err(|_| "Workspace directory does not exist.".to_string())?;
    if !root.is_dir() {
        return Err("Workspace path is not a directory.".to_string());
    }

    let safe_relative = validate_relative_path(relative.unwrap_or(""))?;
    let directory = fs::canonicalize(root.join(safe_relative))
        .map_err(|_| "File explorer directory does not exist.".to_string())?;
    if !directory.starts_with(&root) {
        return Err("File explorer path cannot escape the workspace.".to_string());
    }
    if !directory.is_dir() {
        return Err("File explorer path is not a directory.".to_string());
    }

    Ok((root, directory))
}

fn entry_kind(file_type: &fs::FileType) -> FileExplorerEntryKind {
    if file_type.is_dir() {
        FileExplorerEntryKind::Directory
    } else if file_type.is_file() {
        FileExplorerEntryKind::File
    } else if file_type.is_symlink() {
        FileExplorerEntryKind::Symlink
    } else {
        FileExplorerEntryKind::Other
    }
}

fn entry_sort_key(kind: FileExplorerEntryKind) -> u8 {
    match kind {
        FileExplorerEntryKind::Directory => 0,
        FileExplorerEntryKind::File => 1,
        FileExplorerEntryKind::Symlink => 2,
        FileExplorerEntryKind::Other => 3,
    }
}

pub fn list_directory(request: FileExplorerListRequest) -> Result<Vec<FileExplorerEntry>, String> {
    let (root, directory) =
        resolve_directory(&request.root_path, request.relative_path.as_deref())?;
    let show_hidden = request.show_hidden.unwrap_or(false);
    let mut entries = Vec::new();

    for entry in fs::read_dir(&directory)
        .map_err(|error| format!("Failed to read file explorer directory: {error}"))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let kind = entry_kind(&file_type);
        let path = entry.path();
        let size = (kind == FileExplorerEntryKind::File)
            .then(|| entry.metadata().ok().map(|metadata| metadata.len()))
            .flatten();
        entries.push(FileExplorerEntry {
            name,
            path: path_to_string(&path),
            relative_path: relative_path(&root, &path),
            kind,
            size,
        });
    }

    entries.sort_by(|left, right| {
        entry_sort_key(left.kind)
            .cmp(&entry_sort_key(right.kind))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(entries)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenCommandSpec {
    program: PathBuf,
    args: Vec<String>,
}

fn shell_arg(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

fn shell_program_and_args(command: &str) -> (PathBuf, Vec<String>) {
    #[cfg(windows)]
    {
        let program = std::env::var_os("ComSpec")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        return (
            program,
            vec!["/d".to_string(), "/c".to_string(), command.to_string()],
        );
    }

    #[cfg(not(windows))]
    {
        (
            PathBuf::from("sh"),
            vec!["-c".to_string(), command.to_string()],
        )
    }
}

fn open_command_spec(path: &Path, preferred_editor: Option<&str>) -> OpenCommandSpec {
    let path_string = path.to_string_lossy();
    let preferred = preferred_editor
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(command) = preferred {
        let (program, args) =
            shell_program_and_args(&format!("{command} {}", shell_arg(&path_string)));
        return OpenCommandSpec { program, args };
    }

    #[cfg(windows)]
    {
        let (program, args) =
            shell_program_and_args(&format!("start \"\" {}", shell_arg(&path_string)));
        OpenCommandSpec { program, args }
    }

    #[cfg(target_os = "macos")]
    {
        OpenCommandSpec {
            program: PathBuf::from("open"),
            args: vec![path_string.into_owned()],
        }
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        OpenCommandSpec {
            program: PathBuf::from("xdg-open"),
            args: vec![path_string.into_owned()],
        }
    }
}

pub fn open_path(request: FileExplorerOpenRequest) -> Result<FileExplorerOpenReply, String> {
    let path = fs::canonicalize(PathBuf::from(request.path.trim()))
        .map_err(|_| "File explorer entry does not exist.".to_string())?;
    if !path.is_file() {
        return Err("File explorer can only open files with this action.".to_string());
    }

    let spec = open_command_spec(&path, request.preferred_editor.as_deref());
    Command::new(&spec.program)
        .args(&spec.args)
        .spawn()
        .map_err(|error| format!("Failed to open file: {error}"))?;

    Ok(FileExplorerOpenReply {
        opened: true,
        message: "Opening file.".to_string(),
    })
}

fn resolve_regular_file(path: &str) -> Result<PathBuf, String> {
    let path = fs::canonicalize(PathBuf::from(path.trim()))
        .map_err(|_| "File explorer entry does not exist.".to_string())?;
    if !path.is_file() {
        return Err("File explorer can only read regular files.".to_string());
    }
    Ok(path)
}

pub fn read_file(
    request: FileExplorerReadFileRequest,
) -> Result<FileExplorerReadFileReply, String> {
    let path = resolve_regular_file(&request.path)?;
    let metadata = fs::metadata(&path).map_err(|error| format!("Failed to stat file: {error}"))?;
    if metadata.len() > MAX_TEXT_FILE_BYTES {
        return Err(format!(
            "File is too large to edit in cmux (limit {} MB).",
            MAX_TEXT_FILE_BYTES / (1024 * 1024)
        ));
    }

    let bytes = fs::read(&path).map_err(|error| format!("Failed to read file: {error}"))?;
    let content =
        String::from_utf8(bytes).map_err(|_| "File is not valid UTF-8 text.".to_string())?;
    Ok(FileExplorerReadFileReply {
        path: path_to_string(&path),
        size: metadata.len(),
        content,
    })
}

pub fn write_file(
    request: FileExplorerWriteFileRequest,
) -> Result<FileExplorerWriteFileReply, String> {
    let path = resolve_regular_file(&request.path)?;
    fs::write(&path, request.content.as_bytes())
        .map_err(|error| format!("Failed to save file: {error}"))?;
    let size = fs::metadata(&path)
        .map_err(|error| format!("Failed to stat saved file: {error}"))?
        .len();
    Ok(FileExplorerWriteFileReply {
        saved: true,
        message: "Saved file.".to_string(),
        size,
    })
}

#[tauri::command]
pub fn file_explorer_list_directory(
    request: FileExplorerListRequest,
) -> Result<Vec<FileExplorerEntry>, String> {
    list_directory(request)
}

#[tauri::command]
pub fn file_explorer_open_path(
    request: FileExplorerOpenRequest,
) -> Result<FileExplorerOpenReply, String> {
    open_path(request)
}

#[tauri::command]
pub fn file_explorer_read_file(
    request: FileExplorerReadFileRequest,
) -> Result<FileExplorerReadFileReply, String> {
    read_file(request)
}

#[tauri::command]
pub fn file_explorer_write_file(
    request: FileExplorerWriteFileRequest,
) -> Result<FileExplorerWriteFileReply, String> {
    write_file(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn request(root: &TempDir, relative_path: Option<&str>) -> FileExplorerListRequest {
        FileExplorerListRequest {
            root_path: root.path().to_string_lossy().to_string(),
            relative_path: relative_path.map(str::to_string),
            show_hidden: None,
        }
    }

    #[test]
    fn list_directory_returns_directory_first_sorted_entries() {
        let tmp = TempDir::new().expect("temp dir");
        fs::create_dir(tmp.path().join("src")).expect("mkdir");
        fs::write(tmp.path().join("README.md"), "hello").expect("write");
        fs::write(tmp.path().join("alpha.txt"), "a").expect("write");
        fs::write(tmp.path().join(".env"), "secret").expect("write");

        let entries = list_directory(request(&tmp, None)).expect("list");

        assert_eq!(
            entries
                .iter()
                .map(|entry| (&entry.name, entry.kind))
                .collect::<Vec<_>>(),
            vec![
                (&"src".to_string(), FileExplorerEntryKind::Directory),
                (&"alpha.txt".to_string(), FileExplorerEntryKind::File),
                (&"README.md".to_string(), FileExplorerEntryKind::File),
            ]
        );
        assert_eq!(entries[1].relative_path, "alpha.txt");
        assert_eq!(entries[1].size, Some(1));
    }

    #[test]
    fn list_directory_can_include_dotfiles() {
        let tmp = TempDir::new().expect("temp dir");
        fs::write(tmp.path().join(".env"), "secret").expect("write");
        let mut request = request(&tmp, None);
        request.show_hidden = Some(true);

        let entries = list_directory(request).expect("list");

        assert_eq!(entries[0].name, ".env");
    }

    #[test]
    fn list_directory_resolves_nested_relative_paths() {
        let tmp = TempDir::new().expect("temp dir");
        fs::create_dir_all(tmp.path().join("src/components")).expect("mkdir");
        fs::write(tmp.path().join("src/components/App.tsx"), "export {}").expect("write");

        let entries = list_directory(request(&tmp, Some("src/components"))).expect("list");

        assert_eq!(entries[0].relative_path, "src/components/App.tsx");
    }

    #[test]
    fn list_directory_rejects_workspace_escape() {
        let tmp = TempDir::new().expect("temp dir");

        assert!(list_directory(request(&tmp, Some("../outside"))).is_err());
        assert!(list_directory(request(&tmp, Some("C:/outside"))).is_err());
    }

    #[test]
    fn open_command_spec_uses_preferred_editor_shell_command() {
        let spec = open_command_spec(Path::new("C:/repo/README.md"), Some("code -r"));

        assert!(spec.args.iter().any(|arg| arg.contains("code -r")));
        assert!(spec.args.iter().any(|arg| arg.contains("README.md")));
    }

    #[test]
    fn open_path_rejects_directories() {
        let tmp = TempDir::new().expect("temp dir");

        let result = open_path(FileExplorerOpenRequest {
            path: tmp.path().to_string_lossy().to_string(),
            preferred_editor: Some("code".to_string()),
        });

        assert!(result.is_err());
    }

    #[test]
    fn read_file_returns_utf8_content_and_size() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("notes.txt");
        fs::write(&path, "hello\nworld").expect("write");

        let reply = read_file(FileExplorerReadFileRequest {
            path: path.to_string_lossy().to_string(),
        })
        .expect("read");

        assert_eq!(reply.content, "hello\nworld");
        assert_eq!(reply.size, 11);
    }

    #[test]
    fn read_file_rejects_non_utf8_content() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("binary.bin");
        fs::write(&path, [0xff, 0xfe, 0xfd]).expect("write");

        let result = read_file(FileExplorerReadFileRequest {
            path: path.to_string_lossy().to_string(),
        });

        assert!(result.is_err());
    }

    #[test]
    fn write_file_updates_existing_regular_file() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("notes.txt");
        fs::write(&path, "old").expect("write");

        let reply = write_file(FileExplorerWriteFileRequest {
            path: path.to_string_lossy().to_string(),
            content: "new text".to_string(),
        })
        .expect("save");

        assert!(reply.saved);
        assert_eq!(reply.size, 8);
        assert_eq!(fs::read_to_string(&path).expect("read"), "new text");
    }
}
