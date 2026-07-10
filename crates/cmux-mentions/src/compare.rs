//! Foundation string-comparison approximations used by the mention family.
//!
//! DIVERGENCE: Swift's `localizedStandardCompare` / `localizedCaseInsensitiveCompare`
//! consult locale collation tables (plus width-insensitivity). This
//! dependency-free port approximates them with Unicode lowercasing plus
//! numeric-run comparison; ordering can differ for non-ASCII titles.

use std::cmp::Ordering;

/// Approximation of Swift `String.localizedStandardCompare(_:)` (Finder-like:
/// case-insensitive, numeric, forced ordering).
pub fn localized_standard_compare(a: &str, b: &str) -> Ordering {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let (mut i, mut j) = (0usize, 0usize);

    while i < a_chars.len() && j < b_chars.len() {
        if a_chars[i].is_ascii_digit() && b_chars[j].is_ascii_digit() {
            let a_start = i;
            while i < a_chars.len() && a_chars[i].is_ascii_digit() {
                i += 1;
            }
            let b_start = j;
            while j < b_chars.len() && b_chars[j].is_ascii_digit() {
                j += 1;
            }
            let a_run = &a_chars[a_start..i];
            let b_run = &b_chars[b_start..j];
            let a_digits = trim_leading_zeros(a_run);
            let b_digits = trim_leading_zeros(b_run);
            let ordering = a_digits
                .len()
                .cmp(&b_digits.len())
                .then_with(|| a_digits.cmp(b_digits))
                .then_with(|| a_run.len().cmp(&b_run.len()));
            if ordering != Ordering::Equal {
                return ordering;
            }
        } else {
            let lowered_a: Vec<char> = a_chars[i].to_lowercase().collect();
            let lowered_b: Vec<char> = b_chars[j].to_lowercase().collect();
            let ordering = lowered_a.cmp(&lowered_b);
            if ordering != Ordering::Equal {
                return ordering;
            }
            i += 1;
            j += 1;
        }
    }

    let ordering = (a_chars.len() - i).cmp(&(b_chars.len() - j));
    if ordering != Ordering::Equal {
        return ordering;
    }
    // Case-insensitively equal: `localizedStandardCompare` includes
    // `.forcedOrdering`, which still yields a deterministic order for
    // case-differing strings — approximate with a raw comparison.
    a.cmp(b)
}

/// Approximation of Swift `String.localizedCaseInsensitiveCompare(_:)`.
/// Returns `Ordering::Equal` for case-insensitively equal strings, matching
/// Swift's `.orderedSame` (callers apply their own final tie-break).
pub fn localized_case_insensitive_compare(a: &str, b: &str) -> Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

fn trim_leading_zeros(run: &[char]) -> &[char] {
    let first_nonzero = run.iter().position(|&c| c != '0').unwrap_or(run.len());
    &run[first_nonzero..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_runs_compare_by_value() {
        assert_eq!(
            localized_standard_compare("file2", "file10"),
            Ordering::Less
        );
        assert_eq!(
            localized_standard_compare("file10", "file2"),
            Ordering::Greater
        );
    }

    #[test]
    fn compare_is_case_insensitive_first() {
        assert_eq!(localized_standard_compare("Alpha", "beta"), Ordering::Less);
        assert_eq!(
            localized_case_insensitive_compare("Alpha", "alpha"),
            Ordering::Equal
        );
    }

    #[test]
    fn equal_ignoring_case_falls_back_to_forced_ordering() {
        assert_ne!(
            localized_standard_compare("Alpha", "alpha"),
            Ordering::Equal
        );
    }
}
