#[cfg(windows)]
pub(super) fn print_legacy_workspace_alias_notice(command: &str) {
    if std::env::var_os("CMUX_QUIET").is_some() {
        return;
    }
    let replacement = match command {
        "list-workspaces" => "workspace list",
        "new-workspace" => "workspace create",
        "close-workspace" => "workspace close",
        "select-workspace" => "workspace select",
        "rename-workspace" => "workspace rename",
        _ => return,
    };
    super::safe_stderr(
        format_args!(
            "cmux: '{command}' is now an alias for 'cmux {replacement}'. \
             The legacy form keeps working indefinitely; set CMUX_QUIET=1 to silence this notice."
        ),
        true,
    );
}
