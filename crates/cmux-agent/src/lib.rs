//! Agent executable resolution and launch-plan computation.
//!
//! Pure (environment + filesystem injected) port of the macOS agent resolver:
//! parses `PATH`, walks Windows package-manager roots (scoop, choco, winget,
//! nvm-windows, volta, fnm, bun), filters cmux's own bundled shims, and produces
//! an `AgentSessionLaunchPlan` with a rewritten `PATH` and seeded `CMUX_*`
//! environment. Process spawning is deferred to M3; this crate only computes the
//! plan, which keeps it unit-testable on any OS.
//!
//! Swift parity sources:
//! - `Sources/AgentExecutableResolver.swift`
//! - `Sources/AgentSessionLaunchPlan.swift`
//! - `Sources/AgentSessionProvider.swift`

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use uuid::Uuid;

mod auto_naming_agent_catalog;
mod capture_trust;
mod env_policy;
mod feed_event;
mod hook_payload;
mod opencode;
mod prompt_extraction;
mod spawn_identity;
mod version;
mod workstream_context;
mod workstream_json;
mod workstream_source;

pub use auto_naming_agent_catalog::{
    display_name_for_slug, option_for_slug, other_agents, resolve_summarizer, summarizer_supported,
    supported_agents, AutoNamingAgentOption, SummarizerDecision, AGENTS, AUTO_SLUG, SUPPORTED_SLUGS,
};
pub use capture_trust::{
    argv_looks_like_shell_wrapper, launcher_describes_kind, native_process_describes_kind,
    native_process_describes_known_agent,
};
pub use opencode::OpenCodeServerAuth;
pub use spawn_identity::{resolve_spawn_identity, SpawnIdentity};

pub use env_policy::{
    claude_config_preferred_path, launch_environment, sanitized_node_options, sanitized_value,
    selected_environment, ClaudeConfigContext, ESSENTIAL_WINDOWS_ENV_KEYS,
    HERMES_AGENT_ENVIRONMENT_KEYS, SAFE_ENVIRONMENT_KEYS,
};
pub use version::{
    opencode_version_supports_fork, SemanticVersion, MINIMUM_OPENCODE_FORK_VERSION,
};

pub use feed_event::{HookEventName, WorkstreamEvent};
pub use hook_payload::{
    carried_context, context_for, decode_event, default_title, make_item, next_context,
    parse_questions, TitleProvider, WorkstreamDecision, WorkstreamExitPlanMode, WorkstreamItem,
    WorkstreamKind, WorkstreamPayload, WorkstreamPermissionMode, WorkstreamQuestionOption,
    WorkstreamQuestionPrompt, WorkstreamStatus, WorkstreamTaskTodo, WorkstreamTaskTodoState,
};
pub use prompt_extraction::conversation_message_preview;
pub use workstream_context::{
    WorkstreamAllowedPrompt, WorkstreamContext, WorkstreamExitPlanPreview,
};
pub use workstream_json::{json_from_string, json_to_string};
pub use workstream_source::WorkstreamSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentSessionProviderId {
    Codex,
    Claude,
    OpenCode,
}

impl AgentSessionProviderId {
    pub const ALL: [Self; 3] = [Self::Codex, Self::Claude, Self::OpenCode];

    pub const fn raw_value(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::OpenCode => "opencode",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
            Self::OpenCode => "OpenCode",
        }
    }

    pub const fn executable_name(self) -> &'static str {
        self.raw_value()
    }

    pub fn launch_arguments(self) -> Vec<String> {
        match self {
            Self::Codex => vec!["app-server".into(), "--listen".into(), "stdio://".into()],
            Self::Claude => vec![
                "-p".into(),
                "--output-format".into(),
                "stream-json".into(),
                "--input-format".into(),
                "stream-json".into(),
                "--include-partial-messages".into(),
                "--verbose".into(),
            ],
            Self::OpenCode => vec![
                "serve".into(),
                "--hostname".into(),
                "127.0.0.1".into(),
                "--port".into(),
                "0".into(),
                "--print-logs".into(),
            ],
        }
    }

    pub const fn transport_kind(self) -> &'static str {
        match self {
            Self::Codex => "stdio-jsonrpc",
            Self::Claude => "stdio-jsonl",
            Self::OpenCode => "http-loopback",
        }
    }

    pub const fn should_auto_start_session(self) -> bool {
        match self {
            Self::Codex | Self::OpenCode => true,
            Self::Claude => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionLaunchPlan {
    pub provider: AgentSessionProviderId,
    pub executable_path: PathBuf,
    pub arguments: Vec<String>,
    pub environment: BTreeMap<String, String>,
}

impl AgentSessionLaunchPlan {
    pub fn environment_with_working_directory(
        &self,
        working_directory: Option<&str>,
    ) -> BTreeMap<String, String> {
        let mut launch_environment = self.environment.clone();
        self.apply_working_directory_overrides(&mut launch_environment, working_directory);
        launch_environment
    }

    /// Inject the OpenCode loopback credentials (when missing) and the `PWD`
    /// working-directory override into an already-assembled environment map.
    /// Shared by [`environment_with_working_directory`](Self::environment_with_working_directory)
    /// and [`to_spawn_spec`](Self::to_spawn_spec) so both apply identical overrides.
    fn apply_working_directory_overrides(
        &self,
        launch_environment: &mut BTreeMap<String, String>,
        working_directory: Option<&str>,
    ) {
        if self.provider == AgentSessionProviderId::OpenCode
            && launch_environment
                .get("OPENCODE_SERVER_PASSWORD")
                .is_none_or(String::is_empty)
        {
            launch_environment
                .entry("OPENCODE_SERVER_USERNAME".into())
                .or_insert_with(|| "opencode".into());
            launch_environment.insert(
                "OPENCODE_SERVER_PASSWORD".into(),
                format!("{}-{}", Uuid::new_v4(), Uuid::new_v4()),
            );
        }

        if let Some(working_directory) = working_directory
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let normalized = PathBuf::from(working_directory);
            launch_environment.insert(
                "PWD".into(),
                normalized
                    .components()
                    .as_path()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }

    /// Convert this plan into a [`cmux_process::SpawnSpec`] ready for the Job-Object
    /// supervisor. This is the resolve→env→spawn integration seam.
    ///
    /// The launch environment is the **curated** set, not the resolver's full
    /// inherited block:
    /// * agent-config keys via the [`selected_environment`] allowlist (secrets
    ///   like `AMP_API_KEY` stay excluded — rule 7),
    /// * plus the Windows essential-system-var passthrough ([`ESSENTIAL_WINDOWS_ENV_KEYS`],
    ///   matched case-insensitively) so the child actually runs — unlike macOS,
    ///   where replacing the env with only the allowlist is fine, a Windows child
    ///   needs `SystemRoot`/`TEMP`/`PATHEXT`/… or it fails to start,
    /// * plus the rewritten `PATH` (carried as an essential key) and the OpenCode
    ///   credentials / `PWD` working-directory overrides.
    pub fn to_spawn_spec(
        &self,
        working_directory: Option<&str>,
        kind: Option<&str>,
        claude_context: &ClaudeConfigContext,
    ) -> cmux_process::SpawnSpec {
        let mut environment = env_policy::launch_environment(&self.environment, kind, claude_context);
        self.apply_working_directory_overrides(&mut environment, working_directory);

        let mut spec = cmux_process::SpawnSpec::new(self.executable_path.clone())
            .args(self.arguments.clone())
            .env(environment);
        if let Some(directory) = working_directory
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            spec = spec.current_dir(directory);
        }
        spec
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentExecutableResolverError {
    #[error("missing executable {executable_name} for {display_name}")]
    Missing {
        display_name: String,
        executable_name: String,
        searched_directories: Vec<PathBuf>,
    },
}

#[derive(Debug, Clone)]
pub struct AgentExecutableResolver {
    pub environment: BTreeMap<String, String>,
    pub bundle_resource_path: Option<PathBuf>,
    pub extra_search_directories: Vec<PathBuf>,
    pub include_standard_search_directories: bool,
    pub configured_executable_paths: BTreeMap<AgentSessionProviderId, PathBuf>,
}

impl Default for AgentExecutableResolver {
    fn default() -> Self {
        Self {
            environment: std::env::vars().collect(),
            bundle_resource_path: None,
            extra_search_directories: Vec::new(),
            include_standard_search_directories: true,
            configured_executable_paths: BTreeMap::new(),
        }
    }
}

impl AgentExecutableResolver {
    pub fn resolve(
        &self,
        provider: AgentSessionProviderId,
    ) -> Result<AgentSessionLaunchPlan, AgentExecutableResolverError> {
        let search_directories = self.resolved_search_directories();

        if let Some(configured) = self.resolved_configured_executable_path(provider) {
            return Ok(self.launch_plan(provider, configured, &search_directories));
        }

        for directory in &search_directories {
            if self.should_skip_search_directory(directory) {
                continue;
            }
            for candidate in self.candidate_paths(directory, provider.executable_name()) {
                if !candidate.exists() || candidate.is_dir() {
                    continue;
                }
                if self.is_bundled_provider_executable(&candidate)
                    || self.is_known_cmux_claude_command_shim(&candidate, provider)
                    || self.is_known_cmux_claude_wrapper(&candidate, provider)
                {
                    continue;
                }
                return Ok(self.launch_plan(provider, candidate, &search_directories));
            }
        }

        Err(AgentExecutableResolverError::Missing {
            display_name: provider.display_name().to_owned(),
            executable_name: provider.executable_name().to_owned(),
            searched_directories: search_directories,
        })
    }

    pub fn resolved_search_directories(&self) -> Vec<PathBuf> {
        let mut directories = Vec::new();
        if let Some(path_value) = self.environment.get("PATH") {
            directories.extend(
                path_value
                    .split(';')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from),
            );
        }
        directories.extend(self.extra_search_directories.iter().cloned());
        directories.extend(self.user_runtime_search_directories());

        if self.include_standard_search_directories {
            if let Some(user_profile) = self.user_profile() {
                directories.extend([
                    user_profile.join("scoop").join("shims"),
                    user_profile.join(".bun").join("bin"),
                ]);
            }
            if let Some(local_app_data) = self.environment.get("LOCALAPPDATA") {
                let local_app_data = PathBuf::from(local_app_data);
                directories.push(local_app_data.join("Microsoft").join("WinGet").join("Links"));
                // The official OpenAI Codex native installer drops `codex.exe`
                // under %LOCALAPPDATA%\OpenAI\Codex\bin and does NOT add it to
                // PATH, so a PATH-only search misses it.
                directories.push(local_app_data.join("OpenAI").join("Codex").join("bin"));
            }
            if let Some(program_data) = self.environment.get("ProgramData") {
                directories.push(PathBuf::from(program_data).join("chocolatey").join("bin"));
            }
            if let Some(app_data) = self.environment.get("APPDATA") {
                // `npm i -g` drops its shims (opencode.cmd, claude.cmd, …)
                // under %APPDATA%\npm; a GUI-inherited PATH does not always
                // carry that directory, so a PATH-only search misses them.
                directories.push(PathBuf::from(app_data).join("npm"));
                directories.push(PathBuf::from(app_data).join("nvm"));
            }
            if let Some(nvm_symlink) = self.environment.get("NVM_SYMLINK") {
                directories.push(PathBuf::from(nvm_symlink));
            }
        }

        let mut seen = std::collections::BTreeSet::new();
        directories
            .into_iter()
            .filter_map(|path| {
                let normalized = normalize_path(path);
                if normalized.as_os_str().is_empty() || !seen.insert(normalized.clone()) {
                    return None;
                }
                Some(normalized)
            })
            .collect()
    }

    fn user_runtime_search_directories(&self) -> Vec<PathBuf> {
        let mut directories = Vec::new();
        if let Some(user_profile) = self.user_profile() {
            directories.extend([
                user_profile.join(".local").join("bin"),
                user_profile.join(".bun").join("bin"),
                user_profile.join("bin"),
            ]);
        }
        if let Some(local_app_data) = self.environment.get("LOCALAPPDATA") {
            directories.push(PathBuf::from(local_app_data).join("fnm"));
        }
        if let Some(app_data) = self.environment.get("APPDATA") {
            directories.push(PathBuf::from(app_data).join("Volta").join("bin"));
        }
        directories
    }

    fn user_profile(&self) -> Option<PathBuf> {
        self.environment.get("USERPROFILE").map(PathBuf::from)
    }

    fn runtime_search_path(&self, search_directories: &[PathBuf], executable_path: &Path) -> String {
        let executable_directory = executable_path
            .parent()
            .map(|parent| normalize_path(parent.to_path_buf()))
            .unwrap_or_default();
        let mut runtime_directories: Vec<PathBuf> = search_directories
            .iter()
            .filter(|path| !self.should_skip_search_directory(path))
            .cloned()
            .collect();
        runtime_directories.retain(|path| normalize_path(path.clone()) != executable_directory);
        runtime_directories.insert(0, executable_directory);
        runtime_directories
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(";")
    }

    fn launch_plan(
        &self,
        provider: AgentSessionProviderId,
        executable_path: PathBuf,
        search_directories: &[PathBuf],
    ) -> AgentSessionLaunchPlan {
        let mut environment = self.environment.clone();
        // Windows env var names are case-insensitive; the inherited block often
        // spells it `Path`. Drop every case-variant before inserting the single
        // canonical rewritten `PATH`, so the child never sees two PATH-like vars
        // (which would resolve ambiguously).
        environment.retain(|key, _| !key.eq_ignore_ascii_case("PATH"));
        environment.insert(
            "PATH".into(),
            self.runtime_search_path(search_directories, &executable_path),
        );

        AgentSessionLaunchPlan {
            provider,
            executable_path,
            arguments: provider.launch_arguments(),
            environment,
        }
    }

    fn resolved_configured_executable_path(
        &self,
        provider: AgentSessionProviderId,
    ) -> Option<PathBuf> {
        let candidate = self.configured_executable_paths.get(&provider)?.clone();
        if !candidate.exists()
            || candidate.is_dir()
            || self.is_bundled_provider_executable(&candidate)
            || self.is_known_cmux_claude_command_shim(&candidate, provider)
            || self.is_known_cmux_claude_wrapper(&candidate, provider)
        {
            return None;
        }
        Some(normalize_path(candidate))
    }

    /// The PATHEXT-driven candidate filenames for `executable_name` in
    /// `directory`, in resolution order. Mirrors Windows command resolution:
    /// a name that already carries an extension is tried verbatim first, then
    /// each `%PATHEXT%` entry is appended in order (the milestone default
    /// `.COM;.EXE;.BAT;.CMD;.PS1` when the env var is absent), and finally the
    /// bare extensionless name as a fallback for runtime managers that ship
    /// raw scripts. Duplicates are removed while preserving first-seen order.
    fn candidate_paths(&self, directory: &Path, executable_name: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut push = |path: PathBuf, paths: &mut Vec<PathBuf>| {
            if seen.insert(path.clone()) {
                paths.push(path);
            }
        };

        let has_extension = Path::new(executable_name).extension().is_some();
        if has_extension {
            push(directory.join(executable_name), &mut paths);
        }
        for extension in self.path_extensions() {
            push(
                directory.join(format!("{executable_name}{extension}")),
                &mut paths,
            );
        }
        if !has_extension {
            push(directory.join(executable_name), &mut paths);
        }
        paths
    }

    /// Parse `%PATHEXT%` into lowercased, dot-prefixed extensions in declared
    /// order (deduped). Falls back to the milestone default set when the env
    /// var is missing or blank. Lowercasing gives a stable canonical form;
    /// Windows filename matching is case-insensitive regardless.
    fn path_extensions(&self) -> Vec<String> {
        let raw = self
            .environment
            .get("PATHEXT")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD;.PS1".to_string());

        let mut seen = std::collections::BTreeSet::new();
        raw.split(';')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|extension| {
                let lower = extension.to_ascii_lowercase();
                if lower.starts_with('.') {
                    lower
                } else {
                    format!(".{lower}")
                }
            })
            .filter(|extension| seen.insert(extension.clone()))
            .collect()
    }

    fn should_skip_search_directory(&self, directory: &Path) -> bool {
        let Some(bundle_resource_path) = self.bundle_resource_path.clone() else {
            return false;
        };
        let bundle_bin = normalize_path(bundle_resource_path.join("bin"));
        let normalized = normalize_path(directory.to_path_buf());
        normalized == bundle_bin || self.is_cmux_app_bundle_resource_bin_directory(&normalized)
    }

    fn is_known_cmux_claude_command_shim(
        &self,
        path: &Path,
        provider: AgentSessionProviderId,
    ) -> bool {
        if provider != AgentSessionProviderId::Claude {
            return false;
        }
        let candidate = normalize_path(path.to_path_buf());
        if let Some(shim_path) = self.environment.get("CMUX_CLAUDE_WRAPPER_SHIM") {
            if candidate == normalize_path(PathBuf::from(shim_path)) {
                return true;
            }
        }
        let shim_roots = [
            self.environment.get("CMUX_CLAUDE_WRAPPER_SHIM_ROOT").cloned(),
            self.environment
                .get("TMPDIR")
                .cloned()
                .map(|path| PathBuf::from(path).join("cmux-cli-shims").to_string_lossy().to_string()),
        ];
        shim_roots
            .into_iter()
            .flatten()
            .map(PathBuf::from)
            .map(normalize_path)
            .any(|root| candidate.starts_with(root))
    }

    fn is_bundled_provider_executable(&self, path: &Path) -> bool {
        let candidate = normalize_path(path.to_path_buf());
        if self.is_cmux_app_bundle_resource_bin_child(&candidate) {
            return true;
        }
        self.bundle_resource_path
            .clone()
            .map(normalize_path)
            .is_some_and(|resource_path| candidate.starts_with(resource_path))
    }

    fn is_known_cmux_claude_wrapper(
        &self,
        path: &Path,
        provider: AgentSessionProviderId,
    ) -> bool {
        if provider != AgentSessionProviderId::Claude {
            return false;
        }
        fs::read(path)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes.into_iter().take(512).collect()).ok())
            .is_some_and(|prefix| prefix.contains("cmux claude wrapper - injects hooks and session tracking"))
    }

    fn is_cmux_app_bundle_resource_bin_directory(&self, path: &Path) -> bool {
        let parts: Vec<String> = path
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_string())
            .collect();
        parts.windows(4).any(|window| {
            window[0].to_ascii_lowercase().contains("cmux")
                && window[0].ends_with(".app")
                && window[1] == "Contents"
                && window[2] == "Resources"
                && window[3] == "bin"
                && parts.len() == window.len() + parts.iter().position(|item| item == &window[0]).unwrap_or(0)
        })
    }

    fn is_cmux_app_bundle_resource_bin_child(&self, path: &Path) -> bool {
        let parts: Vec<String> = path
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_string())
            .collect();
        parts.windows(4).enumerate().any(|(index, window)| {
            window[0].to_ascii_lowercase().contains("cmux")
                && window[0].ends_with(".app")
                && window[1] == "Contents"
                && window[2] == "Resources"
                && window[3] == "bin"
                && parts.len() > index + 4
        })
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    PathBuf::from(path.to_string_lossy().replace('/', "\\"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cmux-agent-tests-{unique}"));
        fs::create_dir_all(&root).expect("temp dir");
        root
    }

    fn write_executable(path: &Path, marker: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        fs::write(path, marker).expect("write executable");
    }

    #[test]
    fn resolves_executable_from_injected_path() {
        let root = temp_dir();
        let bin = root.join("bin");
        let executable = bin.join("codex.exe");
        write_executable(&executable, "codex");

        let resolver = AgentExecutableResolver {
            environment: BTreeMap::from([
                ("PATH".into(), bin.to_string_lossy().to_string()),
                ("USERPROFILE".into(), root.to_string_lossy().to_string()),
            ]),
            bundle_resource_path: Some(root.join("Resources")),
            include_standard_search_directories: false,
            ..Default::default()
        };

        let plan = resolver.resolve(AgentSessionProviderId::Codex).expect("plan");
        assert_eq!(plan.executable_path, normalize_path(executable));
        assert_eq!(plan.arguments, AgentSessionProviderId::Codex.launch_arguments());
    }

    #[test]
    fn resolves_codex_from_openai_native_install_location() {
        // The official OpenAI Codex installer drops codex.exe under
        // %LOCALAPPDATA%\OpenAI\Codex\bin and does NOT add it to PATH.
        let root = temp_dir();
        let local_app_data = root.join("LocalAppData");
        let codex = local_app_data
            .join("OpenAI")
            .join("Codex")
            .join("bin")
            .join("codex.exe");
        write_executable(&codex, "codex");

        let resolver = AgentExecutableResolver {
            environment: BTreeMap::from([
                // PATH deliberately excludes the codex bin dir.
                ("PATH".into(), root.join("empty").to_string_lossy().to_string()),
                ("LOCALAPPDATA".into(), local_app_data.to_string_lossy().to_string()),
                ("USERPROFILE".into(), root.to_string_lossy().to_string()),
            ]),
            include_standard_search_directories: true,
            ..Default::default()
        };

        let plan = resolver.resolve(AgentSessionProviderId::Codex).expect("plan");
        assert_eq!(plan.executable_path, normalize_path(codex));
    }

    #[test]
    fn resolves_opencode_from_npm_global_bin_off_path() {
        // `npm i -g opencode-ai` drops opencode.cmd under %APPDATA%\npm; a
        // GUI-inherited PATH does not always carry that directory.
        let root = temp_dir();
        let app_data = root.join("Roaming");
        let opencode = app_data.join("npm").join("opencode.cmd");
        write_executable(&opencode, "opencode");

        let resolver = AgentExecutableResolver {
            environment: BTreeMap::from([
                // PATH deliberately excludes the npm global bin dir.
                ("PATH".into(), root.join("empty").to_string_lossy().to_string()),
                ("APPDATA".into(), app_data.to_string_lossy().to_string()),
                ("USERPROFILE".into(), root.to_string_lossy().to_string()),
            ]),
            include_standard_search_directories: true,
            ..Default::default()
        };

        let plan = resolver.resolve(AgentSessionProviderId::OpenCode).expect("plan");
        assert_eq!(plan.executable_path, normalize_path(opencode));
    }

    #[test]
    fn missing_executable_surfaces_context() {
        let root = temp_dir();
        let resolver = AgentExecutableResolver {
            environment: BTreeMap::from([
                ("PATH".into(), root.to_string_lossy().to_string()),
                ("USERPROFILE".into(), root.to_string_lossy().to_string()),
            ]),
            bundle_resource_path: Some(root.join("Resources")),
            include_standard_search_directories: false,
            ..Default::default()
        };

        match resolver.resolve(AgentSessionProviderId::OpenCode) {
            Err(AgentExecutableResolverError::Missing {
                display_name,
                executable_name,
                ..
            }) => {
                assert_eq!(display_name, "OpenCode");
                assert_eq!(executable_name, "opencode");
            }
            other => panic!("expected missing error, got {other:?}"),
        }
    }

    #[test]
    fn configured_provider_directory_is_prepended_to_runtime_path() {
        let root = temp_dir();
        let configured_bin = root.join("configured");
        let path_bin = root.join("path");
        let configured_claude = configured_bin.join("claude.cmd");
        let path_claude = path_bin.join("claude.cmd");
        write_executable(&configured_claude, "configured");
        write_executable(&path_claude, "path");

        let resolver = AgentExecutableResolver {
            environment: BTreeMap::from([
                ("PATH".into(), path_bin.to_string_lossy().to_string()),
                ("USERPROFILE".into(), root.to_string_lossy().to_string()),
            ]),
            bundle_resource_path: Some(root.join("Resources")),
            configured_executable_paths: BTreeMap::from([(
                AgentSessionProviderId::Claude,
                configured_claude.clone(),
            )]),
            include_standard_search_directories: false,
            ..Default::default()
        };

        let plan = resolver.resolve(AgentSessionProviderId::Claude).expect("plan");
        let runtime_path = plan.environment.get("PATH").expect("path");
        assert_eq!(plan.executable_path, normalize_path(configured_claude));
        assert_eq!(
            runtime_path.split(';').next(),
            Some(normalize_path(configured_bin).to_string_lossy().as_ref())
        );
    }

    #[test]
    fn open_code_adds_loopback_password() {
        let plan = AgentSessionLaunchPlan {
            provider: AgentSessionProviderId::OpenCode,
            executable_path: PathBuf::from("C:\\tmp\\opencode.exe"),
            arguments: AgentSessionProviderId::OpenCode.launch_arguments(),
            environment: BTreeMap::from([("PATH".into(), "C:\\bin".into())]),
        };

        let environment = plan.environment_with_working_directory(None);
        assert_eq!(
            environment.get("OPENCODE_SERVER_USERNAME").map(String::as_str),
            Some("opencode")
        );
        assert!(
            environment
                .get("OPENCODE_SERVER_PASSWORD")
                .is_some_and(|value| value.len() >= 32)
        );
    }

    #[test]
    fn auto_start_policy_matches_app_server_providers() {
        assert!(AgentSessionProviderId::Codex.should_auto_start_session());
        assert!(AgentSessionProviderId::OpenCode.should_auto_start_session());
        assert!(!AgentSessionProviderId::Claude.should_auto_start_session());
    }

    #[test]
    fn resolves_powershell_shim_via_default_pathext() {
        // A provider shipped only as a .ps1 shim must resolve — .ps1 is in the
        // milestone-default PATHEXT set but was missing from the old hardcoded
        // [.exe, .cmd, .bat] list.
        let root = temp_dir();
        let bin = root.join("bin");
        let executable = bin.join("opencode.ps1");
        write_executable(&executable, "opencode");

        let resolver = AgentExecutableResolver {
            environment: BTreeMap::from([
                ("PATH".into(), bin.to_string_lossy().to_string()),
                ("USERPROFILE".into(), root.to_string_lossy().to_string()),
            ]),
            bundle_resource_path: Some(root.join("Resources")),
            include_standard_search_directories: false,
            ..Default::default()
        };

        let plan = resolver.resolve(AgentSessionProviderId::OpenCode).expect("plan");
        assert_eq!(plan.executable_path, normalize_path(executable));
    }

    #[test]
    fn pathext_order_decides_cmd_shadowing_exe() {
        // With a custom PATHEXT putting .CMD before .EXE, the .cmd shadows the
        // .exe in the same directory — proving resolution follows PATHEXT order,
        // not a hardcoded list (WS5 ".cmd shadowing .exe" case).
        let root = temp_dir();
        let bin = root.join("bin");
        write_executable(&bin.join("codex.exe"), "exe");
        write_executable(&bin.join("codex.cmd"), "cmd");

        let resolver = AgentExecutableResolver {
            environment: BTreeMap::from([
                ("PATH".into(), bin.to_string_lossy().to_string()),
                ("PATHEXT".into(), ".CMD;.EXE;.BAT".into()),
                ("USERPROFILE".into(), root.to_string_lossy().to_string()),
            ]),
            bundle_resource_path: Some(root.join("Resources")),
            include_standard_search_directories: false,
            ..Default::default()
        };

        let plan = resolver.resolve(AgentSessionProviderId::Codex).expect("plan");
        assert_eq!(plan.executable_path, normalize_path(bin.join("codex.cmd")));
    }

    #[test]
    fn path_extensions_default_includes_ps1_and_com_in_order() {
        let resolver = AgentExecutableResolver {
            environment: BTreeMap::new(),
            ..Default::default()
        };
        assert_eq!(
            resolver.path_extensions(),
            vec![".com", ".exe", ".bat", ".cmd", ".ps1"]
        );
    }

    #[test]
    fn path_extensions_honors_env_order_and_dedupes() {
        let resolver = AgentExecutableResolver {
            environment: BTreeMap::from([("PATHEXT".into(), ".PS1; EXE ;.ps1;.CMD".into())]),
            ..Default::default()
        };
        // Lowercased, dot-prefixed, declared order, first-seen dedupe.
        assert_eq!(
            resolver.path_extensions(),
            vec![".ps1", ".exe", ".cmd"]
        );
    }

    #[test]
    fn to_spawn_spec_curates_env_sets_program_args_cwd() {
        let plan = AgentSessionLaunchPlan {
            provider: AgentSessionProviderId::Claude,
            executable_path: PathBuf::from("C:\\rt\\claude.cmd"),
            arguments: AgentSessionProviderId::Claude.launch_arguments(),
            environment: BTreeMap::from([
                ("PATH".into(), "C:\\rt;C:\\Windows\\System32".into()),
                ("SystemRoot".into(), "C:\\Windows".into()),
                ("AMP_API_KEY".into(), "secret".into()),
                ("ANTHROPIC_MODEL".into(), "claude-opus-4-8".into()),
            ]),
        };

        let spec = plan.to_spawn_spec(Some("C:\\work\\proj"), None, &ClaudeConfigContext::inert());

        assert_eq!(spec.program, PathBuf::from("C:\\rt\\claude.cmd"));
        assert_eq!(spec.args, AgentSessionProviderId::Claude.launch_arguments());
        assert_eq!(spec.current_dir, Some(PathBuf::from("C:\\work\\proj")));
        // Curated env: rewritten PATH + system var + allowlisted config kept.
        assert_eq!(spec.env.get("PATH").map(String::as_str), Some("C:\\rt;C:\\Windows\\System32"));
        assert_eq!(spec.env.get("SystemRoot").map(String::as_str), Some("C:\\Windows"));
        assert_eq!(spec.env.get("ANTHROPIC_MODEL").map(String::as_str), Some("claude-opus-4-8"));
        // Secret excluded; PWD override applied (forward-slash normalized).
        assert!(!spec.env.contains_key("AMP_API_KEY"), "secret must not cross into launch env");
        assert_eq!(spec.env.get("PWD").map(String::as_str), Some("C:/work/proj"));
    }

    #[test]
    fn to_spawn_spec_mints_opencode_credentials() {
        let plan = AgentSessionLaunchPlan {
            provider: AgentSessionProviderId::OpenCode,
            executable_path: PathBuf::from("C:\\rt\\opencode.exe"),
            arguments: AgentSessionProviderId::OpenCode.launch_arguments(),
            environment: BTreeMap::from([("PATH".into(), "C:\\rt".into())]),
        };

        let spec = plan.to_spawn_spec(None, None, &ClaudeConfigContext::inert());

        assert_eq!(
            spec.env.get("OPENCODE_SERVER_USERNAME").map(String::as_str),
            Some("opencode")
        );
        assert!(spec
            .env
            .get("OPENCODE_SERVER_PASSWORD")
            .is_some_and(|value| value.len() >= 32));
        assert_eq!(spec.current_dir, None);
        assert!(!spec.env.contains_key("PWD"), "no working dir → no PWD");
    }
}
