use std::fmt;
use std::path::PathBuf;

use thiserror::Error;

/// Structural problems found in a parsed plan, collected so a single run
/// surfaces every issue at once instead of failing on the first.
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
}
