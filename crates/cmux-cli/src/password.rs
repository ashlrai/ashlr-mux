//! Socket-password source assembly for the CLI composition root (M4 WS5).
//!
//! The precedence policy and newline normalization live in
//! [`cmux_ipc::resolve_password`]; this module only *reads* the concrete sources
//! (the `--password` flag value, `CMUX_SOCKET_PASSWORD`, and the password file)
//! and hands them to it. It must not trim or reorder — that would break the
//! deliberate "preserve intentional spaces, strip only the file's trailing
//! newline" behavior. Mirrors `CLI/cmux.swift` `SocketPasswordResolver`.

use std::path::PathBuf;

/// Relative location of the socket-control password file under `LOCALAPPDATA`,
/// matching the app's state directory (`%LOCALAPPDATA%\cmux\state\`, the same
/// root `cmux-process` uses for its ledger).
const PASSWORD_FILE_SEGMENTS: [&str; 3] = ["cmux", "state", "socket-control-password"];

/// The full path to the socket-control password file given the `LOCALAPPDATA`
/// directory, or `None` when it is unset. Injected for testability.
pub fn password_file_path(local_app_data: Option<&str>) -> Option<PathBuf> {
    let mut path = PathBuf::from(local_app_data?);
    path.extend(PASSWORD_FILE_SEGMENTS);
    Some(path)
}

/// Read the socket-control password file's **full, untrimmed** UTF-8 contents.
/// Returns `None` when `LOCALAPPDATA` is unset, the file is missing, or its
/// bytes are not valid UTF-8. The trailing newline is intentionally kept here —
/// [`cmux_ipc::resolve_password`] strips newlines while preserving any
/// intentional space padding, so trimming now would be wrong.
pub fn read_password_file(local_app_data: Option<&str>) -> Option<String> {
    let path = password_file_path(local_app_data)?;
    // read_to_string is None on a missing file and on non-UTF-8 bytes, and
    // returns the full contents untrimmed — exactly this read path's contract.
    std::fs::read_to_string(path).ok()
}

// Password precedence + normalization live in `cmux_ipc::resolve_password`; the
// command-dispatch slice fills a `cmux_ipc::PasswordSources` with named fields
// directly (explicit = `--password`, env = `CMUX_SOCKET_PASSWORD`, file =
// [`read_password_file`], keychain = None until Credential Manager is wired) and
// calls it. A positional wrapper here would only reintroduce the source-
// transposition hazard that the named-field struct exists to prevent.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_file_path_joins_state_dir_or_is_none() {
        assert_eq!(password_file_path(None), None);
        let path = password_file_path(Some("C:\\Users\\u\\AppData\\Local")).unwrap();
        assert!(path.ends_with("cmux/state/socket-control-password") || path.ends_with("cmux\\state\\socket-control-password"));
        assert!(path.starts_with("C:\\Users\\u\\AppData\\Local"));
    }

    #[test]
    fn read_password_file_is_none_without_local_app_data() {
        assert_eq!(read_password_file(None), None);
    }

    #[test]
    fn read_password_file_returns_untrimmed_contents_that_resolve_strips() {
        // Write a password file with a trailing newline under a temp LOCALAPPDATA.
        let base = std::env::temp_dir().join(format!("cmux-cli-pw-test-{}", std::process::id()));
        let dir = base.join("cmux").join("state");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("socket-control-password"), b"s3cret \n").expect("write");

        let read = read_password_file(base.to_str()).expect("read");
        assert_eq!(read, "s3cret \n", "contents returned untrimmed");

        // The command-dispatch slice feeds this into PasswordSources; resolve
        // strips only the newline, preserving the intentional trailing space.
        let resolved = cmux_ipc::resolve_password(cmux_ipc::PasswordSources {
            file: Some(&read),
            ..Default::default()
        });
        assert_eq!(resolved.as_deref(), Some("s3cret "));

        std::fs::remove_dir_all(&base).ok();
    }
}
