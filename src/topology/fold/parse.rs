//! Extended notes: `docs/internals/topology/fold/parse.md`

use super::*;

impl TopologyFold {
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
