//! `SettingsNavigationTarget` — the top-level settings sections a search entry
//! can navigate to.
//!
//! Faithful 1:1 port of `enum SettingsNavigationTarget` from
//! `Sources/SettingsNavigation.swift` (lines 3-133). The Swift enum is a
//! `String`-backed `CaseIterable`; the `rawValue` is the case name verbatim
//! (camelCase preserved, e.g. `textBox`, `sidebarAppearance`, `settingsJSON`).
//!
//! Divergence (sanctioned platform swap): every user-facing string in Swift is
//! produced by `String(localized:defaultValue:)`. There is no runtime
//! localization catalogue available headless, so these methods return the
//! English `defaultValue` verbatim — matching the base (English) locale.

/// The destination category for a search result. Mirrors Swift's
/// `SettingsNavigationTarget`. Variant order matches the Swift `case` order
/// (which is also its `CaseIterable.allCases` order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsNavigationTarget {
    Account,
    App,
    Terminal,
    TextBox,
    Mobile,
    SidebarAppearance,
    CustomSidebars,
    BetaFeatures,
    Automation,
    Browser,
    BrowserImport,
    GlobalHotkey,
    KeyboardShortcuts,
    WorkspaceColors,
    SettingsJson,
    Reset,
}

impl SettingsNavigationTarget {
    /// `SettingsNavigationTarget.allCases` order.
    pub const ALL_CASES: [SettingsNavigationTarget; 16] = [
        SettingsNavigationTarget::Account,
        SettingsNavigationTarget::App,
        SettingsNavigationTarget::Terminal,
        SettingsNavigationTarget::TextBox,
        SettingsNavigationTarget::Mobile,
        SettingsNavigationTarget::SidebarAppearance,
        SettingsNavigationTarget::CustomSidebars,
        SettingsNavigationTarget::BetaFeatures,
        SettingsNavigationTarget::Automation,
        SettingsNavigationTarget::Browser,
        SettingsNavigationTarget::BrowserImport,
        SettingsNavigationTarget::GlobalHotkey,
        SettingsNavigationTarget::KeyboardShortcuts,
        SettingsNavigationTarget::WorkspaceColors,
        SettingsNavigationTarget::SettingsJson,
        SettingsNavigationTarget::Reset,
    ];

    /// The Swift `String` raw value (the case name, camelCase preserved).
    pub fn raw_value(self) -> &'static str {
        match self {
            SettingsNavigationTarget::Account => "account",
            SettingsNavigationTarget::App => "app",
            SettingsNavigationTarget::Terminal => "terminal",
            SettingsNavigationTarget::TextBox => "textBox",
            SettingsNavigationTarget::Mobile => "mobile",
            SettingsNavigationTarget::SidebarAppearance => "sidebarAppearance",
            SettingsNavigationTarget::CustomSidebars => "customSidebars",
            SettingsNavigationTarget::BetaFeatures => "betaFeatures",
            SettingsNavigationTarget::Automation => "automation",
            SettingsNavigationTarget::Browser => "browser",
            SettingsNavigationTarget::BrowserImport => "browserImport",
            SettingsNavigationTarget::GlobalHotkey => "globalHotkey",
            SettingsNavigationTarget::KeyboardShortcuts => "keyboardShortcuts",
            SettingsNavigationTarget::WorkspaceColors => "workspaceColors",
            SettingsNavigationTarget::SettingsJson => "settingsJSON",
            SettingsNavigationTarget::Reset => "reset",
        }
    }

    /// Reverse of `raw_value` — mirrors `SettingsNavigationTarget(rawValue:)`.
    pub fn from_raw_value(raw: &str) -> Option<SettingsNavigationTarget> {
        SettingsNavigationTarget::ALL_CASES
            .into_iter()
            .find(|target| target.raw_value() == raw)
    }

    /// `var title` (Swift lines 23-58). English `defaultValue` verbatim.
    pub fn title(self) -> &'static str {
        match self {
            SettingsNavigationTarget::Account => "Account",
            SettingsNavigationTarget::App => "App",
            SettingsNavigationTarget::Terminal => "Terminal",
            SettingsNavigationTarget::TextBox => "TextBox (Beta)",
            SettingsNavigationTarget::Mobile => "Mobile",
            SettingsNavigationTarget::WorkspaceColors => "Workspace Colors",
            SettingsNavigationTarget::SidebarAppearance => "Sidebar",
            SettingsNavigationTarget::CustomSidebars => "Custom Sidebars",
            SettingsNavigationTarget::BetaFeatures => "Beta Features",
            SettingsNavigationTarget::Automation => "Automation",
            SettingsNavigationTarget::Browser => "Browser",
            SettingsNavigationTarget::BrowserImport => "Import Browser Data",
            SettingsNavigationTarget::GlobalHotkey => "Global Hotkey",
            SettingsNavigationTarget::KeyboardShortcuts => "Keyboard Shortcuts",
            SettingsNavigationTarget::SettingsJson => "cmux.json",
            SettingsNavigationTarget::Reset => "Reset",
        }
    }

    /// `var symbolName` (Swift lines 60-95). SF Symbol name verbatim.
    pub fn symbol_name(self) -> &'static str {
        match self {
            SettingsNavigationTarget::Account => "person.crop.circle",
            SettingsNavigationTarget::App => "gearshape",
            SettingsNavigationTarget::Terminal => "terminal",
            SettingsNavigationTarget::TextBox => "textformat",
            SettingsNavigationTarget::Mobile => "iphone",
            SettingsNavigationTarget::WorkspaceColors => "paintpalette",
            SettingsNavigationTarget::SidebarAppearance => "sidebar.left",
            SettingsNavigationTarget::CustomSidebars => "sidebar.squares.left",
            SettingsNavigationTarget::BetaFeatures => "exclamationmark.triangle",
            SettingsNavigationTarget::Automation => "wand.and.sparkles",
            SettingsNavigationTarget::Browser => "globe",
            SettingsNavigationTarget::BrowserImport => "square.and.arrow.down",
            SettingsNavigationTarget::GlobalHotkey => "keyboard.badge.ellipsis",
            SettingsNavigationTarget::KeyboardShortcuts => "keyboard",
            SettingsNavigationTarget::SettingsJson => "doc.text",
            SettingsNavigationTarget::Reset => "arrow.counterclockwise",
        }
    }

    /// `var searchText` (Swift lines 97-132). The Swift source interpolates the
    /// section `title` at the front (`"\(title) ..."`); this reproduces that
    /// exactly by prepending `title()`.
    pub fn search_text(self) -> String {
        let title = self.title();
        let tail = match self {
            SettingsNavigationTarget::Account => "sign in team sync",
            SettingsNavigationTarget::App => {
                "appearance language workspace notifications menu bar telemetry default terminal"
            }
            SettingsNavigationTarget::Terminal => {
                "scrollbar auto resume restore reopen relaunch quit sessions agents claude codex opencode rovodev hibernation idle suspend commands approvals prefixes toggle"
            }
            SettingsNavigationTarget::TextBox => {
                "textbox text box rich input prompt beta new terminal workspace split tab focus height"
            }
            SettingsNavigationTarget::Mobile => "ios iphone ipad mobile pairing local network sync",
            SettingsNavigationTarget::WorkspaceColors => "palette tabs",
            SettingsNavigationTarget::SidebarAppearance => {
                "sidebar details branches badges material terminal background"
            }
            SettingsNavigationTarget::CustomSidebars => {
                "custom sidebars vibe swift json interpreted renderer in-process remote worker isolated"
            }
            SettingsNavigationTarget::BetaFeatures => {
                "beta experimental unstable feed dock right sidebar"
            }
            SettingsNavigationTarget::Automation => {
                "socket integrations hooks ports claude cursor gemini kiro naming auto naming workspace tabs"
            }
            SettingsNavigationTarget::Browser => "search engine links history theme",
            SettingsNavigationTarget::BrowserImport => {
                "browser import data bookmarks history cookies"
            }
            SettingsNavigationTarget::GlobalHotkey => "system wide shortcut",
            SettingsNavigationTarget::KeyboardShortcuts => "keybindings commands chords",
            SettingsNavigationTarget::SettingsJson => {
                "config file preferences editor documentation schema jsonc reload"
            }
            SettingsNavigationTarget::Reset => "defaults",
        };
        format!("{title} {tail}")
    }
}
