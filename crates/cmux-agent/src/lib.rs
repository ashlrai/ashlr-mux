use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use uuid::Uuid;

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

        launch_environment
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
                directories.push(PathBuf::from(local_app_data).join("Microsoft").join("WinGet").join("Links"));
            }
            if let Some(program_data) = self.environment.get("ProgramData") {
                directories.push(PathBuf::from(program_data).join("chocolatey").join("bin"));
            }
            if let Some(app_data) = self.environment.get("APPDATA") {
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

    fn candidate_paths(&self, directory: &Path, executable_name: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let raw = directory.join(executable_name);
        paths.push(raw.clone());
        for extension in [".exe", ".cmd", ".bat"] {
            paths.push(directory.join(format!("{executable_name}{extension}")));
        }
        paths
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
}
