//! Extended notes: `docs/internals/topology/fold/parse.md`

use super::*;

impl TopologyFold {
    pub fn parse_log(bytes: &[u8]) -> Result<Vec<TopologyEvent>, FoldError> {
        let mut events = Vec::new();
        for (position, chunk) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
            let Some(committed) = chunk.strip_suffix(b"\n") else {
                break;
            };
            let line = position + 1;
            let text = std::str::from_utf8(committed).map_err(|error| FoldError::RewrittenLog {
                line,
                detail: error.to_string(),
            })?;
            let event = serde_json::from_str::<TopologyEvent>(text).map_err(|error| {
                FoldError::RewrittenLog {
                    line,
                    detail: error.to_string(),
                }
            })?;
            events.push(event);
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::events::DeferWaitElapsed4;

    fn event(round: u32) -> TopologyEvent {
        TopologyEvent {
            ts: "2026-09-06T00:00:00Z".to_owned(),
            body: TopologyEventBody::DeferWaitElapsed {
                data: DeferWaitElapsed4 {
                    waited_ms: 1,
                    round,
                },
            },
        }
    }

    fn committed(round: u32) -> Vec<u8> {
        let mut line = serde_json::to_string(&event(round)).expect("an event serializes");
        line.push('\n');
        line.into_bytes()
    }

    fn refused(bytes: &[u8]) -> usize {
        match TopologyFold::parse_log(bytes) {
            Err(FoldError::RewrittenLog { line, .. }) => line,
            other => panic!("a rewritten log must be refused as one, and this answered {other:?}"),
        }
    }

    #[test]
    fn the_refusal_names_the_first_committed_line_that_is_not_an_event() {
        let not_an_event = b"{\"event\":\"not_a_kind\"}\n";
        let not_utf8 = [0xff, b'\n'];

        let mut json_first = Vec::new();
        json_first.extend_from_slice(not_an_event);
        json_first.extend_from_slice(&committed(2));
        json_first.extend_from_slice(&not_utf8);
        assert_eq!(
            refused(&json_first),
            1,
            "a line that is not an event is named ahead of a later line that is not UTF-8"
        );

        let mut utf8_first = Vec::new();
        utf8_first.extend_from_slice(&not_utf8);
        utf8_first.extend_from_slice(&committed(2));
        utf8_first.extend_from_slice(not_an_event);
        assert_eq!(
            refused(&utf8_first),
            1,
            "a line that is not UTF-8 is named ahead of a later line that is not an event"
        );

        let mut third = committed(1);
        third.extend_from_slice(&committed(2));
        third.extend_from_slice(&not_utf8);
        assert_eq!(
            refused(&third),
            3,
            "the line a refusal names is the line the bytes are on"
        );
    }
}
