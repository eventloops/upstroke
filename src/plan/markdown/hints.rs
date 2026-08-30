//! Path-hint heuristics: the file paths a task's prose mentions.
//!
//! A bare word is a hint when it looks like a path rather than a sentence —
//! it contains a separator, is not a URL, and either carries a known source
//! extension, globs, or is at least two segments deep. Inline code is judged
//! more loosely, since a backticked token is already a deliberate reference.
//!
//! A source of the DAG with two consumers: [`super::drafts`] harvests hints
//! while walking a task's events, and [`super::assemble`] reuses
//! [`push_unique`] to merge them behind the annotation's own `paths=`.

const HINT_EXTENSIONS: &[&str] = &[
    ".rs", ".md", ".toml", ".json", ".yml", ".yaml", ".txt", ".py", ".ts", ".js", ".lock",
];

fn has_hint_extension(token: &str) -> bool {
    HINT_EXTENSIONS.iter().any(|ext| token.ends_with(ext))
}

pub(super) fn collect_text_hints(text: &str, hints: &mut Vec<String>) {
    for word in text.split_whitespace() {
        let token = word.trim_matches(|c: char| {
            matches!(
                c,
                ',' | '.' | ';' | ':' | '(' | ')' | '`' | '"' | '\'' | '!' | '?'
            )
        });
        if token.contains('/')
            && !token.contains("://")
            && (has_hint_extension(token) || token.contains('*') || token.matches('/').count() >= 2)
        {
            push_unique(hints, token);
        }
    }
}

pub(super) fn collect_code_hint(code: &str, hints: &mut Vec<String>) {
    let token = code.trim();
    if token.contains(' ') || token.contains("://") {
        return;
    }
    if token.contains('/') || has_hint_extension(token) {
        push_unique(hints, token);
    }
}

pub(super) fn push_unique(items: &mut Vec<String>, candidate: &str) {
    if !candidate.is_empty() && !items.iter().any(|i| i == candidate) {
        items.push(candidate.to_owned());
    }
}
