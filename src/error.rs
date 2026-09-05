//! Extended notes: `docs/internals/error.md`

use std::fmt;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug)]
pub struct ValidationErrors(pub Vec<String>);

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
}
