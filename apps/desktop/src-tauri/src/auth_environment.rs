//! Host-side auth/callback environment helpers.
//!
//! This is the Windows/Tauri counterpart of macOS `AuthEnvironment.callbackScheme`:
//! an explicit `CMUX_AUTH_CALLBACK_SCHEME` wins, debug builds use `cmux-dev`
//! (or `cmux-dev-<tag>`), nightly bundle ids use `cmux-nightly`, and release
//! stable builds use `cmux`.

use std::collections::HashMap;

const CALLBACK_SCHEME_ENV: &str = "CMUX_AUTH_CALLBACK_SCHEME";
const TAG_ENV: &str = "CMUX_TAG";
const NIGHTLY_BUNDLE_ID: &str = "com.cmuxterm.app.nightly";

pub(crate) fn active_callback_scheme() -> String {
    let environment = std::env::vars().collect::<HashMap<_, _>>();
    callback_scheme_for(
        &environment,
        option_env!("TAURI_BUNDLE_IDENTIFIER"),
        cfg!(debug_assertions),
    )
}

pub(crate) fn active_navigation_schemes() -> Vec<String> {
    let mut schemes = vec![active_callback_scheme()];
    for scheme in cmux_ssh::ssh_url::SUPPORTED_SCHEMES {
        if !schemes.iter().any(|existing| existing == scheme) {
            schemes.push(scheme.to_string());
        }
    }
    schemes
}

fn callback_scheme_for(
    environment: &HashMap<String, String>,
    bundle_identifier: Option<&str>,
    is_debug_build: bool,
) -> String {
    if let Some(overridden) = environment
        .get(CALLBACK_SCHEME_ENV)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return overridden.to_string();
    }
    if is_debug_build {
        if let Some(tag) = environment
            .get(TAG_ENV)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .and_then(sanitized_callback_scheme_tag)
        {
            return format!("cmux-dev-{tag}");
        }
        return "cmux-dev".to_string();
    }
    if bundle_identifier == Some(NIGHTLY_BUNDLE_ID) {
        return "cmux-nightly".to_string();
    }
    "cmux".to_string()
}

fn sanitized_callback_scheme_tag(raw_tag: &str) -> Option<String> {
    let mut result = String::new();
    let mut previous_was_hyphen = false;
    for character in raw_tag.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            result.push(character);
            previous_was_hyphen = false;
        } else if !previous_was_hyphen {
            result.push('-');
            previous_was_hyphen = true;
        }
    }
    let result = result.trim_matches('-').to_string();
    (!result.is_empty()).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn callback_scheme_prefers_explicit_override() {
        assert_eq!(
            callback_scheme_for(
                &env(&[(CALLBACK_SCHEME_ENV, " cmux-dev-pair-auth ")]),
                None,
                true,
            ),
            "cmux-dev-pair-auth",
        );
    }

    #[test]
    fn callback_scheme_uses_tagged_debug_scheme_when_tag_is_sanitizable() {
        assert_eq!(
            callback_scheme_for(&env(&[(TAG_ENV, " My Branch_42!! ")]), None, true),
            "cmux-dev-my-branch-42",
        );
    }

    #[test]
    fn callback_scheme_uses_debug_nightly_and_stable_fallbacks() {
        assert_eq!(callback_scheme_for(&env(&[]), None, true), "cmux-dev");
        assert_eq!(
            callback_scheme_for(&env(&[]), Some(NIGHTLY_BUNDLE_ID), false),
            "cmux-nightly",
        );
        assert_eq!(callback_scheme_for(&env(&[]), None, false), "cmux");
    }

    #[test]
    fn sanitized_callback_scheme_tag_drops_empty_results() {
        assert_eq!(sanitized_callback_scheme_tag(" -- "), None);
        assert_eq!(
            sanitized_callback_scheme_tag("A/B C").as_deref(),
            Some("a-b-c")
        );
    }
}
