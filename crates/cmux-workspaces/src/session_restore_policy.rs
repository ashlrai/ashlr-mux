//! Pure port of `WorkspaceSessionRestorePolicyService` from
//! `Packages/macOS/CmuxWorkspaces/Sources/CmuxWorkspaces/Session/WorkspaceSessionRestorePolicyService.swift:1-191`.
//!
//! Owns workspace session restore policy decisions. The app target injects
//! concrete approval storage, prompt handling, automated-test detection,
//! scrollback truncation, and Hermes Codex defaults, keeping this crate
//! independent of app DTO storage and UI while preserving the exact restore
//! behavior.
//!
//! The nine decision methods are pure orchestration over injected seams:
//! - [`resolved_snapshot_terminal_scrollback`](WorkspaceSessionRestorePolicyService::resolved_snapshot_terminal_scrollback)
//! - [`should_replay_session_scrollback`](WorkspaceSessionRestorePolicyService::should_replay_session_scrollback)
//! - [`should_auto_connect_restored_remote`](WorkspaceSessionRestorePolicyService::should_auto_connect_restored_remote)
//! - [`surface_resume_startup_input`](WorkspaceSessionRestorePolicyService::surface_resume_startup_input)
//! - [`surface_resume_startup_launch`](WorkspaceSessionRestorePolicyService::surface_resume_startup_launch)
//! - [`approved_surface_resume_binding`](WorkspaceSessionRestorePolicyService::approved_surface_resume_binding)
//! - [`restorable_tmux_start_command`](WorkspaceSessionRestorePolicyService::restorable_tmux_start_command)
//! - [`should_persist_session_scrollback`](WorkspaceSessionRestorePolicyService::should_persist_session_scrollback)
//!
//! DIVERGENCE (seams as trait/closures). Swift threads the concrete Codable
//! payload through the `WorkspaceSurfaceResumeBinding` protocol and passes a
//! `FileManager` + temporary-directory `URL` into the binding's launcher-script
//! methods (real fs I/O). The Rust port models the binding as a trait whose
//! `startup_*_with_launcher_script` methods ARE the injected fs seam (faked in
//! tests, exactly as the Swift tests' `FakeBinding` returns a formatted string),
//! and passes `&Path` where Swift passes the temporary-directory `URL`. The
//! `FileManager` argument is dropped — it only exists to perform the fs write the
//! seam abstracts away.
//!
//! DIVERGENCE (no default arguments). Swift methods use default parameter values
//! (`allowFallbackScrollback: Bool = true`, `promptForApproval: Bool = true`,
//! `approvalSigningSecret: Data? = nil`, ...). Rust has no default arguments, so
//! every parameter is explicit at the call site; the Swift defaults are passed
//! verbatim by callers.

mod hermes_bootstrap;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use hermes_bootstrap::{trim_whitespace_and_newlines, WorkspaceHermesAgentCommandBootstrapper};

/// Protocol seam for the app target's surface resume snapshot DTO.
///
/// Mirrors Swift `WorkspaceSurfaceResumeBinding`
/// (`WorkspaceSurfaceResumeBinding.swift:7-41`). The app target owns the concrete
/// Codable payload that preserves the persisted wire format; this crate owns the
/// restore policy.
pub trait WorkspaceSurfaceResumeBinding {
    /// The binding source, for example `agent-hook` or `process-detected`.
    fn source(&self) -> Option<&str>;
    /// The binding kind, for example `hermes-agent`.
    fn kind(&self) -> Option<&str>;
    /// The shell command restored for this binding.
    fn command(&self) -> &str;
    /// Replaces the restored shell command.
    fn set_command(&mut self, command: String);
    /// The working directory associated with the restored command.
    fn cwd(&self) -> Option<&str>;
    /// Environment values restored with the command.
    fn environment(&self) -> Option<&HashMap<String, String>>;
    /// Replaces the restored environment values.
    fn set_environment(&mut self, environment: Option<HashMap<String, String>>);
    /// Whether this binding came from a process detector.
    fn is_process_detected(&self) -> bool;
    /// Whether this binding came from a managed agent hook.
    fn is_agent_hook_binding(&self) -> bool;
    /// Whether this binding permits automatic resume without prompting.
    fn allows_automatic_resume(&self) -> bool;
    /// Whether this binding's approval policy requires prompting.
    fn requires_prompt_approval(&self) -> bool;
    /// Whether this binding is explicitly configured for automatic resume.
    fn auto_resume(&self) -> Option<bool>;

    /// Returns the startup input used to replay this binding in an interactive
    /// shell. This method is the injected fs seam (Swift passes a `FileManager`
    /// + temporary-directory `URL` and may write a launcher script).
    fn startup_input_with_launcher_script(
        &self,
        temporary_directory: &Path,
        allow_launcher_script: bool,
    ) -> Option<String>;

    /// Returns a launcher command used when the restored terminal should run a
    /// command. This method is the injected fs seam.
    fn startup_command_with_launcher_script(
        &self,
        temporary_directory: &Path,
    ) -> Option<String>;
}

/// Launch action produced for a restored surface resume binding.
///
/// Mirrors Swift `WorkspaceSurfaceResumeStartupLaunch`
/// (`WorkspaceSurfaceResumeStartupLaunch.swift:2-23`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceSurfaceResumeStartupLaunch {
    /// Start the restored terminal with a command.
    Command(String),
    /// Send input to the restored terminal after it starts.
    Input(String),
}

impl WorkspaceSurfaceResumeStartupLaunch {
    /// The command payload when this launch uses command startup.
    pub fn initial_command(&self) -> Option<&str> {
        match self {
            Self::Command(command) => Some(command),
            Self::Input(_) => None,
        }
    }

    /// The input payload when this launch uses post-start input.
    pub fn initial_input(&self) -> Option<&str> {
        match self {
            Self::Input(input) => Some(input),
            Self::Command(_) => None,
        }
    }
}

/// Injected Hermes Codex environment policy used by workspace session restore.
///
/// Mirrors Swift `WorkspaceHermesCodexEnvironment`
/// (`WorkspaceHermesCodexEnvironment.swift:5-39`). Concrete values live in the
/// app target (agent launch domain); this value keeps the crate independent.
pub struct WorkspaceHermesCodexEnvironment {
    /// Environment key carrying the custom Codex-compatible base URL.
    pub custom_base_url_environment_key: String,
    /// Provider name cmux should configure when replaying a Hermes agent binding.
    pub default_provider: String,
    /// Hermes model API mode for Codex Responses-compatible replay.
    pub codex_responses_api_mode: String,
    #[allow(clippy::type_complexity)]
    applying_default_codex_base_url:
        Box<dyn Fn(HashMap<String, String>) -> HashMap<String, String>>,
    #[allow(clippy::type_complexity)]
    resolving_default_codex_model: Box<dyn Fn(&HashMap<String, String>) -> Option<String>>,
}

impl WorkspaceHermesCodexEnvironment {
    /// Creates a Hermes Codex environment policy.
    pub fn new(
        custom_base_url_environment_key: String,
        default_provider: String,
        codex_responses_api_mode: String,
        applying_default_codex_base_url: impl Fn(HashMap<String, String>) -> HashMap<String, String>
            + 'static,
        resolving_default_codex_model: impl Fn(&HashMap<String, String>) -> Option<String> + 'static,
    ) -> Self {
        Self {
            custom_base_url_environment_key,
            default_provider,
            codex_responses_api_mode,
            applying_default_codex_base_url: Box::new(applying_default_codex_base_url),
            resolving_default_codex_model: Box::new(resolving_default_codex_model),
        }
    }

    /// Returns `environment` with the app's default Codex base URL applied.
    pub fn apply_default_codex_base_url(
        &self,
        environment: HashMap<String, String>,
    ) -> HashMap<String, String> {
        (self.applying_default_codex_base_url)(environment)
    }

    /// Returns the default Codex model for the supplied environment, if any.
    pub fn default_codex_model(&self, environment: &HashMap<String, String>) -> Option<String> {
        (self.resolving_default_codex_model)(environment)
    }
}

/// Protocol seam for terminal-specific remote restore metadata.
///
/// Mirrors Swift `WorkspaceSessionRemoteRestoreTerminalSnapshot`
/// (`WorkspaceSessionRemoteRestoreTerminalSnapshot.swift:2-7`).
pub trait WorkspaceSessionRemoteRestoreTerminalSnapshot {
    /// Whether the snapshot represents a remote terminal.
    fn is_remote_terminal(&self) -> Option<bool>;
    /// Persisted remote PTY session identifier, if present.
    fn remote_pty_session_id(&self) -> Option<&str>;
}

/// Protocol seam for panel restore metadata inspected by remote reconnect policy.
///
/// Mirrors Swift `WorkspaceSessionRemoteRestorePanelSnapshot`
/// (`WorkspaceSessionRemoteRestorePanelSnapshot.swift:2-8`).
pub trait WorkspaceSessionRemoteRestorePanelSnapshot {
    /// Terminal restore metadata type.
    type Terminal: WorkspaceSessionRemoteRestoreTerminalSnapshot;

    /// Terminal restore metadata for this panel, if present.
    fn terminal(&self) -> Option<&Self::Terminal>;
}

/// Protocol seam for workspace restore metadata inspected by remote reconnect
/// policy.
///
/// Mirrors Swift `WorkspaceSessionRemoteRestoreSnapshot`
/// (`WorkspaceSessionRemoteRestoreSnapshot.swift:2-8`).
pub trait WorkspaceSessionRemoteRestoreSnapshot {
    /// Panel restore metadata type.
    type Panel: WorkspaceSessionRemoteRestorePanelSnapshot;

    /// Panel snapshots restored with the workspace.
    fn panels(&self) -> &[Self::Panel];
}

/// Service that owns workspace session restore policy decisions.
///
/// Mirrors Swift `WorkspaceSessionRestorePolicyService<Binding>`
/// (`WorkspaceSessionRestorePolicyService.swift:9-191`).
#[allow(clippy::type_complexity)]
pub struct WorkspaceSessionRestorePolicyService<Binding: WorkspaceSurfaceResumeBinding> {
    apply_stored_approval: Box<dyn Fn(Binding, &Path, Option<&[u8]>) -> Binding>,
    should_run_prompted_surface_resume: Box<dyn Fn(&Binding) -> bool>,
    is_running_under_automated_tests: Box<dyn Fn() -> bool>,
    truncate_scrollback: Box<dyn Fn(Option<&str>) -> Option<String>>,
    hermes_codex_environment: WorkspaceHermesCodexEnvironment,
    temporary_directory: PathBuf,
}

impl<Binding: WorkspaceSurfaceResumeBinding> WorkspaceSessionRestorePolicyService<Binding> {
    /// Creates a restore policy service.
    ///
    /// Mirrors Swift `init(...)`
    /// (`WorkspaceSessionRestorePolicyService.swift:18-32`).
    ///
    /// - `apply_stored_approval`: injected approval store (`cmux-resume`), maps
    ///   `(binding, approvalStoreURL, approvalSigningSecret) -> binding`.
    /// - `should_run_prompted_surface_resume`: injected UI prompt decision.
    /// - `is_running_under_automated_tests`: injected env I/O.
    /// - `truncate_scrollback`: injected `SessionPersistencePolicy.truncatedScrollback`.
    pub fn new(
        apply_stored_approval: impl Fn(Binding, &Path, Option<&[u8]>) -> Binding + 'static,
        should_run_prompted_surface_resume: impl Fn(&Binding) -> bool + 'static,
        is_running_under_automated_tests: impl Fn() -> bool + 'static,
        truncate_scrollback: impl Fn(Option<&str>) -> Option<String> + 'static,
        hermes_codex_environment: WorkspaceHermesCodexEnvironment,
        temporary_directory: PathBuf,
    ) -> Self {
        Self {
            apply_stored_approval: Box::new(apply_stored_approval),
            should_run_prompted_surface_resume: Box::new(should_run_prompted_surface_resume),
            is_running_under_automated_tests: Box::new(is_running_under_automated_tests),
            truncate_scrollback: Box::new(truncate_scrollback),
            hermes_codex_environment,
            temporary_directory,
        }
    }

    /// Resolves the scrollback text persisted for a terminal snapshot.
    ///
    /// Mirrors Swift `resolvedSnapshotTerminalScrollback(...)`
    /// (`WorkspaceSessionRestorePolicyService.swift:35-45`). Prefers the captured
    /// (truncated) text; falls back to the fallback text only when
    /// `allow_fallback_scrollback` is set.
    pub fn resolved_snapshot_terminal_scrollback(
        &self,
        captured_scrollback: Option<&str>,
        fallback_scrollback: Option<&str>,
        allow_fallback_scrollback: bool,
    ) -> Option<String> {
        if let Some(captured) = (self.truncate_scrollback)(captured_scrollback) {
            return Some(captured);
        }
        if !allow_fallback_scrollback {
            return None;
        }
        (self.truncate_scrollback)(fallback_scrollback)
    }

    /// Returns whether restored scrollback should be replayed for a terminal.
    ///
    /// Mirrors Swift `shouldReplaySessionScrollback(...)`
    /// (`WorkspaceSessionRestorePolicyService.swift:48-54`).
    pub fn should_replay_session_scrollback(
        &self,
        has_restorable_agent: bool,
        tmux_start_command: Option<&str>,
        has_resume_startup_work: bool,
    ) -> bool {
        !has_restorable_agent
            && self.restorable_tmux_start_command(tmux_start_command).is_none()
            && !has_resume_startup_work
    }

    /// Returns whether a restored remote workspace should auto-connect.
    ///
    /// Mirrors Swift `shouldAutoConnectRestoredRemote(...)`
    /// (`WorkspaceSessionRestorePolicyService.swift:57-77`).
    pub fn should_auto_connect_restored_remote<Snapshot: WorkspaceSessionRemoteRestoreSnapshot>(
        &self,
        foreground_auth_token: Option<&str>,
        snapshot: &Snapshot,
        override_is_running_under_automated_tests: Option<bool>,
    ) -> bool {
        let running_under_tests = override_is_running_under_automated_tests
            .unwrap_or_else(|| (self.is_running_under_automated_tests)());
        if running_under_tests {
            return false;
        }
        let normalized_foreground_auth_token =
            foreground_auth_token.map(trim_whitespace_and_newlines);
        // guard normalizedForegroundAuthToken?.isEmpty == false else { return true }
        match normalized_foreground_auth_token {
            Some(token) if !token.is_empty() => {}
            _ => return true,
        }
        let has_terminal_that_will_authenticate_reconnect = snapshot.panels().iter().any(|panel| {
            let terminal = match panel.terminal() {
                Some(terminal) => terminal,
                None => return false,
            };
            if terminal.is_remote_terminal() != Some(false) {
                return true;
            }
            let remote_pty_session_id = terminal
                .remote_pty_session_id()
                .map(trim_whitespace_and_newlines);
            matches!(remote_pty_session_id, Some(id) if !id.is_empty())
        });
        !has_terminal_that_will_authenticate_reconnect
    }

    /// Returns startup input for an approved restored surface resume binding.
    ///
    /// Mirrors Swift `surfaceResumeStartupInput(...)`
    /// (`WorkspaceSessionRestorePolicyService.swift:80-103`).
    #[allow(clippy::too_many_arguments)]
    pub fn surface_resume_startup_input(
        &self,
        resume_binding: Option<Binding>,
        auto_resume_agent_sessions: bool,
        allow_launcher_script: bool,
        prompt_for_approval: bool,
        approval_store_url: &Path,
        approval_signing_secret: Option<&[u8]>,
    ) -> Option<String> {
        let effective_binding = self.approved_surface_resume_binding(
            resume_binding,
            auto_resume_agent_sessions,
            prompt_for_approval,
            approval_store_url,
            approval_signing_secret,
        )?;
        effective_binding
            .startup_input_with_launcher_script(&self.temporary_directory, allow_launcher_script)
    }

    /// Returns the command or input launch action for a restored surface resume
    /// binding.
    ///
    /// Mirrors Swift `surfaceResumeStartupLaunch(...)`
    /// (`WorkspaceSessionRestorePolicyService.swift:106-129`).
    #[allow(clippy::too_many_arguments)]
    pub fn surface_resume_startup_launch(
        &self,
        resume_binding: Option<Binding>,
        auto_resume_agent_sessions: bool,
        allow_launcher_script: bool,
        prompt_for_approval: bool,
        approval_store_url: &Path,
        approval_signing_secret: Option<&[u8]>,
    ) -> Option<WorkspaceSurfaceResumeStartupLaunch> {
        let effective_binding = self.approved_surface_resume_binding(
            resume_binding,
            auto_resume_agent_sessions,
            prompt_for_approval,
            approval_store_url,
            approval_signing_secret,
        )?;
        self.surface_resume_startup_launch_for_approved(effective_binding, allow_launcher_script)
    }

    /// Returns the command or input launch action for an already approved
    /// binding.
    ///
    /// Mirrors Swift `surfaceResumeStartupLaunch(forApprovedBinding:...)`
    /// (`WorkspaceSessionRestorePolicyService.swift:132-153`).
    pub fn surface_resume_startup_launch_for_approved(
        &self,
        effective_binding: Binding,
        allow_launcher_script: bool,
    ) -> Option<WorkspaceSurfaceResumeStartupLaunch> {
        if effective_binding.is_agent_hook_binding() && allow_launcher_script {
            if let Some(command) =
                effective_binding.startup_command_with_launcher_script(&self.temporary_directory)
            {
                return Some(WorkspaceSurfaceResumeStartupLaunch::Command(command));
            }
        }
        let input = effective_binding
            .startup_input_with_launcher_script(&self.temporary_directory, allow_launcher_script)?;
        Some(WorkspaceSurfaceResumeStartupLaunch::Input(input))
    }

    /// Applies stored approval state and returns the binding allowed to run.
    ///
    /// Mirrors Swift `approvedSurfaceResumeBinding(...)`
    /// (`WorkspaceSessionRestorePolicyService.swift:156-178`).
    ///
    /// ORDERING (load-bearing): stored approval FIRST, THEN Hermes bootstrap,
    /// THEN the agent-hook auto-resume gate, THEN prompt-vs-automatic.
    pub fn approved_surface_resume_binding(
        &self,
        resume_binding: Option<Binding>,
        auto_resume_agent_sessions: bool,
        prompt_for_approval: bool,
        approval_store_url: &Path,
        approval_signing_secret: Option<&[u8]>,
    ) -> Option<Binding> {
        let resume_binding = resume_binding?;
        let mut effective_binding = (self.apply_stored_approval)(
            resume_binding,
            approval_store_url,
            approval_signing_secret,
        );
        effective_binding = WorkspaceHermesAgentCommandBootstrapper::new(
            &self.hermes_codex_environment,
        )
        .binding_for_startup(effective_binding);
        if effective_binding.source() == Some("agent-hook") && !auto_resume_agent_sessions {
            return None;
        }
        if effective_binding.requires_prompt_approval() {
            if !prompt_for_approval {
                return None;
            }
            if !(self.should_run_prompted_surface_resume)(&effective_binding) {
                return None;
            }
            return Some(effective_binding);
        }
        if !effective_binding.allows_automatic_resume() {
            return None;
        }
        Some(effective_binding)
    }

    /// Returns a restorable tmux start command when the command launches an OMX
    /// HUD.
    ///
    /// Mirrors Swift `restorableTmuxStartCommand(_:)`
    /// (`WorkspaceSessionRestorePolicyService.swift:181-185`).
    pub fn restorable_tmux_start_command(&self, raw_command: Option<&str>) -> Option<String> {
        WorkspaceHermesAgentCommandBootstrapper::new(&self.hermes_codex_environment)
            .restorable_tmux_start_command(raw_command)
    }

    /// Returns whether terminal scrollback should be persisted when
    /// closing/restoring.
    ///
    /// Mirrors Swift `shouldPersistSessionScrollback(closeConfirmationRequired:)`
    /// (`WorkspaceSessionRestorePolicyService.swift:188-190`).
    pub fn should_persist_session_scrollback(&self, close_confirmation_required: bool) -> bool {
        !close_confirmation_required
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    // Direct 1:1 port of `WorkspaceSessionRestorePolicyServiceTests.swift:1-293`.

    #[derive(Clone)]
    struct FakeBinding {
        source: Option<String>,
        kind: Option<String>,
        command: String,
        cwd: Option<String>,
        environment: Option<HashMap<String, String>>,
        is_process_detected: bool,
        is_agent_hook_binding: bool,
        allows_automatic_resume: bool,
        requires_prompt_approval: bool,
        auto_resume: Option<bool>,
        startup_input_prefix: String,
        startup_command_prefix: String,
    }

    impl FakeBinding {
        // Mirrors the Swift `FakeBinding.init` defaults
        // (`WorkspaceSessionRestorePolicyServiceTests.swift:27-49`).
        fn new() -> Self {
            Self {
                source: Some("cli".to_string()),
                kind: None,
                command: "echo ok".to_string(),
                cwd: None,
                environment: None,
                is_process_detected: false,
                is_agent_hook_binding: false,
                allows_automatic_resume: true,
                requires_prompt_approval: false,
                auto_resume: None,
                startup_input_prefix: "input".to_string(),
                startup_command_prefix: "command".to_string(),
            }
        }
    }

    impl WorkspaceSurfaceResumeBinding for FakeBinding {
        fn source(&self) -> Option<&str> {
            self.source.as_deref()
        }
        fn kind(&self) -> Option<&str> {
            self.kind.as_deref()
        }
        fn command(&self) -> &str {
            &self.command
        }
        fn set_command(&mut self, command: String) {
            self.command = command;
        }
        fn cwd(&self) -> Option<&str> {
            self.cwd.as_deref()
        }
        fn environment(&self) -> Option<&HashMap<String, String>> {
            self.environment.as_ref()
        }
        fn set_environment(&mut self, environment: Option<HashMap<String, String>>) {
            self.environment = environment;
        }
        fn is_process_detected(&self) -> bool {
            self.is_process_detected
        }
        fn is_agent_hook_binding(&self) -> bool {
            self.is_agent_hook_binding
        }
        fn allows_automatic_resume(&self) -> bool {
            self.allows_automatic_resume
        }
        fn requires_prompt_approval(&self) -> bool {
            self.requires_prompt_approval
        }
        fn auto_resume(&self) -> Option<bool> {
            self.auto_resume
        }
        fn startup_input_with_launcher_script(
            &self,
            _temporary_directory: &Path,
            allow_launcher_script: bool,
        ) -> Option<String> {
            Some(format!(
                "{}:{}:launcher={}",
                self.startup_input_prefix, self.command, allow_launcher_script
            ))
        }
        fn startup_command_with_launcher_script(
            &self,
            _temporary_directory: &Path,
        ) -> Option<String> {
            Some(format!("{}:{}", self.startup_command_prefix, self.command))
        }
    }

    struct FakeTerminalSnapshot {
        is_remote_terminal: Option<bool>,
        remote_pty_session_id: Option<String>,
    }

    impl WorkspaceSessionRemoteRestoreTerminalSnapshot for FakeTerminalSnapshot {
        fn is_remote_terminal(&self) -> Option<bool> {
            self.is_remote_terminal
        }
        fn remote_pty_session_id(&self) -> Option<&str> {
            self.remote_pty_session_id.as_deref()
        }
    }

    struct FakePanelSnapshot {
        terminal: Option<FakeTerminalSnapshot>,
    }

    impl WorkspaceSessionRemoteRestorePanelSnapshot for FakePanelSnapshot {
        type Terminal = FakeTerminalSnapshot;
        fn terminal(&self) -> Option<&FakeTerminalSnapshot> {
            self.terminal.as_ref()
        }
    }

    struct FakeRemoteSnapshot {
        panels: Vec<FakePanelSnapshot>,
    }

    impl WorkspaceSessionRemoteRestoreSnapshot for FakeRemoteSnapshot {
        type Panel = FakePanelSnapshot;
        fn panels(&self) -> &[FakePanelSnapshot] {
            &self.panels
        }
    }

    // Builder mirroring the Swift test's `makeService(...)` helper defaults
    // (`WorkspaceSessionRestorePolicyServiceTests.swift:80-102`). Rust cannot
    // default closure arguments, so a struct with `Option` fields stands in.
    #[allow(clippy::type_complexity)]
    struct ServiceBuilder {
        apply_stored_approval:
            Option<Box<dyn Fn(FakeBinding, &Path, Option<&[u8]>) -> FakeBinding>>,
        should_run_prompted_surface_resume: Option<Box<dyn Fn(&FakeBinding) -> bool>>,
        is_running_under_automated_tests: Option<Box<dyn Fn() -> bool>>,
        truncate_scrollback: Option<Box<dyn Fn(Option<&str>) -> Option<String>>>,
        applying_default_codex_base_url:
            Option<Box<dyn Fn(HashMap<String, String>) -> HashMap<String, String>>>,
        resolving_default_codex_model:
            Option<Box<dyn Fn(&HashMap<String, String>) -> Option<String>>>,
    }

    impl ServiceBuilder {
        fn new() -> Self {
            Self {
                apply_stored_approval: None,
                should_run_prompted_surface_resume: None,
                is_running_under_automated_tests: None,
                truncate_scrollback: None,
                applying_default_codex_base_url: None,
                resolving_default_codex_model: None,
            }
        }

        fn build(self) -> WorkspaceSessionRestorePolicyService<FakeBinding> {
            let apply_stored_approval = self
                .apply_stored_approval
                .unwrap_or_else(|| Box::new(|binding, _, _| binding));
            let should_run_prompted_surface_resume = self
                .should_run_prompted_surface_resume
                .unwrap_or_else(|| Box::new(|_| false));
            let is_running_under_automated_tests = self
                .is_running_under_automated_tests
                .unwrap_or_else(|| Box::new(|| false));
            let truncate_scrollback = self
                .truncate_scrollback
                .unwrap_or_else(|| Box::new(|value| value.map(str::to_string)));
            let applying_default_codex_base_url = self
                .applying_default_codex_base_url
                .unwrap_or_else(|| Box::new(|env| env));
            let resolving_default_codex_model = self
                .resolving_default_codex_model
                .unwrap_or_else(|| Box::new(|_| None));

            WorkspaceSessionRestorePolicyService::new(
                apply_stored_approval,
                should_run_prompted_surface_resume,
                is_running_under_automated_tests,
                truncate_scrollback,
                WorkspaceHermesCodexEnvironment::new(
                    "OPENAI_BASE_URL".to_string(),
                    "codex".to_string(),
                    "responses".to_string(),
                    applying_default_codex_base_url,
                    resolving_default_codex_model,
                ),
                PathBuf::from("/tmp"),
            )
        }
    }

    fn make_service() -> WorkspaceSessionRestorePolicyService<FakeBinding> {
        ServiceBuilder::new().build()
    }

    // @Test("stored approval is injected and can authorize a binding")
    #[test]
    fn stored_approval_authorizes_binding() {
        let approval_url = PathBuf::from("/tmp/cmux-approvals.json");
        let observed_url: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let observed_secret: Rc<RefCell<Option<Vec<u8>>>> = Rc::new(RefCell::new(None));
        let observed_url_c = Rc::clone(&observed_url);
        let observed_secret_c = Rc::clone(&observed_secret);

        let mut builder = ServiceBuilder::new();
        builder.apply_stored_approval = Some(Box::new(move |binding, file_url, signing_secret| {
            *observed_url_c.borrow_mut() = Some(file_url.to_path_buf());
            *observed_secret_c.borrow_mut() = signing_secret.map(<[u8]>::to_vec);
            let mut copy = binding;
            copy.allows_automatic_resume = true;
            copy
        }));
        let service = builder.build();

        let mut binding = FakeBinding::new();
        binding.allows_automatic_resume = false;

        let result = service.surface_resume_startup_input(
            Some(binding),
            true,
            false, // allowLauncherScript default
            true,  // promptForApproval default
            &approval_url,
            Some(b"secret"),
        );

        assert_eq!(result.as_deref(), Some("input:echo ok:launcher=false"));
        assert_eq!(*observed_url.borrow(), Some(approval_url));
        assert_eq!(observed_secret.borrow().as_deref(), Some(&b"secret"[..]));
    }

    // @Test("prompt approval uses the injected prompt decision")
    #[test]
    fn prompt_approval_uses_injected_decision() {
        let approval_url = PathBuf::from("/tmp/cmux-approvals.json");
        let make_binding = || {
            let mut binding = FakeBinding::new();
            binding.allows_automatic_resume = false;
            binding.requires_prompt_approval = true;
            binding
        };

        let denied = {
            let mut b = ServiceBuilder::new();
            b.should_run_prompted_surface_resume = Some(Box::new(|_| false));
            b.build()
        };
        let approved = {
            let mut b = ServiceBuilder::new();
            b.should_run_prompted_surface_resume = Some(Box::new(|_| true));
            b.build()
        };

        assert_eq!(
            denied.surface_resume_startup_input(
                Some(make_binding()),
                true,
                false,
                true,
                &approval_url,
                None
            ),
            None
        );
        assert_eq!(
            approved
                .surface_resume_startup_input(
                    Some(make_binding()),
                    true,
                    false,
                    true,
                    &approval_url,
                    None
                )
                .as_deref(),
            Some("input:echo ok:launcher=false")
        );
        assert_eq!(
            approved.surface_resume_startup_input(
                Some(make_binding()),
                true,
                false,
                false, // promptForApproval: false
                &approval_url,
                None
            ),
            None
        );
    }

    // @Test("agent hook bindings respect the auto-resume gate")
    #[test]
    fn agent_hook_bindings_respect_auto_resume_gate() {
        let service = make_service();
        let approval_url = PathBuf::from("/tmp/cmux-approvals.json");
        let make_binding = || {
            let mut binding = FakeBinding::new();
            binding.source = Some("agent-hook".to_string());
            binding.command = "claude --resume".to_string();
            binding.is_agent_hook_binding = true;
            binding.allows_automatic_resume = true;
            binding
        };

        assert_eq!(
            service.surface_resume_startup_input(
                Some(make_binding()),
                false, // autoResumeAgentSessions: false
                false,
                true,
                &approval_url,
                None
            ),
            None
        );
        assert_eq!(
            service
                .surface_resume_startup_input(
                    Some(make_binding()),
                    true, // autoResumeAgentSessions: true
                    false,
                    true,
                    &approval_url,
                    None
                )
                .as_deref(),
            Some("input:claude --resume:launcher=false")
        );
    }

    // @Test("Hermes agent bindings receive Codex bootstrap and provider rewrite")
    #[test]
    fn hermes_agent_bindings_receive_codex_bootstrap() {
        let service = {
            let mut b = ServiceBuilder::new();
            b.applying_default_codex_base_url = Some(Box::new(|environment| {
                let mut copy = environment;
                copy.insert(
                    "OPENAI_BASE_URL".to_string(),
                    "https://codex.example.test".to_string(),
                );
                copy
            }));
            b.resolving_default_codex_model = Some(Box::new(|_| Some("gpt-5".to_string())));
            b.build()
        };

        let mut binding = FakeBinding::new();
        binding.source = Some("agent-hook".to_string());
        binding.kind = Some("hermes-agent".to_string());
        binding.command = "cd /repo && hermes --provider openai-codex run".to_string();
        binding.is_agent_hook_binding = true;
        binding.allows_automatic_resume = true;

        let launch = service
            .surface_resume_startup_launch(
                Some(binding),
                true,
                true, // allowLauncherScript default (true for launch)
                true,
                &PathBuf::from("/tmp/cmux-approvals.json"),
                None,
            )
            .expect("expected a launch");
        let command = match launch {
            WorkspaceSurfaceResumeStartupLaunch::Command(command) => command,
            WorkspaceSurfaceResumeStartupLaunch::Input(_) => panic!("expected command launch"),
        };

        assert!(command.starts_with("command:cd /repo && "), "got: {command}");
        assert!(command.contains("'hermes' config set model.provider 'codex' >/dev/null"));
        assert!(command
            .contains("'hermes' config set model.base_url 'https://codex.example.test' >/dev/null"));
        assert!(command.contains("'hermes' config set model.api_mode 'responses' >/dev/null"));
        assert!(command.contains("'hermes' config set model.default 'gpt-5' >/dev/null"));
        assert!(command.contains("hermes --provider 'codex' run"));
    }

    // @Test("remote reconnect waits when restored terminals can authenticate")
    #[test]
    fn remote_reconnect_waits_when_terminals_authenticate() {
        let service = make_service();
        let approval_terminal = || FakePanelSnapshot {
            terminal: Some(FakeTerminalSnapshot {
                is_remote_terminal: Some(true),
                remote_pty_session_id: None,
            }),
        };
        let pty_terminal = || FakePanelSnapshot {
            terminal: Some(FakeTerminalSnapshot {
                is_remote_terminal: Some(false),
                remote_pty_session_id: Some("pty-1".to_string()),
            }),
        };

        assert!(service.should_auto_connect_restored_remote(
            None,
            &FakeRemoteSnapshot {
                panels: vec![approval_terminal()]
            },
            None
        ));
        assert!(!service.should_auto_connect_restored_remote(
            Some("token"),
            &FakeRemoteSnapshot {
                panels: vec![approval_terminal()]
            },
            None
        ));
        assert!(!service.should_auto_connect_restored_remote(
            Some("token"),
            &FakeRemoteSnapshot {
                panels: vec![pty_terminal()]
            },
            None
        ));
        assert!(service.should_auto_connect_restored_remote(
            Some("token"),
            &FakeRemoteSnapshot { panels: vec![] },
            None
        ));
        assert!(!service.should_auto_connect_restored_remote(
            None,
            &FakeRemoteSnapshot { panels: vec![] },
            Some(true), // isRunningUnderAutomatedTests: true
        ));
    }

    // @Test("scrollback resolution prefers captured text and gates fallback")
    #[test]
    fn scrollback_resolution_prefers_captured_text_and_gates_fallback() {
        let service = {
            let mut b = ServiceBuilder::new();
            b.truncate_scrollback =
                Some(Box::new(|text| text.map(|t| t.chars().take(5).collect())));
            b.build()
        };

        assert_eq!(
            service
                .resolved_snapshot_terminal_scrollback(Some("captured"), Some("fallback"), true)
                .as_deref(),
            Some("captu")
        );
        assert_eq!(
            service
                .resolved_snapshot_terminal_scrollback(None, Some("fallback"), true)
                .as_deref(),
            Some("fallb")
        );
        assert_eq!(
            service.resolved_snapshot_terminal_scrollback(None, Some("fallback"), false),
            None
        );
    }

    // @Test("scrollback replay skips restorable agents, OMX HUD, and resume startup work")
    #[test]
    fn scrollback_replay_policy() {
        let service = make_service();

        assert!(service.should_replay_session_scrollback(false, None, false));
        assert!(!service.should_replay_session_scrollback(true, None, false));
        assert!(!service.should_replay_session_scrollback(false, Some("oh-my-codex hud"), false));
        assert!(!service.should_replay_session_scrollback(false, None, true));
    }

    // @Test("tmux start command is restorable only for OMX HUD commands")
    #[test]
    fn restorable_tmux_start_command_requires_omx_hud() {
        let service = make_service();

        assert_eq!(
            service
                .restorable_tmux_start_command(Some("  oh-my-codex hud  "))
                .as_deref(),
            Some("oh-my-codex hud")
        );
        assert_eq!(service.restorable_tmux_start_command(Some("omx run")), None);
        assert_eq!(service.restorable_tmux_start_command(Some("hudson omx")), None);
        assert_eq!(
            service.restorable_tmux_start_command(Some("omx hud")).as_deref(),
            Some("omx hud")
        );
    }

    // Extra: should_persist_session_scrollback (Swift :188-190).
    #[test]
    fn should_persist_session_scrollback_negates_close_confirmation() {
        let service = make_service();
        assert!(service.should_persist_session_scrollback(false));
        assert!(!service.should_persist_session_scrollback(true));
    }

    // Extra: launch enum accessors (Swift WorkspaceSurfaceResumeStartupLaunch).
    #[test]
    fn launch_accessors() {
        let command = WorkspaceSurfaceResumeStartupLaunch::Command("c".to_string());
        assert_eq!(command.initial_command(), Some("c"));
        assert_eq!(command.initial_input(), None);
        let input = WorkspaceSurfaceResumeStartupLaunch::Input("i".to_string());
        assert_eq!(input.initial_input(), Some("i"));
        assert_eq!(input.initial_command(), None);
    }
}
