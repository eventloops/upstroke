use std::fmt;
use std::path::PathBuf;

use thiserror::Error;

/// Structural problems found in a parsed plan, collected so a single run
/// surfaces every issue at once instead of failing on the first.
#[derive(Debug)]
pub struct ValidationErrors(pub Vec<String>);

/// An operation's refusal together with warnings gathered before it failed.
/// The original typed error remains available for callers that classify it.
#[derive(Debug)]
pub struct WarnedError {
    /// The unchanged refusal, including its original error category.
    pub error: Box<UpstrokeError>,
    /// Diagnostics in the order they were gathered before the refusal.
    pub warnings: Vec<String>,
}

impl fmt::Display for WarnedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // These errors belong to the formatter; no operation context can be
        // added when its destination refuses a write.
        write!(f, "{}", self.error)?;
        if !self.warnings.is_empty() {
            write!(f, "\nwarnings:")?;
            for warning in &self.warnings {
                write!(f, "\n  - {warning}")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for WarnedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Display already includes the original error, so forwarding its
        // source avoids repeating it when the CLI renders the error chain.
        std::error::Error::source(self.error.as_ref())
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plan validation failed:")?;
        for problem in &self.0 {
            write!(f, "\n  - {problem}")?;
        }
        Ok(())
    }
}

/// Library failures classified by the operation or refusal a caller can handle.
/// A failure with earlier diagnostics retains its category inside
/// [`Self::WithWarnings`].
#[derive(Debug, Error)]
pub enum UpstrokeError {
    #[error("failed to read {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A filesystem operation on a path the engine owns failed. Named for
    /// the operation, because a removal, a write or a rename that fails did
    /// not fail to read (§7's operation-context rule); `Io` stays the
    /// variant for reads.
    #[error("failed to {operation} {}: {source}", .path.display())]
    Filesystem {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("plan parse error: {message}")]
    Parse { message: String },

    #[error("config error in {}: {message}", .path.display())]
    Config { path: PathBuf, message: String },

    #[error(
        "pin references unknown model `{model}` for agent `{agent}`; known models for that agent: \
         {known}. The capability catalog is static data shipped with the binary — fix the pin in \
         upstroke.toml or upgrade upstroke."
    )]
    UnknownPinnedModel {
        agent: String,
        model: String,
        known: String,
    },

    #[error("agent error: {message}")]
    Agent { message: String },

    #[error("git error: {message}")]
    Git { message: String },

    #[error("gate error: {message}")]
    Gate { message: String },

    #[error("event log {}: {message}", .path.display())]
    EventLog { path: PathBuf, message: String },

    /// A resume precondition failed (§15). Always carries what to do about it:
    /// refusing to continue is only useful if the operator can tell which of
    /// the four things moved — the run, the plan, the config, or the branch.
    #[error("cannot resume run `{run_id}`: {message}")]
    Resume { run_id: String, message: String },

    /// A request we could not act on — an id that matches nothing or too many
    /// things, a question already answered, an option that does not exist.
    /// Carries its own whole sentence, because prefixing these with a
    /// command's name (`cannot resume …` on a `status` lookup) misdescribes
    /// what the operator was actually doing.
    #[error("{message}")]
    Refused { message: String },

    #[error("{0}")]
    Validation(ValidationErrors),

    /// Non-fatal diagnostics do not replace or hide the operation's refusal.
    #[error(transparent)]
    WithWarnings(WarnedError),
}

impl UpstrokeError {
    /// Carry earlier warnings through a refusal without changing a clean
    /// error's variant. An existing diagnostic bundle is flattened in order.
    pub(crate) fn with_warnings(self, mut warnings: Vec<String>) -> Self {
        if warnings.is_empty() {
            return self;
        }
        match self {
            Self::WithWarnings(mut error) => {
                warnings.append(&mut error.warnings);
                error.warnings = warnings;
                Self::WithWarnings(error)
            }
            error => Self::WithWarnings(WarnedError {
                error: Box::new(error),
                warnings,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warnings_preserve_the_typed_refusal_and_its_original_source_chain() {
        let error = UpstrokeError::Io {
            path: PathBuf::from("plan.md"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied"),
        }
        .with_warnings(vec!["unknown annotation attribute `wibble`".to_owned()]);
        let UpstrokeError::WithWarnings(bundle) = &error else {
            panic!("nonempty warnings must accompany the typed refusal");
        };
        assert!(matches!(bundle.error.as_ref(), UpstrokeError::Io { .. }));
        assert_eq!(bundle.warnings, ["unknown annotation attribute `wibble`"]);
        let source = std::error::Error::source(&error).expect("the I/O cause survives");
        assert_eq!(source.to_string(), "permission denied");
        assert!(std::error::Error::source(source).is_none());
        assert_eq!(
            error.to_string(),
            "failed to read plan.md: permission denied\nwarnings:\n  - unknown annotation attribute `wibble`"
        );
    }

    #[test]
    fn accumulated_warnings_flatten_in_order_and_empty_warnings_preserve_the_variant() {
        let clean = UpstrokeError::Parse {
            message: "no tasks found".to_owned(),
        }
        .with_warnings(Vec::new());
        assert!(matches!(clean, UpstrokeError::Parse { .. }));
        let error = clean
            .with_warnings(vec!["second".to_owned()])
            .with_warnings(vec!["first".to_owned()]);
        let UpstrokeError::WithWarnings(bundle) = error else {
            panic!("nonempty warnings must accompany the typed refusal");
        };
        assert!(matches!(bundle.error.as_ref(), UpstrokeError::Parse { .. }));
        assert_eq!(bundle.warnings, ["first", "second"]);
    }
}
