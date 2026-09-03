//! Reading a topology log into events.
//!
//! The commit marker is the newline, so a torn tail is dropped and a committed
//! line that will not parse is a rewritten log rather than a short read.

use super::*;

impl TopologyFold {
    /// Every committed line of a topology log, in order.
    ///
    /// The newline is the commit marker: an unterminated final line is a torn
    /// tail and is dropped, exactly as [`crate::events`] drops it. A
    /// newline-terminated line that will not parse is the opposite situation —
    /// the line was committed and is not an event, which means the log was
    /// rewritten rather than appended to, and no amount of reading further
    /// recovers it.
    ///
    /// # Errors
    ///
    /// [`FoldError::RewrittenLog`] naming the first committed line that is not
    /// a valid event.
    pub fn parse_log(bytes: &[u8]) -> Result<Vec<TopologyEvent>, FoldError> {
        let committed_end = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |position| position + 1);
        let committed = std::str::from_utf8(&bytes[..committed_end]).map_err(|error| {
            FoldError::RewrittenLog {
                line: bytes[..error.valid_up_to()]
                    .iter()
                    .filter(|byte| **byte == b'\n')
                    .count()
                    + 1,
                detail: error.to_string(),
            }
        })?;

        let mut events = Vec::new();
        for (position, line) in committed.lines().enumerate() {
            // Every committed line is one event, including a blank or
            // whitespace-only one. refusals[23] is about the *commit marker*,
            // not about what the bytes look like: a newline-terminated line
            // that is not a valid event means the log was rewritten, and a line
            // that is empty is not a valid event. Skipping it would fold a log
            // whose physical shape nobody can account for.
            events.push(
                serde_json::from_str::<TopologyEvent>(line).map_err(|error| {
                    FoldError::RewrittenLog {
                        line: position + 1,
                        detail: error.to_string(),
                    }
                })?,
            );
        }
        Ok(events)
    }
}
