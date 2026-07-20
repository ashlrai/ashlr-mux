#[test]
fn mapped_socket_command_help_is_concrete_for_control_aliases() {
    for command in [
        "browser-reload",
        "clear-history",
        "clear-notifications",
        "close-workspaces",
        "current-window",
        "dismiss-notification",
        "focus-webview",
        "get-url",
        "is-webview-focused",
        "jump-to-unread",
        "list-windows",
        "list-notifications",
        "mark-notification-read",
        "open-notification",
        "new-browser-workspace",
        "new-terminal-tab",
        "notify",
        "reload-config",
        "refresh-surfaces",
        "move-workspace-to-window",
        "move-surface",
        "reorder-workspace",
        "reorder-workspaces",
        "reorder-surface",
        "right-sidebar",
        "rename-window",
        "reopen-closed-browser-tab",
        "restore-session",
        "trigger-flash",
        "read-screen",
        "capture-pane",
        "restore-previous-launch",
        "split-browser",
        "split-off",
        "drag-surface-to-split",
        "swap-pane",
        "break-pane",
        "join-pane",
        "last-pane",
        "last-window",
        "resize-pane",
    ] {
        let plan = plan(
            &PreSocketAction::SubcommandHelp {
                command: command.to_owned(),
            },
            command,
        );
        match plan {
            DispatchPlan::PrintLine(text) => {
                assert!(text.starts_with(&format!("cmux {command}\n\n")));
                assert!(
                    text.contains("Usage:\n  cmux") || text.contains("Usage: cmux"),
                    "{command}: {text}"
                );
                assert!(!text.contains("not yet ported"), "{command}: {text}");
            }
            other => panic!("expected PrintLine for {command}, got {other:?}"),
        }
    }
}
