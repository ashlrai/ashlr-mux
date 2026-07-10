//! Oracle tests: table-driven per-kind fixtures plus one assertion per policy
//! branch (dropped/rejected/variadic/optional-value/prompt-boundary/hook-settings
//! /codex-fork). Expected outputs were derived by hand-tracing the Swift
//! `preserveOptions` scanner against each policy table.

use super::*;

/// Build a `Vec<String>` from string literals.
fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// `Some(v(items))`.
fn some(items: &[&str]) -> Option<Vec<String>> {
    Some(v(items))
}

// ---------------------------------------------------------------------------
// claude — value/variadic/dropped/reject + injected hook `--settings` stripping.
// ---------------------------------------------------------------------------

#[test]
fn claude_preserves_value_option_pairs() {
    assert_eq!(
        preserved_arguments("claude", &v(&["--model", "sonnet"])),
        some(&["--model", "sonnet"])
    );
}

#[test]
fn claude_drops_resume_span_and_worktree() {
    // --resume <id> and --worktree <dir> are dropped with their values.
    assert_eq!(
        preserved_arguments(
            "claude",
            &v(&["--resume", "abc", "--worktree", "/wt", "--model", "sonnet"])
        ),
        some(&["--model", "sonnet"])
    );
}

#[test]
fn claude_drops_equals_form_via_prefix() {
    assert_eq!(
        preserved_arguments("claude", &v(&["--resume=abc", "--model", "sonnet"])),
        some(&["--model", "sonnet"])
    );
}

#[test]
fn claude_variadic_add_dir_consumes_until_next_option() {
    assert_eq!(
        preserved_arguments("claude", &v(&["--add-dir", "a", "b", "c", "--model", "x"])),
        some(&["--add-dir", "a", "b", "c", "--model", "x"])
    );
}

#[test]
fn claude_rejects_print_mode() {
    assert_eq!(preserved_arguments("claude", &v(&["--print"])), None);
    assert_eq!(preserved_arguments("claude", &v(&["-p"])), None);
    assert_eq!(
        preserved_arguments("claude", &v(&["--no-session-persistence"])),
        None
    );
}

#[test]
fn claude_non_restorable_subcommand_returns_none() {
    assert_eq!(preserved_arguments("claude", &v(&["mcp", "list"])), None);
    assert_eq!(preserved_arguments("claude", &v(&["config"])), None);
}

#[test]
fn claude_runtime_only_option_is_dropped() {
    assert_eq!(
        preserved_arguments("claude", &v(&["--use-system-ca", "--model", "x"])),
        some(&["--model", "x"])
    );
}

#[test]
fn claude_injected_hook_settings_object_is_dropped_entirely() {
    // The cmux-injected hooks object leaves nothing user-authored behind.
    assert_eq!(
        preserved_arguments(
            "claude",
            &v(&[
                "--model",
                "sonnet",
                "--settings",
                r#"{"hooks":"claude-hook"}"#
            ])
        ),
        some(&["--model", "sonnet"])
    );
}

#[test]
fn claude_injected_hook_settings_equals_form_is_dropped() {
    assert_eq!(
        preserved_arguments(
            "claude",
            &v(&["--settings={\"hooks\":\"claude-hook\"}", "--model", "x"])
        ),
        some(&["--model", "x"])
    );
}

#[test]
fn claude_disabled_notif_channel_only_object_is_dropped() {
    assert_eq!(
        preserved_arguments(
            "claude",
            &v(&[
                "--settings",
                r#"{"preferredNotifChannel":"notifications_disabled"}"#
            ])
        ),
        some(&[])
    );
}

#[test]
fn claude_hook_settings_keeps_user_keys_sorted() {
    // cmux keys are stripped; the surviving user keys are re-emitted with sorted
    // keys (model < zebra) and preserved value types (number stays a number).
    assert_eq!(
        preserved_arguments(
            "claude",
            &v(&[
                "--settings",
                r#"{"zebra":1,"hooks":"claude-hook","model":"opus"}"#
            ])
        ),
        some(&["--settings", r#"{"model":"opus","zebra":1}"#])
    );
}

#[test]
fn claude_legacy_string_settings_value_is_dropped() {
    // Not JSON, but the legacy marker string triggers a drop.
    assert_eq!(
        preserved_arguments("claude", &v(&["--settings", "claude-hook-path"])),
        some(&[])
    );
}

#[test]
fn claude_non_hook_settings_object_passes_through() {
    assert_eq!(
        preserved_arguments("claude", &v(&["--settings", r#"{"model":"opus"}"#])),
        some(&["--settings", r#"{"model":"opus"}"#])
    );
}

// ---------------------------------------------------------------------------
// prompt-boundary termination in the generic scanner.
// ---------------------------------------------------------------------------

#[test]
fn double_dash_terminates_option_scanning() {
    assert_eq!(
        preserved_arguments(
            "claude",
            &v(&["--model", "opus", "--", "--allowedTools", "x"])
        ),
        some(&["--model", "opus"])
    );
}

#[test]
fn prompt_positional_terminates_option_scanning() {
    assert_eq!(
        preserved_arguments(
            "claude",
            &v(&["--model", "opus", "hello", "--allowedTools", "x"])
        ),
        some(&["--model", "opus"])
    );
}

// ---------------------------------------------------------------------------
// codex — resume subcommand, dropped image, and fork positional handling.
// ---------------------------------------------------------------------------

#[test]
fn codex_strips_resume_subcommand_and_its_positional() {
    assert_eq!(
        preserved_arguments("codex", &v(&["resume", "sess-id", "--model", "gpt"])),
        some(&["--model", "gpt"])
    );
}

#[test]
fn codex_drops_image_variadic_option() {
    assert_eq!(
        preserved_arguments(
            "codex",
            &v(&["--image", "a.png", "b.png", "--model", "gpt"])
        ),
        some(&["--model", "gpt"])
    );
}

#[test]
fn codex_fork_is_non_restorable_via_plain_preserved_arguments() {
    // `fork` is a non-restorable codex subcommand for the plain entrypoint.
    assert_eq!(
        preserved_arguments(
            "codex",
            &v(&["fork", "0199abcd-1234-5678-9abc-def012345678"])
        ),
        None
    );
}

#[test]
fn codex_fork_arguments_drop_fork_command_and_session() {
    assert_eq!(
        preserved_codex_fork_arguments(&v(&[
            "fork",
            "0199abcd-1234-5678-9abc-def012345678",
            "--model",
            "gpt"
        ])),
        some(&["--model", "gpt"])
    );
}

#[test]
fn codex_fork_without_session_identifier_is_not_a_fork() {
    // No session-id after `fork` ⇒ not treated as a fork command; falls back to
    // the plain codex policy, where `fork` is non-restorable ⇒ None.
    assert_eq!(
        preserved_codex_fork_arguments(&v(&["fork", "short", "--model", "gpt"])),
        None
    );
}

// ---------------------------------------------------------------------------
// claudeTeams — with/without a known option + the `--tmux` prompt boundary.
// ---------------------------------------------------------------------------

#[test]
fn claude_teams_has_option_after_prompt_positional() {
    // Scanning does NOT stop at the first positional prompt.
    assert!(claude_teams_launch_has_option(
        "--dangerously-skip-permissions",
        &v(&["do x", "--dangerously-skip-permissions"])
    ));
}

#[test]
fn claude_teams_has_option_false_when_absent() {
    assert!(!claude_teams_launch_has_option(
        "--dangerously-skip-permissions",
        &v(&["--model", "opus"])
    ));
}

#[test]
fn claude_teams_has_option_skips_classic_launch_mode_and_continues() {
    assert!(claude_teams_launch_has_option(
        "--model",
        &v(&["--tmux", "classic", "--model", "opus"])
    ));
}

#[test]
fn claude_teams_has_option_ignores_flag_inside_tmux_prompt_payload() {
    // A flag-shaped token buried in the `--tmux <prompt>` payload is never an option.
    assert!(!claude_teams_launch_has_option(
        "--dangerously-skip-permissions",
        &v(&["--tmux", "please run --dangerously-skip-permissions now"])
    ));
}

#[test]
fn claude_teams_has_option_false_on_double_dash() {
    assert!(!claude_teams_launch_has_option(
        "--dangerously-skip-permissions",
        &v(&["--", "--dangerously-skip-permissions"])
    ));
}

#[test]
fn claude_teams_has_option_not_reported_after_real_prompt() {
    // preserveOptions *recovers* --model after a real prompt, but the trust-boundary
    // opt-in check discards recovered options and reports absence.
    assert!(!claude_teams_launch_has_option(
        "--model",
        &v(&["--tmux", "a real prompt", "--model", "opus"])
    ));
}

#[test]
fn claude_teams_preserve_skips_classic_launch_mode() {
    assert_eq!(
        preserved_claude_teams_launch_arguments(&v(&["--tmux", "classic", "--model", "opus"])),
        some(&["--model", "opus"])
    );
}

#[test]
fn claude_teams_preserve_recovers_safe_options_after_prompt() {
    // After a real `--tmux <prompt>`, only the safe allow-list is recovered:
    // --model and a `--permission-mode auto`, stopping at `--permission-mode plan`.
    assert_eq!(
        preserved_claude_teams_launch_arguments(&v(&[
            "--tmux",
            "run this please",
            "--model",
            "opus",
            "--permission-mode",
            "auto",
            "--permission-mode",
            "plan"
        ])),
        some(&["--model", "opus", "--permission-mode", "auto"])
    );
}

#[test]
fn claude_teams_preserves_worktree_as_greedy_optional_value() {
    // Unlike base claude (which drops --worktree), Teams keeps it as a greedy
    // optional-value option.
    assert_eq!(
        preserved_claude_teams_launch_arguments(&v(&[
            "--worktree",
            "/some/path",
            "--model",
            "opus"
        ])),
        some(&["--worktree", "/some/path", "--model", "opus"])
    );
}

#[test]
fn claude_teams_prompt_suggestions_uses_choice_set() {
    assert_eq!(
        preserved_claude_teams_launch_arguments(&v(&[
            "--prompt-suggestions",
            "true",
            "--model",
            "opus"
        ])),
        some(&["--prompt-suggestions", "true", "--model", "opus"])
    );
    // A non-choice value ⇒ boolean flag (width 1); the stray positional terminates.
    assert_eq!(
        preserved_claude_teams_launch_arguments(&v(&["--prompt-suggestions", "maybe", "extra"])),
        some(&["--prompt-suggestions"])
    );
}

// ---------------------------------------------------------------------------
// grok / pi / gemini / antigravity — optional-value + reject branches.
// ---------------------------------------------------------------------------

#[test]
fn grok_rejects_single_shot_flag() {
    assert_eq!(preserved_arguments("grok", &v(&["-p"])), None);
    assert_eq!(preserved_arguments("grok", &v(&["--best-of-n", "3"])), None);
}

#[test]
fn grok_drops_worktree_and_resume() {
    assert_eq!(
        preserved_arguments("grok", &v(&["--resume", "id", "--model", "grok"])),
        some(&["--model", "grok"])
    );
}

#[test]
fn pi_and_omp_share_policy() {
    assert_eq!(
        preserved_arguments("pi", &v(&["--model", "pi-1"])),
        some(&["--model", "pi-1"])
    );
    assert_eq!(
        preserved_arguments("omp", &v(&["--model", "pi-1"])),
        some(&["--model", "pi-1"])
    );
    assert_eq!(preserved_arguments("pi", &v(&["--print"])), None);
}

#[test]
fn gemini_drops_resume_span_and_rejects_prompt() {
    assert_eq!(
        preserved_arguments("gemini", &v(&["--model", "gpt", "--resume", "sess"])),
        some(&["--model", "gpt"])
    );
    assert_eq!(preserved_arguments("gemini", &v(&["-p"])), None);
}

#[test]
fn antigravity_drops_continue_boolean_but_keeps_sandbox() {
    assert_eq!(
        preserved_arguments("antigravity", &v(&["--continue", "--sandbox", "strict"])),
        some(&["--sandbox", "strict"])
    );
}

// ---------------------------------------------------------------------------
// amp / cursor / kiro / rovodev / opencode / hermes — subcommand stripping.
// ---------------------------------------------------------------------------

#[test]
fn amp_strips_threads_continue_id_prefix() {
    assert_eq!(
        preserved_arguments("amp", &v(&["t", "c", "01xyz", "-m", "gpt"])),
        some(&["-m", "gpt"])
    );
    assert_eq!(
        preserved_arguments(
            "amp",
            &v(&["threads", "continue", "id-1", "--mode", "fast"])
        ),
        some(&["--mode", "fast"])
    );
}

#[test]
fn cursor_strips_leading_agent_subcommand() {
    assert_eq!(
        preserved_arguments("cursor", &v(&["agent", "--model", "x"])),
        some(&["--model", "x"])
    );
}

#[test]
fn kiro_requires_chat_or_option_leading_token() {
    assert_eq!(
        preserved_arguments("kiro", &v(&["chat", "--agent", "a"])),
        some(&["--agent", "a"])
    );
    // A non-chat bare positional ⇒ not restorable.
    assert_eq!(preserved_arguments("kiro", &v(&["diagnostic"])), None);
}

#[test]
fn rovodev_strips_rovodev_run_prefix() {
    assert_eq!(
        preserved_arguments("rovodev", &v(&["rovodev", "run", "--model", "x"])),
        some(&["--model", "x"])
    );
}

#[test]
fn opencode_strips_internal_bunfs_worker_and_keeps_first_positional() {
    assert_eq!(
        preserved_arguments("opencode", &v(&["tui-settings", "--model", "gpt"])),
        some(&["--model", "gpt"])
    );
    assert_eq!(
        preserved_arguments(
            "opencode",
            &v(&[
                "/tmp/$bunfs/root/tui/worker.js",
                "hello",
                "--model",
                "gpt",
                "world"
            ])
        ),
        some(&["hello", "--model", "gpt"])
    );
}

#[test]
fn hermes_replaces_openai_codex_provider() {
    assert_eq!(
        preserved_arguments(
            "hermes-agent",
            &v(&["--provider", "openai-codex", "--model", "x"])
        ),
        some(&["--provider", "custom", "--model", "x"])
    );
    assert_eq!(
        preserved_arguments(
            "hermes-agent",
            &v(&["--provider=openai-codex", "--model", "x"])
        ),
        some(&["--provider=custom", "--model", "x"])
    );
    assert_eq!(
        preserved_arguments("hermes-agent", &v(&["--oneshot"])),
        None
    );
}

// ---------------------------------------------------------------------------
// copilot / codebuddy / factory / qoder — smoke + non-restorable.
// ---------------------------------------------------------------------------

#[test]
fn copilot_smoke() {
    assert_eq!(
        preserved_arguments("copilot", &v(&["--model", "gpt"])),
        some(&["--model", "gpt"])
    );
    assert_eq!(preserved_arguments("copilot", &v(&["login"])), None);
}

#[test]
fn codebuddy_smoke() {
    assert_eq!(
        preserved_arguments("codebuddy", &v(&["--model", "gpt"])),
        some(&["--model", "gpt"])
    );
    assert_eq!(preserved_arguments("codebuddy", &v(&["config"])), None);
}

#[test]
fn factory_smoke() {
    assert_eq!(
        preserved_arguments("factory", &v(&["--settings", "/x"])),
        some(&["--settings", "/x"])
    );
    assert_eq!(preserved_arguments("factory", &v(&["exec"])), None);
}

#[test]
fn qoder_smoke() {
    assert_eq!(
        preserved_arguments("qoder", &v(&["--model", "gpt"])),
        some(&["--model", "gpt"])
    );
    assert_eq!(preserved_arguments("qoder", &v(&["--print"])), None);
}

#[test]
fn unknown_kind_is_not_restorable() {
    assert_eq!(
        preserved_arguments("totally-unknown", &v(&["--model", "x"])),
        None
    );
}

// ---------------------------------------------------------------------------
// sanitized_launch_arguments — top switch routing.
// ---------------------------------------------------------------------------

#[test]
fn sanitized_requires_non_empty_executable() {
    assert_eq!(
        sanitized_launch_arguments(&v(&[]), "default", "claude"),
        None
    );
    assert_eq!(
        sanitized_launch_arguments(&v(&["", "--model", "x"]), "default", "claude"),
        None
    );
}

#[test]
fn sanitized_claude_teams_prepends_wrapper_verb() {
    assert_eq!(
        sanitized_launch_arguments(
            &v(&[
                "/bin/claude",
                "claude-teams",
                "--tmux",
                "classic",
                "--model",
                "opus"
            ]),
            "claudeTeams",
            "claude"
        ),
        some(&["/bin/claude", "claude-teams", "--model", "opus"])
    );
}

#[test]
fn sanitized_codex_teams_uses_fork_aware_codex_path() {
    assert_eq!(
        sanitized_launch_arguments(
            &v(&["/bin/codex", "codex-teams", "--model", "gpt"]),
            "codexTeams",
            "codex"
        ),
        some(&["/bin/codex", "codex-teams", "--model", "gpt"])
    );
}

#[test]
fn sanitized_omo_routes_to_opencode() {
    assert_eq!(
        sanitized_launch_arguments(
            &v(&["/bin/oc", "omo", "--model", "gpt", "hello"]),
            "omo",
            "opencode"
        ),
        some(&["/bin/oc", "omo", "--model", "gpt", "hello"])
    );
}

#[test]
fn sanitized_omx_and_omc_are_never_restorable() {
    assert_eq!(
        sanitized_launch_arguments(&v(&["/x", "a"]), "omx", "claude"),
        None
    );
    assert_eq!(
        sanitized_launch_arguments(&v(&["/x", "a"]), "omc", "claude"),
        None
    );
}

#[test]
fn sanitized_codex_fallback_preserves_fork() {
    assert_eq!(
        sanitized_launch_arguments(
            &v(&[
                "/bin/codex",
                "fork",
                "0199abcd-1234-5678-9abc-def012345678",
                "--model",
                "gpt"
            ]),
            "unknown-launcher",
            "codex"
        ),
        some(&["/bin/codex", "--model", "gpt"])
    );
}

#[test]
fn sanitized_rovodev_fallback_prepends_run() {
    assert_eq!(
        sanitized_launch_arguments(
            &v(&["/bin/rovo", "--model", "x"]),
            "unknown-launcher",
            "rovodev"
        ),
        some(&["/bin/rovo", "rovodev", "run", "--model", "x"])
    );
}

#[test]
fn sanitized_default_fallback_prepends_executable_only() {
    assert_eq!(
        sanitized_launch_arguments(&v(&["/bin/claude", "--model", "opus"]), "default", "claude"),
        some(&["/bin/claude", "--model", "opus"])
    );
}

#[test]
fn sanitized_returns_none_when_tail_is_not_restorable() {
    assert_eq!(
        sanitized_launch_arguments(&v(&["/bin/claude", "--print"]), "default", "claude"),
        None
    );
}

// ---------------------------------------------------------------------------
// removing_saved_working_directory_options.
// ---------------------------------------------------------------------------

#[test]
fn removes_matching_cd_option_pair() {
    assert_eq!(
        removing_saved_working_directory_options(
            &v(&["--cd", "/repo", "--model", "x"]),
            Some("/repo")
        ),
        v(&["--model", "x"])
    );
}

#[test]
fn removes_matching_equals_form() {
    assert_eq!(
        removing_saved_working_directory_options(
            &v(&["--cwd=/repo", "--model", "x"]),
            Some("/repo")
        ),
        v(&["--model", "x"])
    );
}

#[test]
fn keeps_non_matching_working_directory_option() {
    assert_eq!(
        removing_saved_working_directory_options(
            &v(&["--cd", "/other", "--model", "x"]),
            Some("/repo")
        ),
        v(&["--cd", "/other", "--model", "x"])
    );
}

#[test]
fn passes_through_after_double_dash() {
    assert_eq!(
        removing_saved_working_directory_options(
            &v(&["--cd", "/repo", "--", "--cd", "/repo"]),
            Some("/repo")
        ),
        v(&["--", "--cd", "/repo"])
    );
}

#[test]
fn returns_args_unchanged_when_working_directory_absent() {
    assert_eq!(
        removing_saved_working_directory_options(&v(&["--cd", "/repo"]), None),
        v(&["--cd", "/repo"])
    );
    assert_eq!(
        removing_saved_working_directory_options(&v(&["--cd", "/repo"]), Some("   ")),
        v(&["--cd", "/repo"])
    );
}
