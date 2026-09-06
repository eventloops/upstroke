//! Extended notes: `docs/internals/events/log/premove.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::UpstrokeError;
use crate::events::{Event, EventBody};

#[derive(Debug)]
pub struct PremoveEventLog {
    path: PathBuf,
    file: File,
}

impl PremoveEventLog {
    pub fn open(path: &Path, warnings: &mut Vec<String>) -> Result<Self, UpstrokeError> {
        let io = |source| UpstrokeError::Io {
            path: path.to_path_buf(),
            source,
        };
        match std::fs::read(path) {
            Ok(existing) if !existing.is_empty() && existing.last() != Some(&b'\n') => {
                let keep = existing
                    .iter()
                    .rposition(|byte| *byte == b'\n')
                    .map_or(0, |index| index + 1);
                OpenOptions::new()
                    .write(true)
                    .open(path)
                    .map_err(io)?
                    .set_len(keep as u64)
                    .map_err(io)?;
                warnings.push(format!(
                    "{}: discarded {} trailing byte(s) of an event that was never finished being \
                     written — the shape an interrupted run leaves behind",
                    path.display(),
                    existing.len() - keep
                ));
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(io(source)),
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(io)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
        })
    }

    pub fn append(&mut self, body: EventBody) -> Result<Event, UpstrokeError> {
        let event = Event::now(body);
        let mut line = serde_json::to_string(&event).map_err(|e| UpstrokeError::EventLog {
            path: self.path.clone(),
            message: format!("serializing {}: {e}", event.body.kind()),
        })?;
        let written = serde_json::from_str(&line).map_err(|e| UpstrokeError::EventLog {
            path: self.path.clone(),
            message: format!(
                "{} does not survive its own wire format ({e}); the log could not be replayed",
                event.body.kind()
            ),
        })?;
        line.push('\n');
        let io = |source| UpstrokeError::Io {
            path: self.path.clone(),
            source,
        };
        self.file.write_all(line.as_bytes()).map_err(io)?;
        self.file.flush().map_err(io)?;
        self.file.sync_data().map_err(io)?;
        Ok(written)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
