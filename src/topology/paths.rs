//! Extended notes: `docs/internals/topology/paths.md`

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathPolicy {
    pub version: PathPolicyVersion,
    pub case_fold: bool,
    pub grammar: PathGrammar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathPolicyVersion {
    V1,
    V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathGrammar {
    Globset,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GitPath(pub String);

impl GitPath {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for GitPath {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl fmt::Display for GitPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "region", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathSet {
    RepoWide,
    Prefixes { paths: Vec<GitPath> },
}

impl PathSet {
    pub fn is_repo_wide(&self) -> bool {
        matches!(self, Self::RepoWide)
    }

    pub fn prefixes(&self) -> Option<&[GitPath]> {
        match self {
            Self::RepoWide => None,
            Self::Prefixes { paths } => Some(paths),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hostile_prefixes() -> Vec<GitPath> {
        vec![
            GitPath::from("src/Zebra/ÜBER.rs"),
            GitPath::from("  leading-and-trailing  "),
            GitPath::from("Docs/ADR/0001-ünicode-decisions.md"),
            GitPath::from(
                "a/very/deep/directory/chain/that/keeps/going/well/past/any/plausible/buffer/size/file.rs",
            ),
            GitPath::from("build.rs"),
        ]
    }

    fn hostile_policy() -> PathPolicy {
        PathPolicy {
            version: PathPolicyVersion::V2,
            case_fold: true,
            grammar: PathGrammar::Globset,
        }
    }

    #[test]
    fn path_policy_round_trips_every_field_it_records() {
        let policy = hostile_policy();
        let json = serde_json::to_string(&policy).expect("serialize");
        assert_eq!(
            serde_json::from_str::<PathPolicy>(&json).expect("deserialize"),
            policy
        );
        assert!(json.contains(r#""version":"v2""#), "{json}");
        assert!(json.contains(r#""case_fold":true"#), "{json}");
        assert!(json.contains(r#""grammar":"globset""#), "{json}");
    }

    #[test]
    fn a_path_policy_refuses_an_unknown_field() {
        let json = r#"{"version":"v2","case_fold":true,"grammar":"globset","ordering":"lexical"}"#;
        assert!(serde_json::from_str::<PathPolicy>(json).is_err());
    }

    #[test]
    fn an_unknown_key_stays_unknown_when_it_replaces_a_required_field_as_well_as_when_it_joins_one()
    {
        let intruders: [(&str, &str, &str); 3] = [
            ("version", "policy_version", r#""v2""#),
            ("case_fold", "fold_case", "true"),
            ("grammar", "ordering", r#""globset""#),
        ];
        for (required, intruder, value) in intruders {
            let mut replaced: serde_json::Value =
                serde_json::from_str(r#"{"version":"v2","case_fold":true,"grammar":"globset"}"#)
                    .expect("fixture parses");
            let object = replaced.as_object_mut().expect("object");
            object.remove(required).expect("field present");
            object.insert(
                intruder.to_owned(),
                serde_json::from_str(value).expect("value parses"),
            );
            assert!(
                serde_json::from_value::<PathPolicy>(replaced).is_err(),
                "`{intruder}` was accepted in place of `{required}`",
            );

            let mut added: serde_json::Value =
                serde_json::from_str(r#"{"version":"v2","case_fold":true,"grammar":"globset"}"#)
                    .expect("fixture parses");
            added.as_object_mut().expect("object").insert(
                intruder.to_owned(),
                serde_json::from_str(value).expect("value parses"),
            );
            assert!(
                serde_json::from_value::<PathPolicy>(added).is_err(),
                "`{intruder}` was accepted alongside `{required}`",
            );
        }
    }

    #[test]
    fn both_case_fold_values_survive_the_wire_exactly_as_written() {
        let expectations = [
            (
                false,
                r#"{"version":"v2","case_fold":false,"grammar":"globset"}"#,
            ),
            (
                true,
                r#"{"version":"v2","case_fold":true,"grammar":"globset"}"#,
            ),
        ];
        for (case_fold, expected) in expectations {
            let policy = PathPolicy {
                version: PathPolicyVersion::V2,
                case_fold,
                grammar: PathGrammar::Globset,
            };
            assert_eq!(
                serde_json::to_string(&policy).expect("serialize"),
                expected,
                "case_fold {case_fold} did not serialize to the frozen payload"
            );
            assert_eq!(
                serde_json::from_str::<PathPolicy>(expected).expect("deserialize"),
                policy,
                "the frozen payload for case_fold {case_fold} did not decode to it"
            );
            assert_eq!(policy.case_fold, case_fold);
        }
        assert_ne!(expectations[0].1, expectations[1].1);
    }

    #[test]
    fn an_unsupported_policy_version_or_grammar_spelling_is_refused_rather_than_folded_into_one() {
        for version in ["v3", "V1", "v1 ", "", "v10", "v0"] {
            let json = format!(r#"{{"version":"{version}","case_fold":true,"grammar":"globset"}}"#);
            assert!(
                serde_json::from_str::<PathPolicy>(&json).is_err(),
                "version `{version}` was accepted",
            );
        }
        for grammar in ["globset2", "Globset", "glob", "", "globset "] {
            let json = format!(r#"{{"version":"v2","case_fold":true,"grammar":"{grammar}"}}"#);
            assert!(
                serde_json::from_str::<PathPolicy>(&json).is_err(),
                "grammar `{grammar}` was accepted",
            );
        }
        assert_eq!(
            serde_json::from_str::<PathPolicy>(
                r#"{"version":"v2","case_fold":true,"grammar":"globset"}"#
            )
            .expect("the canonical policy decodes"),
            hostile_policy()
        );
        assert_eq!(
            serde_json::to_string(&PathPolicyVersion::V2).expect("serialize"),
            r#""v2""#
        );
        assert_eq!(
            serde_json::to_string(&PathPolicyVersion::V1).expect("serialize"),
            r#""v1""#
        );
        assert_eq!(
            serde_json::from_str::<PathPolicyVersion>(r#""v1""#).expect("v1 decodes"),
            PathPolicyVersion::V1,
            "a run recorded under v1 has to decode before the fold can refuse it by name"
        );
        assert_eq!(
            serde_json::to_string(&PathGrammar::Globset).expect("serialize"),
            r#""globset""#
        );
    }

    #[test]
    fn a_path_policy_refuses_a_missing_field_rather_than_defaulting_it() {
        for absent in ["version", "case_fold", "grammar"] {
            let mut value: serde_json::Value =
                serde_json::from_str(r#"{"version":"v2","case_fold":true,"grammar":"globset"}"#)
                    .expect("fixture parses");
            value
                .as_object_mut()
                .expect("object")
                .remove(absent)
                .expect("field present");
            assert!(
                serde_json::from_value::<PathPolicy>(value).is_err(),
                "a policy without `{absent}` was accepted"
            );
        }
    }

    #[test]
    fn the_three_regions_are_distinguishable_on_the_wire() {
        let repo_wide = PathSet::RepoWide;
        let empty = PathSet::Prefixes { paths: Vec::new() };
        let bounded = PathSet::Prefixes {
            paths: hostile_prefixes(),
        };

        let rendered: Vec<String> = [&repo_wide, &empty, &bounded]
            .iter()
            .map(|set| serde_json::to_string(set).expect("serialize"))
            .collect();
        assert_ne!(rendered[0], rendered[1]);
        assert_ne!(rendered[1], rendered[2]);
        assert_ne!(rendered[0], rendered[2]);

        for (set, json) in [&repo_wide, &empty, &bounded].iter().zip(&rendered) {
            assert_eq!(
                &&serde_json::from_str::<PathSet>(json).expect("deserialize"),
                set
            );
        }
        assert!(
            rendered[0].contains(r#""region":"repo_wide""#),
            "{}",
            rendered[0]
        );
        assert!(
            rendered[1].contains(r#""region":"prefixes""#),
            "{}",
            rendered[1]
        );
    }

    #[test]
    fn the_unbounded_region_is_the_only_one_without_prefixes() {
        assert!(PathSet::RepoWide.is_repo_wide());
        assert_eq!(PathSet::RepoWide.prefixes(), None);

        let empty = PathSet::Prefixes { paths: Vec::new() };
        assert!(!empty.is_repo_wide());
        assert_eq!(empty.prefixes(), Some(&[][..]));

        let bounded = PathSet::Prefixes {
            paths: hostile_prefixes(),
        };
        assert!(!bounded.is_repo_wide());
        assert_eq!(bounded.prefixes(), Some(&hostile_prefixes()[..]));
    }

    #[test]
    fn prefixes_survive_in_the_order_and_bytes_they_were_recorded_in() {
        let bounded = PathSet::Prefixes {
            paths: hostile_prefixes(),
        };
        let json = serde_json::to_string(&bounded).expect("serialize");
        let back: PathSet = serde_json::from_str(&json).expect("deserialize");
        let paths = back.prefixes().expect("bounded");
        assert_eq!(paths, hostile_prefixes());
        assert_eq!(paths[0].as_str(), "src/Zebra/ÜBER.rs");
        assert_eq!(paths[1].as_str(), "  leading-and-trailing  ");
        assert_eq!(paths[4].as_str(), "build.rs");

        let mut sorted = hostile_prefixes();
        sorted.sort();
        assert_ne!(paths, sorted, "the fixture must not already be sorted");
    }

    const LONG_PREFIX_LITERAL: &str =
        "a/very/deep/directory/chain/that/keeps/going/well/past/any/plausible/buffer/size/file.rs";

    const HOSTILE_REGION_JSON: &str = concat!(
        r#"{"region":"prefixes","paths":["#,
        r#""src/Zebra/ÜBER.rs","#,
        r#""  leading-and-trailing  ","#,
        r#""Docs/ADR/0001-ünicode-decisions.md","#,
        r#""a/very/deep/directory/chain/that/keeps/going/well/past/any/plausible/buffer/size/file.rs","#,
        r#""build.rs""#,
        r#"]}"#
    );

    #[test]
    fn an_over_length_path_keeps_every_byte_it_was_given() {
        assert_eq!(LONG_PREFIX_LITERAL.len(), 88);
        assert!(
            LONG_PREFIX_LITERAL.len() > 64,
            "the fixture must exceed any plausible buffer"
        );

        let path = GitPath::from(LONG_PREFIX_LITERAL);
        assert_eq!(path.as_str(), LONG_PREFIX_LITERAL);
        assert_eq!(path.as_str().len(), 88);
        assert_eq!(path.to_string(), LONG_PREFIX_LITERAL);
        assert!(path.as_str().ends_with("size/file.rs"), "{path}");

        assert_eq!(
            serde_json::to_string(&path).expect("serialize"),
            format!("\"{LONG_PREFIX_LITERAL}\"")
        );

        let recorded = PathSet::Prefixes {
            paths: hostile_prefixes(),
        };
        let paths = recorded.prefixes().expect("bounded");
        assert_eq!(paths[3].as_str(), LONG_PREFIX_LITERAL);
        assert_eq!(paths[2].as_str(), "Docs/ADR/0001-ünicode-decisions.md");
        assert_eq!(paths.len(), 5);
        let json = serde_json::to_string(&recorded).expect("serialize");
        assert!(
            json.contains(LONG_PREFIX_LITERAL),
            "the recorded region lost bytes of its longest prefix: {json}"
        );
    }

    #[test]
    fn every_region_encodes_to_the_payload_written_out_here_and_decodes_from_it() {
        let cases: [(PathSet, &str); 3] = [
            (PathSet::RepoWide, r#"{"region":"repo_wide"}"#),
            (
                PathSet::Prefixes { paths: Vec::new() },
                r#"{"region":"prefixes","paths":[]}"#,
            ),
            (
                PathSet::Prefixes {
                    paths: hostile_prefixes(),
                },
                HOSTILE_REGION_JSON,
            ),
        ];
        for (set, expected) in cases {
            assert_eq!(
                serde_json::to_string(&set).expect("serialize"),
                expected,
                "{set:?} did not serialize to its frozen payload"
            );
            assert_eq!(
                serde_json::from_str::<PathSet>(expected).expect("deserialize"),
                set,
                "the frozen payload did not decode to {set:?}"
            );
        }
    }

    #[test]
    fn a_git_path_is_transparent_on_the_wire() {
        let path = GitPath::from("src/Zebra/ÜBER.rs");
        assert_eq!(
            serde_json::to_string(&path).expect("serialize"),
            r#""src/Zebra/ÜBER.rs""#
        );
        assert_eq!(path.to_string(), "src/Zebra/ÜBER.rs");
    }
}
