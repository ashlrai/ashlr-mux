//! Canonical no-socket `cmux welcome` rendering.

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";

fn color(red: u8, green: u8, blue: u8) -> String {
    format!("\x1b[38;2;{red};{green};{blue}m")
}

pub fn render_welcome(is_dark: bool) -> String {
    let colors = [
        color(0, 212, 255),
        color(24, 181, 250),
        color(48, 150, 245),
        color(72, 119, 241),
        color(96, 88, 239),
        color(110, 73, 238),
        color(124, 58, 237),
    ];
    let tagline = if is_dark {
        color(130, 130, 140)
    } else {
        color(90, 90, 98)
    };
    let subdued = if is_dark {
        "\x1b[2m".to_string()
    } else {
        color(100, 100, 108)
    };
    let logo = format!(
        "{}  ::{RESET}\n{}    ::::{RESET}              {}c{}m{}u{}x{RESET}\n{}      ::::::{RESET}\n{}        ::::::{RESET}        {tagline}the open source terminal{RESET}\n{}      ::::::{RESET}          {tagline}built for coding agents{RESET}\n{}    ::::{RESET}\n{}  ::{RESET}",
        colors[0], colors[1], colors[0], colors[1], colors[2], colors[6], colors[2], colors[3], colors[4], colors[5], colors[6]
    );
    let shortcuts = [
        ("⌘N", "New workspace"),
        ("⌘T", "New tab"),
        ("⌘P", "Go to workspace"),
        ("⌘B", "Toggle Left Sidebar"),
        ("⌘⌥B", "Toggle Right Sidebar"),
        ("⌘D", "Split right"),
        ("⌘⇧D", "Split down"),
        ("⌘⇧P", "Command palette"),
        ("⌘⇧R", "Rename workspace"),
        ("⌘⇧L", "New browser"),
        ("⌘⇧U", "Jump to latest unread"),
        ("⌥⌘U", "Toggle unread"),
    ];
    let mut output = format!("\n{logo}\n\n  {BOLD}Shortcuts{RESET}\n\n");
    for (keys, description) in shortcuts {
        append_row(&mut output, keys, description, &subdued);
    }
    output.push('\n');
    for (label, value) in [
        ("Docs", "https://cmux.com/docs"),
        ("Discord", "https://discord.gg/xsgFEVrWCZ"),
        (
            "GitHub",
            "https://github.com/manaflow-ai/cmux (please leave a star ⭐)",
        ),
        ("Email", "founders@manaflow.com"),
    ] {
        append_row(&mut output, label, value, &subdued);
    }
    output.push_str(&format!(
        "\n  {subdued}Run {RESET}{BOLD}cmux --help{RESET}{subdued} for all commands.{RESET}\n  {subdued}Run {RESET}{BOLD}cmux shortcuts{RESET}{subdued} to edit shortcuts.{RESET}\n  {subdued}Run {RESET}{BOLD}cmux feedback{RESET}{subdued} to report a bug.{RESET}\n\n"
    ));
    output
}

fn append_row(output: &mut String, label: &str, value: &str, subdued: &str) {
    let padding = 20usize.saturating_sub(label.chars().count());
    output.push_str(&format!(
        "  {BOLD}{label}{RESET}{subdued}{}{value}{RESET}\n",
        " ".repeat(padding)
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_canonical_light_welcome_content_and_spacing() {
        let output = render_welcome(false);
        assert!(output.starts_with("\n\x1b[38;2;0;212;255m  ::"));
        assert!(output.contains("\x1b[38;2;90;90;98mthe open source terminal"));
        assert!(output.contains("\x1b[1m⌘⇧P\x1b[0m"));
        assert!(output.contains(
            "  \x1b[1m⌘N\x1b[0m\x1b[38;2;100;100;108m                  New workspace\x1b[0m\n"
        ));
        assert!(output.contains("https://github.com/manaflow-ai/cmux (please leave a star ⭐)"));
        assert!(output.ends_with("to report a bug.\x1b[0m\n\n"));
    }
}
