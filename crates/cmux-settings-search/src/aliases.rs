//! `SettingsSearchAliasIndex` — the alias keyword tables used to widen search
//! matches for settings sections and individual setting rows.
//!
//! Faithful 1:1 port of `enum SettingsSearchAliasIndex` from
//! `Sources/SettingsSearchAliases.swift` (lines 3-177).
//!
//! Divergences (sanctioned platform swaps):
//! - `localized(_:defaultValue:)` (Swift line 174) collapses to the provided
//!   English `defaultValue` verbatim — there is no runtime localization
//!   catalogue available headless.
//! - `keyboardShortcutActionAliases` (Swift line 170) joins the labels of
//!   `KeyboardShortcutSettings.settingsVisibleActions`. That type lives in the
//!   AppKit settings UI and is not available headless, so
//!   [`keyboard_shortcut_action_aliases`] returns `""`. The structural append in
//!   [`SettingsSearchAliasIndex::aliases`] is preserved exactly (so the
//!   `keyboardShortcuts:shortcuts` result gains a trailing space, matching the
//!   Swift `"\(aliases) \(keyboardShortcutActionAliases)"` shape when the action
//!   list is empty).

use crate::target::SettingsNavigationTarget;

/// Namespace mirroring Swift's `enum SettingsSearchAliasIndex`.
pub struct SettingsSearchAliasIndex;

impl SettingsSearchAliasIndex {
    /// `sectionAliases(for:)` (Swift lines 4-39). English `defaultValue`
    /// verbatim for each section.
    pub fn section_aliases(target: SettingsNavigationTarget) -> &'static str {
        match target {
            SettingsNavigationTarget::Account => {
                "auth authentication login logout sign in sign out email user profile team"
            }
            SettingsNavigationTarget::App => {
                "general preferences prefs behavior chrome dock menubar menu bar status notifications telemetry"
            }
            SettingsNavigationTarget::Terminal => {
                "shell scrollback scrollbar scroll bar ghostty tty pty"
            }
            SettingsNavigationTarget::TextBox => {
                "textbox text box rich input prompt beta focus composer compose attachments"
            }
            SettingsNavigationTarget::Mobile => {
                "ios iphone ipad mobile pairing local network permission sync"
            }
            SettingsNavigationTarget::SidebarAppearance => {
                "sidebar left rail navigation details branches badges material terminal background"
            }
            SettingsNavigationTarget::CustomSidebars => {
                "custom sidebars vibe code swift json interpreted renderer in-process remote worker isolated"
            }
            SettingsNavigationTarget::BetaFeatures => {
                "beta experimental unstable preview feed dock right sidebar"
            }
            SettingsNavigationTarget::Automation => {
                "api cli control socket mcp agents hooks ports"
            }
            SettingsNavigationTarget::Browser => {
                "web webview address bar omnibar links urls embedded default browser"
            }
            SettingsNavigationTarget::BrowserImport => {
                "chrome safari firefox brave edge arc bookmarks history cookies profiles"
            }
            SettingsNavigationTarget::GlobalHotkey => {
                "system shortcut global keyboard show hide bring forward"
            }
            SettingsNavigationTarget::KeyboardShortcuts => {
                "keybinds key bindings hotkeys chords accelerators commands"
            }
            SettingsNavigationTarget::WorkspaceColors => {
                "tab colors palette accent badge selected highlight"
            }
            SettingsNavigationTarget::SettingsJson => {
                "configuration config file json jsonc dotfile ~/.config schema docs"
            }
            SettingsNavigationTarget::Reset => "factory defaults restore clear preferences",
        }
    }

    /// `aliases(target:idSuffix:)` (Swift lines 41-47). Looks up the
    /// `"\(rawValue):\(idSuffix)"` key in the `settingAliases` table (missing key
    /// yields `""`, matching `?? ""`), and appends the keyboard-shortcut action
    /// labels for the `keyboardShortcuts:shortcuts` row.
    pub fn aliases(target: SettingsNavigationTarget, id_suffix: &str) -> String {
        let key = format!("{}:{}", target.raw_value(), id_suffix);
        let aliases = setting_aliases(&key).unwrap_or("");
        if target == SettingsNavigationTarget::KeyboardShortcuts && id_suffix == "shortcuts" {
            return format!("{} {}", aliases, keyboard_shortcut_action_aliases());
        }
        aliases.to_string()
    }
}

/// `keyboardShortcutActionAliases` (Swift line 170). See module divergence note:
/// `KeyboardShortcutSettings.settingsVisibleActions` is unavailable headless, so
/// this yields the empty join.
fn keyboard_shortcut_action_aliases() -> &'static str {
    ""
}

/// `settingAliases` (Swift lines 49-168). The `[String: String]` map, ported as
/// an exact key -> value lookup. Returns `None` for absent keys (Swift's
/// subscript returns `nil`, handled with `?? ""` by the caller). Every
/// `defaultValue` string is transcribed verbatim.
pub fn setting_aliases(key: &str) -> Option<&'static str> {
    let value = match key {
        "account:account" => {
            "auth authentication login logout signin sign-in signout sign-out email user profile stack team"
        }
        "app:language" => {
            "app.language locale l10n localization translation japanese english ja en nihongo restart"
        }
        "app:appearance" => {
            "app.appearance theme color scheme light mode dark mode system mode"
        }
        "app:app-icon" => {
            "app.appIcon dock icon application icon app switcher alternate icon"
        }
        "app:default-terminal" => {
            "app.defaultTerminal default terminal ssh links command tool unix executable launch services handler"
        }
        "app:new-workspace-placement" => {
            "app.newWorkspacePlacement new tab insert position order top bottom end"
        }
        "app:workspace-group-new-workspace-placement" => {
            "workspaceGroups.newWorkspacePlacement group new workspace command n cmd-n plus insert position after current top end"
        }
        "app:fork-conversation-default" => {
            "app.forkConversationDefaultDestination fork conversation right left top bottom split tab workspace default"
        }
        "app:workspace-inherit-working-directory" => {
            "app.workspaceInheritWorkingDirectory workspace cwd directory inherit current focused ghostty working-directory"
        }
        "app:minimal-mode" => {
            "app.minimalMode minimal layout simple chrome compact titlebar controls"
        }
        "app:keep-workspace-open" => {
            "app.keepWorkspaceOpenWhenClosingLastSurface close last pane surface keep tab workspace"
        }
        "app:focus-pane-first-click" => {
            "app.focusPaneOnFirstClick click to focus focus follows mouse first click mouse activation"
        }
        "app:preferred-editor" => {
            "app.preferredEditor editor open file code vscode visual studio zed sublime subl cursor"
        }
        "app:supported-file-previews" => {
            "app.openSupportedFilesInCmux cmd click file preview pdf image video audio quicklook quick look editor external"
        }
        "app:terminal-config" => {
            "ghostty config configuration terminal settings preview merged file reload macos-option-as-alt option as alt left option right option alt key meta"
        }
        "app:markdown-viewer" => {
            "app.openMarkdownInCmuxViewer md markdown mdx viewer preview readme"
        }
        "app:markdown-font-size" => {
            "markdown.fontSize md markdown viewer font size points zoom scale text bigger smaller larger default"
        }
        "app:markdown-font-family" => {
            "markdown.fontFamily md markdown viewer font font-family family typeface system stack custom"
        }
        "app:markdown-max-width" => {
            "markdown.maxWidth md markdown viewer max width column reading line length pixels px narrow wide"
        }
        "app:file-editor-word-wrap" => {
            "fileEditor.wordWrap file editor word wrap soft wrap reflow lines text horizontal scroll preview"
        }
        "app:imessage-mode" => {
            "app.iMessageMode imessage message messages chat prompt prompts submitted message texting reorder move workspace top agent send"
        }
        "app:reorder-notification" => {
            "app.reorderOnNotification notification reorder move workspace top unread sort"
        }
        "app:dock-badge" => {
            "notifications.dockBadge badge dock unread count icon notifications red bubble"
        }
        "app:menu-bar-only" => {
            "app.menuBarOnly menubar menu bar dockless hide dock app switcher cmd-tab command-tab"
        }
        "app:show-menu-bar" => {
            "notifications.showInMenuBar menubar menu bar status item tray extra"
        }
        "app:unread-pane-ring" => {
            "notifications.unreadPaneRing blue border unread ring notification pane outline"
        }
        "app:pane-flash" => {
            "notifications.paneFlash flash blink highlight pane notification pulse"
        }
        "app:desktop-notifications" => {
            "macos desktop notifications system settings permission alerts notify test"
        }
        "app:notification-sound" => {
            "notifications.sound notifications.customSoundFilePath sound audio alert chime beep custom file wav mp3 caf aiff"
        }
        "app:notification-command" => {
            "notifications.command shell command hook script env environment variable variables done agent"
        }
        "app:telemetry" => {
            "app.sendAnonymousTelemetry analytics crash reports sentry posthog usage anonymous privacy"
        }
        "app:warn-before-quit" => {
            "app.warnBeforeQuit quit confirmation command-q cmd-q exit close app"
        }
        "app:warn-before-closing-tab" => {
            "app.warnBeforeClosingTab close tab confirmation command-w cmd-w terminal surface"
        }
        "app:warn-before-closing-tab-x-button" => {
            "app.warnBeforeClosingTabXButton close tab x button confirmation terminal surface"
        }
        "app:hide-tab-close-button" => {
            "app.hideTabCloseButton hide close tab x button terminal surface"
        }
        "app:rename-selects-name" => {
            "app.renameSelectsExistingName rename select all existing title command palette workspace name"
        }
        "app:palette-search-all" => {
            "app.commandPaletteSearchesAllSurfaces command palette search all surfaces cmd-p terminal browser markdown"
        }
        "app:canvas-pane-gap" => {
            "canvas.paneGap canvas pane gap spacing freeform layout panes snapping tidy distribute align"
        }
        "app:canvas-snapping" => {
            "canvas.snappingEnabled canvas snap snapping enabled edges drag resize align panes freeform layout"
        }
        "terminal:scrollbar" => {
            "terminal.showScrollBar scrollback scrollbar scroll bar right edge alternate screen tui"
        }
        "terminal:copy-on-select" => {
            "terminal.copyOnSelect copy on selection select clipboard mouse double click triple click iterm"
        }
        "terminal:tab-bar-font-size" => {
            "surface-tab-bar-font-size tab bar font size text scale terminal browser pane tab title"
        }
        "terminal:resume-commands" => {
            "surface resume commands approvals command prefixes auto restore ask manual tmux hibernation sticky process"
        }
        "textBox:show-textbox-new-terminals" => {
            "terminal.showTextBoxOnNewTerminals show textbox text box rich input prompt default new terminal workspace split tab beta"
        }
        "textBox:focus-textbox-new-terminals" => {
            "terminal.focusTextBoxOnNewTerminals focus textbox text box rich input prompt default new terminal workspace split tab beta"
        }
        "textBox:textbox-max-lines" => {
            "terminal.textBoxMaxLines textbox text box rich input prompt max height lines grow scroll beta"
        }
        "sidebarAppearance:match-terminal" => {
            "sidebarAppearance.matchTerminalBackground transparent background material terminal background sync"
        }
        "sidebarAppearance:font-size" => {
            "sidebar-font-size sidebar font size text scale workspace title badge metadata shortcut hint"
        }
        "sidebarAppearance:hide-sidebar-details" => {
            "sidebar.hideAllDetails compact sidebar hide details only title minimal left rail"
        }
        "sidebarAppearance:wrap-workspace-titles" => {
            "sidebar.wrapWorkspaceTitles workspace title wrap multiline pr pull request"
        }
        "sidebarAppearance:show-workspace-description" => {
            "sidebar.showWorkspaceDescription workspace description notes markdown sidebar"
        }
        "sidebarAppearance:sidebar-branch-layout" => {
            "sidebar.branchLayout git branch layout vertical inline cwd directory"
        }
        "sidebarAppearance:stack-branch-directory" => {
            "sidebar.stackBranchDirectory git branch directory cwd path stack stacked separate lines two rows"
        }
        "sidebarAppearance:path-last-segment-only" => {
            "sidebar.pathLastSegmentOnly cwd path directory last segment basename short truncate folder repo"
        }
        "sidebarAppearance:show-notification-message" => {
            "sidebar.showNotificationMessage latest message unread notification text sidebar"
        }
        "sidebarAppearance:show-branch-directory" => {
            "sidebar.showBranchDirectory git branch cwd path directory folder repo sidebar"
        }
        "sidebarAppearance:show-pull-requests" => {
            "sidebar.showPullRequests pr mr review github gitlab bitbucket pull request merge request"
        }
        "sidebarAppearance:watch-git-status" => {
            "sidebar.watchGitStatus git status branch watcher index lock"
        }
        "sidebarAppearance:make-pr-clickable" => {
            "sidebar.makePullRequestsClickable clickable pull requests pr mr reviews links select workspace row"
        }
        "sidebarAppearance:open-pr-links" => {
            "sidebar.openPullRequestLinksInCmuxBrowser pr links github browser default external embedded"
        }
        "sidebarAppearance:open-port-links" => {
            "sidebar.openPortLinksInCmuxBrowser ports localhost links browser default external embedded"
        }
        "sidebarAppearance:show-ssh" => "sidebar.showSSH remote host target ssh server",
        "sidebarAppearance:show-ports" => {
            "sidebar.showPorts localhost port listener dev server url"
        }
        "sidebarAppearance:show-log" => "sidebar.showLog log status latest message imperative",
        "sidebarAppearance:show-progress" => {
            "sidebar.showProgress progress bar percent status set_progress"
        }
        "sidebarAppearance:show-metadata" => {
            "sidebar.showCustomMetadata metadata meta report_meta status custom block"
        }
        "sidebarAppearance:right-max-width" => {
            "sidebar.rightMaxWidth dock right sidebar max width terminal reservation cap logs lazygit"
        }
        "betaFeatures:feed" => {
            "feed right sidebar agent decisions permissions questions approval beta unstable"
        }
        "betaFeatures:dock" => "dock right sidebar terminal controls tui beta unstable",
        "mobile:iOSPairingHost" => {
            "ios iphone ipad mobile pairing local network permission sync"
        }
        "mobile:iOSPairingPort" => {
            "mobile ios iphone pairing port tcp listener firewall conflict bind"
        }
        "mobile:iOSPairingDisplayName" => {
            "mobile ios iphone pairing display name mac hostname device label"
        }
        "automation:socket-mode" => {
            "automation.socketControlMode api socket unix domain control server auth allow password disabled"
        }
        "automation:socket-password" => {
            "automation.socketPassword auth token credential secret password access key"
        }
        "automation:claude-code" => {
            "automation.claudeCodeIntegration claude code hooks agent integration status notifications"
        }
        "automation:claude-path" => {
            "automation.claudeBinaryPath claude binary executable path cli command custom"
        }
        "automation:workspace-auto-naming" => {
            "automation.workspaceAutoNaming automation.autoNamingAgent ai auto naming auto-name auto name workspace tab workspaces tabs title titles rename workspace rename tab renaming generated name summarize summary summarizer conversation agent picker naming agent"
        }
        "automation:ripgrep-path" => {
            "automation.ripgrepBinaryPath ripgrep rg binary executable path search find nix custom"
        }
        "automation:subagent-notifications" => {
            "automation.suppressSubagentNotifications subagent nested child agent codex claude hooks notifications"
        }
        "automation:cursor" => {
            "automation.cursorIntegration cursor ide agent hooks notifications"
        }
        "automation:gemini" => {
            "automation.geminiIntegration gemini cli google agent hooks notifications"
        }
        "automation:kiro" => {
            "automation.kiroIntegration kiro cli amazon q agent hooks notifications"
        }
        "automation:kiro-notification-level" => {
            "automation.kiroNotificationLevel kiro cli notification verbosity minimal standard verbose tool events"
        }
        "automation:port-base" => {
            "automation.portBase cmux_port start first base env environment variable"
        }
        "automation:port-range" => {
            "automation.portRange cmux_port_end range size count env ports"
        }
        "browser:enable-browser" => {
            "browser.enabled enable disable webview embedded browser tabs links"
        }
        "browser:search-engine" => {
            "browser.defaultSearchEngine browser.customSearchEngineName browser.customSearchEngineURLTemplate omnibar address bar google duckduckgo bing kagi brave startpage perplexity exa yahoo ecosia qwant mojeek wikipedia github baidu yandex custom search provider"
        }
        "browser:search-suggestions" => {
            "browser.showSearchSuggestions suggest autocomplete address bar search suggestions"
        }
        "browser:theme" => "browser.theme web page theme color scheme light dark system",
        "browser:hidden-webview-discard" => {
            "browser.discardHiddenWebViews memory hidden tabs webview discard unload reclaim"
        }
        "browser:hidden-webview-discard-delay" => {
            "browser.hiddenWebViewDiscardDelaySeconds memory hidden tabs delay seconds discard unload"
        }
        "browser:terminal-links" => {
            "browser.openTerminalLinksInCmuxBrowser click url terminal links open in browser href"
        }
        "browser:intercept-open" => {
            "browser.interceptTerminalOpenCommandInCmuxBrowser open command http https url terminal intercept"
        }
        "browser:host-whitelist" => {
            "browser.hostsToOpenInEmbeddedBrowser allowlist whitelist host wildcard domain embedded browser"
        }
        "browser:external-patterns" => {
            "browser.urlsToAlwaysOpenExternally denylist blocklist regex rules external default browser"
        }
        "browser:http-allowlist" => {
            "browser.insecureHttpHostsAllowedInEmbeddedBrowser insecure http allowlist localhost localtest non-https warning"
        }
        "browserImport:import-data" => {
            "chrome safari firefox brave edge arc bookmarks history cookies profiles migration"
        }
        "browserImport:import-hint" => {
            "browser.showImportHintOnBlankTabs blank tab onboarding hint import prompt dismiss"
        }
        "browser:react-grab" => {
            "browser.reactGrabVersion react grab npm version toolbar cmd-shift-g inspect component"
        }
        "browser:history" => "clear browser history visited pages suggestions omnibar",
        "globalHotkey:enable-hotkey" => {
            "global hotkey enable system wide show hide all windows"
        }
        "globalHotkey:shortcut" => {
            "global hotkey shortcut recorder key command option control"
        }
        "keyboardShortcuts:shortcut-chords" => {
            "tmux prefix ctrl-b control-b multi key sequence chord cmux json"
        }
        "keyboardShortcuts:reset-defaults" => {
            "reset restore default defaults built in builtin shortcuts hotkeys keybindings commands"
        }
        "keyboardShortcuts:shortcuts" => {
            "hotkeys keybindings key bindings commands keyboard accelerators shortcuts cmux json"
        }
        "workspaceColors:indicator" => {
            "workspaceColors.indicatorStyle tab indicator active workspace style color stripe dot"
        }
        "workspaceColors:selection" => {
            "workspaceColors.selectionColor selected workspace color highlight background active tab"
        }
        "workspaceColors:badge" => {
            "workspaceColors.notificationBadgeColor unread notification badge color dot count"
        }
        "workspaceColors:palette" => {
            "workspaceColors.colors workspace palette named colors custom color reset built-in"
        }
        "settingsJSON:open-file" => {
            "open config file json jsonc config editor ~/.config cmux preferences"
        }
        "settingsJSON:documentation" => {
            "docs documentation schema reference cmux json keys configuration"
        }
        "reset:reset-all" => "factory reset restore defaults clear preferences",
        _ => return None,
    };
    Some(value)
}
