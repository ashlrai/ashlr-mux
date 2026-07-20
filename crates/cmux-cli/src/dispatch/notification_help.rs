pub(super) fn usage(command: &str) -> Option<&'static str> {
    match command {
        "notify" => Some(
            "Usage: cmux notify [flags]\n\nSend a notification to a workspace/surface.\n\nFlags:\n  --title <text>         Notification title (default: \"Notification\")\n  --subtitle <text>      Notification subtitle\n  --body <text>          Notification body\n  --workspace <id|ref|index>   Target workspace, except explicit surface UUIDs resolve globally\n  --surface <id|ref|index>     Target surface (refs/indexes use workspace/window context)\n  --window <id|ref|index>      Window context for workspace/surface refs and indexes\n\nExample:\n  cmux notify --title \"Build done\" --body \"All tests passed\"\n  cmux notify --title \"Error\" --subtitle \"test.swift\" --body \"Line 42: syntax error\"\n  cmux notify --surface <uuid> --title \"Build done\"",
        ),
        "list-notifications" => Some(
            "Usage: cmux list-notifications\n\nList queued notifications.",
        ),
        "dismiss-notification" => Some(
            "Usage: cmux dismiss-notification (--id <uuid> | --all-read)\n\nRemove one notification, or remove every already-read notification.\n\nFlags:\n  --id <uuid>           Notification id to remove\n  --all-read            Remove every already-read notification\n  --json                Print JSON\n  --id-format <mode>    refs, uuids, or both",
        ),
        "mark-notification-read" => Some(
            "Usage: cmux mark-notification-read (--id <uuid> | --workspace <id|ref|index> [--surface <id|ref|index>] [--window <id|ref|index>] | --all)\n\nMark notifications read without opening them. Exactly one selector is required.\n\nFlags:\n  --id <uuid>           Mark one notification read\n  --workspace <id|ref|index>  Mark notifications for a workspace\n  --surface <id|ref|index>    Narrow --workspace to one surface\n  --window <id|ref|index>     Window context for workspace/surface refs and indexes\n  --all                 Mark every notification read\n  --json                Print JSON\n  --id-format <mode>    refs, uuids, or both",
        ),
        "open-notification" => Some(
            "Usage: cmux open-notification --id <uuid>\n\nFocus the notification's workspace and surface, then mark the row read.\n\nFlags:\n  --id <uuid>           Notification id to open\n  --json                Print JSON\n  --id-format <mode>    refs, uuids, or both",
        ),
        "jump-to-unread" => Some(
            "Usage: cmux jump-to-unread\n\nFocus the latest unread notification, matching the Notifications page action.\n\nFlags:\n  --json                Print JSON\n  --id-format <mode>    refs, uuids, or both",
        ),
        "clear-notifications" => Some(
            "Usage: cmux clear-notifications [--workspace <id|ref|index>] [--window <id|ref|index>]\n\nClear all queued notifications, or only the selected/targeted workspace when --window or --workspace is set.",
        ),
        _ => None,
    }
}
