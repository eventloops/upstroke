//! Extended notes: `docs/internals/error.md`
use std::fmt;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug)]
pub struct ValidationErrors(pub Vec<String>);

#[derive(Debug)]
pub struct WarnedError {
    pub error: Box<UpstrokeError>,
    pub warnings: Vec<String>,
}

impl fmt::Display for WarnedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

#[derive(Debug, Error)]
pub enum UpstrokeError {
    #[error("failed to read {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

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

    #[error("cannot resume run `{run_id}`: {message}")]
    Resume { run_id: String, message: String },

    #[error("{message}")]
    Refused { message: String },

    #[error("{0}")]
    Validation(ValidationErrors),

    #[error(transparent)]
    WithWarnings(WarnedError),
}

impl UpstrokeError {
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
