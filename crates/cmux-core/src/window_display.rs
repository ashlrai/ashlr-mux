//! Platform-neutral display matching and centered window geometry.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowControlIdentity {
    pub label: String,
    pub id: String,
    pub reference: String,
}

pub fn ordered_window_identities(
    labels: impl IntoIterator<Item = String>,
) -> Vec<WindowControlIdentity> {
    let mut labels: Vec<_> = labels.into_iter().collect();
    labels.sort_by_key(|label| {
        if label == "main" {
            1
        } else {
            label
                .strip_prefix("window-")
                .and_then(|number| number.parse::<usize>().ok())
                .unwrap_or(usize::MAX)
        }
    });
    labels
        .into_iter()
        .enumerate()
        .map(|(index, label)| WindowControlIdentity {
            id: if label == "main" {
                "window-1".to_string()
            } else {
                label.clone()
            },
            label,
            reference: format!("window:{}", index + 1),
        })
        .collect()
}

pub fn resolve_window_selector(
    identities: &[WindowControlIdentity],
    selector: &str,
) -> Option<usize> {
    selector
        .strip_prefix("window:")
        .and_then(|index| index.parse::<usize>().ok())
        .and_then(|index| index.checked_sub(1))
        .filter(|index| *index < identities.len())
        .or_else(|| {
            identities
                .iter()
                .position(|identity| identity.id == selector)
        })
}

/// Resolve a display query by case-insensitive exact name, then substring,
/// then zero-based index, matching canonical cmux.
pub fn matching_monitor_index(names: &[Option<String>], query: &str) -> Option<usize> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let lowered = query.to_lowercase();
    names
        .iter()
        .position(|name| {
            name.as_ref()
                .is_some_and(|name| name.to_lowercase() == lowered)
        })
        .or_else(|| {
            names.iter().position(|name| {
                name.as_ref()
                    .is_some_and(|name| name.to_lowercase().contains(&lowered))
            })
        })
        .or_else(|| {
            query
                .parse::<usize>()
                .ok()
                .filter(|index| *index < names.len())
        })
}

/// Clamp a window to a monitor work area and center it without changing focus.
pub fn centered_window_geometry(
    monitor_position: (i32, i32),
    monitor_size: (u32, u32),
    window_size: (u32, u32),
) -> ((i32, i32), (u32, u32)) {
    let width = window_size.0.min(monitor_size.0);
    let height = window_size.1.min(monitor_size.1);
    let x = monitor_position.0 + ((monitor_size.0 - width) / 2) as i32;
    let y = monitor_position.1 + ((monitor_size.1 - height) / 2) as i32;
    ((x, y), (width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_and_centering_follow_canonical_rules() {
        let names = vec![
            Some("Built-in Display".to_string()),
            Some("LG HDR 4K".to_string()),
        ];
        assert_eq!(matching_monitor_index(&names, "lg hdr 4k"), Some(1));
        assert_eq!(matching_monitor_index(&names, " hdr "), Some(1));
        assert_eq!(matching_monitor_index(&names, "0"), Some(0));
        assert_eq!(matching_monitor_index(&names, "missing"), None);

        assert_eq!(
            centered_window_geometry((1920, 0), (1920, 1040), (1000, 800)),
            ((2380, 120), (1000, 800))
        );
        assert_eq!(
            centered_window_geometry((1920, 0), (1920, 1040), (2200, 1200)),
            ((1920, 0), (1920, 1040))
        );

        let identities = ordered_window_identities([
            "window-10".to_string(),
            "main".to_string(),
            "window-2".to_string(),
        ]);
        assert_eq!(
            identities
                .iter()
                .map(|identity| identity.id.as_str())
                .collect::<Vec<_>>(),
            ["window-1", "window-2", "window-10"]
        );
        assert_eq!(resolve_window_selector(&identities, "window:2"), Some(1));
        assert_eq!(resolve_window_selector(&identities, "window-10"), Some(2));
        assert_eq!(resolve_window_selector(&identities, "window:9"), None);
    }
}
