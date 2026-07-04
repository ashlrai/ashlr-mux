//! Conversation context attached to feed events, plus the ExitPlanMode preview
//! parser.
//!
//! Swift parity source:
//! `Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/Workstream/WorkstreamContext.swift:1-212`.
//!
//! SANCTIONED DIVERGENCE (whitespace trimming): Swift trims with Foundation's
//! `.whitespacesAndNewlines` (Unicode general-category `Zs` + TAB + the newline
//! family U+000A–U+000D, U+0085, U+2028, U+2029). Rust's [`str::trim`] trims the
//! Unicode `White_Space` property, which covers the same members (and a superset
//! in the separator range), so the two agree on all realistic inputs. This
//! matches the convention already established in
//! `crates/cmux-agent-hook-config/src/common.rs`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Foundation `trimmingCharacters(in: .whitespacesAndNewlines)` then map empty to
/// nil — Swift `WorkstreamContext.cleaned(_:)` (`WorkstreamContext.swift:82-86`).
fn cleaned(value: Option<&str>) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// One allowed follow-up prompt Claude attaches to an ExitPlanMode plan.
///
/// Swift `WorkstreamAllowedPrompt` (`WorkstreamContext.swift:89-97`). Note the
/// Swift memberwise `init(tool:prompt:)` trims both fields, but the *synthesized*
/// `Codable` conformance decodes them verbatim (no trim). We mirror that split:
/// [`WorkstreamAllowedPrompt::new`] trims; `Deserialize` does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkstreamAllowedPrompt {
    pub tool: String,
    pub prompt: String,
}

impl WorkstreamAllowedPrompt {
    /// Trims both fields — Swift `init(tool:prompt:)` (`WorkstreamContext.swift:93-96`).
    pub fn new(tool: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            tool: tool.into().trim().to_string(),
            prompt: prompt.into().trim().to_string(),
        }
    }
}

/// Extra nearby conversation state attached to a feed event.
///
/// Swift `WorkstreamContext` (`WorkstreamContext.swift:8-87`).
///
/// The stored form is always "cleaned": string fields are trimmed and mapped to
/// `None` when empty, and `allowed_prompts` drops entries whose `prompt` is
/// empty. [`WorkstreamContext::new`] and `Deserialize` both enforce this, exactly
/// like Swift's `init(...)` (which the custom `init(from:)` delegates to).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkstreamContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_user_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant_preamble: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_summary: Option<String>,
    /// Always encoded, even when empty — mirrors the non-optional Swift property
    /// whose synthesized `encode(to:)` always emits `allowedPrompts`.
    pub allowed_prompts: Vec<WorkstreamAllowedPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
}

/// Raw shape used to decode a `WorkstreamContext` before cleaning — the wire
/// fields with `decodeIfPresent` semantics (absent or `null` → `None`, and
/// `allowedPrompts` absent → `[]`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkstreamContextRaw {
    #[serde(default)]
    last_user_message: Option<String>,
    #[serde(default)]
    assistant_preamble: Option<String>,
    #[serde(default)]
    plan_summary: Option<String>,
    #[serde(default)]
    allowed_prompts: Option<Vec<WorkstreamAllowedPrompt>>,
    #[serde(default)]
    tool_summary: Option<String>,
    #[serde(default)]
    permission_mode: Option<String>,
}

impl<'de> Deserialize<'de> for WorkstreamContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Swift `init(from:)` (WorkstreamContext.swift:44-57) decodes the raw
        // fields then routes through `init(...)`, which cleans/filters.
        let raw = WorkstreamContextRaw::deserialize(deserializer)?;
        Ok(WorkstreamContext::new(
            raw.last_user_message,
            raw.assistant_preamble,
            raw.plan_summary,
            raw.allowed_prompts.unwrap_or_default(),
            raw.tool_summary,
            raw.permission_mode,
        ))
    }
}

impl WorkstreamContext {
    /// Swift `init(...)` (`WorkstreamContext.swift:19-33`): clean each string
    /// field and drop allowed prompts with an empty `prompt`.
    pub fn new(
        last_user_message: Option<String>,
        assistant_preamble: Option<String>,
        plan_summary: Option<String>,
        allowed_prompts: Vec<WorkstreamAllowedPrompt>,
        tool_summary: Option<String>,
        permission_mode: Option<String>,
    ) -> Self {
        Self {
            last_user_message: cleaned(last_user_message.as_deref()),
            assistant_preamble: cleaned(assistant_preamble.as_deref()),
            plan_summary: cleaned(plan_summary.as_deref()),
            allowed_prompts: allowed_prompts
                .into_iter()
                .filter(|p| !p.prompt.is_empty())
                .collect(),
            tool_summary: cleaned(tool_summary.as_deref()),
            permission_mode: cleaned(permission_mode.as_deref()),
        }
    }

    /// Swift `isEmpty` (`WorkstreamContext.swift:59-66`).
    pub fn is_empty(&self) -> bool {
        self.last_user_message.is_none()
            && self.assistant_preamble.is_none()
            && self.plan_summary.is_none()
            && self.allowed_prompts.is_empty()
            && self.tool_summary.is_none()
            && self.permission_mode.is_none()
    }

    /// Returns a context where non-empty values from `self` win and missing
    /// fields fall back to `fallback`.
    ///
    /// Swift `mergingMissing(from:)` (`WorkstreamContext.swift:70-80`).
    pub fn merging_missing(&self, fallback: Option<&WorkstreamContext>) -> WorkstreamContext {
        let Some(fallback) = fallback else {
            return self.clone();
        };
        WorkstreamContext::new(
            self.last_user_message
                .clone()
                .or_else(|| fallback.last_user_message.clone()),
            self.assistant_preamble
                .clone()
                .or_else(|| fallback.assistant_preamble.clone()),
            self.plan_summary
                .clone()
                .or_else(|| fallback.plan_summary.clone()),
            if self.allowed_prompts.is_empty() {
                fallback.allowed_prompts.clone()
            } else {
                self.allowed_prompts.clone()
            },
            self.tool_summary
                .clone()
                .or_else(|| fallback.tool_summary.clone()),
            self.permission_mode
                .clone()
                .or_else(|| fallback.permission_mode.clone()),
        )
    }
}

/// Parsed view of Claude's ExitPlanMode tool input.
///
/// Swift `WorkstreamExitPlanPreview` (`WorkstreamContext.swift:104-212`). Recent
/// Claude versions pass a JSON object with `plan`, `allowedPrompts`, and
/// `planFilePath`; older/unknown agents pass plain markdown. Both stay
/// displayable. This type is not `Codable` in Swift; it is a pure derived view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkstreamExitPlanPreview {
    pub plan_text: String,
    pub allowed_prompts: Vec<WorkstreamAllowedPrompt>,
    pub plan_file_path: Option<String>,
    pub summary: Option<String>,
}

impl WorkstreamExitPlanPreview {
    /// Swift `init(rawPlan:)` (`WorkstreamContext.swift:110-116`).
    pub fn new(raw_plan: &str) -> Self {
        let (plan_text, allowed_prompts, plan_file_path) = Self::parse(raw_plan);
        let summary = Self::summary(&plan_text);
        Self {
            plan_text,
            allowed_prompts,
            plan_file_path,
            summary,
        }
    }

    /// Swift `parse(_:)` (`WorkstreamContext.swift:118-142`).
    fn parse(raw_plan: &str) -> (String, Vec<WorkstreamAllowedPrompt>, Option<String>) {
        let Some(Value::Object(dict)) = serde_json::from_str::<Value>(raw_plan).ok() else {
            return (raw_plan.to_string(), Vec::new(), None);
        };

        let plan_text = cleaned(dict.get("plan").and_then(Value::as_str))
            .unwrap_or_else(|| raw_plan.to_string());
        let plan_file_path = cleaned(
            dict.get("planFilePath")
                .and_then(Value::as_str)
                .or_else(|| dict.get("plan_file_path").and_then(Value::as_str)),
        );
        (
            plan_text,
            Self::parse_allowed_prompts(dict.get("allowedPrompts")),
            plan_file_path,
        )
    }

    /// Swift `parseAllowedPrompts(_:)` (`WorkstreamContext.swift:144-175`).
    ///
    /// Swift routes all-object arrays through `[[String:Any]]` and mixed/string
    /// arrays through `[Any]`; both branches produce the same prompts for
    /// objects, and only the second yields string entries. A single loop that
    /// handles both element shapes is behaviourally identical.
    fn parse_allowed_prompts(raw: Option<&Value>) -> Vec<WorkstreamAllowedPrompt> {
        let Some(Value::Array(rows)) = raw else {
            return Vec::new();
        };
        rows.iter()
            .filter_map(|row| {
                if let Some(text) = cleaned(row.as_str()) {
                    return Some(WorkstreamAllowedPrompt::new("", text));
                }
                let obj = row.as_object()?;
                let prompt = cleaned(obj.get("prompt").and_then(Value::as_str))
                    .or_else(|| cleaned(obj.get("description").and_then(Value::as_str)))
                    .or_else(|| cleaned(obj.get("text").and_then(Value::as_str)))?;
                let tool = cleaned(obj.get("tool").and_then(Value::as_str))
                    .or_else(|| cleaned(obj.get("toolName").and_then(Value::as_str)))
                    .unwrap_or_default();
                Some(WorkstreamAllowedPrompt::new(tool, prompt))
            })
            .collect()
    }

    /// Swift `summary(from:)` (`WorkstreamContext.swift:177-205`): the first
    /// bullet / numbered item / plain line, or else the first `#` heading.
    fn summary(plan_text: &str) -> Option<String> {
        let mut first_heading: Option<String> = None;
        // Swift `split(separator: "\n", omittingEmptySubsequences: false)`.
        for raw_line in plan_text.split('\n') {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('#') {
                // Trim leading/trailing '#' and ' ' — Swift trims the
                // CharacterSet "# ".
                let heading = line.trim_matches(|c| c == '#' || c == ' ');
                if first_heading.is_none() && !heading.is_empty() {
                    first_heading = Some(heading.to_string());
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
                return Some(rest.trim().to_string());
            }
            if let Some(rest) = Self::numbered_list_body(line) {
                return Some(rest);
            }
            return Some(line.to_string());
        }
        first_heading
    }

    /// Swift's numbered-list regex `^\d+\.\s+(.+)$` followed by "return the
    /// text after the first '.'". Implemented without a regex dependency: the
    /// line must start with one or more ASCII digits, then `.`, then at least
    /// one whitespace, then at least one more character. Returns the substring
    /// after the first `.`, trimmed.
    fn numbered_list_body(line: &str) -> Option<String> {
        let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        let after_digits = &line[digits..];
        let rest = after_digits.strip_prefix('.')?;
        // `\s+`: at least one whitespace char.
        let first = rest.chars().next()?;
        if !first.is_whitespace() {
            return None;
        }
        // `.+`: at least one non-whitespace-run character remains after the ws.
        let body = rest.trim_start();
        if body.is_empty() {
            return None;
        }
        // Swift returns everything after the FIRST '.' in the matched line,
        // trimmed with `.whitespaces`.
        Some(rest.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cleans_and_filters() {
        let ctx = WorkstreamContext::new(
            Some("  hi  ".to_string()),
            Some("   ".to_string()),
            None,
            vec![
                WorkstreamAllowedPrompt::new("Bash", "run"),
                WorkstreamAllowedPrompt {
                    tool: "x".to_string(),
                    prompt: String::new(),
                },
            ],
            None,
            Some("plan".to_string()),
        );
        assert_eq!(ctx.last_user_message.as_deref(), Some("hi"));
        assert_eq!(ctx.assistant_preamble, None);
        assert_eq!(ctx.allowed_prompts.len(), 1);
        assert_eq!(ctx.permission_mode.as_deref(), Some("plan"));
    }

    #[test]
    fn merging_prefers_self_then_fallback() {
        let base = WorkstreamContext::new(
            Some("prev".to_string()),
            Some("preamble".to_string()),
            None,
            vec![],
            None,
            Some("plan".to_string()),
        );
        let merged = WorkstreamContext::new(
            Some("new".to_string()),
            None,
            None,
            vec![],
            None,
            None,
        )
        .merging_missing(Some(&base));
        assert_eq!(merged.last_user_message.as_deref(), Some("new"));
        assert_eq!(merged.assistant_preamble.as_deref(), Some("preamble"));
        assert_eq!(merged.permission_mode.as_deref(), Some("plan"));
    }

    #[test]
    fn encode_omits_nil_optionals_keeps_allowed_prompts() {
        let ctx = WorkstreamContext::new(
            Some("hi".to_string()),
            None,
            None,
            vec![],
            None,
            None,
        );
        // camelCase keys, nil optionals omitted, allowedPrompts always present.
        assert_eq!(
            serde_json::to_string(&ctx).unwrap(),
            r#"{"lastUserMessage":"hi","allowedPrompts":[]}"#
        );
    }

    #[test]
    fn decode_cleans_via_new() {
        let ctx: WorkstreamContext =
            serde_json::from_str(r#"{"lastUserMessage":"  hi  ","permissionMode":"plan"}"#).unwrap();
        assert_eq!(ctx.last_user_message.as_deref(), Some("hi"));
        assert_eq!(ctx.permission_mode.as_deref(), Some("plan"));
        assert!(ctx.allowed_prompts.is_empty());
    }

    #[test]
    fn exit_plan_preview_parses_object_form() {
        let preview = WorkstreamExitPlanPreview::new(
            "{\"plan\":\"# Demo Plan\\n\\n## Context\\nShow the new feed UI.\",\"allowedPrompts\":[{\"tool\":\"Bash\",\"prompt\":\"run reload.sh --tag feedctx\"}],\"planFilePath\":\"/tmp/demo.md\"}",
        );
        assert_eq!(preview.summary.as_deref(), Some("Show the new feed UI."));
        assert_eq!(preview.allowed_prompts.first().map(|p| p.tool.as_str()), Some("Bash"));
        assert_eq!(
            preview.allowed_prompts.first().map(|p| p.prompt.as_str()),
            Some("run reload.sh --tag feedctx")
        );
        assert_eq!(preview.plan_file_path.as_deref(), Some("/tmp/demo.md"));
    }

    #[test]
    fn exit_plan_preview_plain_markdown_falls_back() {
        let preview = WorkstreamExitPlanPreview::new("- first step\n- second");
        assert_eq!(preview.plan_text, "- first step\n- second");
        assert_eq!(preview.summary.as_deref(), Some("first step"));
        assert!(preview.allowed_prompts.is_empty());
    }

    #[test]
    fn summary_numbered_list() {
        assert_eq!(
            WorkstreamExitPlanPreview::new("12.  do the thing").summary.as_deref(),
            Some("do the thing")
        );
        // Not a numbered list: "12.3. text" has no whitespace after the first dot.
        assert_eq!(
            WorkstreamExitPlanPreview::new("12.3. text").summary.as_deref(),
            Some("12.3. text")
        );
    }

    #[test]
    fn summary_heading_only() {
        assert_eq!(
            WorkstreamExitPlanPreview::new("# Title\n").summary.as_deref(),
            Some("Title")
        );
    }
}
