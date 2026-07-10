use std::collections::BTreeMap;

use base64::Engine;

use crate::invocation::CliError;

pub const SSH_USAGE_TEXT: &str = "\
Usage:
  cmux ssh DESTINATION [--port PORT] [--identity PATH] [--name TITLE] [--ssh-option KEY=VALUE]

Create a new workspace connected to DESTINATION over SSH, install the remote cmux relay bootstrap, and configure remote workspace metadata.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshCommandBuildOptions {
    pub unique_id: String,
    pub remote_relay_port: u16,
    pub startup_script_path: String,
    pub existing_ghostty_shell_features: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshCommandPlan {
    pub destination: String,
    pub title: String,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub ssh_options: Vec<String>,
    pub effective_ssh_options: Vec<String>,
    pub control_path: String,
    pub remote_relay_port: u16,
    pub persistent_daemon_slot: String,
    pub ssh_command: String,
    pub ssh_terminal_command: String,
    pub ssh_startup_command: String,
    pub ssh_startup_command_text: String,
    pub ssh_startup_script: String,
    pub remote_bootstrap_script: String,
    pub ssh_env_overrides: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSshCommand {
    destination: String,
    title: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
    ssh_options: Vec<String>,
}

pub fn build_ssh_command_plan(
    args: &[String],
    options: SshCommandBuildOptions,
) -> Result<SshCommandPlan, CliError> {
    let parsed = parse_ssh_command(args)?;
    let unique_id = normalized_unique_id(&options.unique_id);
    let persistent_daemon_slot = format!("ssh-workspace-{unique_id}");
    let title = parsed
        .title
        .clone()
        .unwrap_or_else(|| parsed.destination.clone());
    let control_path = first_ssh_option_value(&parsed.ssh_options, "ControlPath")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("/tmp/cmux-ssh-{unique_id}-%C"));
    let effective_ssh_options =
        effective_interactive_ssh_options(&parsed.ssh_options, &control_path);
    let ssh_command = interactive_ssh_command(
        &parsed.destination,
        parsed.port,
        parsed.identity_file.as_deref(),
        &effective_ssh_options,
        None,
    );
    let remote_bootstrap_script = remote_bootstrap_script(
        options.remote_relay_port,
        "__CMUX_WORKSPACE_ID__",
        "__CMUX_SURFACE_ID__",
    );
    let ssh_startup_script = startup_script(options.remote_relay_port, &remote_bootstrap_script);
    let remote_startup = remote_startup_command(options.remote_relay_port, &ssh_startup_script);
    let ssh_terminal_command = interactive_ssh_command(
        &parsed.destination,
        parsed.port,
        parsed.identity_file.as_deref(),
        &effective_ssh_options,
        Some(&remote_startup),
    );
    let ssh_env_overrides = BTreeMap::from_iter([(
        "GHOSTTY_SHELL_FEATURES".to_string(),
        merged_shell_features(options.existing_ghostty_shell_features.as_deref()),
    )]);

    Ok(SshCommandPlan {
        destination: parsed.destination,
        title,
        port: parsed.port,
        identity_file: parsed.identity_file,
        ssh_options: parsed.ssh_options,
        effective_ssh_options,
        control_path,
        remote_relay_port: options.remote_relay_port,
        persistent_daemon_slot,
        ssh_command,
        ssh_terminal_command,
        ssh_startup_command: options.startup_script_path,
        ssh_startup_command_text: remote_startup,
        ssh_startup_script,
        remote_bootstrap_script,
        ssh_env_overrides,
    })
}

fn parse_ssh_command(args: &[String]) -> Result<ParsedSshCommand, CliError> {
    let mut destination = None;
    let mut title = None;
    let mut port = None;
    let mut identity_file = None;
    let mut ssh_options = Vec::new();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--port" | "-p" => {
                let value = take_ssh_value(args, index, "cmux ssh --port requires a value")?;
                port = Some(parse_u16(&value, "cmux ssh --port must be 1-65535")?);
                index += 2;
            }
            "--name" => {
                title = Some(take_ssh_value(
                    args,
                    index,
                    "cmux ssh --name requires a value",
                )?);
                index += 2;
            }
            "--identity" | "-i" => {
                identity_file = Some(take_ssh_value(
                    args,
                    index,
                    "cmux ssh --identity requires a path",
                )?);
                index += 2;
            }
            "--ssh-option" => {
                ssh_options.push(take_ssh_value(
                    args,
                    index,
                    "cmux ssh --ssh-option requires KEY=VALUE",
                )?);
                index += 2;
            }
            "--relay-port" => {
                // The executor owns relay-port allocation. Accepting the legacy
                // flag here keeps presentation parsing stable while preventing
                // duplicate/conflicting relay ownership in the pure planner.
                let _ = take_ssh_value(args, index, "cmux ssh --relay-port requires a value")?;
                index += 2;
            }
            "--" => {
                index += 1;
                while index < args.len() {
                    set_destination(&mut destination, &args[index])?;
                    index += 1;
                }
            }
            value if value.starts_with('-') => {
                return Err(CliError::new(format!("unknown cmux ssh option: {value}")));
            }
            value => {
                set_destination(&mut destination, value)?;
                index += 1;
            }
        }
    }

    let destination = destination.ok_or_else(|| {
        CliError::new("cmux ssh requires a destination, e.g. cmux ssh user@example.com")
    })?;

    Ok(ParsedSshCommand {
        destination,
        title,
        port,
        identity_file,
        ssh_options: cmux_ssh::trimmed_ssh_options(&ssh_options),
    })
}

fn set_destination(destination: &mut Option<String>, value: &str) -> Result<(), CliError> {
    if destination.is_some() {
        return Err(CliError::new("cmux ssh accepts exactly one destination"));
    }
    let value = value.trim();
    if value.is_empty() {
        return Err(CliError::new("cmux ssh destination cannot be empty"));
    }
    *destination = Some(value.to_string());
    Ok(())
}

fn take_ssh_value(args: &[String], index: usize, error: &str) -> Result<String, CliError> {
    args.get(index + 1)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::new(error))
}

fn parse_u16(value: &str, error: &str) -> Result<u16, CliError> {
    let parsed = value.parse::<u16>().map_err(|_| CliError::new(error))?;
    if parsed == 0 {
        return Err(CliError::new(error));
    }
    Ok(parsed)
}

fn normalized_unique_id(unique_id: &str) -> String {
    let normalized: String = unique_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let normalized = normalized.trim_matches('-');
    if normalized.is_empty() {
        "workspace".to_string()
    } else {
        normalized.to_string()
    }
}

fn effective_interactive_ssh_options(configured: &[String], control_path: &str) -> Vec<String> {
    let mut options = cmux_ssh::trimmed_ssh_options(configured);
    if !has_ssh_option_key(&options, "StrictHostKeyChecking") {
        options.push("StrictHostKeyChecking=accept-new".to_string());
    }
    if !has_ssh_option_key(&options, "ControlMaster") {
        options.push("ControlMaster=auto".to_string());
    }
    if !has_ssh_option_key(&options, "ControlPersist") {
        options.push("ControlPersist=600".to_string());
    }
    if !has_ssh_option_key(&options, "ControlPath") {
        options.push(format!("ControlPath={control_path}"));
    }
    options
}

fn merged_shell_features(existing: Option<&str>) -> String {
    let mut features = Vec::<String>::new();
    for feature in existing
        .unwrap_or_default()
        .split(',')
        .chain(["ssh-env", "ssh-terminfo"])
    {
        let feature = feature.trim();
        if feature.is_empty() {
            continue;
        }
        if !features.iter().any(|existing| existing == feature) {
            features.push(feature.to_string());
        }
    }
    features.join(",")
}

fn interactive_ssh_command(
    destination: &str,
    port: Option<u16>,
    identity_file: Option<&str>,
    ssh_options: &[String],
    remote_command: Option<&str>,
) -> String {
    let mut tokens = vec!["ssh".to_string()];
    if let Some(port) = port {
        tokens.push("-p".to_string());
        tokens.push(port.to_string());
    }
    if let Some(identity_file) = identity_file {
        if !identity_file.trim().is_empty() {
            tokens.push("-i".to_string());
            tokens.push(identity_file.to_string());
        }
    }
    for option in ssh_options {
        tokens.push("-o".to_string());
        tokens.push(option.clone());
    }
    tokens.push(destination.to_string());
    if let Some(remote_command) = remote_command {
        tokens.push(remote_command.to_string());
    }
    shell_join(&tokens)
}

fn remote_startup_command(remote_relay_port: u16, startup_script: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(startup_script.as_bytes());
    format!(
        "cmux_tmp=$(mktemp -t cmux-ssh-startup-XXXXXX.sh) && \
         cmux_startup_b64={encoded} && \
         cmux_bootstrap_tty=\"$(tty 2>/dev/null || true)\" && \
         mkdir -p \"$HOME/.cmux/relay\" && \
         printf '%s\\n' \"$cmux_bootstrap_tty\" > \"$HOME/.cmux/relay/{remote_relay_port}.tty\" && \
         export CMUX_BOOTSTRAP_TTY=\"$cmux_bootstrap_tty\" && \
         (printf '%s' \"$cmux_startup_b64\" | base64 -d > \"$cmux_tmp\" 2>/dev/null || \
          printf '%s' \"$cmux_startup_b64\" | base64 --decode > \"$cmux_tmp\") && \
         chmod +x \"$cmux_tmp\" && \
         PATH=\"$HOME/.cmux/bin:$PATH\" CMUX_SOCKET_PATH=127.0.0.1:{remote_relay_port} /bin/sh \"$cmux_tmp\""
    )
}

fn startup_script(remote_relay_port: u16, remote_bootstrap: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(remote_bootstrap.as_bytes());
    format!(
        "#!/bin/sh\n\
         set -eu\n\
         cmux_bootstrap_path=\"$HOME/.cmux/relay/{remote_relay_port}.bootstrap.sh\"\n\
         cmux_remote_bootstrap_b64={encoded}\n\
         mkdir -p \"$HOME/.cmux/relay\" \"$HOME/.cmux/bin\"\n\
         cat > \"$cmux_bootstrap_path\" <<'CMUX_BOOTSTRAP_PAYLOAD'\n\
         {remote_bootstrap}\n\
         CMUX_BOOTSTRAP_PAYLOAD\n\
         chmod +x \"$cmux_bootstrap_path\"\n\
         cmux_bootstrap_tty=\"${{CMUX_BOOTSTRAP_TTY:-$(tty 2>/dev/null || true)}}\"\n\
         export CMUX_BOOTSTRAP_TTY=\"$cmux_bootstrap_tty\"\n\
         /bin/sh -c /bin/true\n\
         /bin/sh \"$HOME/.cmux/relay/{remote_relay_port}.bootstrap.sh\"\n"
    )
}

fn remote_bootstrap_script(
    remote_relay_port: u16,
    workspace_id_placeholder: &str,
    surface_id_placeholder: &str,
) -> String {
    format!(
        "#!/bin/sh\n\
         export PATH=\"$HOME/.cmux/bin:$PATH\"\n\
         export CMUX_SOCKET_PATH=127.0.0.1:{remote_relay_port}\n\
         export CMUX_WORKSPACE_ID='{workspace_id_placeholder}'\n\
         export CMUX_TAB_ID='{workspace_id_placeholder}'\n\
         export CMUX_SURFACE_ID='{surface_id_placeholder}'\n\
         export CMUX_PANEL_ID='{surface_id_placeholder}'\n\
         cmux_relay_cli=\"$HOME/.cmux/bin/cmux\"\n\
         cmux_relay_tty=\"${{CMUX_BOOTSTRAP_TTY:-}}\"\n\
         cmux_relay_report_tty='{{\"tty\":\"'\"$cmux_relay_tty\"'\"}}'\n\
         \"$cmux_relay_cli\" rpc surface.report_tty \"$cmux_relay_report_tty\" || true\n\
         cmux_relay_ports_kick='{{}}'\n\
         \"$cmux_relay_cli\" rpc surface.ports_kick \"$cmux_relay_ports_kick\" || true\n\
         CMUX_LOGIN_SHELL=\"${{SHELL:-/bin/sh}}\"\n\
         cmux_shell_dir=\"$HOME/.cmux/shell\"\n\
         mkdir -p \"$cmux_shell_dir\"\n\
         cat > \"$cmux_shell_dir/.zshrc\" <<'CMUX_ZSHRC'\n\
         export PATH=\"$HOME/.cmux/bin:$PATH\"\n\
         CMUX_ZSHRC\n\
         cat > \"$cmux_shell_dir/.bashrc\" <<'CMUX_BASHRC'\n\
         export PATH=\"$HOME/.cmux/bin:$PATH\"\n\
         CMUX_BASHRC\n\
         case \"${{CMUX_LOGIN_SHELL##*/}}\" in\n\
           bash) exec \"$CMUX_LOGIN_SHELL\" --rcfile \"$cmux_shell_dir/.bashrc\" -i ;;\n\
           zsh) ZDOTDIR=\"$cmux_shell_dir\" exec \"$CMUX_LOGIN_SHELL\" -i ;;\n\
           *) exec \"$CMUX_LOGIN_SHELL\" -i ;;\n\
         esac\n"
    )
}

fn has_ssh_option_key(options: &[String], key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    options
        .iter()
        .any(|option| ssh_option_key(option).as_deref() == Some(key.as_str()))
}

fn first_ssh_option_value(options: &[String], key: &str) -> Option<String> {
    let key = key.to_ascii_lowercase();
    options.iter().find_map(|option| {
        let (option_key, value) = split_ssh_option(option)?;
        if option_key == key && !value.trim().is_empty() {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn ssh_option_key(option: &str) -> Option<String> {
    split_ssh_option(option).map(|(key, _)| key)
}

fn split_ssh_option(option: &str) -> Option<(String, String)> {
    let trimmed = option.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(index) = trimmed.find('=') {
        let key = trimmed[..index].trim();
        let value = trimmed[index + 1..].trim();
        if key.is_empty() {
            None
        } else {
            Some((key.to_ascii_lowercase(), value.to_string()))
        }
    } else {
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let key = parts.next()?.trim();
        let value = parts.next().unwrap_or("").trim();
        if key.is_empty() {
            None
        } else {
            Some((key.to_ascii_lowercase(), value.to_string()))
        }
    }
}

fn shell_join(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| shell_quote(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(token: &str) -> String {
    if !token.is_empty()
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_./:@%=+,[]".contains(ch))
    {
        token.to_string()
    } else {
        format!("'{}'", token.replace('\'', "'\"'\"'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn plan(items: &[&str]) -> SshCommandPlan {
        build_ssh_command_plan(
            &args(items),
            SshCommandBuildOptions {
                unique_id: "Test Workspace".to_string(),
                remote_relay_port: 64007,
                startup_script_path: "/tmp/cmux-ssh-startup-test.sh".to_string(),
                existing_ghostty_shell_features: None,
            },
        )
        .expect("ssh plan")
    }

    #[test]
    fn ssh_plan_adds_cmux_defaults_and_startup_metadata() {
        let plan = plan(&["dev.example.com", "--port", "2222", "--name", "dev"]);

        assert_eq!(plan.destination, "dev.example.com");
        assert_eq!(plan.title, "dev");
        assert_eq!(plan.port, Some(2222));
        assert!(plan.ssh_command.starts_with("ssh "));
        assert!(plan.ssh_command.contains("-p 2222"));
        assert!(plan
            .ssh_command
            .contains("-o StrictHostKeyChecking=accept-new"));
        assert!(plan.ssh_command.contains("-o ControlMaster=auto"));
        assert!(plan.ssh_command.contains("-o ControlPersist=600"));
        assert!(plan
            .ssh_command
            .contains("ControlPath=/tmp/cmux-ssh-test-workspace-%C"));
        assert!(plan.ssh_terminal_command.contains("cmux_tmp=$(mktemp "));
        assert!(plan.ssh_terminal_command.contains("cmux_startup_b64="));
        assert!(plan
            .ssh_terminal_command
            .contains("base64 -d > \"$cmux_tmp\""));
        assert!(plan.ssh_terminal_command.contains("/bin/sh \"$cmux_tmp\""));
        assert!(plan
            .ssh_terminal_command
            .contains("CMUX_SOCKET_PATH=127.0.0.1:64007"));
        assert!(plan
            .ssh_startup_command_text
            .contains("PATH=\"$HOME/.cmux/bin:$PATH\""));
        assert!(plan
            .ssh_startup_command_text
            .contains("CMUX_SOCKET_PATH=127.0.0.1:64007"));
        assert!(plan
            .ssh_startup_script
            .contains("cmux_bootstrap_path=\"$HOME/.cmux/relay/64007.bootstrap.sh\""));
        assert!(plan
            .remote_bootstrap_script
            .contains("export CMUX_WORKSPACE_ID='__CMUX_WORKSPACE_ID__'"));
        assert!(plan
            .ssh_env_overrides
            .get("GHOSTTY_SHELL_FEATURES")
            .is_some_and(|value| value == "ssh-env,ssh-terminfo"));
    }

    #[test]
    fn ssh_startup_script_embeds_canonical_remote_bootstrap_payload() {
        let plan = plan(&["dev.example.com"]);
        let encoded = plan
            .ssh_startup_script
            .lines()
            .find_map(|line| line.strip_prefix("cmux_remote_bootstrap_b64="))
            .expect("embedded bootstrap b64");
        let decoded = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .expect("valid bootstrap b64"),
        )
        .expect("utf8 bootstrap");

        assert!(plan
            .ssh_startup_script
            .contains("cat > \"$cmux_bootstrap_path\""));
        assert!(plan.ssh_startup_script.contains("/bin/sh -c "));
        assert!(!plan.ssh_startup_script.contains("/bin/sh -lc "));
        assert!(plan
            .ssh_startup_script
            .contains("/bin/sh \"$HOME/.cmux/relay/64007.bootstrap.sh\""));
        assert!(plan
            .ssh_startup_script
            .contains("export CMUX_BOOTSTRAP_TTY=\"$cmux_bootstrap_tty\""));
        assert!(decoded.contains("export PATH=\"$HOME/.cmux/bin:$PATH\""));
        assert!(decoded.contains("export CMUX_SOCKET_PATH=127.0.0.1:64007"));
        assert!(decoded.contains("export CMUX_WORKSPACE_ID='__CMUX_WORKSPACE_ID__'"));
        assert!(decoded.contains("export CMUX_TAB_ID='__CMUX_WORKSPACE_ID__'"));
        assert!(decoded.contains("export CMUX_SURFACE_ID='__CMUX_SURFACE_ID__'"));
        assert!(decoded.contains("export CMUX_PANEL_ID='__CMUX_SURFACE_ID__'"));
        assert!(decoded.contains("case \"${CMUX_LOGIN_SHELL##*/}\" in"));
        assert!(decoded.contains("cat > \"$cmux_shell_dir/.zshrc\""));
        assert!(decoded
            .contains("\"$cmux_relay_cli\" rpc surface.report_tty \"$cmux_relay_report_tty\""));
        assert!(decoded.contains("cmux_relay_tty=\"${CMUX_BOOTSTRAP_TTY:-}\""));
        assert!(decoded
            .contains("\"$cmux_relay_cli\" rpc surface.ports_kick \"$cmux_relay_ports_kick\""));
        assert!(
            decoded.contains("exec \"$CMUX_LOGIN_SHELL\" --rcfile \"$cmux_shell_dir/.bashrc\" -i")
        );
        assert!(decoded.contains("exec \"$CMUX_LOGIN_SHELL\" -i"));
    }

    #[test]
    fn ssh_plan_merges_existing_shell_features() {
        let plan = build_ssh_command_plan(
            &args(&["dev.example.com"]),
            SshCommandBuildOptions {
                unique_id: "Test Workspace".to_string(),
                remote_relay_port: 64007,
                startup_script_path: "/tmp/cmux-ssh-startup-test.sh".to_string(),
                existing_ghostty_shell_features: Some("cursor,title".to_string()),
            },
        )
        .expect("ssh plan");

        assert_eq!(
            plan.ssh_env_overrides.get("GHOSTTY_SHELL_FEATURES"),
            Some(&"cursor,title,ssh-env,ssh-terminfo".to_string())
        );
    }

    #[test]
    fn ssh_plan_preserves_user_option_overrides_case_insensitively() {
        let plan = plan(&[
            "dev.example.com",
            "--ssh-option",
            "stricthostkeychecking=no",
            "--ssh-option",
            "controlmaster=no",
            "--ssh-option",
            "controlpersist=0",
            "--ssh-option",
            "controlpath=/tmp/custom-%C",
        ]);
        let command = plan.ssh_command.to_ascii_lowercase();

        assert!(command.contains("-o stricthostkeychecking=no"));
        assert!(!command.contains("stricthostkeychecking=accept-new"));
        assert!(command.contains("-o controlmaster=no"));
        assert!(!command.contains("controlmaster=auto"));
        assert!(command.contains("-o controlpersist=0"));
        assert!(!command.contains("controlpersist=600"));
        assert_eq!(command.matches("controlpath=").count(), 1);
        assert!(command.contains("controlpath=/tmp/custom-%c"));
    }

    #[test]
    fn ssh_plan_rejects_missing_destination() {
        let error = build_ssh_command_plan(
            &[],
            SshCommandBuildOptions {
                unique_id: "x".to_string(),
                remote_relay_port: 1,
                startup_script_path: "/tmp/x".to_string(),
                existing_ghostty_shell_features: None,
            },
        )
        .unwrap_err();
        assert!(error.message.contains("requires a destination"));
    }
}
