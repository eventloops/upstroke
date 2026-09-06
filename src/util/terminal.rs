//! Text boundaries for terminal output. Each payload is assembled before
//! sanitation, and only the line builder appends layout newlines.
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

/// One assembled line with control characters made visible or replaced by
/// spaces. Clean text keeps its allocation. Persisted values are unchanged.
pub(crate) fn one_line(text: String) -> String {
    if !text.chars().any(char::is_control) {
        return text;
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_control() => out.extend(c.escape_default()),
            c => out.push(c),
        }
    }
    out
}

/// Owns a terminal report's layout. Callers supply one complete line at a
/// time; interpolated data cannot insert layout or terminal controls.
#[derive(Default)]
pub(crate) struct TerminalLines(String);

impl TerminalLines {
    pub(crate) fn push(&mut self, line: std::fmt::Arguments<'_>) {
        self.0.push_str(&one_line(line.to_string()));
        self.0.push('\n');
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_line_builder_introduces_terminal_layout() {
        let mut lines = TerminalLines::default();
        lines.push(format_args!("value: {}", "a\nb\r\t\u{1b}\u{85}\u{7f}"));
        lines.push(format_args!("second line"));
        assert_eq!(
            lines.into_string(),
            "value: a b  \\u{1b}\\u{85}\\u{7f}\nsecond line\n"
        );
    }
}
