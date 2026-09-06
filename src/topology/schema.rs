//! Extended notes: `docs/internals/topology/schema.md`

use std::fmt;

use serde::Deserialize;
use thiserror::Error;

pub const LATEST_LEGACY_SCHEMA: u32 = 3;

pub const TOPOLOGY_SCHEMA: u32 = LATEST_LEGACY_SCHEMA + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TopologyActivation {
    Inactive,
    Active,
}

impl fmt::Display for TopologyActivation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Inactive => "inactive",
            Self::Active => "active",
        })
    }
}

pub const TOPOLOGY_ACTIVATION: TopologyActivation = TopologyActivation::Inactive;

#[must_use]
pub const fn max_readable_schema(activation: TopologyActivation) -> u32 {
    match activation {
        TopologyActivation::Inactive => LATEST_LEGACY_SCHEMA,
        TopologyActivation::Active => TOPOLOGY_SCHEMA,
    }
}

pub const MAX_READABLE_SCHEMA: u32 = max_readable_schema(TOPOLOGY_ACTIVATION);

const _: () = assert!(matches!(TOPOLOGY_ACTIVATION, TopologyActivation::Inactive));
const _: () = assert!(MAX_READABLE_SCHEMA == LATEST_LEGACY_SCHEMA);
const _: () = assert!(MAX_READABLE_SCHEMA == 3);
const _: () = assert!(TOPOLOGY_SCHEMA == LATEST_LEGACY_SCHEMA + 1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriterSelector {
    Production,
    TopologyPreview,
}

impl fmt::Display for WriterSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Production => "production",
            Self::TopologyPreview => "topology-preview",
        })
    }
}

#[must_use]
pub const fn fresh_writer_schema(selector: WriterSelector) -> u32 {
    match selector {
        WriterSelector::Production => LATEST_LEGACY_SCHEMA,
        WriterSelector::TopologyPreview => TOPOLOGY_SCHEMA,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogHeader {
    pub event: String,
    pub schema: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderSelection {
    Legacy { schema: u32 },
    Topology,
}

#[derive(Debug, Deserialize)]
struct ProbeLine {
    event: String,
    #[serde(default)]
    data: Option<ProbeData>,
}

#[derive(Debug, Deserialize)]
struct ProbeData {
    #[serde(default)]
    schema: Option<u32>,
}

const RUN_STARTED: &str = "run_started";

pub fn probe_header(bytes: &[u8]) -> Result<LogHeader, SchemaRefusal> {
    let end = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or(SchemaRefusal::NoCommittedHeader)?;
    let line =
        std::str::from_utf8(&bytes[..end]).map_err(|error| SchemaRefusal::FirstLineUnreadable {
            detail: error.to_string(),
        })?;
    let probe: ProbeLine =
        serde_json::from_str(line).map_err(|error| SchemaRefusal::FirstLineUnreadable {
            detail: error.to_string(),
        })?;
    if probe.event != RUN_STARTED {
        return Err(SchemaRefusal::RunStartedNotFirst { found: probe.event });
    }
    let schema = probe
        .data
        .and_then(|data| data.schema)
        .ok_or(SchemaRefusal::HeaderWithoutSchema)?;
    Ok(LogHeader {
        event: probe.event,
        schema,
    })
}

pub fn select_for_schema(schema: u32, ceiling: u32) -> Result<ReaderSelection, SchemaRefusal> {
    if schema > ceiling {
        if schema == TOPOLOGY_SCHEMA {
            return Err(SchemaRefusal::TopologyLogUnreadable { schema, ceiling });
        }
        return Err(SchemaRefusal::NewerThanReadable { schema, ceiling });
    }
    if schema == TOPOLOGY_SCHEMA {
        return Ok(ReaderSelection::Topology);
    }
    Ok(ReaderSelection::Legacy { schema })
}

pub fn select_reader_with(bytes: &[u8], ceiling: u32) -> Result<ReaderSelection, SchemaRefusal> {
    select_for_schema(probe_header(bytes)?.schema, ceiling)
}

pub fn select_reader(bytes: &[u8]) -> Result<ReaderSelection, SchemaRefusal> {
    select_reader_with(bytes, MAX_READABLE_SCHEMA)
}

pub fn check_upgrade_transition(from: u32, to: u32) -> Result<(), SchemaRefusal> {
    if to >= TOPOLOGY_SCHEMA {
        return Err(SchemaRefusal::NoUpgradePath { from, to });
    }
    if to <= from {
        return Err(SchemaRefusal::NotAnUpgrade { from, to });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SchemaRefusal {
    #[error(
        "the event log has no committed first line — every byte present is an unterminated \
         write. The newline is what commits a line, so this log records nothing yet; a run \
         interrupted this early left no state to recover."
    )]
    NoCommittedHeader,

    #[error(
        "the event log's first line is not a readable event ({detail}). This is not a torn \
         tail — the first line is newline-terminated, so it was committed, and a committed \
         line that will not parse means the log was rewritten rather than appended to."
    )]
    FirstLineUnreadable { detail: String },

    #[error(
        "the event log begins with `{found}` rather than `run_started`. The first line records \
         how the run began and which schema everything after it is written in, so a log that \
         starts anywhere else cannot be interpreted at all — not even to say what is wrong \
         with it."
    )]
    RunStartedNotFirst { found: String },

    #[error(
        "the event log's `run_started` records no schema, so there is no way to tell which \
         execution model the rest of the file describes. Every writer records one; a record \
         without it was not written by upstroke."
    )]
    HeaderWithoutSchema,

    #[error(
        "this log is a parallel-execution-topology run (event schema {schema}); this binary \
         reads up to schema {ceiling}, which is sequential runs only. Upgrade upstroke to read \
         it. It will never become a schema-{ceiling} run: the topology is a different \
         execution model — a merge queue, per-task worktrees, and a recorded runner identity — \
         and no upgrade path into or out of it exists."
    )]
    TopologyLogUnreadable { schema: u32, ceiling: u32 },

    #[error(
        "this log was written by a newer upstroke (event schema {schema}); this binary reads up \
         to schema {ceiling}. Upgrade rather than interpret it — deriving state from a log we \
         only half understand would be confidently wrong."
    )]
    NewerThanReadable { schema: u32, ceiling: u32 },

    #[error(
        "refusing a schema upgrade {from} -> {to}: schemas {LATEST_LEGACY_SCHEMA} and below \
         are sequential runs and schema {TOPOLOGY_SCHEMA} is the parallel execution topology. \
         They are different execution models, not successive versions of one, and no run \
         crosses between them — start a new run instead."
    )]
    NoUpgradePath { from: u32, to: u32 },

    #[error(
        "invalid schema transition {from} -> {to}: an upgrade record must move the log \
         forwards, and this one does not."
    )]
    NotAnUpgrade { from: u32, to: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_line(event: &str, schema: Option<u32>) -> String {
        let schema_field = match schema {
            Some(value) => format!(r#""schema":{value},"#),
            None => String::new(),
        };
        format!(
            r#"{{"ts":"2026-08-17T03:04:05.678Z","event":"{event}","data":{{"branch":" Ünïcode/BrÄnch  ","run_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV",{schema_field}"upstroke_version":"0.0.1-Ünicode"}}}}"#
        )
    }

    fn committed(event: &str, schema: Option<u32>) -> Vec<u8> {
        let mut bytes = header_line(event, schema).into_bytes();
        bytes.push(b'\n');
        bytes
    }

    const HOSTILE_SCHEMAS: [u32; 22] = [
        0,
        1,
        2,
        3,
        LATEST_LEGACY_SCHEMA,
        TOPOLOGY_SCHEMA,
        5,
        6,
        7,
        8,
        127,
        128,
        255,
        256,
        257,
        259,
        511,
        512,
        65_535,
        65_536,
        u32::MAX - 1,
        u32::MAX,
    ];

    const HOSTILE_CEILINGS: [u32; 5] = [0, 1, 2, LATEST_LEGACY_SCHEMA, TOPOLOGY_SCHEMA];

    fn expected_selection(schema: u32, ceiling: u32) -> Result<ReaderSelection, SchemaRefusal> {
        if schema > ceiling {
            if schema == TOPOLOGY_SCHEMA {
                return Err(SchemaRefusal::TopologyLogUnreadable { schema, ceiling });
            }
            return Err(SchemaRefusal::NewerThanReadable { schema, ceiling });
        }
        if schema == TOPOLOGY_SCHEMA {
            return Ok(ReaderSelection::Topology);
        }
        Ok(ReaderSelection::Legacy { schema })
    }

    fn hostile_later_header(schema: u32) -> Vec<u8> {
        let mut bytes = committed(RUN_STARTED, Some(schema));
        bytes.extend_from_slice(&committed("task_merged", Some(schema)));
        bytes
    }

    #[test]
    fn schema_constants_are_the_frozen_values_and_adjacent() {
        assert_eq!(LATEST_LEGACY_SCHEMA, 3);
        assert_eq!(TOPOLOGY_SCHEMA, 4);
        assert_eq!(TOPOLOGY_SCHEMA, LATEST_LEGACY_SCHEMA + 1);
        assert_eq!(LATEST_LEGACY_SCHEMA, crate::events::SCHEMA_VERSION);
    }

    #[test]
    fn max_readable_is_the_activation_switch_and_production_is_inactive() {
        assert_eq!(
            max_readable_schema(TopologyActivation::Inactive),
            LATEST_LEGACY_SCHEMA
        );
        assert_eq!(
            max_readable_schema(TopologyActivation::Active),
            TOPOLOGY_SCHEMA
        );
        assert_ne!(
            max_readable_schema(TopologyActivation::Inactive),
            max_readable_schema(TopologyActivation::Active)
        );
        assert_eq!(TOPOLOGY_ACTIVATION, TopologyActivation::Inactive);
        assert_eq!(
            MAX_READABLE_SCHEMA,
            max_readable_schema(TOPOLOGY_ACTIVATION)
        );
        assert_eq!(MAX_READABLE_SCHEMA, 3);
    }

    #[test]
    fn fresh_writer_schema_maps_each_selector_to_a_different_model() {
        assert_eq!(
            fresh_writer_schema(WriterSelector::Production),
            LATEST_LEGACY_SCHEMA
        );
        assert_eq!(
            fresh_writer_schema(WriterSelector::TopologyPreview),
            TOPOLOGY_SCHEMA
        );
        assert_ne!(
            fresh_writer_schema(WriterSelector::Production),
            fresh_writer_schema(WriterSelector::TopologyPreview)
        );
        assert!(fresh_writer_schema(WriterSelector::Production) <= MAX_READABLE_SCHEMA);
    }

    #[test]
    fn reader_selection_is_a_relation_over_every_ceiling_and_schema() {
        for ceiling in [LATEST_LEGACY_SCHEMA, TOPOLOGY_SCHEMA] {
            for schema in 0..=6 {
                let expected = if schema > ceiling {
                    if schema == TOPOLOGY_SCHEMA {
                        Err(SchemaRefusal::TopologyLogUnreadable { schema, ceiling })
                    } else {
                        Err(SchemaRefusal::NewerThanReadable { schema, ceiling })
                    }
                } else if schema == TOPOLOGY_SCHEMA {
                    Ok(ReaderSelection::Topology)
                } else {
                    Ok(ReaderSelection::Legacy { schema })
                };
                assert_eq!(
                    select_for_schema(schema, ceiling),
                    expected,
                    "ceiling {ceiling}, schema {schema}"
                );
            }
        }
    }

    #[test]
    fn production_refuses_a_topology_log_and_reads_every_legacy_one() {
        assert_eq!(
            select_reader(&committed(RUN_STARTED, Some(TOPOLOGY_SCHEMA))),
            Err(SchemaRefusal::TopologyLogUnreadable {
                schema: TOPOLOGY_SCHEMA,
                ceiling: LATEST_LEGACY_SCHEMA,
            })
        );
        for schema in 1..=LATEST_LEGACY_SCHEMA {
            assert_eq!(
                select_reader(&committed(RUN_STARTED, Some(schema))),
                Ok(ReaderSelection::Legacy { schema })
            );
        }
    }

    #[test]
    fn activating_the_ceiling_is_the_only_thing_that_admits_a_topology_log() {
        let log = committed(RUN_STARTED, Some(TOPOLOGY_SCHEMA));
        assert!(
            select_reader_with(&log, max_readable_schema(TopologyActivation::Inactive)).is_err()
        );
        assert_eq!(
            select_reader_with(&log, max_readable_schema(TopologyActivation::Active)),
            Ok(ReaderSelection::Topology)
        );
    }

    #[test]
    fn a_first_line_is_a_header_only_once_its_newline_commits_it() {
        let line = header_line(RUN_STARTED, Some(2));
        let torn = line.clone().into_bytes();
        let mut commit = torn.clone();
        commit.push(b'\n');

        assert_eq!(probe_header(&torn), Err(SchemaRefusal::NoCommittedHeader));
        assert_eq!(
            probe_header(&commit),
            Ok(LogHeader {
                event: RUN_STARTED.to_owned(),
                schema: 2,
            })
        );
        assert_eq!(commit.len(), torn.len() + 1);
    }

    #[test]
    fn an_empty_or_newline_only_log_has_no_header() {
        assert_eq!(probe_header(b""), Err(SchemaRefusal::NoCommittedHeader));
        assert!(matches!(
            probe_header(b"\n"),
            Err(SchemaRefusal::FirstLineUnreadable { .. })
        ));
        assert!(matches!(
            probe_header(b"   \n"),
            Err(SchemaRefusal::FirstLineUnreadable { .. })
        ));
    }

    #[test]
    fn the_probe_reads_line_one_and_refuses_to_look_further() {
        let mut log = committed("task_merged", None);
        log.extend_from_slice(&committed(RUN_STARTED, Some(3)));
        assert_eq!(
            probe_header(&log),
            Err(SchemaRefusal::RunStartedNotFirst {
                found: "task_merged".to_owned(),
            })
        );

        let mut good = committed(RUN_STARTED, Some(1));
        good.extend_from_slice(b"{ this is not JSON at all\n\x80\x81\n");
        assert_eq!(
            probe_header(&good),
            Ok(LogHeader {
                event: RUN_STARTED.to_owned(),
                schema: 1,
            })
        );
    }

    #[test]
    fn a_committed_first_line_that_is_not_an_event_is_a_rewritten_log() {
        assert!(matches!(
            probe_header(b"{\"event\":\"run_started\",\n"),
            Err(SchemaRefusal::FirstLineUnreadable { .. })
        ));
        assert!(matches!(
            probe_header(b"{\"event\":\"run_\x80started\"}\n"),
            Err(SchemaRefusal::FirstLineUnreadable { .. })
        ));
    }

    #[test]
    fn a_run_started_without_a_schema_is_not_a_header() {
        assert_eq!(
            probe_header(&committed(RUN_STARTED, None)),
            Err(SchemaRefusal::HeaderWithoutSchema)
        );
    }

    #[test]
    fn the_topology_refusal_is_a_different_message_from_the_generic_newer_one() {
        let topology = SchemaRefusal::TopologyLogUnreadable {
            schema: 4,
            ceiling: 2,
        }
        .to_string();
        let newer = SchemaRefusal::NewerThanReadable {
            schema: 9,
            ceiling: 7,
        }
        .to_string();

        assert_ne!(topology, newer);
        assert!(topology.contains("schema 4"), "{topology}");
        assert!(topology.contains("schema 2"), "{topology}");
        assert!(!topology.contains('3'), "{topology}");
        assert!(topology.contains("topology"), "{topology}");
        assert!(topology.contains("no upgrade path"), "{topology}");

        assert!(newer.contains("schema 9"), "{newer}");
        assert!(newer.contains("schema 7"), "{newer}");
        assert!(!newer.contains("topology"), "{newer}");
    }

    #[test]
    fn every_refusal_names_what_it_refused() {
        let cases: Vec<(SchemaRefusal, &[&str])> = vec![
            (SchemaRefusal::NoCommittedHeader, &["newline", "committed"]),
            (
                SchemaRefusal::FirstLineUnreadable {
                    detail: "expected value at line 1 column 1".to_owned(),
                },
                &["expected value at line 1 column 1", "rewritten"],
            ),
            (
                SchemaRefusal::RunStartedNotFirst {
                    found: "merge_prepared".to_owned(),
                },
                &["merge_prepared", "run_started"],
            ),
            (SchemaRefusal::HeaderWithoutSchema, &["no schema"]),
            (
                SchemaRefusal::TopologyLogUnreadable {
                    schema: 4,
                    ceiling: 3,
                },
                &["4", "3", "topology"],
            ),
            (
                SchemaRefusal::NewerThanReadable {
                    schema: 5,
                    ceiling: 3,
                },
                &["5", "3", "newer"],
            ),
            (
                SchemaRefusal::NoUpgradePath { from: 3, to: 4 },
                &["3 -> 4", "different execution models"],
            ),
            (
                SchemaRefusal::NotAnUpgrade { from: 2, to: 2 },
                &["2 -> 2", "forwards"],
            ),
        ];
        for (refusal, fragments) in cases {
            let rendered = refusal.to_string();
            for fragment in fragments {
                assert!(
                    rendered.contains(fragment),
                    "{refusal:?} does not name `{fragment}`: {rendered}"
                );
            }
        }
    }

    #[test]
    fn no_upgrade_reaches_the_topology_from_any_legacy_schema() {
        for from in 0..=5 {
            for to in 0..=6 {
                let expected = if to >= TOPOLOGY_SCHEMA {
                    Err(SchemaRefusal::NoUpgradePath { from, to })
                } else if to <= from {
                    Err(SchemaRefusal::NotAnUpgrade { from, to })
                } else {
                    Ok(())
                };
                assert_eq!(
                    check_upgrade_transition(from, to),
                    expected,
                    "upgrade {from} -> {to}"
                );
            }
        }
    }

    #[test]
    fn the_legacy_upgrade_ladder_still_runs_to_its_own_ceiling() {
        assert_eq!(check_upgrade_transition(1, 2), Ok(()));
        assert_eq!(check_upgrade_transition(2, 3), Ok(()));
        assert_eq!(check_upgrade_transition(1, 3), Ok(()));
        for from in 1..=LATEST_LEGACY_SCHEMA {
            assert_eq!(
                check_upgrade_transition(from, TOPOLOGY_SCHEMA),
                Err(SchemaRefusal::NoUpgradePath {
                    from,
                    to: TOPOLOGY_SCHEMA
                })
            );
        }
    }

    #[test]
    fn reader_selection_holds_across_every_partition_and_integer_boundary() {
        let mut cells = 0_u32;
        for ceiling in HOSTILE_CEILINGS {
            for schema in HOSTILE_SCHEMAS {
                assert_eq!(
                    select_for_schema(schema, ceiling),
                    expected_selection(schema, ceiling),
                    "ceiling {ceiling}, schema {schema}"
                );
                cells += 1;
            }
        }
        assert_eq!(
            cells,
            (HOSTILE_CEILINGS.len() * HOSTILE_SCHEMAS.len()) as u32
        );

        assert_eq!(
            select_for_schema(7, LATEST_LEGACY_SCHEMA),
            Err(SchemaRefusal::NewerThanReadable {
                schema: 7,
                ceiling: LATEST_LEGACY_SCHEMA
            })
        );
        assert_eq!(
            select_for_schema(259, LATEST_LEGACY_SCHEMA),
            Err(SchemaRefusal::NewerThanReadable {
                schema: 259,
                ceiling: LATEST_LEGACY_SCHEMA
            })
        );
        assert_eq!(
            select_for_schema(u32::MAX, TOPOLOGY_SCHEMA),
            Err(SchemaRefusal::NewerThanReadable {
                schema: u32::MAX,
                ceiling: TOPOLOGY_SCHEMA
            })
        );
    }

    #[test]
    fn no_upgrade_reaches_any_destination_at_or_above_the_topology_schema() {
        let froms: [u32; 9] = [0, 1, 2, 3, 4, 5, 255, 256, u32::MAX];
        let mut cells = 0_u32;
        for from in froms {
            for to in HOSTILE_SCHEMAS {
                let expected = if to >= TOPOLOGY_SCHEMA {
                    Err(SchemaRefusal::NoUpgradePath { from, to })
                } else if to <= from {
                    Err(SchemaRefusal::NotAnUpgrade { from, to })
                } else {
                    Ok(())
                };
                assert_eq!(
                    check_upgrade_transition(from, to),
                    expected,
                    "upgrade {from} -> {to}"
                );
                cells += 1;
            }
        }
        assert_eq!(cells, (froms.len() * HOSTILE_SCHEMAS.len()) as u32);

        assert_eq!(
            check_upgrade_transition(3, 7),
            Err(SchemaRefusal::NoUpgradePath { from: 3, to: 7 })
        );
        assert_eq!(
            check_upgrade_transition(3, 256),
            Err(SchemaRefusal::NoUpgradePath { from: 3, to: 256 })
        );
        assert_eq!(
            check_upgrade_transition(3, u32::MAX),
            Err(SchemaRefusal::NoUpgradePath {
                from: 3,
                to: u32::MAX
            })
        );
    }

    #[test]
    fn the_production_wrapper_refuses_every_future_schema_its_inner_selector_refuses() {
        for schema in HOSTILE_SCHEMAS {
            let log = committed(RUN_STARTED, Some(schema));
            assert_eq!(
                select_reader(&log),
                expected_selection(schema, MAX_READABLE_SCHEMA),
                "select_reader at schema {schema}"
            );
            assert_eq!(
                select_reader(&log),
                select_for_schema(schema, MAX_READABLE_SCHEMA),
                "select_reader is not select_for_schema at MAX_READABLE_SCHEMA, schema {schema}"
            );
            for ceiling in HOSTILE_CEILINGS {
                assert_eq!(
                    select_reader_with(&log, ceiling),
                    expected_selection(schema, ceiling),
                    "select_reader_with ceiling {ceiling}, schema {schema}"
                );
            }
        }

        assert_eq!(
            select_reader(&committed(RUN_STARTED, Some(5))),
            Err(SchemaRefusal::NewerThanReadable {
                schema: 5,
                ceiling: LATEST_LEGACY_SCHEMA
            })
        );
        assert_eq!(
            select_reader(&committed(RUN_STARTED, Some(u32::MAX))),
            Err(SchemaRefusal::NewerThanReadable {
                schema: u32::MAX,
                ceiling: LATEST_LEGACY_SCHEMA
            })
        );
    }

    #[test]
    fn a_future_schema_survives_the_probe_at_its_recorded_width() {
        for schema in [5_u32, 6, 7, 9, 255, 256, 257, 259, 65_536, u32::MAX] {
            let log = committed(RUN_STARTED, Some(schema));
            assert_eq!(
                probe_header(&log),
                Ok(LogHeader {
                    event: RUN_STARTED.to_owned(),
                    schema,
                }),
                "the probe did not preserve schema {schema}"
            );
            let refusal = select_reader_with(&log, LATEST_LEGACY_SCHEMA)
                .expect_err("every one of these is above the ceiling");
            match refusal {
                SchemaRefusal::NewerThanReadable {
                    schema: found,
                    ceiling,
                } => {
                    assert_eq!(found, schema);
                    assert_eq!(ceiling, LATEST_LEGACY_SCHEMA);
                    let rendered = SchemaRefusal::NewerThanReadable {
                        schema: found,
                        ceiling,
                    }
                    .to_string();
                    assert!(
                        rendered.contains(&format!("event schema {schema}")),
                        "{rendered}"
                    );
                    assert!(
                        rendered.contains(&format!("reads up to schema {LATEST_LEGACY_SCHEMA}")),
                        "{rendered}"
                    );
                }
                other => panic!("schema {schema} was refused as {other:?}"),
            }
        }
    }

    #[test]
    fn the_line_feed_is_the_only_byte_that_commits_a_first_line() {
        let line = header_line(RUN_STARTED, Some(2));
        for byte in 0_u8..=255 {
            let mut bytes = line.clone().into_bytes();
            bytes.push(byte);
            let observed = probe_header(&bytes);
            if byte == b'\n' {
                assert_eq!(
                    observed,
                    Ok(LogHeader {
                        event: RUN_STARTED.to_owned(),
                        schema: 2,
                    }),
                    "0x0A did not commit the line"
                );
            } else {
                assert_eq!(
                    observed,
                    Err(SchemaRefusal::NoCommittedHeader),
                    "0x{byte:02X} committed a line the newline had not"
                );
            }
        }

        let mut crlf = line.clone().into_bytes();
        crlf.extend_from_slice(b"\r\n");
        assert_eq!(
            probe_header(&crlf),
            Ok(LogHeader {
                event: RUN_STARTED.to_owned(),
                schema: 2,
            })
        );
    }

    #[test]
    fn commitment_depends_on_the_newline_and_on_nothing_the_header_says() {
        for schema in [
            1_u32,
            2,
            LATEST_LEGACY_SCHEMA,
            TOPOLOGY_SCHEMA,
            5,
            259,
            u32::MAX,
        ] {
            let line = header_line(RUN_STARTED, Some(schema));
            let torn = line.clone().into_bytes();
            let mut commit = torn.clone();
            commit.push(b'\n');

            assert_eq!(
                probe_header(&torn),
                Err(SchemaRefusal::NoCommittedHeader),
                "an unterminated schema-{schema} header was committed"
            );
            assert_eq!(
                probe_header(&commit),
                Ok(LogHeader {
                    event: RUN_STARTED.to_owned(),
                    schema,
                }),
                "a terminated schema-{schema} header was not committed"
            );
            assert_eq!(commit.len(), torn.len() + 1);

            assert_eq!(
                select_reader_with(&torn, LATEST_LEGACY_SCHEMA),
                Err(SchemaRefusal::NoCommittedHeader),
                "an uncommitted schema-{schema} header reached selection"
            );
        }
    }

    #[test]
    fn a_committed_header_outranks_every_kind_of_damage_after_it() {
        let head = committed(RUN_STARTED, Some(5));
        let tails: [&[u8]; 5] = [
            b"",
            b"{\"event\":",
            b"{\"event\":\"task_merged\"}",
            b"\x80\x81\x82",
            b"{\"event\":\"run_started\",\"data\":{\"schema\":1}}\n{ broken",
        ];
        for tail in tails {
            let mut log = head.clone();
            log.extend_from_slice(tail);
            assert_eq!(
                select_reader_with(&log, LATEST_LEGACY_SCHEMA),
                Err(SchemaRefusal::NewerThanReadable {
                    schema: 5,
                    ceiling: LATEST_LEGACY_SCHEMA
                }),
                "damage after a committed header changed the refusal"
            );
        }

        for schema in [1_u32, 5, 9, u32::MAX] {
            let torn = header_line(RUN_STARTED, Some(schema)).into_bytes();
            assert_eq!(
                select_reader_with(&torn, LATEST_LEGACY_SCHEMA),
                Err(SchemaRefusal::NoCommittedHeader),
                "uncommitted bytes claiming schema {schema} were selected on"
            );
        }
    }

    #[test]
    fn no_later_line_repairs_any_first_line_refusal() {
        let first_lines: Vec<(&str, Vec<u8>, SchemaRefusal)> = vec![
            (
                "malformed JSON",
                b"{broken\n".to_vec(),
                SchemaRefusal::FirstLineUnreadable {
                    detail: String::new(),
                },
            ),
            (
                "truncated object",
                b"{\"event\":\"run_started\",\n".to_vec(),
                SchemaRefusal::FirstLineUnreadable {
                    detail: String::new(),
                },
            ),
            (
                "invalid UTF-8",
                b"{\"event\":\"run_\x80started\"}\n".to_vec(),
                SchemaRefusal::FirstLineUnreadable {
                    detail: String::new(),
                },
            ),
            (
                "blank line",
                b"\n".to_vec(),
                SchemaRefusal::FirstLineUnreadable {
                    detail: String::new(),
                },
            ),
            (
                "whitespace-only line",
                b"   \t \n".to_vec(),
                SchemaRefusal::FirstLineUnreadable {
                    detail: String::new(),
                },
            ),
            (
                "a schema-bearing wrong tag",
                committed("task_merged", Some(3)),
                SchemaRefusal::RunStartedNotFirst {
                    found: "task_merged".to_owned(),
                },
            ),
            (
                "run_started without a schema",
                committed(RUN_STARTED, None),
                SchemaRefusal::HeaderWithoutSchema,
            ),
        ];

        for (label, line_one, expected) in first_lines {
            let alone = probe_header(&line_one);
            for later in [1_u32, 2, LATEST_LEGACY_SCHEMA, TOPOLOGY_SCHEMA, 7] {
                let mut log = line_one.clone();
                log.extend_from_slice(&hostile_later_header(later));
                let observed = probe_header(&log);
                assert_eq!(
                    observed, alone,
                    "{label}: a later schema-{later} header changed the line-1 answer"
                );
                match (&expected, &observed) {
                    (
                        SchemaRefusal::FirstLineUnreadable { .. },
                        Err(SchemaRefusal::FirstLineUnreadable { .. }),
                    ) => {}
                    (want, Err(got)) => assert_eq!(want, got, "{label}"),
                    (_, Ok(header)) => panic!("{label} was accepted as {header:?}"),
                }
                assert!(
                    select_reader_with(&log, LATEST_LEGACY_SCHEMA).is_err(),
                    "{label}: selection accepted a log whose first line refuses"
                );
                assert!(select_reader(&log).is_err(), "{label}");
            }
        }

        let mut good = committed(RUN_STARTED, Some(1));
        good.extend_from_slice(&hostile_later_header(TOPOLOGY_SCHEMA));
        assert_eq!(
            probe_header(&good),
            Ok(LogHeader {
                event: RUN_STARTED.to_owned(),
                schema: 1,
            })
        );
    }

    #[test]
    fn a_schema_read_out_of_invalid_committed_bytes_is_not_a_header() {
        let damaged: [&[u8]; 4] = [
            b"{\"event\":\"run_started\",\"data\":{\"schema\":9},BROKEN\n",
            b"{\"event\":\"run_started\",\"data\":{\"schema\":259}\n",
            b"\"schema\":4\n",
            b"{\"event\":\"run_started\",\"data\":{\"schema\":\n",
        ];
        for bytes in damaged {
            assert!(
                matches!(
                    probe_header(bytes),
                    Err(SchemaRefusal::FirstLineUnreadable { .. })
                ),
                "{} was not treated as a rewritten log",
                String::from_utf8_lossy(bytes)
            );
            assert!(
                matches!(
                    select_reader_with(bytes, LATEST_LEGACY_SCHEMA),
                    Err(SchemaRefusal::FirstLineUnreadable { .. })
                ),
                "{} reached schema selection",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn one_physical_line_holds_exactly_one_event() {
        let one = header_line(RUN_STARTED, Some(3));
        let hostile: Vec<Vec<u8>> = vec![
            format!("{one}{one}\n").into_bytes(),
            format!("{one} trailing junk\n").into_bytes(),
            format!("{one},\n").into_bytes(),
            format!("{one}{{\"event\":\"task_merged\"}}\n").into_bytes(),
            format!("[{one}]\n").into_bytes(),
        ];
        for bytes in hostile {
            assert!(
                matches!(
                    probe_header(&bytes),
                    Err(SchemaRefusal::FirstLineUnreadable { .. })
                ),
                "a line holding more than one value was accepted: {}",
                String::from_utf8_lossy(&bytes)
            );
        }
        assert_eq!(
            probe_header(format!("{one}\n").as_bytes()),
            Ok(LogHeader {
                event: RUN_STARTED.to_owned(),
                schema: 3,
            })
        );
    }

    #[test]
    fn the_first_tag_is_compared_exactly_and_reported_verbatim() {
        for found in [
            "RUN_STARTED",
            "Run_Started",
            "run started",
            "run-started",
            " run_started",
            "run_started ",
            "run_started\u{200b}",
            "",
            "task_merged",
        ] {
            let log = committed(found, Some(3));
            assert_eq!(
                probe_header(&log),
                Err(SchemaRefusal::RunStartedNotFirst {
                    found: found.to_owned()
                }),
                "`{found}` was not refused verbatim"
            );
            assert_eq!(
                select_reader_with(&log, LATEST_LEGACY_SCHEMA),
                Err(SchemaRefusal::RunStartedNotFirst {
                    found: found.to_owned()
                }),
                "`{found}` survived composition"
            );
        }
        assert_eq!(
            probe_header(&committed(RUN_STARTED, Some(3))),
            Ok(LogHeader {
                event: RUN_STARTED.to_owned(),
                schema: 3,
            })
        );
    }

    #[test]
    fn a_non_run_started_first_line_refuses_whatever_schema_it_carries() {
        for schema in [
            None,
            Some(1),
            Some(LATEST_LEGACY_SCHEMA),
            Some(TOPOLOGY_SCHEMA),
            Some(9),
        ] {
            let log = committed("task_merged", schema);
            assert_eq!(
                probe_header(&log),
                Err(SchemaRefusal::RunStartedNotFirst {
                    found: "task_merged".to_owned()
                }),
                "a task_merged header carrying schema {schema:?} was read"
            );
            assert_eq!(
                select_reader(&log),
                Err(SchemaRefusal::RunStartedNotFirst {
                    found: "task_merged".to_owned()
                })
            );
        }
    }

    #[test]
    fn a_first_line_that_is_not_an_event_envelope_is_unreadable_rather_than_wrong_tagged() {
        let not_envelopes: [&[u8]; 9] = [
            b"{}\n",
            b"{\"data\":{\"schema\":3}}\n",
            b"{\"event\":null}\n",
            b"{\"event\":42}\n",
            b"{\"event\":[\"run_started\"]}\n",
            b"[]\n",
            b"1\n",
            b"\"run_started\"\n",
            b"null\n",
        ];
        for bytes in not_envelopes {
            let observed = probe_header(bytes);
            assert!(
                matches!(observed, Err(SchemaRefusal::FirstLineUnreadable { .. })),
                "{} was classified as {observed:?}",
                String::from_utf8_lossy(bytes)
            );
            let Err(SchemaRefusal::FirstLineUnreadable { detail }) = observed else {
                unreachable!("asserted above")
            };
            assert!(
                !detail.trim().is_empty(),
                "the refusal for {} says nothing about why",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn the_newer_schema_diagnostics_bind_each_number_to_its_role() {
        for (schema, ceiling) in [(9_u32, 7_u32), (5, 3), (4, 2), (256, 255), (u32::MAX, 0)] {
            let rendered = SchemaRefusal::NewerThanReadable { schema, ceiling }.to_string();
            assert!(
                rendered.contains(&format!("event schema {schema}")),
                "{rendered}"
            );
            assert!(
                rendered.contains(&format!("reads up to schema {ceiling}")),
                "{rendered}"
            );
        }

        for (schema, ceiling) in [
            (TOPOLOGY_SCHEMA, 2_u32),
            (TOPOLOGY_SCHEMA, 0),
            (TOPOLOGY_SCHEMA, LATEST_LEGACY_SCHEMA),
        ] {
            let rendered = SchemaRefusal::TopologyLogUnreadable { schema, ceiling }.to_string();
            assert!(
                rendered.contains(&format!("event schema {schema}")),
                "{rendered}"
            );
            assert!(
                rendered.contains(&format!("reads up to schema {ceiling}")),
                "{rendered}"
            );
            assert!(
                rendered.contains(&format!("schema-{ceiling} run")),
                "the sentence about what the log will never become names the wrong \
                 number: {rendered}"
            );
        }
    }

    #[test]
    fn the_no_upgrade_refusal_never_advises_the_upgrade_it_refuses() {
        for (from, to) in [(3_u32, TOPOLOGY_SCHEMA), (1, 4), (2, 9), (0, u32::MAX)] {
            let rendered = SchemaRefusal::NoUpgradePath { from, to }.to_string();
            assert!(rendered.contains(&format!("{from} -> {to}")), "{rendered}");
            assert!(
                rendered.contains("start a new run"),
                "the refusal does not say what to do instead: {rendered}"
            );
            assert!(
                !rendered.contains("continue"),
                "the refusal advises continuing the existing run: {rendered}"
            );
            assert!(
                !rendered.contains("append"),
                "the refusal advises appending the transition it refuses: {rendered}"
            );
        }

        let not_an_upgrade = SchemaRefusal::NotAnUpgrade { from: 2, to: 2 }.to_string();
        assert!(!not_an_upgrade.contains("continue"), "{not_an_upgrade}");
        assert!(!not_an_upgrade.contains("append"), "{not_an_upgrade}");
    }

    #[test]
    fn the_activation_constant_is_asserted_outside_the_test_configuration() {
        const { assert!(matches!(TOPOLOGY_ACTIVATION, TopologyActivation::Inactive)) };
        const { assert!(MAX_READABLE_SCHEMA == 3) };
        assert_eq!(TOPOLOGY_ACTIVATION, TopologyActivation::Inactive);
        assert_eq!(MAX_READABLE_SCHEMA, LATEST_LEGACY_SCHEMA);
        assert!(select_reader(&committed(RUN_STARTED, Some(TOPOLOGY_SCHEMA))).is_err());
    }
}
