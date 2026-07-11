//! Platform-neutral display matching and centered window geometry.

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
    }
}
