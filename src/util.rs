//! Small shared helpers used across engine, gates, and reporting.

/// Last `max` bytes of trimmed text, cut on a char boundary, with an ellipsis
/// marker when truncated.
pub fn tail(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= max {
        return trimmed.to_owned();
    }
    let start = trimmed.len() - max;
    let start = (start..trimmed.len())
        .find(|i| trimmed.is_char_boundary(*i))
        .unwrap_or(start);
    format!("…{}", &trimmed[start..])
}

/// Make an arbitrary string (task id, gate name — both user-authored) safe to
/// use as a single file-name component: no separators, no Windows-reserved
/// characters, no dot-only names, bounded length.
pub fn filename_component(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    out.truncate(64);
    if out.trim_matches(['.', '-']).is_empty() {
        return "x".to_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_truncates_on_char_boundaries() {
        assert_eq!(tail("  short  ", 400), "short");
        let long = "a".repeat(500);
        let cut = tail(&long, 400);
        assert!(cut.starts_with('…') && cut.len() < 500);
        // Multibyte content must not split a char.
        let multi = "é".repeat(300);
        let cut = tail(&multi, 401);
        assert!(cut.chars().all(|c| c == 'é' || c == '…'));
    }

    #[test]
    fn filename_component_neutralizes_hostile_names() {
        assert_eq!(filename_component("lint:fast"), "lint-fast");
        assert_eq!(filename_component("unit/fast"), "unit-fast");
        assert_eq!(filename_component("a\\b"), "a-b");
        assert_eq!(filename_component(".."), "x");
        assert_eq!(filename_component("check"), "check");
        assert!(filename_component(&"x".repeat(200)).len() <= 64);
    }
}
