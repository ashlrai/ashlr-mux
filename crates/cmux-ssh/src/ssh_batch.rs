//! Batch (non-interactive) SSH argv composition + SSH-option normalization —
//! pure transforms of a remote configuration.
//!
//! Port of the canonical macOS Swift sources
//! `Packages/macOS/CmuxCore/Sources/CmuxCore/Remote/`
//! `WorkspaceRemoteConfiguration+SSHBatchCommands.swift` (daemon transport /
//! socket-forward / reverse-relay ControlMaster argv, plus the private
//! `batchSSHArguments` / `backgroundSSHOptions` / `firstSSHOptionValue`
//! helpers and the `shellSingleQuoted` receiver) and
//! `WorkspaceRemoteConfiguration+SSHOptionNormalization.swift` (the pure
//! `durableSSHOptions` / `forkedWorkspaceSSHOptions` / `trimmedSSHOptions` /
//! `normalizedOptionalValue` vocabulary).
//!
//! The Swift originals are instance/static members of
//! `WorkspaceRemoteConfiguration`; the argv-composing instance methods read
//! only `destination` / `port` / `identityFile` / `sshOptions` /
//! `persistentDaemonSlot`, so this port takes those five fields as an input
//! value slice ([`SshBatchConfiguration`]). The argument text is wire/process
//! behavior and is preserved byte-for-byte; do not alter it.
//!
//! Left out of this lane (live-I/O / FileManager / agent-socket surface that
//! stays in the host layer): `normalizedPersistentDaemonSlot`,
//! `normalizedIdentityPath`, `normalizedAgentSocketPath`,
//! `existingAgentSocketPath`, `hasSSHOptionKey`, `sshAgentSocketPath` (the
//! last reuses the already-ported [`crate::option_key`] via the resolver).

// The Swift `SSHAgentSocketResolver().optionKey(_:)` used throughout these
// helpers is already ported as the crate-private `option_key`; the batch
// helpers reuse it (and `has_ssh_option_key`) from the crate root.

// ---------------------------------------------------------------------------
// SSH-option normalization vocabulary
// (Swift: WorkspaceRemoteConfiguration+SSHOptionNormalization.swift)
// ---------------------------------------------------------------------------

// Swift: `transientControlSocketKeys` (SSHOptionNormalization.swift:12-16).
const TRANSIENT_CONTROL_SOCKET_KEYS: &[&str] = &["controlmaster", "controlpath", "controlpersist"];

// Swift: `batchSSHControlOptionKeys` (SSHBatchCommands.swift:10-13).
const BATCH_SSH_CONTROL_OPTION_KEYS: &[&str] = &["controlmaster", "controlpersist"];

/// Options that survive snapshot/restore: trimmed, with transient
/// control-socket options (`ControlMaster`/`ControlPath`/`ControlPersist`)
/// dropped.
///
/// Swift: `WorkspaceRemoteConfiguration.durableSSHOptions(_:)`
/// (SSHOptionNormalization.swift:20-22).
pub fn durable_ssh_options(options: &[String]) -> Vec<String> {
    filtered_ssh_options(options, TRANSIENT_CONTROL_SOCKET_KEYS)
}

/// Options propagated to a forked workspace (same as the durable subset).
///
/// Swift: `WorkspaceRemoteConfiguration.forkedWorkspaceSSHOptions(_:)`
/// (SSHOptionNormalization.swift:25-27).
pub fn forked_workspace_ssh_options(options: &[String]) -> Vec<String> {
    durable_ssh_options(options)
}

/// Options trimmed of whitespace and empties, with nothing dropped.
///
/// Swift: `WorkspaceRemoteConfiguration.trimmedSSHOptions(_:)`
/// (SSHOptionNormalization.swift:30-32).
pub fn trimmed_ssh_options(options: &[String]) -> Vec<String> {
    filtered_ssh_options(options, &[])
}

/// Swift: `WorkspaceRemoteConfiguration.filteredSSHOptions(_:droppingKeys:)`
/// (SSHOptionNormalization.swift:34-42). Trims each option, drops empties,
/// then drops any whose lowercased option key is in `dropping_keys`.
fn filtered_ssh_options(options: &[String], dropping_keys: &[&str]) -> Vec<String> {
    options
        .iter()
        .filter_map(|option| {
            let trimmed = option.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .filter(|option| match crate::option_key(option) {
            // Swift: `guard let key … else { return true }` — un-keyable
            // options are kept.
            Some(key) => !dropping_keys.contains(&key.as_str()),
            None => true,
        })
        .collect()
}

/// Trims `value`; returns `None` for `None` or whitespace-only input.
///
/// Swift: `WorkspaceRemoteConfiguration.normalizedOptionalValue(_:)`
/// (SSHOptionNormalization.swift:45-49).
pub fn normalized_optional_value(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// ---------------------------------------------------------------------------
// Batch SSH argv composition
// (Swift: WorkspaceRemoteConfiguration+SSHBatchCommands.swift)
// ---------------------------------------------------------------------------

/// The slice of `WorkspaceRemoteConfiguration` fields the batch-command argv
/// composers read (Swift instance members on `WorkspaceRemoteConfiguration`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SshBatchConfiguration {
    /// `[user@]host` destination.
    pub destination: String,
    /// `-p` port (`Int?`).
    pub port: Option<i64>,
    /// `-i` identity file.
    pub identity_file: Option<String>,
    /// Configured `-o` options (raw, unfiltered).
    pub ssh_options: Vec<String>,
    /// Persistent daemon slot name, when configured.
    pub persistent_daemon_slot: Option<String>,
}

impl SshBatchConfiguration {
    /// `ssh` argv that execs `<remote_path> serve --stdio` (plus
    /// `--persistent --slot <slot>` when a persistent daemon slot is
    /// configured) on the destination for the stdio daemon transport.
    ///
    /// Swift: `daemonTransportArguments(remotePath:)`
    /// (SSHBatchCommands.swift:19-33). Argument text is wire/process
    /// behavior; do not alter.
    pub fn daemon_transport_arguments(&self, remote_path: &str) -> Vec<String> {
        let mut serve_arguments = vec!["serve".to_string(), "--stdio".to_string()];
        if let Some(slot) = &self.persistent_daemon_slot {
            let slot = slot.trim();
            if !slot.is_empty() {
                serve_arguments.push("--persistent".to_string());
                serve_arguments.push("--slot".to_string());
                serve_arguments.push(slot.to_string());
            }
        }
        let daemon_command = std::iter::once(remote_path.to_string())
            .chain(serve_arguments)
            .map(|token| shell_single_quoted(&token))
            .collect::<Vec<_>>()
            .join(" ");
        let script = format!("exec {daemon_command}");
        let command = format!("sh -c {}", shell_single_quoted(&script));

        let mut args = vec!["-T".to_string()];
        args.extend(self.batch_ssh_arguments());
        args.push("-o".to_string());
        args.push("RequestTTY=no".to_string());
        args.push(self.destination.clone());
        args.push(command);
        args
    }

    /// `ssh` argv that forwards `127.0.0.1:<local_port>` to the baked VM
    /// daemon's Unix socket (`-N`, no remote command).
    ///
    /// Swift: `daemonSocketForwardArguments(localPort:remoteSocketPath:)`
    /// (SSHBatchCommands.swift:38-47). Argument text is wire/process
    /// behavior; do not alter.
    pub fn daemon_socket_forward_arguments(
        &self,
        local_port: i64,
        remote_socket_path: &str,
    ) -> Vec<String> {
        let mut args = vec![
            "-N".to_string(),
            "-T".to_string(),
            "-S".to_string(),
            "none".to_string(),
        ];
        args.extend(self.batch_ssh_arguments());
        args.push("-o".to_string());
        args.push("ExitOnForwardFailure=yes".to_string());
        args.push("-o".to_string());
        args.push("RequestTTY=no".to_string());
        args.push("-L".to_string());
        args.push(format!("127.0.0.1:{local_port}:{remote_socket_path}"));
        args.push(self.destination.clone());
        args
    }

    /// `ssh -O <control_command>` argv that drives a reverse forward on the
    /// configured ControlMaster socket, or `None` when no usable `ControlPath`
    /// option is configured.
    ///
    /// Swift: `reverseRelayControlMasterArguments(controlCommand:forwardSpec:)`
    /// (SSHBatchCommands.swift:53-67). Argument text is wire/process
    /// behavior; do not alter.
    pub fn reverse_relay_control_master_arguments(
        &self,
        control_command: &str,
        forward_spec: &str,
    ) -> Option<Vec<String>> {
        let control_path = self.first_ssh_option_value("ControlPath")?;
        let control_path = control_path.trim();
        if control_path.is_empty() || control_path.to_lowercase() == "none" {
            return None;
        }

        let mut args = self.batch_ssh_arguments();
        args.push("-O".to_string());
        args.push(control_command.to_string());
        args.push("-R".to_string());
        args.push(forward_spec.to_string());
        args.push(self.destination.clone());
        Some(args)
    }

    /// [`Self::reverse_relay_control_master_arguments`] specialized to
    /// `-O cancel` for the relay's remote listen port, or `None` for a
    /// non-positive port.
    ///
    /// Swift: `reverseRelayControlMasterCancelArguments(relayPort:)`
    /// (SSHBatchCommands.swift:73-79). Argument text is wire/process
    /// behavior; do not alter.
    pub fn reverse_relay_control_master_cancel_arguments(
        &self,
        relay_port: i64,
    ) -> Option<Vec<String>> {
        if relay_port <= 0 {
            return None;
        }
        self.reverse_relay_control_master_arguments("cancel", &format!("127.0.0.1:{relay_port}"))
    }

    /// Shared batch-mode `ssh` options: keepalives, BatchMode, no new
    /// ControlMaster (existing ControlPath sockets may be reused), port,
    /// identity, then the configuration's options minus
    /// ControlMaster/ControlPersist.
    ///
    /// Swift: `batchSSHArguments()` (SSHBatchCommands.swift:85-109).
    fn batch_ssh_arguments(&self) -> Vec<String> {
        let effective_ssh_options = self.background_ssh_options();
        let mut args = vec![
            "-o".to_string(),
            "ConnectTimeout=6".to_string(),
            "-o".to_string(),
            "ServerAliveInterval=20".to_string(),
            "-o".to_string(),
            "ServerAliveCountMax=2".to_string(),
        ];
        if !crate::has_ssh_option_key(&effective_ssh_options, "StrictHostKeyChecking") {
            args.push("-o".to_string());
            args.push("StrictHostKeyChecking=accept-new".to_string());
        }
        args.push("-o".to_string());
        args.push("BatchMode=yes".to_string());
        // Batch helpers may reuse an existing ControlPath, but must not
        // negotiate a new master.
        args.push("-o".to_string());
        args.push("ControlMaster=no".to_string());
        if let Some(port) = self.port {
            args.push("-p".to_string());
            args.push(port.to_string());
        }
        if let Some(identity_file) = &self.identity_file {
            if !identity_file.trim().is_empty() {
                args.push("-i".to_string());
                args.push(identity_file.clone());
            }
        }
        for option in &effective_ssh_options {
            args.push("-o".to_string());
            args.push(option.clone());
        }
        args
    }

    /// Trimmed options minus ControlMaster/ControlPersist (ControlPath is
    /// kept so batch helpers can reuse an existing master's socket).
    ///
    /// Swift: `backgroundSSHOptions()` (SSHBatchCommands.swift:113-119).
    fn background_ssh_options(&self) -> Vec<String> {
        trimmed_ssh_options(&self.ssh_options)
            .into_iter()
            .filter(|option| match crate::option_key(option) {
                // Swift: `guard let key … else { return false }` — un-keyable
                // options are dropped here (differs from `filteredSSHOptions`,
                // though `trimmedSSHOptions` already removed empties so every
                // survivor is keyable).
                Some(key) => !BATCH_SSH_CONTROL_OPTION_KEYS.contains(&key.as_str()),
                None => false,
            })
            .collect()
    }

    /// First non-empty value for an option key, scanning forward. This
    /// deliberately differs from the resolver's reverse (last-wins) scan: the
    /// legacy batch builder used first-match and the reverse-relay behavior is
    /// pinned to it.
    ///
    /// Swift: `firstSSHOptionValue(named:)` (SSHBatchCommands.swift:126-143).
    fn first_ssh_option_value(&self, key: &str) -> Option<String> {
        let lowered_key = key.to_lowercase();
        for option in trimmed_ssh_options(&self.ssh_options) {
            let Some((token, remainder)) = split_first_option_token(&option) else {
                continue;
            };
            if token.to_lowercase() != lowered_key {
                continue;
            }
            let value = remainder.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
        None
    }
}

/// POSIX single-quoting for embedding a value in an `sh -c` script (`'`
/// becomes `'"'"'`).
///
/// Swift: `String.shellSingleQuoted` (SSHBatchCommands.swift:150-152).
/// Quoting output is wire/process behavior; do not alter.
fn shell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Swift `option.split(maxSplits: 1, omittingEmptySubsequences: true,
/// whereSeparator: { $0 == "=" || $0.isWhitespace })` restricted to the two
/// cases `firstSSHOptionValue` inspects: returns `Some((parts[0], parts[1]))`
/// exactly when the split yields two subsequences, else `None`.
///
/// Leading separators are omitted (they do not consume the single split
/// budget); the first separator after the leading token consumes the split,
/// and everything after that one separator char is the raw remainder (which is
/// trimmed by the caller). A remainder that is empty yields a one-element
/// split → `None`.
fn split_first_option_token(option: &str) -> Option<(&str, &str)> {
    let is_separator = |c: char| c == '=' || c.is_whitespace();
    // Skip leading separators (omittingEmptySubsequences); if the whole string
    // is separators there is no non-empty subsequence.
    let start = option.find(|c: char| !is_separator(c))?;
    let rest = &option[start..];
    // First separator after the leading token consumes the single split.
    let separator_index = rest.find(is_separator)?;
    let token = &rest[..separator_index];
    let after = &rest[separator_index..];
    // Drop exactly one separator character; the raw remainder is parts[1].
    let separator_char = after.chars().next()?;
    let remainder = &after[separator_char.len_utf8()..];
    if remainder.is_empty() {
        None
    } else {
        Some((token, remainder))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // === Option normalization ============================================
    // Ported from WorkspaceRemoteConfigurationNormalizationTests.swift.

    /// Swift: `durableOptionsDropControlSocketKeys`
    /// (WorkspaceRemoteConfigurationTests.swift:7-22).
    #[test]
    fn durable_options_drop_control_socket_keys() {
        let options = strings(&[
            "ControlMaster=auto",
            "controlpath /tmp/sock-%C",
            "ControlPersist=600",
            "ServerAliveInterval=20",
            "  ForwardAgent yes  ",
            "",
            "   ",
        ]);
        assert_eq!(
            durable_ssh_options(&options),
            strings(&["ServerAliveInterval=20", "ForwardAgent yes"])
        );
    }

    /// Swift: `trimmedOptionsKeepControlKeys`
    /// (WorkspaceRemoteConfigurationTests.swift:24-31).
    #[test]
    fn trimmed_options_keep_control_keys() {
        let options = strings(&[" ControlMaster=auto ", "", "ServerAliveInterval=20"]);
        assert_eq!(
            trimmed_ssh_options(&options),
            strings(&["ControlMaster=auto", "ServerAliveInterval=20"])
        );
    }

    /// Swift: `forkedOptionsMatchDurable`
    /// (WorkspaceRemoteConfigurationTests.swift:33-44). Only the
    /// `forkedWorkspaceSSHOptions == durableSSHOptions` half is in this lane
    /// (`forkedAgentSSHOptions` lives in a different extension).
    #[test]
    fn forked_options_match_durable() {
        let options = strings(&["ControlPath=/tmp/x", "ForwardAgent=yes"]);
        assert_eq!(
            forked_workspace_ssh_options(&options),
            durable_ssh_options(&options)
        );
    }

    /// Swift: `normalizedOptionalValueBehavior`
    /// (WorkspaceRemoteConfigurationTests.swift:46-51).
    #[test]
    fn normalized_optional_value_behavior() {
        assert_eq!(normalized_optional_value(Some("  x  ")).as_deref(), Some("x"));
        assert_eq!(normalized_optional_value(Some("   ")), None);
        assert_eq!(normalized_optional_value(None), None);
    }

    // === Batch command composition =======================================
    // Ported from WorkspaceRemoteConfigurationSSHBatchCommandsTests.swift.

    /// Swift test fixture `configuration(...)`
    /// (SSHBatchCommandsTests.swift:6-30).
    fn configuration(ssh_options: &[&str], persistent_daemon_slot: Option<&str>) -> SshBatchConfiguration {
        SshBatchConfiguration {
            destination: "cmux-macmini".to_string(),
            port: Some(2222),
            identity_file: Some("/Users/test/.ssh/id_ed25519".to_string()),
            ssh_options: strings(ssh_options),
            persistent_daemon_slot: persistent_daemon_slot.map(str::to_string),
        }
    }

    /// The default `configuration()` sshOptions (SSHBatchCommandsTests.swift:7-12).
    fn default_config() -> SshBatchConfiguration {
        configuration(
            &[
                "ControlMaster=auto",
                "ControlPersist=600",
                "ControlPath=/tmp/cmux-ssh-%C",
                "StrictHostKeyChecking=accept-new",
            ],
            None,
        )
    }

    /// Swift: `expectedBatchArguments` (SSHBatchCommandsTests.swift:36-46).
    fn expected_batch_arguments() -> Vec<String> {
        strings(&[
            "-o", "ConnectTimeout=6",
            "-o", "ServerAliveInterval=20",
            "-o", "ServerAliveCountMax=2",
            "-o", "BatchMode=yes",
            "-o", "ControlMaster=no",
            "-p", "2222",
            "-i", "/Users/test/.ssh/id_ed25519",
            "-o", "ControlPath=/tmp/cmux-ssh-%C",
            "-o", "StrictHostKeyChecking=accept-new",
        ])
    }

    /// Swift: `daemonTransportArgumentsWithoutSlot`
    /// (SSHBatchCommandsTests.swift:48-57).
    #[test]
    fn daemon_transport_arguments_without_slot() {
        let arguments = default_config().daemon_transport_arguments("/remote/cmuxd-remote");
        let expected_command =
            r#"sh -c 'exec '"'"'/remote/cmuxd-remote'"'"' '"'"'serve'"'"' '"'"'--stdio'"'"''"#;
        let mut expected = strings(&["-T"]);
        expected.extend(expected_batch_arguments());
        expected.extend(strings(&["-o", "RequestTTY=no", "cmux-macmini"]));
        expected.push(expected_command.to_string());
        assert_eq!(arguments, expected);
    }

    /// Swift: `daemonTransportArgumentsWithSlot`
    /// (SSHBatchCommandsTests.swift:59-71).
    #[test]
    fn daemon_transport_arguments_with_slot() {
        let arguments = configuration(
            &[
                "ControlMaster=auto",
                "ControlPersist=600",
                "ControlPath=/tmp/cmux-ssh-%C",
                "StrictHostKeyChecking=accept-new",
            ],
            Some("ws-1"),
        )
        .daemon_transport_arguments("/remote/cmuxd-remote");
        let expected_command = r#"sh -c 'exec '"'"'/remote/cmuxd-remote'"'"' '"'"'serve'"'"' '"'"'--stdio'"'"' '"'"'--persistent'"'"' '"'"'--slot'"'"' '"'"'ws-1'"'"''"#;
        let mut expected = strings(&["-T"]);
        expected.extend(expected_batch_arguments());
        expected.extend(strings(&["-o", "RequestTTY=no", "cmux-macmini"]));
        expected.push(expected_command.to_string());
        assert_eq!(arguments, expected);
    }

    /// Swift: `daemonTransportArgumentsInjectsStrictHostKeyChecking`
    /// (SSHBatchCommandsTests.swift:73-100). Space-separated control options,
    /// no StrictHostKeyChecking configured → `accept-new` injected, and
    /// ControlMaster/ControlPersist dropped while the space-form ControlPath
    /// survives verbatim.
    #[test]
    fn daemon_transport_arguments_injects_strict_host_key_checking() {
        let arguments = configuration(
            &[
                "ControlMaster auto",
                "ControlPersist 600",
                "ControlPath /tmp/cmux-ssh-%C",
            ],
            None,
        )
        .daemon_transport_arguments("/remote/cmuxd-remote");
        let expected_command =
            r#"sh -c 'exec '"'"'/remote/cmuxd-remote'"'"' '"'"'serve'"'"' '"'"'--stdio'"'"''"#;
        assert_eq!(
            arguments,
            strings(&[
                "-T",
                "-o", "ConnectTimeout=6",
                "-o", "ServerAliveInterval=20",
                "-o", "ServerAliveCountMax=2",
                "-o", "StrictHostKeyChecking=accept-new",
                "-o", "BatchMode=yes",
                "-o", "ControlMaster=no",
                "-p", "2222",
                "-i", "/Users/test/.ssh/id_ed25519",
                "-o", "ControlPath /tmp/cmux-ssh-%C",
                "-o", "RequestTTY=no",
                "cmux-macmini",
                expected_command,
            ])
        );
    }

    /// Swift: `daemonSocketForwardArguments`
    /// (SSHBatchCommandsTests.swift:102-118).
    #[test]
    fn daemon_socket_forward_arguments() {
        let arguments =
            default_config().daemon_socket_forward_arguments(64123, "/run/cmuxd-remote.sock");
        let mut expected = strings(&["-N", "-T", "-S", "none"]);
        expected.extend(expected_batch_arguments());
        expected.extend(strings(&[
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "RequestTTY=no",
            "-L",
            "127.0.0.1:64123:/run/cmuxd-remote.sock",
            "cmux-macmini",
        ]));
        assert_eq!(arguments, expected);
    }

    /// Swift: `reverseRelayControlMasterArguments`
    /// (SSHBatchCommandsTests.swift:120-132).
    #[test]
    fn reverse_relay_control_master_arguments() {
        let arguments = default_config()
            .reverse_relay_control_master_arguments("forward", "127.0.0.1:64007:127.0.0.1:54321")
            .expect("arguments");
        let mut expected = expected_batch_arguments();
        expected.extend(strings(&[
            "-O",
            "forward",
            "-R",
            "127.0.0.1:64007:127.0.0.1:54321",
            "cmux-macmini",
        ]));
        assert_eq!(arguments, expected);
    }

    /// Swift: `reverseRelayControlMasterCancelArguments`
    /// (SSHBatchCommandsTests.swift:134-143).
    #[test]
    fn reverse_relay_control_master_cancel_arguments() {
        let arguments = default_config()
            .reverse_relay_control_master_cancel_arguments(64007)
            .expect("arguments");
        let mut expected = expected_batch_arguments();
        expected.extend(strings(&["-O", "cancel", "-R", "127.0.0.1:64007", "cmux-macmini"]));
        assert_eq!(arguments, expected);
    }

    /// Swift: `reverseRelayRequiresControlPath`
    /// (SSHBatchCommandsTests.swift:145-162).
    #[test]
    fn reverse_relay_requires_control_path() {
        // No ControlPath configured at all → nil.
        assert_eq!(
            configuration(&["StrictHostKeyChecking=accept-new"], None)
                .reverse_relay_control_master_arguments(
                    "forward",
                    "127.0.0.1:64007:127.0.0.1:54321"
                ),
            None
        );
        // ControlPath=None (case-insensitive) → nil.
        assert_eq!(
            configuration(&["ControlPath=None"], None).reverse_relay_control_master_arguments(
                "forward",
                "127.0.0.1:64007:127.0.0.1:54321"
            ),
            None
        );
        // Non-positive relay port → nil.
        assert_eq!(
            default_config().reverse_relay_control_master_cancel_arguments(0),
            None
        );
    }

    // === Parity-risk edge cases (hand-computed from the Swift formula) ====

    /// A space-form `ControlPath foo` still supplies a usable ControlPath for
    /// the reverse-relay guard (Swift `firstSSHOptionValue` splits on `=` OR
    /// whitespace).
    #[test]
    fn reverse_relay_accepts_space_form_control_path() {
        let arguments = configuration(&["ControlPath /tmp/sock"], None)
            .reverse_relay_control_master_arguments("forward", "spec")
            .expect("arguments");
        assert_eq!(arguments.last().unwrap(), "cmux-macmini");
        assert!(arguments.contains(&"-O".to_string()));
    }

    /// `firstSSHOptionValue` scans forward (first match wins), unlike the
    /// resolver's reverse last-wins scan. An empty first value is skipped and
    /// the next matching option's value is taken.
    #[test]
    fn first_ssh_option_value_scans_forward_skipping_empties() {
        // First ControlPath has an empty value ("ControlPath=") → skipped;
        // the second is used.
        let config = SshBatchConfiguration {
            ssh_options: strings(&["ControlPath=", "ControlPath=/tmp/second"]),
            ..default_config()
        };
        assert_eq!(config.first_ssh_option_value("ControlPath").as_deref(), Some("/tmp/second"));
        // Forward order: the FIRST non-empty wins.
        let config = SshBatchConfiguration {
            ssh_options: strings(&["ControlPath=/tmp/first", "ControlPath=/tmp/second"]),
            ..default_config()
        };
        assert_eq!(config.first_ssh_option_value("ControlPath").as_deref(), Some("/tmp/first"));
    }

    /// `shellSingleQuoted` wraps in single quotes and escapes embedded quotes
    /// as `'"'"'` (Swift SSHBatchCommands.swift:150-152).
    #[test]
    fn shell_single_quoted_escapes_embedded_quotes() {
        assert_eq!(shell_single_quoted("plain"), "'plain'");
        assert_eq!(shell_single_quoted("a'b"), r#"'a'"'"'b'"#);
        assert_eq!(shell_single_quoted(""), "''");
    }

    /// `split_first_option_token` mirrors Swift `split(maxSplits: 1,
    /// omittingEmptySubsequences: true)` on the `=`/whitespace separator set.
    #[test]
    fn split_first_option_token_cases() {
        assert_eq!(split_first_option_token("Key=value"), Some(("Key", "value")));
        assert_eq!(split_first_option_token("Key value"), Some(("Key", "value")));
        // maxSplits: 1 → only the first separator splits; the rest is raw.
        assert_eq!(split_first_option_token("Key=a=b"), Some(("Key", "a=b")));
        assert_eq!(split_first_option_token("Key  value"), Some(("Key", " value")));
        // Empty remainder → one subsequence → None.
        assert_eq!(split_first_option_token("Key="), None);
        assert_eq!(split_first_option_token("Key"), None);
        // Leading separators are omitted and do not consume the split budget.
        assert_eq!(split_first_option_token("=Key=value"), Some(("Key", "value")));
        assert_eq!(split_first_option_token("==="), None);
    }

    /// Empty/whitespace-only identity file is not emitted as `-i`
    /// (Swift batchSSHArguments identity guard).
    #[test]
    fn batch_arguments_skip_blank_identity_file() {
        let config = SshBatchConfiguration {
            identity_file: Some("   ".to_string()),
            ssh_options: strings(&["StrictHostKeyChecking=accept-new"]),
            ..default_config()
        };
        let args = config.batch_ssh_arguments();
        assert!(!args.contains(&"-i".to_string()));
    }

    /// A `None` port omits the `-p` pair (Swift `if let port`).
    #[test]
    fn batch_arguments_omit_absent_port() {
        let config = SshBatchConfiguration {
            port: None,
            ssh_options: strings(&["StrictHostKeyChecking=accept-new"]),
            ..default_config()
        };
        let args = config.batch_ssh_arguments();
        assert!(!args.contains(&"-p".to_string()));
    }
}
