//! Extended notes: `docs/internals/runner/host/tests.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::runner::invocation::{AttemptRole, InvocationId};
use crate::runner::policy::resolve_host;
use crate::runner::{HarnessHooks, ProbeTarget, SPAWN_SITE};
use crate::topology::effects::{HookHarness, Injection, InjectionMode, Platform, SubEffectPoint};
use crate::topology::events::{AttemptNumber, GenerationId};
use crate::topology::registry::TaskKey;

fn os(value: &str) -> OsString {
    OsString::from(value)
}

fn gate_invocation() -> InvocationId {
    InvocationId::attempt(
        TaskKey(0),
        GenerationId(0),
        AttemptNumber(1),
        AttemptRole::Gate(0),
        0,
    )
}

fn shell_probe_invocation() -> InvocationId {
    InvocationId::probe(ProbeTarget::Shell, 0).expect("the shell probe identity")
}

fn worker_invocation() -> InvocationId {
    InvocationId::attempt(
        TaskKey(0),
        GenerationId(0),
        AttemptNumber(1),
        AttemptRole::Worker,
        0,
    )
}

fn review_invocation() -> InvocationId {
    InvocationId::attempt(
        TaskKey(0),
        GenerationId(0),
        AttemptNumber(1),
        AttemptRole::ReviewPass(0),
        0,
    )
}

fn fixture_agent() -> AgentId {
    AgentId::new(claude::ADAPTER_ID)
}

fn synthetic_base() -> Vec<(OsString, OsString)> {
    vec![
        (os("PATH"), os("/usr/bin:/bin")),
        (os("HOME"), os("/home/upstroke")),
        (os("USERPROFILE"), os("C:\\Users\\upstroke")),
        (os("CLAUDE_CONFIG_DIR"), os("/home/upstroke/.claude")),
        (os("COPILOT_HOME"), os("/home/upstroke/.copilot")),
        (os("CODEX_HOME"), os("/home/upstroke/.codex")),
        (os("LANG"), os("C.UTF-8")),
        (os("UPSTROKE_RUN"), os("01ABCDEF")),
    ]
}

fn value<'a>(composed: &'a [(OsString, OsString)], key: &str, case: KeyCase) -> Option<&'a OsStr> {
    composed
        .iter()
        .find(|(name, _)| case.same_key(name, OsStr::new(key)))
        .map(|(_, v)| v.as_os_str())
}

fn native() -> ShellKind {
    ShellKind::native()
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "upstroke-pr4-{tag}-{}-{}",
        std::process::id(),
        crate::ulid::ulid()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

#[test]
fn new_resolves_the_same_record_as_resolve_host() {
    let runner = HostRunner::new();
    let resolved = resolve_host().expect("host policy resolves");
    assert_eq!(runner.policy(), &resolved);
    assert_eq!(
        runner.policy_digest(),
        crate::runner::policy::runner_policy_sha256(&resolved)
    );
}

#[test]
fn environment_composition_fixtures() {
    struct Fixture {
        name: &'static str,
        role: ExecutionRole,
        agent: Option<AgentId>,
        overlay: Vec<(String, String)>,
        expect: Vec<(&'static str, Option<&'static str>)>,
    }

    let fixtures = vec![
        Fixture {
            name: "a gate inherits the base and is handed no credential location",
            role: ExecutionRole::Gate,
            agent: None,
            overlay: Vec::new(),
            expect: vec![
                ("PATH", Some("/usr/bin:/bin")),
                ("HOME", Some("/home/upstroke")),
                ("LANG", Some("C.UTF-8")),
                ("UPSTROKE_RUN", Some("01ABCDEF")),
                ("CLAUDE_CONFIG_DIR", None),
                ("COPILOT_HOME", None),
                ("CODEX_HOME", None),
            ],
        },
        Fixture {
            name: "a gate that names an agent is still handed no credential location",
            role: ExecutionRole::Gate,
            agent: Some(AgentId::new(codex::ADAPTER_ID)),
            overlay: Vec::new(),
            expect: vec![
                ("PATH", Some("/usr/bin:/bin")),
                ("CODEX_HOME", None),
                ("CLAUDE_CONFIG_DIR", None),
                ("COPILOT_HOME", None),
            ],
        },
        Fixture {
            name: "an overlay key absent from the base is added",
            role: ExecutionRole::Implement,
            agent: Some(AgentId::new(claude::ADAPTER_ID)),
            overlay: vec![(
                "CLAUDE_CODE_MAX_OUTPUT_TOKENS".to_owned(),
                "8000".to_owned(),
            )],
            expect: vec![
                ("CLAUDE_CODE_MAX_OUTPUT_TOKENS", Some("8000")),
                ("CLAUDE_CONFIG_DIR", Some("/home/upstroke/.claude")),
                ("PATH", Some("/usr/bin:/bin")),
                ("CODEX_HOME", None),
                ("COPILOT_HOME", None),
            ],
        },
        Fixture {
            name: "an overlay key present in the base overrides it",
            role: ExecutionRole::Review,
            agent: Some(AgentId::new(codex::ADAPTER_ID)),
            overlay: vec![("LANG".to_owned(), "en_GB.UTF-8".to_owned())],
            expect: vec![
                ("LANG", Some("en_GB.UTF-8")),
                ("CODEX_HOME", Some("/home/upstroke/.codex")),
                ("CLAUDE_CONFIG_DIR", None),
                ("COPILOT_HOME", None),
                ("PATH", Some("/usr/bin:/bin")),
                ("HOME", Some("/home/upstroke")),
                ("USERPROFILE", Some("C:\\Users\\upstroke")),
            ],
        },
        Fixture {
            name: "the shell probe binds no agent and gets the base minus every credential",
            role: ExecutionRole::Probe(ProbeTarget::Shell),
            agent: None,
            overlay: Vec::new(),
            expect: vec![
                ("PATH", Some("/usr/bin:/bin")),
                ("HOME", Some("/home/upstroke")),
                ("LANG", Some("C.UTF-8")),
                ("CODEX_HOME", None),
                ("CLAUDE_CONFIG_DIR", None),
                ("COPILOT_HOME", None),
            ],
        },
        Fixture {
            name: "an agent probe composes what the worker will compose",
            role: ExecutionRole::Probe(ProbeTarget::Agent(AgentId::new(copilot::ADAPTER_ID))),
            agent: Some(AgentId::new(copilot::ADAPTER_ID)),
            overlay: vec![("COPILOT_MODEL".to_owned(), "gpt-x".to_owned())],
            expect: vec![
                ("COPILOT_HOME", Some("/home/upstroke/.copilot")),
                ("COPILOT_MODEL", Some("gpt-x")),
                ("CODEX_HOME", None),
                ("CLAUDE_CONFIG_DIR", None),
            ],
        },
        Fixture {
            name: "an unknown agent has no credential location and that is not an error",
            role: ExecutionRole::Implement,
            agent: Some(AgentId::new("gemini")),
            overlay: Vec::new(),
            expect: vec![
                ("PATH", Some("/usr/bin:/bin")),
                ("CLAUDE_CONFIG_DIR", None),
                ("COPILOT_HOME", None),
                ("CODEX_HOME", None),
            ],
        },
    ];

    let roles: BTreeSet<String> = fixtures.iter().map(|f| f.role.label()).collect();
    let agents: BTreeSet<Option<String>> = fixtures
        .iter()
        .map(|f| f.agent.as_ref().map(|a| a.as_str().to_owned()))
        .collect();
    let overlay_sizes: BTreeSet<usize> = fixtures.iter().map(|f| f.overlay.len()).collect();
    let overlay_keys: BTreeSet<String> = fixtures
        .iter()
        .flat_map(|f| f.overlay.iter().map(|(k, _)| k.clone()))
        .collect();
    assert_eq!(roles.len(), 5, "every role, both probe targets");
    assert_eq!(agents.len(), 5, "none, claude-code, codex, copilot, gemini");
    assert_eq!(overlay_sizes.len(), 2, "empty and non-empty overlays");
    assert_eq!(overlay_keys.len(), 3, "three distinct overlay keys");

    for case in KeyCase::ALL {
        let environment = HostEnvironment::with_base(synthetic_base(), *case);
        for fixture in &fixtures {
            let composed = environment
                .compose(&fixture.role, fixture.agent.as_ref(), &fixture.overlay)
                .unwrap_or_else(|error| panic!("{} ({case:?}) was refused: {error}", fixture.name));
            for (key, expected) in &fixture.expect {
                assert_eq!(
                    value(&composed, key, *case).map(|v| v.to_string_lossy().into_owned()),
                    expected.map(str::to_owned),
                    "{} ({case:?}): {key}",
                    fixture.name
                );
            }
            let mut seen: Vec<&OsString> = Vec::new();
            for (name, _) in &composed {
                assert!(
                    !seen.iter().any(|other| case.same_key(other, name)),
                    "{} ({case:?}): `{}` appears twice",
                    fixture.name,
                    name.to_string_lossy()
                );
                seen.push(name);
            }
        }
    }
}

#[test]
fn a_reserved_key_the_base_does_not_carry_is_not_supplied() {
    let environment =
        HostEnvironment::with_base(vec![(os("PATH"), os("/usr/bin"))], KeyCase::Sensitive);
    let composed = environment
        .compose(&ExecutionRole::Gate, None, &[])
        .expect("compose");
    assert_eq!(
        value(&composed, "PATH", KeyCase::Sensitive),
        Some(OsStr::new("/usr/bin"))
    );
    assert_eq!(value(&composed, "HOME", KeyCase::Sensitive), None);
    assert_eq!(value(&composed, "USERPROFILE", KeyCase::Sensitive), None);
}

#[test]
fn a_reserved_key_in_the_overlay_is_a_preflight_error() {
    let expected_reserved = [
        "PATH",
        "HOME",
        "USERPROFILE",
        "CLAUDE_CONFIG_DIR",
        "COPILOT_HOME",
        "CODEX_HOME",
    ];
    assert_eq!(
        reserved_keys(),
        expected_reserved,
        "the reserved set moved away from the one DESIGN.md and capacity.rs name"
    );

    for case in KeyCase::ALL {
        let environment = HostEnvironment::with_base(synthetic_base(), *case);
        for key in expected_reserved {
            for role in ExecutionRole::all() {
                let overlay = vec![(key.to_owned(), "/tmp/hijacked".to_owned())];
                let error = environment
                    .compose(&role, Some(&AgentId::new(claude::ADAPTER_ID)), &overlay)
                    .expect_err(&format!("{key} was accepted for {role} ({case:?})"));
                let message = error.to_string();
                assert!(
                    message.contains(key),
                    "the refusal must name the key: {message}"
                );
            }
        }
    }
}

#[test]
fn a_reserved_key_is_refused_at_every_position_in_the_overlay() {
    const HARMLESS: [&str; 3] = ["UPSTROKE_ALPHA", "UPSTROKE_BETA", "UPSTROKE_GAMMA"];
    let agent = AgentId::new(claude::ADAPTER_ID);
    let mut refusals = 0_usize;
    for case in KeyCase::ALL {
        let environment = HostEnvironment::with_base(synthetic_base(), *case);
        let harmless: Vec<(String, String)> = HARMLESS
            .iter()
            .chain(std::iter::once(&"UPSTROKE_DELTA"))
            .map(|key| ((*key).to_owned(), "harmless".to_owned()))
            .collect();
        environment
            .compose(&ExecutionRole::Implement, Some(&agent), &harmless)
            .expect("four harmless pairs compose");

        for key in reserved_keys() {
            for position in 0..=HARMLESS.len() {
                let mut overlay: Vec<(String, String)> = HARMLESS
                    .iter()
                    .map(|key| ((*key).to_owned(), "harmless".to_owned()))
                    .collect();
                overlay.insert(position, (key.to_owned(), "/tmp/hijacked".to_owned()));
                assert_eq!(overlay.len(), 4);
                let error = environment
                    .compose(&ExecutionRole::Implement, Some(&agent), &overlay)
                    .expect_err(&format!(
                        "{key} at position {position} of {} was accepted ({case:?})",
                        overlay.len()
                    ));
                assert!(
                    error.to_string().contains(key),
                    "the refusal must name the key: {error}"
                );
                refusals += 1;
            }
        }
    }
    assert_eq!(refusals, 6 * 4 * KeyCase::ALL.len());
}

#[test]
fn an_overlay_that_restates_a_reserved_key_with_the_runners_own_value_is_still_refused() {
    for case in KeyCase::ALL {
        let environment = HostEnvironment::with_base(synthetic_base(), *case);
        let agent = AgentId::new(codex::ADAPTER_ID);
        let mut equal_values = 0_usize;
        for key in reserved_keys() {
            let ours = environment
                .lookup(key)
                .unwrap_or_else(|| panic!("the synthetic base carries {key}"))
                .to_string_lossy()
                .into_owned();
            let overlay = vec![(key.to_owned(), ours.clone())];
            for role in ExecutionRole::all() {
                let error = environment
                    .compose(&role, Some(&agent), &overlay)
                    .expect_err(&format!(
                        "{role} ({case:?}) accepted an overlay restating {key}={ours}"
                    ));
                assert!(error.to_string().contains(key), "{error}");
            }
            equal_values += 1;
        }
        assert_eq!(
            equal_values, 6,
            "all six reserved keys, each at its own value"
        );
    }
}

#[test]
fn windows_treats_reserved_keys_case_insensitively_and_unix_does_not() {
    let overlay = vec![("Path".to_owned(), "C:\\hijack".to_owned())];

    let insensitive = HostEnvironment::with_base(synthetic_base(), KeyCase::Insensitive);
    assert!(
        insensitive.preflight(&overlay).is_err(),
        "`Path` is `PATH` on Windows and must be refused"
    );

    let sensitive = HostEnvironment::with_base(synthetic_base(), KeyCase::Sensitive);
    assert!(
        sensitive.preflight(&overlay).is_ok(),
        "`Path` and `PATH` are two variables on Unix"
    );
    let composed = insensitive
        .compose(
            &ExecutionRole::Gate,
            None,
            &[("lang".to_owned(), "de".to_owned())],
        )
        .expect("compose");
    assert_eq!(
        composed
            .iter()
            .filter(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case("LANG"))
            .count(),
        1,
        "`lang` and `LANG` are one variable on Windows"
    );
    let composed = sensitive
        .compose(
            &ExecutionRole::Gate,
            None,
            &[("lang".to_owned(), "de".to_owned())],
        )
        .expect("compose");
    assert_eq!(
        composed
            .iter()
            .filter(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case("LANG"))
            .count(),
        2,
        "`lang` and `LANG` are two variables on Unix"
    );
}

#[test]
fn probe_and_execution_compose_the_same_environment() {
    let environment = HostEnvironment::with_base(synthetic_base(), KeyCase::current());
    let agent = AgentId::new(claude::ADAPTER_ID);
    let overlay = vec![(
        "CLAUDE_CODE_MAX_OUTPUT_TOKENS".to_owned(),
        "8000".to_owned(),
    )];
    let probe = environment
        .compose(
            &ExecutionRole::Probe(ProbeTarget::Agent(agent.clone())),
            Some(&agent),
            &overlay,
        )
        .expect("probe composes");
    let execution = environment
        .compose(&ExecutionRole::Implement, Some(&agent), &overlay)
        .expect("execution composes");
    assert_eq!(probe, execution);
}

#[test]
#[ignore = "subprocess helper"]
fn environment_dump_helper() {
    if std::env::var_os("UPSTROKE_ENV_DUMP").is_none() {
        return;
    }
    let mut entries: Vec<String> = std::env::vars_os()
        .map(|(key, value)| format!("{}={}", key.to_string_lossy(), value.to_string_lossy()))
        .collect();
    entries.sort();
    println!("<<ENV");
    for entry in entries {
        println!("{entry}");
    }
    println!("ENV>>");
}

fn dumped_environment(runner: &HostRunner, request: &RunnerRequest) -> Vec<String> {
    let output = runner.run(request).expect("run the dump helper");
    assert_eq!(output.code, Some(0), "{output:?}");
    let body = output
        .stdout
        .split("<<ENV")
        .nth(1)
        .expect("the dump helper never ran")
        .split("ENV>>")
        .next()
        .expect("the dump was never terminated");
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[test]
fn a_probe_child_and_an_execution_child_of_one_adapter_carry_the_same_environment() {
    let agent = AgentId::new(claude::ADAPTER_ID);
    let mut base = synthetic_base();
    base.push((os("UPSTROKE_BASE_SENTINEL"), os("base-only-value")));
    let runner =
        HostRunner::new().with_environment(HostEnvironment::with_base(base, KeyCase::current()));
    let command = CommandSpec {
        program: std::env::current_exe()
            .expect("test executable")
            .to_string_lossy()
            .into_owned(),
        args: vec![
            "environment_dump_helper".to_owned(),
            "--ignored".to_owned(),
            "--nocapture".to_owned(),
        ],
        env: vec![
            ("UPSTROKE_ENV_DUMP".to_owned(), "1".to_owned()),
            (
                "UPSTROKE_OVERLAY_SENTINEL".to_owned(),
                "overlay-only-value".to_owned(),
            ),
        ],
        stdin: Vec::new(),
    };
    let probe = crate::agent::probe_request(
        claude::ADAPTER_ID,
        command.clone(),
        1,
        Duration::from_secs(120),
    )
    .expect("production builds the probe request");
    let workspace = scratch("probe-parity");
    let execution = crate::runner::worker_request(
        command,
        workspace.clone(),
        agent,
        Duration::from_secs(120),
        worker_invocation(),
    );

    let probe_environment = dumped_environment(&runner, &probe);
    let execution_environment = dumped_environment(&runner, &execution);
    let _ = std::fs::remove_dir_all(&workspace);

    for (label, environment) in [
        ("probe", &probe_environment),
        ("execution", &execution_environment),
    ] {
        assert!(
            environment.contains(&"UPSTROKE_BASE_SENTINEL=base-only-value".to_owned()),
            "the {label} child did not inherit the runner's base: {environment:?}"
        );
        assert!(
            environment.contains(&"UPSTROKE_OVERLAY_SENTINEL=overlay-only-value".to_owned()),
            "the {label} child did not receive the adapter's overlay: {environment:?}"
        );
        assert!(
            environment.len() >= 5,
            "the {label} child carried almost nothing, so equality below is \
             equality between two absences: {environment:?}"
        );
    }
    assert_eq!(
        probe_environment, execution_environment,
        "pre-flight certified an environment the attempt will not run in"
    );
}

#[test]
fn every_credential_supplied_role_composes_one_environment_per_binding() {
    let workspace = scratch("binding-parity");
    let mut base = synthetic_base();
    base.push((os("UPSTROKE_BASE_SENTINEL"), os("base-only-value")));
    for (_, key) in CREDENTIAL_LOCATIONS {
        base.push((os(key), os(&format!("/host/{key}"))));
    }
    let runner =
        HostRunner::new().with_environment(HostEnvironment::with_base(base, KeyCase::current()));

    let command = CommandSpec {
        program: std::env::current_exe()
            .expect("test executable")
            .to_string_lossy()
            .into_owned(),
        args: vec![
            "environment_dump_helper".to_owned(),
            "--ignored".to_owned(),
            "--nocapture".to_owned(),
        ],
        env: vec![
            ("UPSTROKE_ENV_DUMP".to_owned(), "1".to_owned()),
            (
                "UPSTROKE_OVERLAY_SENTINEL".to_owned(),
                "overlay-only-value".to_owned(),
            ),
        ],
        stdin: Vec::new(),
    };

    let bound: Vec<ExecutionRole> = ExecutionRole::all()
        .into_iter()
        .filter(supplies_credentials)
        .collect();
    assert_eq!(
        bound.len(),
        3,
        "`supplies_credentials` names three roles: {bound:?}"
    );

    let mut cells = 0_usize;
    for (id, key) in CREDENTIAL_LOCATIONS {
        let agent = AgentId::new(*id);
        let mut per_role: Vec<(String, Vec<String>)> = Vec::new();
        for role in &bound {
            let request = match role {
                ExecutionRole::Probe(ProbeTarget::Agent(_)) => {
                    crate::agent::probe_request(id, command.clone(), 1, Duration::from_secs(120))
                        .expect("production builds the probe request")
                }
                ExecutionRole::Implement => crate::runner::worker_request(
                    command.clone(),
                    workspace.clone(),
                    agent.clone(),
                    Duration::from_secs(120),
                    worker_invocation(),
                ),
                ExecutionRole::Review => crate::runner::review_request(
                    command.clone(),
                    workspace.clone(),
                    agent.clone(),
                    Duration::from_secs(120),
                    review_invocation(),
                ),
                other => panic!("`supplies_credentials` grew a role with no builder: {other}"),
            };
            assert_eq!(
                request.agent.as_ref(),
                Some(&agent),
                "{role}: production binds this role to the agent"
            );
            let environment = dumped_environment(&runner, &request);
            assert!(
                environment.contains(&"UPSTROKE_BASE_SENTINEL=base-only-value".to_owned()),
                "{id}/{role}: the base did not reach the child: {environment:?}"
            );
            assert!(
                environment.contains(&"UPSTROKE_OVERLAY_SENTINEL=overlay-only-value".to_owned()),
                "{id}/{role}: the overlay did not reach the child: {environment:?}"
            );
            assert!(
                environment
                    .iter()
                    .any(|line| line.starts_with(&format!("{key}="))),
                "{id}/{role}: no `{key}` reached the child, so pre-flight certifies \
                 a different credential location than the spending process: {environment:?}"
            );
            per_role.push((role.to_string(), environment));
            cells += 1;
        }

        let (first_role, first) = &per_role[0];
        for (role, environment) in &per_role[1..] {
            assert_eq!(
                environment, first,
                "{id}: `{role}` composes a different environment than `{first_role}`, \
                 so pre-flight certifies an environment that will not spend"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&workspace);
    assert_eq!(cells, 9, "three bound roles x three shipped bindings");
}

#[test]
fn the_reserved_values_every_role_gets_are_the_host_boundarys_own() {
    let base = synthetic_base();
    let environment = HostEnvironment::with_base(base.clone(), KeyCase::current());
    let agent = AgentId::new(claude::ADAPTER_ID);
    let roles = ExecutionRole::all();
    assert_eq!(roles.len(), 5, "the grid covers every role");

    let from_the_boundary = ["PATH", "HOME", "USERPROFILE"];
    assert_eq!(from_the_boundary.len(), 3);
    let in_the_base = |key: &str| -> OsString {
        base.iter()
            .find(|(name, _)| name.as_os_str() == OsStr::new(key))
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("the synthetic base carries {key}"))
    };

    for role in &roles {
        let supplied = environment.reserved_values(role, Some(&agent));
        for key in from_the_boundary {
            let value = supplied
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| panic!("{role} was not supplied `{key}` at all"));
            assert_eq!(
                value,
                in_the_base(key),
                "{role} was supplied a `{key}` the host boundary does not carry"
            );
        }
    }

    let certified: &[(&str, Vec<ExecutionRole>)] = &[
        (
            "DESIGN.md:263 — probe and execution compose the same reserved values",
            vec![
                ExecutionRole::Probe(ProbeTarget::Agent(agent.clone())),
                ExecutionRole::Implement,
                ExecutionRole::Review,
            ],
        ),
        (
            "the packet — gate-shell availability is checked inside the same boundary",
            vec![
                ExecutionRole::Probe(ProbeTarget::Shell),
                ExecutionRole::Gate,
            ],
        ),
    ];
    assert_eq!(
        certified
            .iter()
            .flat_map(|(_, group)| group.iter().cloned())
            .collect::<BTreeSet<_>>(),
        roles.iter().cloned().collect::<BTreeSet<_>>(),
        "the two groups must partition the five roles, or a role is unasserted"
    );
    for (passage, group) in certified {
        let first = environment.reserved_values(&group[0], Some(&agent));
        assert!(
            !first.is_empty(),
            "{passage}: the group's reserved values are empty, so equality below \
             is equality between two absences"
        );
        for role in group {
            assert_eq!(
                environment.reserved_values(role, Some(&agent)),
                first,
                "{passage}: {role} and {} were supplied different reserved values",
                group[0]
            );
        }
    }

    assert_eq!(
        reserved_keys().len(),
        6,
        "PATH, HOME, USERPROFILE and three credential locations — the reserved set is the \
         one an overlay may not name"
    );
    assert_eq!(
        reserved_keys().into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "PATH",
            "HOME",
            "USERPROFILE",
            "CLAUDE_CONFIG_DIR",
            "COPILOT_HOME",
            "CODEX_HOME",
        ])
    );
}

#[test]
fn credential_locations_are_role_scoped() {
    let environment = HostEnvironment::with_base(synthetic_base(), KeyCase::current());
    let agent = AgentId::new(codex::ADAPTER_ID);
    let expected: Vec<(ExecutionRole, bool)> = vec![
        (ExecutionRole::Implement, true),
        (ExecutionRole::Review, true),
        (
            ExecutionRole::Probe(ProbeTarget::Agent(agent.clone())),
            true,
        ),
        (ExecutionRole::Gate, false),
        (ExecutionRole::Probe(ProbeTarget::Shell), false),
    ];
    assert_eq!(
        expected.len(),
        ExecutionRole::all().len(),
        "every role in the grid, and no more"
    );
    assert_eq!(
        expected.iter().filter(|(_, gets)| *gets).count(),
        3,
        "three roles execute an agent CLI"
    );
    for (role, gets) in expected {
        let supplied = environment.reserved_values(&role, Some(&agent));
        let has = supplied.iter().any(|(key, _)| *key == "CODEX_HOME");
        assert_eq!(
            has,
            gets,
            "{role} was {} the agent's credential location",
            if has { "given" } else { "denied" }
        );
        assert_eq!(
            supplied.len(),
            if gets { 4 } else { 3 },
            "{role}: PATH, HOME, USERPROFILE{}",
            if gets {
                " and the credential location"
            } else {
                ""
            }
        );
    }
}

#[test]
fn compose_gives_a_child_the_credential_location_of_its_own_agent_and_no_other() {
    let all_credentials = [
        ("claude-code", "CLAUDE_CONFIG_DIR", "/home/upstroke/.claude"),
        ("copilot", "COPILOT_HOME", "/home/upstroke/.copilot"),
        ("codex", "CODEX_HOME", "/home/upstroke/.codex"),
    ];
    let runs_an_agent_cli = |role: &ExecutionRole| match role {
        ExecutionRole::Implement
        | ExecutionRole::Review
        | ExecutionRole::Probe(ProbeTarget::Agent(_)) => true,
        ExecutionRole::Gate | ExecutionRole::Probe(ProbeTarget::Shell) => false,
    };

    let mut supplied_count = 0_usize;
    let mut denied_count = 0_usize;
    for case in KeyCase::ALL {
        let environment = HostEnvironment::with_base(synthetic_base(), *case);
        for role in ExecutionRole::all() {
            for agent in [
                None,
                Some("claude-code"),
                Some("copilot"),
                Some("codex"),
                Some("gemini"),
            ] {
                let agent = agent.map(AgentId::new);
                let composed = environment
                    .compose(&role, agent.as_ref(), &[])
                    .expect("compose");
                for (id, key, expected) in all_credentials {
                    let bound = agent.as_ref().is_some_and(|a| a.as_str() == id);
                    let earned = bound && runs_an_agent_cli(&role);
                    let seen = value(&composed, key, *case)
                        .map(|value| value.to_string_lossy().into_owned());
                    assert_eq!(
                        seen,
                        earned.then(|| expected.to_owned()),
                        "{role} bound to {agent:?} ({case:?}): {key}"
                    );
                    if earned {
                        supplied_count += 1;
                    } else {
                        denied_count += 1;
                    }
                }
                for (key, expected) in [
                    ("PATH", "/usr/bin:/bin"),
                    ("HOME", "/home/upstroke"),
                    ("USERPROFILE", "C:\\Users\\upstroke"),
                ] {
                    assert_eq!(
                        value(&composed, key, *case).map(|v| v.to_string_lossy().into_owned()),
                        Some(expected.to_owned()),
                        "{role} bound to {agent:?} ({case:?}) lost `{key}`"
                    );
                }
                for (key, expected) in [("LANG", "C.UTF-8"), ("UPSTROKE_RUN", "01ABCDEF")] {
                    assert_eq!(
                        value(&composed, key, *case).map(|v| v.to_string_lossy().into_owned()),
                        Some(expected.to_owned()),
                        "{role} bound to {agent:?} ({case:?}) lost the unreserved `{key}`"
                    );
                }
            }
        }
    }
    assert_eq!(supplied_count + denied_count, 2 * 5 * 5 * 3);
    assert_eq!(
        supplied_count,
        2 * 3 * 3,
        "two name rules x three agent-CLI roles x the one bound agent each"
    );
}

const BASE_WITNESS: &[(&str, &str)] = &[
    ("UPSTROKE_PR4_BASE_WITNESS", "café=;value"),
    ("UPSTROKE_PR4_DRIVE_CWD_SENTINEL", "=D:=D:\\base;café"),
];

#[test]
#[ignore = "subprocess helper"]
fn base_witness_helper() {
    if std::env::var_os(BASE_WITNESS[0].0).is_none() {
        return;
    }
    let base = HostEnvironment::from_process();

    let expected: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    assert_eq!(
        base.base().len(),
        expected.len(),
        "from_process dropped or invented an entry in the child"
    );
    assert_eq!(
        base.base(),
        expected.as_slice(),
        "entry for entry, in a child whose environment this test chose"
    );

    let composed = base
        .compose(&ExecutionRole::Gate, None, &[])
        .expect("compose");
    for (key, want) in BASE_WITNESS {
        assert_eq!(
            base.base()
                .iter()
                .find(|(name, _)| name == OsStr::new(key))
                .map(|(_, value)| value.clone()),
            Some(OsString::from(*want)),
            "`{key}` never reached the composed base"
        );
        assert_eq!(
            value(&composed, key, KeyCase::current()),
            Some(OsStr::new(*want)),
            "`{key}` was lost composing a gate"
        );
    }
}

#[test]
fn the_base_of_a_process_environment_is_the_process_environment() {
    let expected: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let base = HostEnvironment::from_process();
    assert_eq!(base.case(), KeyCase::current());
    assert!(
        !expected.is_empty(),
        "a process with no environment cannot witness this"
    );
    assert_eq!(
        base.base().len(),
        expected.len(),
        "from_process dropped or invented an entry"
    );
    assert_eq!(
        base.base(),
        expected.as_slice(),
        "entry for entry, in order"
    );

    let mut child = Command::new(std::env::current_exe().expect("test executable"));
    child.args([
        "runner::host::tests::base_witness_helper",
        "--ignored",
        "--exact",
    ]);
    for (key, value) in BASE_WITNESS {
        child.env(key, value);
    }
    let out = child.output().expect("spawn the base-witness helper");
    let report = String::from_utf8_lossy(&out.stdout);
    assert!(
        report.contains("1 passed"),
        "the helper did not run, so this witnesses nothing: {}",
        report.trim()
    );
    assert!(
        out.status.success(),
        "an inherited variable did not survive collection into the base: {}\n{report}",
        out.status
    );
}

#[test]
fn the_credential_location_is_the_bound_agents_and_no_others_value() {
    let environment = HostEnvironment::with_base(synthetic_base(), KeyCase::current());
    for (agent, key, expected) in [
        ("claude-code", "CLAUDE_CONFIG_DIR", "/home/upstroke/.claude"),
        ("copilot", "COPILOT_HOME", "/home/upstroke/.copilot"),
        ("codex", "CODEX_HOME", "/home/upstroke/.codex"),
    ] {
        let agent = AgentId::new(agent);
        assert_eq!(credential_location(&agent), Some(key));
        let supplied = environment.reserved_values(&ExecutionRole::Implement, Some(&agent));
        assert!(
            supplied.contains(&(key, os(expected))),
            "{agent} did not receive {key}"
        );
        let others: Vec<&'static str> = CREDENTIAL_LOCATIONS
            .iter()
            .map(|(_, k)| *k)
            .filter(|k| *k != key)
            .collect();
        for other in others {
            assert!(
                !supplied.iter().any(|(k, _)| *k == other),
                "{agent} was also supplied {other}"
            );
        }
    }
    assert_eq!(credential_location(&AgentId::new("gemini")), None);
}

struct ParityFixture {
    name: &'static str,
    script: &'static str,
    stdin: &'static str,
    timeout: Duration,
    floor: Option<Duration>,
}

fn parity_fixtures() -> Vec<ParityFixture> {
    vec![
        ParityFixture {
            name: "stdout and exit 0",
            script: "echo hello",
            stdin: "",
            timeout: Duration::from_secs(30),
            floor: None,
        },
        ParityFixture {
            name: "non-zero exit",
            script: "exit 7",
            stdin: "",
            timeout: Duration::from_secs(30),
            floor: None,
        },
        ParityFixture {
            name: "stderr",
            script: "echo problem 1>&2",
            stdin: "",
            timeout: Duration::from_secs(30),
            floor: None,
        },
        ParityFixture {
            name: "a quoted argument survives the spec",
            script: "echo \"quoted arg\"",
            stdin: "",
            timeout: Duration::from_secs(30),
            floor: None,
        },
        ParityFixture {
            name: "stdin is delivered",
            script: if cfg!(windows) {
                "findstr /R \".*\""
            } else {
                "cat"
            },
            stdin: "payload-from-the-spec\n",
            timeout: Duration::from_secs(30),
            floor: None,
        },
        ParityFixture {
            name: "a sleeping child is measured, not the timeout",
            script: if cfg!(windows) {
                "ping -n 2 127.0.0.1 > NUL"
            } else {
                "sleep 1"
            },
            stdin: "",
            timeout: Duration::from_secs(30),
            floor: Some(Duration::from_millis(500)),
        },
        ParityFixture {
            name: "timeout kills the tree",
            script: if cfg!(windows) {
                "ping -n 30 127.0.0.1 > NUL"
            } else {
                "sleep 30"
            },
            stdin: "",
            timeout: Duration::from_millis(400),
            floor: Some(Duration::from_millis(400)),
        },
    ]
}

#[test]
fn supervision_parity_tests() {
    let workspace = scratch("parity");
    let shell = native();
    let runner = HostRunner::new();
    let mut codes = BTreeSet::new();
    let mut stdouts = BTreeSet::new();
    let mut stderr_nonempty = 0_usize;
    let mut timed_out = BTreeSet::new();

    for fixture in parity_fixtures() {
        let mut direct = shell.command(fixture.script);
        direct.current_dir(&workspace);
        let direct_started = std::time::Instant::now();
        let expected = proc::test_support::run_with_timeout(direct, fixture.stdin, fixture.timeout)
            .unwrap_or_else(|error| panic!("{}: direct supervision: {error}", fixture.name));
        let direct_elapsed = direct_started.elapsed();

        let template = shell.command(fixture.script);
        let command = CommandSpec {
            program: template.get_program().to_string_lossy().into_owned(),
            args: template
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            env: Vec::new(),
            stdin: fixture.stdin.as_bytes().to_vec(),
        };
        let request = RunnerRequest {
            command,
            workspace: workspace.clone(),
            role: ExecutionRole::Gate,
            timeout: fixture.timeout,
            agent: None,
            invocation: gate_invocation(),
        };
        let started = std::time::Instant::now();
        let actual = runner
            .run(&request)
            .unwrap_or_else(|error| panic!("{}: runner supervision: {error}", fixture.name));
        let elapsed = started.elapsed();

        assert_eq!(actual.code, expected.code, "{}: exit code", fixture.name);
        assert_eq!(actual.stdout, expected.stdout, "{}: stdout", fixture.name);
        assert_eq!(actual.stderr, expected.stderr, "{}: stderr", fixture.name);
        assert_eq!(
            actual.timed_out, expected.timed_out,
            "{}: timed_out",
            fixture.name
        );
        assert_eq!(
            actual.output_limited, expected.output_limited,
            "{}: output_limited",
            fixture.name
        );
        for (what, measured, bound) in [
            ("the runner", actual.duration, elapsed),
            ("direct supervision", expected.duration, direct_elapsed),
        ] {
            assert!(
                measured > Duration::ZERO,
                "{}: {what} reported a zero duration",
                fixture.name
            );
            assert!(
                measured <= bound,
                "{}: {what} reported {measured:?} for a call this test measured at {bound:?}",
                fixture.name
            );
            if let Some(floor) = fixture.floor {
                assert!(
                    measured >= floor,
                    "{}: {what} reported {measured:?}, less than the {floor:?} this child \
                     cannot have finished sooner than",
                    fixture.name
                );
            }
        }

        codes.insert(expected.code);
        stdouts.insert(expected.stdout.trim().to_owned());
        if !expected.stderr.trim().is_empty() {
            stderr_nonempty += 1;
        }
        timed_out.insert(expected.timed_out);
    }

    let _ = std::fs::remove_dir_all(&workspace);
    assert!(codes.len() >= 3, "distinct exit codes: {codes:?}");
    assert!(stdouts.len() >= 3, "distinct stdout values: {stdouts:?}");
    assert!(stderr_nonempty >= 1, "no fixture wrote to stderr");
    assert_eq!(timed_out.len(), 2, "both timeout outcomes are exercised");
    assert_eq!(
        parity_fixtures()
            .iter()
            .filter(|fixture| fixture.floor.is_some())
            .count(),
        2,
        "the fixtures that pin duration from below"
    );
}

const TRANSPARENT_STDOUT: &[&str] = &[
    r#"{"type":"thread.started","thread_id":"th-transparency"}"#,
    r#"{"type":"item.completed","item":{"type":"agent_message","text":"the verdict"}}"#,
    r#"{"type":"turn.completed","usage":{"input_tokens":11,"output_tokens":7}}"#,
];

const TRANSPARENT_STDERR: &[&str] = &["tracing line one", "tracing line two"];

fn transparency_shim(dir: &Path, name: &str) -> String {
    std::fs::create_dir_all(dir).expect("create the shim directory");
    let path = dir.join(name);
    let mut script = String::new();
    if cfg!(windows) {
        script.push_str("@echo off\r\n");
        for line in TRANSPARENT_STDOUT {
            script.push_str(&format!("echo {line}\r\n"));
        }
        for line in TRANSPARENT_STDERR {
            script.push_str(&format!("1>&2 echo {line}\r\n"));
        }
        script.push_str("exit /b 0\r\n");
    } else {
        script.push_str("#!/bin/sh\n");
        for line in TRANSPARENT_STDOUT {
            script.push_str(&format!("printf '%s\\n' '{line}'\n"));
        }
        for line in TRANSPARENT_STDERR {
            script.push_str(&format!("printf '%s\\n' '{line}' 1>&2\n"));
        }
        script.push_str("exit 0\n");
    }
    write_shim(&path, &script);
    path.to_str()
        .expect("a scratch path this crate can name")
        .to_owned()
}

fn captured_lines(stream: &str) -> Vec<String> {
    stream
        .replace("\r\n", "\n")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn transparency_run(agent: &str, resume: Option<&str>) -> crate::agent::TaskRun {
    crate::agent::TaskRun {
        prompt: "Do the thing.".to_owned(),
        profile: crate::ir::WorkerProfile {
            name: "impl-mid".to_owned(),
            agent: agent.to_owned(),
            model: "a-model".to_owned(),
            pool: "a-pool".to_owned(),
            permissions: crate::ir::PermissionMode::ReadOnly,
            effort: Some(crate::ir::Effort::Medium),
            max_turns: Some(30),
            extra_args: Vec::new(),
        },
        workspace: PathBuf::from("."),
        gate_cmds: Vec::new(),
        resume_session: resume.map(str::to_owned),
        settings_path: None,
    }
}

#[test]
fn the_runner_returns_the_childs_whole_output_for_every_production_request_shape() {
    let root = scratch("transparency");
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let shim = transparency_shim(
        &root,
        if cfg!(windows) {
            "upstroke-transparency.cmd"
        } else {
            "upstroke-transparency"
        },
    );
    let runner = HostRunner::new();

    let mut cells = 0_usize;
    let mut roles = BTreeSet::new();
    let mut agents = BTreeSet::new();
    let mut argv_heads = BTreeSet::new();

    for adapter in crate::agent::ADAPTERS {
        let id = adapter.id();
        for (what, args) in [
            ("fresh", build_args_for(id, None)),
            ("resumed", build_args_for(id, Some("session-1"))),
        ] {
            let command = CommandSpec {
                program: shim.clone(),
                args: args.clone(),
                env: Vec::new(),
                stdin: b"the materialized prompt\n".to_vec(),
            };
            for (role_name, request) in [
                (
                    "Implement",
                    crate::runner::worker_request(
                        command.clone(),
                        workspace.clone(),
                        crate::runner::AgentId::new(id),
                        crate::engine::DEFAULT_ATTEMPT_TIMEOUT,
                        worker_invocation(),
                    ),
                ),
                (
                    "Review",
                    crate::runner::review_request(
                        command.clone(),
                        workspace.clone(),
                        crate::runner::AgentId::new(id),
                        crate::config::DEFAULT_REVIEW_PASS_TIMEOUT,
                        review_invocation(),
                    ),
                ),
            ] {
                let cell = format!("{id}/{what}/{role_name}");
                let output = runner
                    .run(&request)
                    .unwrap_or_else(|error| panic!("{cell}: {error}"));
                assert_transparent(&cell, &output);
                roles.insert(role_name);
                agents.insert(id);
                argv_heads.insert(args.first().cloned().unwrap_or_default());
                cells += 1;
            }
        }
    }

    for adapter in crate::agent::ADAPTERS {
        let request = crate::agent::probe_request(
            adapter.id(),
            CommandSpec {
                program: shim.clone(),
                args: vec!["--version".to_owned()],
                env: Vec::new(),
                stdin: Vec::new(),
            },
            0,
            AGENT_PROBE_TIMEOUT,
        )
        .expect("a probe identity for a shipped adapter");
        let cell = format!("{}/probe", adapter.id());
        let output = runner
            .run(&request)
            .unwrap_or_else(|error| panic!("{cell}: {error}"));
        assert_transparent(&cell, &output);
        roles.insert("Probe");
        argv_heads.insert("--version".to_owned());
        cells += 1;
    }

    let script = if cfg!(windows) {
        TRANSPARENT_STDOUT
            .iter()
            .map(|line| format!("echo {line}"))
            .chain(
                TRANSPARENT_STDERR
                    .iter()
                    .map(|line| format!("1>&2 echo {line}")),
            )
            .collect::<Vec<_>>()
            .join("& ")
    } else {
        TRANSPARENT_STDOUT
            .iter()
            .map(|line| format!("printf '%s\\n' '{line}'"))
            .chain(
                TRANSPARENT_STDERR
                    .iter()
                    .map(|line| format!("printf '%s\\n' '{line}' 1>&2")),
            )
            .collect::<Vec<_>>()
            .join("; ")
    };
    let request = crate::runner::gate_request(
        native().spec(&script),
        workspace.clone(),
        crate::config::DEFAULT_GATE_TIMEOUT,
        gate_invocation(),
    );
    let output = runner.run(&request).expect("the gate role");
    assert_transparent("gate/shell", &output);
    roles.insert("Gate");
    cells += 1;

    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(
        cells, 16,
        "3 adapters x 2 shapes x 2 roles, 3 probes, 1 gate"
    );
    assert_eq!(roles.len(), 4, "four roles: {roles:?}");
    assert_eq!(agents.len(), 3, "all three shipped bindings: {agents:?}");
    assert!(
        argv_heads.contains("exec"),
        "Codex's own subcommand must be among the argument vectors sent: {argv_heads:?}"
    );
    assert!(
        argv_heads.len() >= 3,
        "the argument vector is an axis, not a constant: {argv_heads:?}"
    );
}

fn build_args_for(id: &str, resume: Option<&str>) -> Vec<String> {
    let run = transparency_run(id, resume);
    match id {
        "claude-code" => crate::agent::claude::build_args(&run),
        "codex" => crate::agent::codex::build_args(&run),
        "copilot" => crate::agent::copilot::build_args(&run),
        other => panic!("an adapter shipped without an entry here: {other}"),
    }
}

fn assert_transparent(cell: &str, output: &ProcessOutput) {
    assert_eq!(
        output.code,
        Some(0),
        "{cell}: the child exited 0: {output:?}"
    );
    assert!(!output.timed_out, "{cell}: not a timeout");
    assert!(!output.output_limited, "{cell}: not output-limited");
    assert_eq!(
        captured_lines(&output.stdout),
        TRANSPARENT_STDOUT
            .iter()
            .map(|line| (*line).to_owned())
            .collect::<Vec<_>>(),
        "{cell}: stdout is not what the child wrote"
    );
    assert_eq!(
        captured_lines(&output.stderr),
        TRANSPARENT_STDERR
            .iter()
            .map(|line| (*line).to_owned())
            .collect::<Vec<_>>(),
        "{cell}: stderr is not what the child wrote"
    );
    assert!(
        output.stdout.contains("thread.started"),
        "{cell}: the *first* line is gone, which is the session and the \
         verdict: {:?}",
        output.stdout
    );
}

#[test]
fn the_runner_bounds_output_at_the_same_allowance_the_direct_funnel_does() {
    const EXPECTED_LIMIT: usize = 16 * 1024 * 1024;
    const HELPER_BUDGET: usize = 64 * 1024 * 1024;
    const {
        assert!(
            HELPER_BUDGET > EXPECTED_LIMIT,
            "the helper must be able to overrun the allowance under test"
        );
    }
    let budget = HELPER_BUDGET.to_string();
    let exe = std::env::current_exe().expect("test executable");
    let helper_args = ["excessive_output_helper", "--ignored", "--nocapture"];

    let mut direct = Command::new(&exe);
    direct
        .args(helper_args)
        .env("UPSTROKE_EXCESSIVE_OUTPUT_HELPER", &budget);
    let started = std::time::Instant::now();
    let direct = proc::test_support::run_with_timeout(direct, "", Duration::from_secs(120))
        .expect("direct supervision of a noisy child");
    let direct_elapsed = started.elapsed();

    let workspace = scratch("output-limit");
    let request = RunnerRequest {
        command: CommandSpec {
            program: exe.to_string_lossy().into_owned(),
            args: helper_args.iter().map(|arg| (*arg).to_owned()).collect(),
            env: vec![(
                "UPSTROKE_EXCESSIVE_OUTPUT_HELPER".to_owned(),
                budget.clone(),
            )],
            stdin: Vec::new(),
        },
        workspace: workspace.clone(),
        role: ExecutionRole::Gate,
        timeout: Duration::from_secs(120),
        agent: None,
        invocation: gate_invocation(),
    };
    let started = std::time::Instant::now();
    let routed = HostRunner::new()
        .run(&request)
        .expect("runner supervision of a noisy child");
    let routed_elapsed = started.elapsed();
    let _ = std::fs::remove_dir_all(&workspace);

    for (name, output, elapsed) in [
        ("direct", &direct, direct_elapsed),
        ("routed", &routed, routed_elapsed),
    ] {
        assert!(
            output.output_limited,
            "{name}: the funnel did not bound this child's output: {output:?}"
        );
        assert!(!output.timed_out, "{name}: this is not a timeout");
        assert!(
            output.code.is_none(),
            "{name}: an output-limited tree is terminated, not exited"
        );
        assert!(
            output.stdout.len() <= EXPECTED_LIMIT,
            "{name}: captured {} bytes, more than the 16 MiB allowance",
            output.stdout.len()
        );
        assert!(
            output.stdout.len() >= EXPECTED_LIMIT / 2,
            "{name}: captured only {} bytes, so the allowance under test is not 16 MiB",
            output.stdout.len()
        );
        assert!(
            elapsed < Duration::from_secs(60),
            "{name}: the tree was not terminated promptly: {elapsed:?}"
        );
    }
}

#[test]
fn the_runner_executes_in_the_requested_workspace() {
    let workspace = scratch("cwd");
    let marker = workspace.join("marker.txt");
    let shell = native();
    let template = shell.command("echo here > marker.txt");
    let request = RunnerRequest {
        command: CommandSpec {
            program: template.get_program().to_string_lossy().into_owned(),
            args: template
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            env: Vec::new(),
            stdin: Vec::new(),
        },
        workspace: workspace.clone(),
        role: ExecutionRole::Gate,
        timeout: Duration::from_secs(30),
        agent: None,
        invocation: gate_invocation(),
    };
    let output = HostRunner::new().run(&request).expect("run");
    assert_eq!(output.code, Some(0), "{output:?}");
    assert!(marker.exists(), "the child did not run in the workspace");
    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn the_overlay_reaches_the_child_and_a_reserved_key_never_gets_that_far() {
    let workspace = scratch("overlay");
    let shell = native();
    let script = if cfg!(windows) {
        "echo [%UPSTROKE_OVERLAY_PROBE%]"
    } else {
        "echo \"[$UPSTROKE_OVERLAY_PROBE]\""
    };
    let template = shell.command(script);
    let spec = CommandSpec {
        program: template.get_program().to_string_lossy().into_owned(),
        args: template
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        env: vec![("UPSTROKE_OVERLAY_PROBE".to_owned(), "reached".to_owned())],
        stdin: Vec::new(),
    };
    let request = RunnerRequest {
        command: spec.clone(),
        workspace: workspace.clone(),
        role: ExecutionRole::Gate,
        timeout: Duration::from_secs(30),
        agent: None,
        invocation: gate_invocation(),
    };
    let output = HostRunner::new().run(&request).expect("run");
    assert!(
        output.stdout.contains("[reached]"),
        "the overlay did not reach the child: {output:?}"
    );

    let hijack = RunnerRequest {
        command: CommandSpec {
            env: vec![("PATH".to_owned(), "/nowhere".to_owned())],
            ..spec
        },
        ..request
    };
    let error = HostRunner::new()
        .run(&hijack)
        .expect_err("a reserved key must be refused before any spawn");
    assert!(error.to_string().contains("PATH"), "{error}");
    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn a_gate_child_is_told_where_no_agents_credentials_live() {
    let workspace = scratch("credential-scope");
    let shell = native();
    let script = if cfg!(windows) {
        "echo [%CODEX_HOME%][%CLAUDE_CONFIG_DIR%][%COPILOT_HOME%]"
    } else {
        "echo \"[$CODEX_HOME][$CLAUDE_CONFIG_DIR][$COPILOT_HOME]\""
    };
    let template = shell.command(script);
    let spec = CommandSpec {
        program: template.get_program().to_string_lossy().into_owned(),
        args: template
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        env: Vec::new(),
        stdin: Vec::new(),
    };
    let runner = HostRunner::new().with_environment(HostEnvironment::with_base(
        synthetic_base()
            .into_iter()
            .filter(|(name, _)| !KeyCase::current().same_key(name, OsStr::new("PATH")))
            .chain(std::env::vars_os().filter(|(name, _)| {
                KeyCase::current().same_key(name, OsStr::new("PATH"))
                    || KeyCase::current().same_key(name, OsStr::new("SYSTEMROOT"))
                    || KeyCase::current().same_key(name, OsStr::new("COMSPEC"))
            }))
            .collect(),
        KeyCase::current(),
    ));
    let request = RunnerRequest {
        command: spec.clone(),
        workspace: workspace.clone(),
        role: ExecutionRole::Gate,
        timeout: Duration::from_secs(30),
        agent: Some(AgentId::new(codex::ADAPTER_ID)),
        invocation: gate_invocation(),
    };
    let gate = runner.run(&request).expect("gate runs");
    assert_eq!(gate.code, Some(0), "{gate:?}");
    for value in [
        "/home/upstroke/.codex",
        "/home/upstroke/.claude",
        "/home/upstroke/.copilot",
    ] {
        assert!(
            !gate.stdout.contains(value),
            "a gate child was told where credentials live: {:?}",
            gate.stdout
        );
    }

    let worker = runner
        .run(&RunnerRequest {
            role: ExecutionRole::Implement,
            ..request
        })
        .expect("worker runs");
    assert_eq!(worker.code, Some(0), "{worker:?}");
    assert!(
        worker.stdout.contains("/home/upstroke/.codex"),
        "the worker was not told where its own credentials live: {:?}",
        worker.stdout
    );
    for value in ["/home/upstroke/.claude", "/home/upstroke/.copilot"] {
        assert!(
            !worker.stdout.contains(value),
            "a codex worker was told where another agent's credentials live: {:?}",
            worker.stdout
        );
    }
    let _ = std::fs::remove_dir_all(&workspace);
}

struct StubRunner(Box<dyn Fn() -> Result<ProcessOutput, UpstrokeError> + Send + Sync>);

impl Runner for StubRunner {
    fn run(&self, _request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        (self.0)()
    }
}

fn stub_output(code: Option<i32>, timed_out: bool) -> ProcessOutput {
    ProcessOutput {
        code,
        stdout: String::new(),
        stderr: "no such file or directory".to_owned(),
        duration: Duration::from_millis(1),
        timed_out,
        output_limited: false,
    }
}

#[test]
fn a_shell_probe_that_did_not_exit_zero_is_a_preflight_error_however_it_failed() {
    struct Case {
        name: &'static str,
        code: Option<i32>,
        timed_out: bool,
        output_limited: bool,
        ok: bool,
    }
    let cases = [
        Case {
            name: "exit 0 and nothing else",
            code: Some(0),
            timed_out: false,
            output_limited: false,
            ok: true,
        },
        Case {
            name: "a non-zero exit",
            code: Some(127),
            timed_out: false,
            output_limited: false,
            ok: false,
        },
        Case {
            name: "killed, with no exit code and no timeout",
            code: None,
            timed_out: false,
            output_limited: false,
            ok: false,
        },
        Case {
            name: "the probe timeout killed it",
            code: None,
            timed_out: true,
            output_limited: false,
            ok: false,
        },
        Case {
            name: "output-limited after exiting zero",
            code: Some(0),
            timed_out: false,
            output_limited: true,
            ok: false,
        },
        Case {
            name: "output-limited with no exit code",
            code: None,
            timed_out: false,
            output_limited: true,
            ok: false,
        },
    ];
    assert_eq!(cases.len(), 6);
    assert_eq!(
        cases.iter().filter(|case| case.ok).count(),
        1,
        "exactly one of these is a shell this pre-flight may certify"
    );

    for case in cases {
        let output = ProcessOutput {
            output_limited: case.output_limited,
            ..stub_output(case.code, case.timed_out)
        };
        let runner = StubRunner(Box::new(move || Ok(output.clone())));
        let result = run_shell_probe(
            &runner,
            ShellKind::Bash,
            PathBuf::from("."),
            shell_probe_invocation(),
        );
        assert_eq!(result.is_ok(), case.ok, "{}: {result:?}", case.name);
        if let Err(error) = result {
            let message = error.to_string();
            assert!(message.contains("pre-flight"), "{}: {message}", case.name);
            assert!(
                message.contains(ShellKind::Bash.program()),
                "{}: {message}",
                case.name
            );
        }
    }
}

const MISSING_SHELL: ShellKind = ShellKind::Pwsh;

const MISSING_SHELL_MARKER: &str = "UPSTROKE_MISSING_SHELL_PROBE";

const MISSING_SHELL_OK: &str = "<<MISSING-SHELL-REFUSED";

#[cfg(windows)]
fn windows_program_search_dirs() -> Vec<PathBuf> {
    let windows_dir = PathBuf::from(
        std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("windir"))
            .expect("a Windows host names its own directory"),
    );
    vec![
        std::env::current_exe()
            .expect("test executable")
            .parent()
            .expect("the application directory")
            .to_path_buf(),
        windows_dir.join("System32"),
        windows_dir,
    ]
}

#[test]
#[ignore = "subprocess helper"]
fn shell_probe_missing_shell_helper() {
    if std::env::var_os(MISSING_SHELL_MARKER).is_none() {
        return;
    }

    let path = std::env::var_os("PATH").expect("the parent supplies a PATH");
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    assert_eq!(dirs.len(), 1, "PATH must be one directory: {dirs:?}");
    assert!(dirs[0].is_dir(), "{} is not a directory", dirs[0].display());
    assert_eq!(
        std::fs::read_dir(&dirs[0])
            .expect("read the empty PATH directory")
            .count(),
        0,
        "the one directory on PATH is not empty: {}",
        dirs[0].display()
    );

    #[cfg(windows)]
    for dir in windows_program_search_dirs() {
        let candidate = dir.join(format!("{}.exe", MISSING_SHELL.program()));
        assert!(
            !candidate.exists(),
            "the premise of this case: {} exists, so `{}` is not missing on this machine \
             however empty PATH is",
            candidate.display(),
            MISSING_SHELL.program()
        );
    }

    let workspace = scratch("missing-shell");
    assert!(
        workspace.is_dir(),
        "the premise: {} must exist",
        workspace.display()
    );

    let error = HostRunner::new()
        .shell_probe(MISSING_SHELL, &workspace, shell_probe_invocation())
        .expect_err("a recorded shell that is not installed is a returned pre-flight error");
    let message = error.to_string();
    assert!(message.contains("pre-flight"), "{message}");
    assert!(message.contains(MISSING_SHELL.program()), "{message}");
    assert!(
        message.contains("could not be run through the runner"),
        "the refusal must be the spawn failure, not an exit code — a `{}` that ran and \
         returned non-zero would mean this machine has one after all: {message}",
        MISSING_SHELL.program()
    );
    assert!(
        workspace.is_dir(),
        "the workspace stopped existing during the probe, so this case no longer \
         composes an existing workspace with an absent shell"
    );
    let _ = std::fs::remove_dir_all(&workspace);
    println!("{MISSING_SHELL_OK} {message}");
}

#[test]
fn host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing() {
    let workspace = scratch("shell-probe");
    let runner = HostRunner::new();

    runner
        .shell_probe(native(), &workspace, shell_probe_invocation())
        .expect("the platform's native shell runs `exit 0`");

    let empty_path_dir = scratch("empty-path");
    assert_eq!(
        std::fs::read_dir(&empty_path_dir)
            .expect("read the empty PATH directory")
            .count(),
        0,
        "the directory this test puts on the child's PATH is not empty"
    );
    let helper = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "shell_probe_missing_shell_helper",
            "--ignored",
            "--nocapture",
        ])
        .env(MISSING_SHELL_MARKER, "1")
        .env("PATH", &empty_path_dir)
        .current_dir(&workspace)
        .output()
        .expect("run the missing-shell helper");
    let stdout = String::from_utf8_lossy(&helper.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&helper.stderr).into_owned();
    assert!(
        helper.status.success(),
        "the missing-shell helper failed:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    assert!(
        stdout.contains(MISSING_SHELL_OK),
        "the helper never reached its refusal:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed"),
        "the filter matched no test, so nothing was proved:\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&empty_path_dir);

    let absent = workspace.join(format!("absent-{}", crate::ulid::ulid()));
    assert!(
        !absent.exists(),
        "the premise of (c): {} must not exist",
        absent.display()
    );
    let error = runner
        .shell_probe(native(), &absent, shell_probe_invocation())
        .expect_err("a probe the host cannot spawn is a returned pre-flight error");
    let message = error.to_string();
    assert!(message.contains("pre-flight"), "{message}");
    assert!(message.contains(native().program()), "{message}");

    let mut request = shell_probe_request(native(), workspace.clone(), shell_probe_invocation());
    request.command.program = absent.join("shell").to_string_lossy().into_owned();
    assert!(
        !Path::new(&request.command.program).exists(),
        "the premise of (d): {} must not exist",
        request.command.program
    );
    let error = runner
        .run(&request)
        .expect_err("a program that is not there cannot be spawned");
    let message = error.to_string();
    assert!(message.contains("spawn"), "{message}");
    assert!(message.contains(&request.command.program), "{message}");

    let refusing = StubRunner(Box::new(|| Ok(stub_output(Some(127), false))));
    let error = run_shell_probe(
        &refusing,
        ShellKind::Bash,
        workspace.clone(),
        shell_probe_invocation(),
    )
    .expect_err("a non-zero exit is a failing probe");
    assert!(error.to_string().contains("127"), "{error}");

    let hanging = StubRunner(Box::new(|| Ok(stub_output(None, true))));
    let error = run_shell_probe(
        &hanging,
        ShellKind::Bash,
        workspace.clone(),
        shell_probe_invocation(),
    )
    .expect_err("a timed-out probe is a failing probe");
    assert!(error.to_string().contains("bash"), "{error}");

    let exploding = StubRunner(Box::new(|| {
        Err(UpstrokeError::Agent {
            message: "failed to spawn `bash`".to_owned(),
        })
    }));
    let error = run_shell_probe(
        &exploding,
        ShellKind::Bash,
        workspace.clone(),
        shell_probe_invocation(),
    )
    .expect_err("a spawn failure is a failing probe");
    assert!(error.to_string().contains("pre-flight"), "{error}");

    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn the_shell_probe_request_is_non_slotted_and_names_its_target() {
    let request = shell_probe_request(
        ShellKind::Sh,
        PathBuf::from("/tmp"),
        shell_probe_invocation(),
    );
    assert_eq!(request.role, ExecutionRole::Probe(ProbeTarget::Shell));
    assert!(
        !request.role.is_slotted(),
        "R3: the shell probe and gates are non-slotted"
    );
    assert_eq!(request.agent, None);
    assert_eq!(
        request.invocation.probe_target(),
        Some(&ProbeTarget::Shell),
        "the identity's own third form is (probe, target: Shell, ordinal)"
    );
    assert_eq!(request.invocation.render(), "p.shell.o0");
    assert_eq!(request.command.program, "sh");
    assert_eq!(
        request.command.args,
        vec!["-c".to_owned(), "exit 0".to_owned()]
    );
    assert!(request.command.env.is_empty(), "the probe adds no overlay");
}

#[test]
fn the_shell_probe_spells_every_shell_the_way_gates_do() {
    for shell in [
        ShellKind::Cmd,
        ShellKind::Sh,
        ShellKind::Bash,
        ShellKind::PowerShell,
        ShellKind::Pwsh,
    ] {
        let gate_form = shell.command(SHELL_PROBE_COMMAND);
        let expected_program = gate_form.get_program().to_string_lossy().into_owned();
        let expected_args: Vec<String> = gate_form
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let request = shell_probe_request(shell, PathBuf::from("/tmp"), shell_probe_invocation());
        assert_eq!(request.command.program, expected_program, "{shell:?}");
        assert_eq!(request.command.args, expected_args, "{shell:?}");
        assert!(
            expected_args.iter().any(|a| a.contains("exit 0")),
            "{shell:?} does not carry `exit 0`: {expected_args:?}"
        );
    }
}

const WINDOWS_POINTS: &[SubEffectPoint] = &[
    SubEffectPoint::AmbientJobJoined,
    SubEffectPoint::CreatedSuspended,
    SubEffectPoint::PrivateJobAssigned,
    SubEffectPoint::Resumed,
];
const UNIX_POINTS: &[SubEffectPoint] = &[
    SubEffectPoint::ReaperStarted,
    SubEffectPoint::PreExecPgidAndRegister,
    SubEffectPoint::Exec,
    SubEffectPoint::Registered,
];

#[test]
fn the_declared_platform_of_every_containment_point_matches_the_packet() {
    for point in WINDOWS_POINTS {
        assert_eq!(point.platform(), Platform::Windows, "{point}");
    }
    for point in UNIX_POINTS {
        assert_eq!(point.platform(), Platform::Unix, "{point}");
    }
    assert_eq!(
        WINDOWS_POINTS.len() + UNIX_POINTS.len(),
        SPAWN_SITE.sub_effects().len(),
        "Process.Spawn declares a containment point this table does not name"
    );
}

fn observed_points(runner_hooks: HarnessHooks) -> BTreeSet<SubEffectPoint> {
    let workspace = scratch("hooks");
    let harness = Arc::clone(runner_hooks.harness());
    let runner = HostRunner::new().with_hooks(Box::new(runner_hooks));
    let template = native().command("exit 0");
    let request = RunnerRequest {
        command: CommandSpec {
            program: template.get_program().to_string_lossy().into_owned(),
            args: template
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            env: Vec::new(),
            stdin: Vec::new(),
        },
        workspace: workspace.clone(),
        role: ExecutionRole::Gate,
        timeout: Duration::from_secs(30),
        agent: None,
        invocation: gate_invocation(),
    };
    let output = runner.run(&request).expect("run through the funnel");
    assert_eq!(output.code, Some(0), "{output:?}");
    let _ = std::fs::remove_dir_all(&workspace);
    let harness = harness.lock().expect("harness");
    SubEffectPoint::ALL
        .iter()
        .copied()
        .filter(|point| {
            point
                .modes()
                .iter()
                .any(|mode| harness.reached_point(SPAWN_SITE, *point, *mode))
        })
        .collect()
}

struct OrderedHooks(Arc<Mutex<Vec<SubEffectPoint>>>);

impl SpawnHooks for OrderedHooks {
    fn point(&mut self, point: SubEffectPoint) -> crate::topology::effects::Injection {
        self.0.lock().expect("order").push(point);
        crate::topology::effects::Injection::Proceed
    }
}

fn point_order() -> Vec<SubEffectPoint> {
    let workspace = scratch("hook-order");
    let order = Arc::new(Mutex::new(Vec::new()));
    let runner = HostRunner::new().with_hooks(Box::new(OrderedHooks(Arc::clone(&order))));
    let template = native().command("exit 0");
    let request = RunnerRequest {
        command: CommandSpec {
            program: template.get_program().to_string_lossy().into_owned(),
            args: template
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            env: Vec::new(),
            stdin: Vec::new(),
        },
        workspace: workspace.clone(),
        role: ExecutionRole::Gate,
        timeout: Duration::from_secs(30),
        agent: None,
        invocation: gate_invocation(),
    };
    let output = runner.run(&request).expect("run through the funnel");
    assert_eq!(output.code, Some(0), "{output:?}");
    let _ = std::fs::remove_dir_all(&workspace);
    order.lock().expect("order").clone()
}

#[test]
fn the_containment_points_of_a_spawn_fire_in_the_packets_order() {
    let observed = point_order();
    let expected: Vec<SubEffectPoint> = if cfg!(windows) {
        vec![
            SubEffectPoint::CreatedSuspended,
            SubEffectPoint::PrivateJobAssigned,
            SubEffectPoint::Resumed,
        ]
    } else {
        vec![
            SubEffectPoint::ReaperStarted,
            SubEffectPoint::PreExecPgidAndRegister,
            SubEffectPoint::Exec,
            SubEffectPoint::Registered,
        ]
    };
    assert_eq!(observed, expected, "the funnel's containment order moved");
    assert_eq!(
        observed.iter().collect::<BTreeSet<_>>().len(),
        observed.len(),
        "a containment point fired twice in one spawn: {observed:?}"
    );
}

#[cfg(unix)]
#[test]
fn unix_containment_points_execute_and_windows_points_do_not() {
    let hooks = HarnessHooks::new(Arc::new(Mutex::new(HookHarness::new())));
    let observed = observed_points(hooks);
    for point in UNIX_POINTS {
        assert!(
            observed.contains(point),
            "the Unix funnel never reached {point}; reached {observed:?}"
        );
    }
    for point in WINDOWS_POINTS {
        assert!(
            !observed.contains(point),
            "a Unix host reached the Windows containment point {point}"
        );
    }
    assert_eq!(observed.len(), UNIX_POINTS.len());
}

#[cfg(windows)]
#[test]
fn windows_containment_points_execute_and_unix_points_do_not() {
    let hooks = HarnessHooks::new(Arc::new(Mutex::new(HookHarness::new())));
    let observed = observed_points(hooks);
    for point in &[
        SubEffectPoint::CreatedSuspended,
        SubEffectPoint::PrivateJobAssigned,
        SubEffectPoint::Resumed,
    ] {
        assert!(
            observed.contains(point),
            "the Windows funnel never reached {point}; reached {observed:?}"
        );
    }
    for point in UNIX_POINTS {
        assert!(
            !observed.contains(point),
            "a Windows host reached the Unix containment point {point}"
        );
    }
    assert_eq!(observed.len(), 3);
}

#[test]
fn the_two_points_whose_operation_is_not_parent_side_are_named_and_counted() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Coordinate {
        Parent,
        ForkedChildBeforeExec,
        Child,
    }
    let table: &[(SubEffectPoint, Coordinate, Coordinate)] = &[
        (
            SubEffectPoint::AmbientJobJoined,
            Coordinate::Parent,
            Coordinate::Parent,
        ),
        (
            SubEffectPoint::CreatedSuspended,
            Coordinate::Parent,
            Coordinate::Parent,
        ),
        (
            SubEffectPoint::PrivateJobAssigned,
            Coordinate::Parent,
            Coordinate::Parent,
        ),
        (
            SubEffectPoint::Resumed,
            Coordinate::Parent,
            Coordinate::Parent,
        ),
        (
            SubEffectPoint::ReaperStarted,
            Coordinate::Parent,
            Coordinate::Parent,
        ),
        (
            SubEffectPoint::PreExecPgidAndRegister,
            Coordinate::ForkedChildBeforeExec,
            Coordinate::Parent,
        ),
        (SubEffectPoint::Exec, Coordinate::Child, Coordinate::Parent),
        (
            SubEffectPoint::Registered,
            Coordinate::Parent,
            Coordinate::Parent,
        ),
    ];
    let named: BTreeSet<SubEffectPoint> = table.iter().map(|(point, _, _)| *point).collect();
    let declared: BTreeSet<SubEffectPoint> =
        WINDOWS_POINTS.iter().chain(UNIX_POINTS).copied().collect();
    assert_eq!(named, declared, "the table covers the eight points");
    assert_eq!(table.len(), 8);

    let elsewhere: Vec<SubEffectPoint> = table
        .iter()
        .filter(|(_, operation, _)| *operation != Coordinate::Parent)
        .map(|(point, _, _)| *point)
        .collect();
    assert_eq!(
        elsewhere,
        vec![SubEffectPoint::PreExecPgidAndRegister, SubEffectPoint::Exec],
        "exactly these two operate outside the parent"
    );
    assert_eq!(elsewhere.len(), 2, "two of eight, and this is the count");
    assert!(
        table
            .iter()
            .all(|(_, _, injection)| *injection == Coordinate::Parent),
        "every fault is introduced parent-side"
    );
    for point in &elsewhere {
        assert_eq!(point.modes(), &[InjectionMode::Kill], "{point}");
    }
    assert_eq!(
        SubEffectPoint::AmbientJobJoined.modes(),
        InjectionMode::ALL,
        "the ambient join is the one containment point with an error contract"
    );
}

fn containment_points() -> Vec<SubEffectPoint> {
    let host = if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Unix
    };
    SPAWN_SITE
        .sub_effects()
        .iter()
        .copied()
        .filter(|point| point.platform() == host || point.platform() == Platform::Any)
        .collect()
}

fn per_spawn_points() -> Vec<SubEffectPoint> {
    containment_points()
        .into_iter()
        .filter(|point| !STARTUP_POINTS.contains(point))
        .collect()
}

const STARTUP_POINTS: &[SubEffectPoint] = &[SubEffectPoint::AmbientJobJoined];

#[test]
fn the_startup_and_per_spawn_domains_partition_this_platforms_points() {
    let all = containment_points();
    let per_spawn = per_spawn_points();
    let startup: Vec<SubEffectPoint> = all
        .iter()
        .copied()
        .filter(|point| STARTUP_POINTS.contains(point))
        .collect();
    assert_eq!(
        all.len(),
        per_spawn.len() + startup.len(),
        "a point is in both domains or in neither: all={all:?} per_spawn={per_spawn:?} \
         startup={startup:?}"
    );
    assert_eq!(all.len(), 4, "this platform's containment points: {all:?}");
    assert_eq!(
        startup.len(),
        usize::from(cfg!(windows)),
        "the ambient join is Windows' one startup point and Unix has none: {startup:?}"
    );
    let expected: &[SubEffectPoint] = if cfg!(windows) {
        WINDOWS_POINTS
    } else {
        UNIX_POINTS
    };
    assert_eq!(all, expected, "the derived domain left the packet's split");
}

fn shell_command() -> CommandSpec {
    native().spec(SHELL_PROBE_COMMAND)
}

const NO_SUCH_TEST: &str = "upstroke_pr4_role_grid_matches_no_test";

fn agent_cli_command(stdin: &[u8]) -> CommandSpec {
    agent_cli_command_at(&this_test_binary(), stdin)
}

fn this_test_binary() -> String {
    std::env::current_exe()
        .expect("this test binary's own path")
        .to_str()
        .expect("a target directory this crate can name")
        .to_owned()
}

fn agent_cli_command_at(program: &str, stdin: &[u8]) -> CommandSpec {
    CommandSpec {
        program: program.to_owned(),
        args: vec!["--exact".to_owned(), NO_SUCH_TEST.to_owned()],
        env: Vec::new(),
        stdin: stdin.to_vec(),
    }
}

struct ProgramShape {
    what: &'static str,
    command: CommandSpec,
    reports: bool,
}

fn forwarding_shim(dir: &Path, name: &str) -> String {
    std::fs::create_dir_all(dir).expect("create the shim directory");
    let path = dir.join(name);
    let exe = this_test_binary();
    let script = if cfg!(windows) {
        format!("@echo off\r\n\"{exe}\" %*\r\n")
    } else {
        format!("#!/bin/sh\nexec \"{exe}\" \"$@\"\n")
    };
    write_shim(&path, &script);
    path.to_str()
        .expect("a scratch path this crate can name")
        .to_owned()
}

const A_DIRECTORY_WITH_A_SPACE: &str = "John Smith";

fn program_shapes(root: &Path, stdin: &[u8]) -> Vec<ProgramShape> {
    let shell = shell_command();
    let mut shapes = vec![ProgramShape {
        what: "an absolute path to a native executable — `bin::locate` on a \
               normally-installed CLI",
        command: agent_cli_command(stdin),
        reports: true,
    }];
    if cfg!(windows) {
        shapes.push(ProgramShape {
            what: "an absolute `.cmd` batch shim — how npm installs `claude`, `codex` \
                   and `copilot` on Windows",
            command: agent_cli_command_at(&forwarding_shim(root, "upstroke-shim.cmd"), stdin),
            reports: true,
        });
        shapes.push(ProgramShape {
            what: "an absolute `.cmd` batch shim whose path contains a space — \
                   `C:\\Users\\John Smith\\npm\\copilot.cmd`, verbatim",
            command: agent_cli_command_at(
                &forwarding_shim(&root.join(A_DIRECTORY_WITH_A_SPACE), "upstroke-shim.cmd"),
                stdin,
            ),
            reports: true,
        });
        shapes.push(ProgramShape {
            what: "an absolute `.bat` batch shim — the other batch extension \
                   `CreateProcessW` routes through an interpreter",
            command: agent_cli_command_at(&forwarding_shim(root, "upstroke-shim.bat"), stdin),
            reports: true,
        });
    } else {
        shapes.push(ProgramShape {
            what: "an absolute shebang script with no extension — how npm installs a \
                   CLI on Unix",
            command: agent_cli_command_at(&forwarding_shim(root, "upstroke-shim"), stdin),
            reports: true,
        });
        shapes.push(ProgramShape {
            what: "an absolute shebang script whose path contains a space",
            command: agent_cli_command_at(
                &forwarding_shim(&root.join(A_DIRECTORY_WITH_A_SPACE), "upstroke-shim"),
                stdin,
            ),
            reports: true,
        });
    }
    shapes.push(ProgramShape {
        what: "a bare program name resolved on `PATH` — `gates::ShellKind::spec`, the \
               only production spec that is not an absolute path",
        command: CommandSpec {
            stdin: Vec::new(),
            ..shell
        },
        reports: false,
    });
    shapes
}

const WORKER_STDIN: &str = "## Task one\n\nthe materialized worker prompt, delivered on \
                            stdin the way `AgentAdapter::stdin_payload` says\n";

const REVIEW_STDIN: &str = "review the candidate diff and answer with the structured \
                            verdict\n";

const AGENT_PROBE_TIMEOUT: Duration = Duration::from_secs(60);

fn production_request(
    role: &ExecutionRole,
    workspace: &Path,
) -> Result<RunnerRequest, UpstrokeError> {
    Ok(match role {
        ExecutionRole::Probe(ProbeTarget::Shell) => {
            shell_probe_request(native(), workspace.to_path_buf(), shell_probe_invocation())
        }
        ExecutionRole::Probe(ProbeTarget::Agent(agent)) => crate::agent::probe_request(
            agent.as_str(),
            agent_cli_command(b""),
            0,
            AGENT_PROBE_TIMEOUT,
        )?,
        ExecutionRole::Implement => crate::runner::worker_request(
            agent_cli_command(WORKER_STDIN.as_bytes()),
            workspace.to_path_buf(),
            fixture_agent(),
            crate::engine::DEFAULT_ATTEMPT_TIMEOUT,
            worker_invocation(),
        ),
        ExecutionRole::Gate => crate::runner::gate_request(
            shell_command(),
            workspace.to_path_buf(),
            crate::config::DEFAULT_GATE_TIMEOUT,
            gate_invocation(),
        ),
        ExecutionRole::Review => crate::runner::review_request(
            agent_cli_command(REVIEW_STDIN.as_bytes()),
            workspace.to_path_buf(),
            fixture_agent(),
            crate::config::DEFAULT_REVIEW_PASS_TIMEOUT,
            review_invocation(),
        ),
    })
}

fn run_in_role(
    runner: &HostRunner,
    role: &ExecutionRole,
    workspace: &Path,
) -> Result<(), UpstrokeError> {
    if matches!(role, ExecutionRole::Probe(ProbeTarget::Shell)) {
        return runner.shell_probe(native(), workspace, shell_probe_invocation());
    }
    let request = production_request(role, workspace)?;
    let output = runner.run(&request)?;
    assert_eq!(output.code, Some(0), "{role}: {output:?}");
    Ok(())
}

struct RoleWitness {
    harness: Arc<Mutex<HookHarness>>,
    order: Arc<Mutex<Vec<SubEffectPoint>>>,
    children: Arc<Mutex<Vec<u32>>>,
    #[cfg(unix)]
    led_own_group: Arc<Mutex<Vec<proc::GroupObservation>>>,
}

#[cfg(unix)]
const GROUP_OBSERVATION_MARKER: &str = "<<GROUP-OBSERVATION";

// Printed on every call, not only inside a failing `assert_eq!`'s message, so
// a `--nocapture` run of a passing grid still carries the raw
// `proc::GroupObservation` records — including, on macOS, whatever
// `proc_pidinfo`'s non-zero argument answered — rather than only ever
// producing them on the (rare, on `leads_own_group`'s current predicate)
// failing path (PR173-LIVE-RECORD-ANSWER-NEVER-PRINTED). The predicate itself
// is unchanged by this: on macOS it accepts either a child that currently
// leads its own group or an exited, unreaped child whose own record names
// its own pid as leader.
#[cfg(unix)]
fn group_leadership(observations: &[proc::GroupObservation]) -> (Vec<bool>, String) {
    let leads = observations
        .iter()
        .map(proc::GroupObservation::leads_own_group)
        .collect();
    let seen = observations
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" | ");
    println!("{GROUP_OBSERVATION_MARKER} {seen}");
    (leads, seen)
}

impl RoleWitness {
    fn new() -> Self {
        Self {
            harness: Arc::new(Mutex::new(HookHarness::new())),
            order: Arc::new(Mutex::new(Vec::new())),
            children: Arc::new(Mutex::new(Vec::new())),
            #[cfg(unix)]
            led_own_group: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn handle(&self) -> Self {
        Self {
            harness: Arc::clone(&self.harness),
            order: Arc::clone(&self.order),
            children: Arc::clone(&self.children),
            #[cfg(unix)]
            led_own_group: Arc::clone(&self.led_own_group),
        }
    }
}

impl SpawnHooks for RoleWitness {
    fn point(&mut self, point: SubEffectPoint) -> Injection {
        self.order.lock().expect("order").push(point);
        HarnessHooks::new(Arc::clone(&self.harness)).point(point)
    }

    #[cfg(unix)]
    fn child_created(&mut self, pid: u32) {
        self.children.lock().expect("children").push(pid);
        self.led_own_group
            .lock()
            .expect("groups")
            .push(proc::observe_child_group(pid));
    }

    #[cfg(windows)]
    fn child_created(&mut self, pid: u32) {
        self.children.lock().expect("children").push(pid);
    }
}

#[test]
fn the_role_grid_sends_the_shapes_production_sends() {
    let workspace = scratch("role-shapes");
    let roles = ExecutionRole::all();
    let requests: Vec<RunnerRequest> = roles
        .iter()
        .map(|role| {
            production_request(role, &workspace).unwrap_or_else(|error| panic!("{role}: {error}"))
        })
        .collect();
    let _ = std::fs::remove_dir_all(&workspace);
    assert_eq!(requests.len(), 5, "one request per role");

    let labels: BTreeSet<String> = requests.iter().map(|r| r.role.label()).collect();
    assert_eq!(labels.len(), 5, "five distinct roles: {labels:?}");
    let bound = requests.iter().filter(|r| r.agent.is_some()).count();
    assert_eq!(bound, 3, "the worker, the reviewer and the agent probe");
    assert_eq!(requests.len() - bound, 2, "the gate and the shell probe");
    for request in &requests {
        assert_eq!(
            request.agent.is_some(),
            request.role.is_slotted(),
            "{}: R3 — \"agent slot + pool slot pair (worker, review, re-ask, agent probe) … \
             the shell probe and gates are non-slotted\"",
            request.role
        );
    }
    let ids: BTreeSet<String> = requests.iter().map(|r| r.invocation.render()).collect();
    assert_eq!(ids.len(), 5, "five distinct identities: {ids:?}");
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.invocation.probe_target().is_some())
            .count(),
        2,
        "both probes carry a probe identity"
    );
    let attempt_tokens: BTreeSet<&str> = ids
        .iter()
        .filter(|id| id.starts_with('k'))
        .filter_map(|id| id.split('.').nth(3))
        .collect();
    assert_eq!(
        attempt_tokens,
        BTreeSet::from(["worker", "gate0", "review_pass0"]),
        "the three in-attempt roles carry their own role token, not one shared identity"
    );
    let paired: BTreeSet<(String, bool, String)> = requests
        .iter()
        .map(|r| (r.role.label(), r.agent.is_some(), r.invocation.render()))
        .collect();
    assert_eq!(paired.len(), 5, "five distinct (role, binding, identity)");

    let carrying_a_payload: BTreeSet<String> = requests
        .iter()
        .filter(|r| !r.command.stdin.is_empty())
        .map(|r| r.role.label())
        .collect();
    assert_eq!(
        carrying_a_payload,
        BTreeSet::from(["implement".to_owned(), "review".to_owned()]),
        "the roles whose adapter delivers a prompt on stdin"
    );
    let payloads: BTreeSet<&[u8]> = requests
        .iter()
        .map(|r| r.command.stdin.as_slice())
        .collect();
    assert_eq!(
        payloads.len(),
        3,
        "empty, a worker prompt and a reviewer prompt — three distinct payloads, so a \
         suppression keyed on the payload cannot be green in either direction"
    );

    let shell_program = native().spec(SHELL_PROBE_COMMAND).program;
    let on_the_recorded_shell: BTreeSet<String> = requests
        .iter()
        .filter(|r| r.command.program == shell_program)
        .map(|r| r.role.label())
        .collect();
    assert_eq!(
        on_the_recorded_shell,
        BTreeSet::from(["gate".to_owned(), "probe(shell)".to_owned()]),
        "the two roles production runs on the recorded shell"
    );
    let programs: BTreeSet<&str> = requests
        .iter()
        .map(|r| r.command.program.as_str())
        .collect();
    assert_eq!(
        programs.len(),
        2,
        "a shell and a CLI: {programs:?} — a grid whose every child was a shell proves \
         nothing about the three roles that never run one"
    );
    let argv: BTreeSet<&[String]> = requests.iter().map(|r| r.command.args.as_slice()).collect();
    assert_eq!(argv.len(), 2, "and two distinct argument vectors: {argv:?}");
    for request in &requests {
        assert_eq!(
            request.command.program != shell_program,
            request.agent.is_some(),
            "{}: a request runs a CLI without a binding, or is bound and runs a shell",
            request.role
        );
    }
    assert_ne!(
        carrying_a_payload,
        requests
            .iter()
            .filter(|r| r.agent.is_some())
            .map(|r| r.role.label())
            .collect::<BTreeSet<String>>(),
        "the stdin payload and the agent binding partition the grid identically, so a \
         suppression keyed on either would be indistinguishable"
    );

    let timeouts: BTreeSet<(String, Duration)> = requests
        .iter()
        .map(|r| (r.role.label(), r.timeout))
        .collect();
    assert_eq!(
        timeouts,
        BTreeSet::from([
            ("probe(shell)".to_owned(), SHELL_PROBE_TIMEOUT),
            ("probe(claude-code)".to_owned(), AGENT_PROBE_TIMEOUT),
            (
                "implement".to_owned(),
                crate::engine::DEFAULT_ATTEMPT_TIMEOUT
            ),
            ("gate".to_owned(), crate::config::DEFAULT_GATE_TIMEOUT),
            (
                "review".to_owned(),
                crate::config::DEFAULT_REVIEW_PASS_TIMEOUT
            ),
        ]),
        "each role carries the timeout production gives it"
    );
    assert_eq!(
        requests
            .iter()
            .map(|r| r.timeout)
            .collect::<BTreeSet<Duration>>()
            .len(),
        5,
        "five distinct timeouts, so a suppression keyed on one value cannot survive"
    );

    assert!(
        requests.iter().all(|r| r.command.env.is_empty()),
        "production composes an overlay now; the grid has to carry one too"
    );

    let workspaces: BTreeSet<&Path> = requests.iter().map(|r| r.workspace.as_path()).collect();
    assert_eq!(
        workspaces.len(),
        2,
        "the coordinator's directory and the worktree: {workspaces:?}"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.workspace == crate::agent::probe_workspace())
            .map(|r| r.role.label())
            .collect::<BTreeSet<String>>(),
        BTreeSet::from(["probe(claude-code)".to_owned()]),
        "`probe_request` is the one builder that chooses its own workspace"
    );
    assert!(
        requests.iter().all(|r| r.workspace.is_absolute()),
        "every request's workspace is absolute — `HostRunner::run` clears the environment, \
         so a drive-relative path would resolve against nothing"
    );
}

#[test]
fn every_production_invocation_identity_reaches_the_containment_points() {
    let workspace = scratch("identity-hooks");
    let attempt = |role| InvocationId::legacy_attempt(TaskKey(0), AttemptNumber(1), role, 0);
    let cases: Vec<(&str, RunnerRequest)> = vec![
        (
            "a review re-ask",
            crate::runner::review_request(
                agent_cli_command(REVIEW_STDIN.as_bytes()),
                workspace.clone(),
                fixture_agent(),
                crate::config::DEFAULT_REVIEW_PASS_TIMEOUT,
                attempt(AttemptRole::ReviewReask(0)),
            ),
        ),
        (
            "the third review pass",
            crate::runner::review_request(
                agent_cli_command(REVIEW_STDIN.as_bytes()),
                workspace.clone(),
                fixture_agent(),
                crate::config::DEFAULT_REVIEW_PASS_TIMEOUT,
                attempt(AttemptRole::ReviewPass(2)),
            ),
        ),
        (
            "the fourth gate of an attempt's gate list",
            crate::runner::gate_request(
                shell_command(),
                workspace.clone(),
                crate::config::DEFAULT_GATE_TIMEOUT,
                attempt(AttemptRole::Gate(3)),
            ),
        ),
        (
            "an agent probe at the auth-status ordinal",
            crate::agent::probe_request(
                claude::ADAPTER_ID,
                agent_cli_command(b""),
                2,
                AGENT_PROBE_TIMEOUT,
            )
            .expect("a shipped adapter id"),
        ),
    ];
    let grid: BTreeSet<String> = ExecutionRole::all()
        .iter()
        .map(|role| {
            production_request(role, &workspace)
                .unwrap_or_else(|error| panic!("{role}: {error}"))
                .invocation
                .render()
        })
        .collect();
    let here: BTreeSet<String> = cases
        .iter()
        .map(|(_, request)| request.invocation.render())
        .collect();
    assert_eq!(here.len(), 4, "four distinct identities: {here:?}");
    assert!(
        here.is_disjoint(&grid),
        "an identity the role grid already sends: {here:?} vs {grid:?}"
    );

    let points = per_spawn_points();
    for (what, request) in &cases {
        let witness = RoleWitness::new();
        let runner = HostRunner::new().with_hooks(Box::new(witness.handle()));
        let output = runner
            .run(request)
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        assert_eq!(output.code, Some(0), "{what}: {output:?}");
        let harness = witness.harness.lock().expect("harness");
        for point in &points {
            assert!(
                point
                    .modes()
                    .iter()
                    .any(|mode| harness.reached_point(SPAWN_SITE, *point, *mode)),
                "{what}: the funnel never reached {point}, so this identity's spawn \
                 produces no containment-hook evidence"
            );
        }
        assert_eq!(
            witness.children.lock().expect("children").len(),
            1,
            "{what}: one spawn, one child"
        );
    }
    let _ = std::fs::remove_dir_all(&workspace);
}

#[test]
fn every_shipped_agent_binding_reaches_the_containment_points() {
    let points = per_spawn_points();
    let roster: Vec<&str> = CREDENTIAL_LOCATIONS.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        roster.len(),
        3,
        "the shipped adapters host-v1 supplies credentials for: {roster:?}"
    );
    assert!(
        roster.contains(&claude::ADAPTER_ID),
        "the grid's own id is in the roster, so this test is a superset of it"
    );

    for id in &roster {
        let workspace = scratch("agent-binding");
        let witness = RoleWitness::new();
        let runner = HostRunner::new().with_hooks(Box::new(witness.handle()));
        let request = crate::runner::worker_request(
            agent_cli_command(WORKER_STDIN.as_bytes()),
            workspace.clone(),
            AgentId::new(*id),
            crate::engine::DEFAULT_ATTEMPT_TIMEOUT,
            worker_invocation(),
        );
        assert_eq!(request.agent.as_ref().map(AgentId::as_str), Some(*id));
        let output = runner.run(&request);
        let _ = std::fs::remove_dir_all(&workspace);
        let output = output.unwrap_or_else(|error| panic!("{id}: {error}"));
        assert_eq!(output.code, Some(0), "{id}: {output:?}");

        let harness = witness.harness.lock().expect("harness");
        for point in &points {
            assert!(
                point
                    .modes()
                    .iter()
                    .any(|mode| harness.reached_point(SPAWN_SITE, *point, *mode)),
                "{id}: the funnel never reached {point}, so a worker bound to this agent \
                 produces no containment-hook evidence"
            );
        }
        assert_eq!(
            witness.children.lock().expect("children").len(),
            1,
            "{id}: one spawn, one child"
        );
    }
}

#[test]
fn every_production_program_shape_reaches_the_containment_points() {
    let points = per_spawn_points();
    let root = scratch("program-shapes");
    let shapes = program_shapes(&root, WORKER_STDIN.as_bytes());

    assert_eq!(
        shapes.len(),
        if cfg!(windows) { 5 } else { 4 },
        "the program shapes this platform's production can produce"
    );
    let programs: BTreeSet<&str> = shapes
        .iter()
        .map(|shape| shape.command.program.as_str())
        .collect();
    assert_eq!(
        programs.len(),
        shapes.len(),
        "two shapes share a program, so one of them proves nothing: {programs:?}"
    );
    assert_eq!(
        shapes
            .iter()
            .filter(|shape| !Path::new(&shape.command.program).is_absolute())
            .count(),
        1,
        "exactly one shape is a bare name the child's program search has to resolve"
    );
    assert_eq!(
        shapes
            .iter()
            .filter(|shape| shape.command.program.contains(' '))
            .count(),
        1,
        "exactly one shape's path needs quoting, so quoting and file kind are two \
         fields and not one"
    );
    #[cfg(windows)]
    {
        let batch = |suffix: &str| {
            shapes
                .iter()
                .filter(|shape| shape.command.program.to_ascii_lowercase().ends_with(suffix))
                .count()
        };
        assert_eq!(batch(".cmd"), 2, "the npm shape, with and without a space");
        assert_eq!(batch(".bat"), 1, "the other batch extension");
    }

    for shape in &shapes {
        let workspace = scratch("program-shape");
        let witness = RoleWitness::new();
        let runner = HostRunner::new().with_hooks(Box::new(witness.handle()));
        let request = crate::runner::worker_request(
            shape.command.clone(),
            workspace.clone(),
            fixture_agent(),
            crate::engine::DEFAULT_ATTEMPT_TIMEOUT,
            worker_invocation(),
        );
        let output = runner.run(&request);
        let _ = std::fs::remove_dir_all(&workspace);
        let output = output.unwrap_or_else(|error| panic!("{}: {error}", shape.what));
        assert_eq!(output.code, Some(0), "{}: {output:?}", shape.what);
        if shape.reports {
            assert!(
                output.stdout.contains("0 passed"),
                "{}: the launcher never reached this binary, so the child under \
                 observation is not the one this shape names: {}",
                shape.what,
                output.stdout
            );
        }

        let harness = witness.harness.lock().expect("harness");
        for point in &points {
            assert!(
                point
                    .modes()
                    .iter()
                    .any(|mode| harness.reached_point(SPAWN_SITE, *point, *mode)),
                "{}: the funnel never reached {point}, so a CLI installed in this \
                 shape produces no containment-hook evidence",
                shape.what
            );
        }
        assert_eq!(
            witness.children.lock().expect("children").len(),
            1,
            "{}: one spawn, one child",
            shape.what
        );
    }

    let mut injections = 0_usize;
    for shape in &shapes {
        for point in &points {
            let workspace = scratch("program-shape-fault");
            let runner = HostRunner::new().with_hooks(Box::new(FailAt(*point)));
            let request = crate::runner::worker_request(
                shape.command.clone(),
                workspace.clone(),
                fixture_agent(),
                crate::engine::DEFAULT_ATTEMPT_TIMEOUT,
                worker_invocation(),
            );
            let outcome = runner.run(&request);
            let _ = std::fs::remove_dir_all(&workspace);
            let error = outcome.err().unwrap_or_else(|| {
                panic!(
                    "{}: the fault armed at {point} was never introduced",
                    shape.what
                )
            });
            assert!(
                error.to_string().contains(&point.to_string()),
                "{}: the failure does not name the point it was armed at ({point}): {error}",
                shape.what
            );
            injections += 1;
        }
    }
    assert_eq!(
        injections,
        shapes.len() * points.len(),
        "every shape at every point, counted so the grid cannot shrink in silence"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_cli_roles_of_the_grid_run_a_shim_shaped_program_through_the_funnel() {
    let points = per_spawn_points();
    let root = scratch("shim-roles");
    let shim = forwarding_shim(
        &root,
        if cfg!(windows) {
            "upstroke-role-shim.cmd"
        } else {
            "upstroke-role-shim"
        },
    );
    let workspace = scratch("shim-role-workspace");

    let cases: Vec<(&str, RunnerRequest)> = vec![
        (
            "implement",
            crate::runner::worker_request(
                agent_cli_command_at(&shim, WORKER_STDIN.as_bytes()),
                workspace.clone(),
                fixture_agent(),
                crate::engine::DEFAULT_ATTEMPT_TIMEOUT,
                worker_invocation(),
            ),
        ),
        (
            "review",
            crate::runner::review_request(
                agent_cli_command_at(&shim, REVIEW_STDIN.as_bytes()),
                workspace.clone(),
                fixture_agent(),
                crate::config::DEFAULT_REVIEW_PASS_TIMEOUT,
                review_invocation(),
            ),
        ),
        (
            "probe(claude-code)",
            crate::agent::probe_request(
                claude::ADAPTER_ID,
                agent_cli_command_at(&shim, b""),
                0,
                AGENT_PROBE_TIMEOUT,
            )
            .expect("a shipped adapter id"),
        ),
    ];
    assert_eq!(
        cases.len(),
        ExecutionRole::all()
            .iter()
            .filter(|role| {
                !matches!(
                    role,
                    ExecutionRole::Gate | ExecutionRole::Probe(ProbeTarget::Shell)
                )
            })
            .count(),
        "every role production runs on an agent CLI, and only those"
    );

    for (what, request) in &cases {
        assert_eq!(request.command.program, shim, "{what}: not the shim");
        let witness = RoleWitness::new();
        let runner = HostRunner::new().with_hooks(Box::new(witness.handle()));
        std::fs::create_dir_all(&request.workspace).expect("the request's workspace");
        let output = runner
            .run(request)
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        assert_eq!(output.code, Some(0), "{what}: {output:?}");
        assert!(
            output.stdout.contains("0 passed"),
            "{what}: the shim never reached this binary: {}",
            output.stdout
        );
        let harness = witness.harness.lock().expect("harness");
        for point in &points {
            assert!(
                point
                    .modes()
                    .iter()
                    .any(|mode| harness.reached_point(SPAWN_SITE, *point, *mode)),
                "{what}: the funnel never reached {point} for a shim-shaped program"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_grids_agent_cli_child_runs_no_test_and_exits_zero() {
    let workspace = scratch("agent-cli-child");
    let output = HostRunner::new()
        .run(&RunnerRequest {
            command: agent_cli_command(WORKER_STDIN.as_bytes()),
            workspace: workspace.clone(),
            role: ExecutionRole::Gate,
            timeout: SHELL_PROBE_TIMEOUT,
            invocation: gate_invocation(),
            agent: None,
        })
        .expect("the grid's non-shell child runs");
    let _ = std::fs::remove_dir_all(&workspace);

    assert_eq!(output.code, Some(0), "{output:?}");
    assert!(
        output.stdout.contains("0 passed"),
        "`{NO_SUCH_TEST}` matched something and the grid is running tests inside its own \
         fixture: {}",
        output.stdout
    );
    assert!(
        !output.timed_out && !output.output_limited,
        "the grid's child did not settle: {output:?}"
    );
}

#[test]
fn every_role_reaches_the_containment_points_of_this_platform() {
    let roles = ExecutionRole::all();
    assert_eq!(
        roles.len(),
        5,
        "the grid covers every role this slice routes"
    );
    let points = per_spawn_points();
    let mut probe_paths: Vec<String> = Vec::new();
    for role in &roles {
        let witness = RoleWitness::new();
        let runner = HostRunner::new().with_hooks(Box::new(witness.handle()));
        let workspace = scratch("role-hooks");
        let outcome = run_in_role(&runner, role, &workspace);
        let _ = std::fs::remove_dir_all(&workspace);
        outcome.unwrap_or_else(|error| panic!("{role} did not run: {error}"));

        {
            let harness = witness.harness.lock().expect("harness");
            for point in &points {
                assert!(
                    point
                        .modes()
                        .iter()
                        .any(|mode| harness.reached_point(SPAWN_SITE, *point, *mode)),
                    "{role}: the funnel never reached {point}, so this role's spawn \
                     produces no containment-hook evidence"
                );
            }
        }
        assert_eq!(
            *witness.order.lock().expect("order"),
            points.to_vec(),
            "{role}: the funnel's containment order"
        );
        assert_eq!(
            witness.children.lock().expect("children").len(),
            1,
            "{role}: one spawn, one child"
        );
        #[cfg(unix)]
        {
            let (leads, seen) = group_leadership(&witness.led_own_group.lock().expect("groups"));
            assert_eq!(
                leads,
                vec![true],
                "{role}: the child did not lead its own process group, so the \
                 pre-exec containment step did not run for this role; observed: {seen}"
            );
        }
        if matches!(role, ExecutionRole::Probe(_)) {
            probe_paths.push(role.label());
        }
    }
    assert_eq!(
        probe_paths,
        vec!["probe(shell)".to_owned(), "probe(claude-code)".to_owned()],
        "both contract-named probe paths, each observed through its own \
         production entry point"
    );
}

const SPAWN_KILL_POINT: &str = "UPSTROKE_SPAWN_KILL_POINT";

struct KillAtPoint(SubEffectPoint);

impl SpawnHooks for KillAtPoint {
    fn point(&mut self, point: SubEffectPoint) -> crate::topology::effects::Injection {
        if point == self.0 {
            crate::topology::effects::Injection::Kill
        } else {
            crate::topology::effects::Injection::Proceed
        }
    }

    fn point_mode(
        &mut self,
        point: SubEffectPoint,
        mode: InjectionMode,
    ) -> crate::topology::effects::Injection {
        if mode == InjectionMode::Kill {
            self.point(point)
        } else {
            crate::topology::effects::Injection::Proceed
        }
    }
}

#[test]
#[ignore = "subprocess helper"]
fn spawn_funnel_kill_helper() {
    let Ok(name) = std::env::var(SPAWN_KILL_POINT) else {
        return;
    };
    let point = containment_points()
        .into_iter()
        .find(|point| point.name() == name)
        .unwrap_or_else(|| panic!("the parent named a point this platform has not: {name}"));
    let workspace = scratch("kill-helper");
    if STARTUP_POINTS.contains(&point) {
        let _ = start_write_command(&mut KillAtPoint(point));
        std::process::exit(0);
    }
    start_write_command(&mut proc::NoHooks)
        .expect("the helper establishes containment before it spawns anything");
    let runner = HostRunner::new().with_hooks(Box::new(KillAtPoint(point)));
    let _ = runner.run(&crate::runner::gate_request(
        shell_command(),
        workspace.clone(),
        crate::config::DEFAULT_GATE_TIMEOUT,
        gate_invocation(),
    ));
    let _ = std::fs::remove_dir_all(&workspace);
    std::process::exit(0);
}

#[test]
fn a_kill_armed_at_any_containment_point_actually_kills() {
    let helper = format!(
        "{}::spawn_funnel_kill_helper",
        module_path!()
            .split_once("::")
            .expect("this module is not the crate root")
            .1
    );
    let points = containment_points();
    assert!(
        !points.is_empty(),
        "this platform declares no containment point, so nothing is measured"
    );
    let capture = scratch("kill-capture");
    for point in &points {
        assert!(
            point.modes().contains(&InjectionMode::Kill),
            "{point}: this grid is about the kill mode and this point does not declare it"
        );
        let out_path = capture.join(format!("{}.out", point.name()));
        let err_path = capture.join(format!("{}.err", point.name()));
        let status = Command::new(std::env::current_exe().expect("the test executable"))
            .args([helper.as_str(), "--ignored", "--exact"])
            .env(SPAWN_KILL_POINT, point.name())
            .stdout(std::fs::File::create(&out_path).expect("capture stdout"))
            .stderr(std::fs::File::create(&err_path).expect("capture stderr"))
            .status()
            .unwrap_or_else(|error| panic!("{point}: spawning the helper: {error}"));
        let stdout = std::fs::read_to_string(&out_path).unwrap_or_default();
        let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();

        assert!(
            !status.success(),
            "{point}: the helper exited cleanly, so the kill never fired.\n{stdout}"
        );
        assert!(
            !stderr.contains("panicked at"),
            "{point}: the helper panicked rather than aborting, so destructors ran \
             and this is not what a coordinator kill leaves:\n{stderr}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(
                status.signal(),
                Some(libc::SIGABRT),
                "{point}: a kill is an abort, and this child died some other way \
                 (code {:?})",
                status.code()
            );
        }
        #[cfg(not(unix))]
        assert_ne!(
            status.code(),
            Some(101),
            "{point}: 101 is the harness's panic status, not an abort"
        );
    }
    let _ = std::fs::remove_dir_all(&capture);
    assert_eq!(
        points.len(),
        4,
        "the frozen inventory's containment points for this platform: {points:?}"
    );
    assert_eq!(
        points.contains(&SubEffectPoint::AmbientJobJoined),
        cfg!(windows),
        "the Windows startup point is in the kill grid's domain exactly on Windows"
    );
}

struct FailAt(SubEffectPoint);

impl SpawnHooks for FailAt {
    fn point(&mut self, point: SubEffectPoint) -> Injection {
        if point == self.0 {
            Injection::Error
        } else {
            Injection::Proceed
        }
    }
}

#[test]
fn a_fault_armed_at_any_containment_point_stops_any_role() {
    let roles = ExecutionRole::all();
    let points = per_spawn_points();
    assert_eq!(
        points.len(),
        if cfg!(windows) { 3 } else { 4 },
        "the per-spawn containment points of this platform"
    );
    let mut injections = 0_usize;
    for role in &roles {
        for point in &points {
            let runner = HostRunner::new().with_hooks(Box::new(FailAt(*point)));
            let workspace = scratch("role-fault");
            let outcome = run_in_role(&runner, role, &workspace);
            let _ = std::fs::remove_dir_all(&workspace);
            let error = outcome.err().unwrap_or_else(|| {
                panic!("{role}: the fault armed at {point} was never introduced")
            });
            assert!(
                error.to_string().contains(&point.to_string()),
                "{role}: the failure does not name the point it was armed at \
                 ({point}): {error}"
            );
            injections += 1;
        }
    }
    assert_eq!(
        injections,
        roles.len() * points.len(),
        "every role at every point, counted so the grid cannot shrink in silence"
    );
}

#[cfg(unix)]
#[test]
fn the_pre_exec_containment_step_runs_in_the_forked_child() {
    #[derive(Clone, Default)]
    struct Witness {
        led_own_group: Arc<Mutex<Vec<proc::GroupObservation>>>,
        order: Arc<Mutex<Vec<SubEffectPoint>>>,
    }
    impl proc::SpawnHooks for Witness {
        fn point(&mut self, point: SubEffectPoint) -> crate::topology::effects::Injection {
            self.order.lock().expect("order").push(point);
            crate::topology::effects::Injection::Proceed
        }
        fn child_created(&mut self, pid: u32) {
            self.led_own_group
                .lock()
                .expect("groups")
                .push(proc::observe_child_group(pid));
        }
    }

    let witness = Witness::default();
    let workspace = scratch("pre-exec");
    let runner = HostRunner::new().with_hooks(Box::new(witness.clone()));
    let template = native().command("exit 0");
    let request = RunnerRequest {
        command: CommandSpec {
            program: template.get_program().to_string_lossy().into_owned(),
            args: template
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            env: Vec::new(),
            stdin: Vec::new(),
        },
        workspace: workspace.clone(),
        role: ExecutionRole::Gate,
        timeout: Duration::from_secs(30),
        agent: None,
        invocation: gate_invocation(),
    };
    let output = runner.run(&request).expect("run through the funnel");
    assert_eq!(output.code, Some(0), "{output:?}");
    let _ = std::fs::remove_dir_all(&workspace);

    let (leads, seen) = group_leadership(&witness.led_own_group.lock().expect("groups"));
    assert_eq!(
        leads,
        vec![true],
        "the child did not lead its own process group, so the pre-exec \
         closure did not run in it; observed: {seen}"
    );
    assert_eq!(
        *witness.order.lock().expect("order"),
        vec![
            SubEffectPoint::ReaperStarted,
            SubEffectPoint::PreExecPgidAndRegister,
            SubEffectPoint::Exec,
            SubEffectPoint::Registered,
        ],
        "the packet's order: ReaperStarted, PreExecPgidAndRegister, Exec, Registered"
    );
}

#[cfg(unix)]
#[test]
fn a_passing_grid_prints_the_group_observation_it_promises() {
    let exe = std::env::current_exe().expect("test executable");
    let helper = Command::new(&exe)
        .args([
            "runner::host::tests::the_pre_exec_containment_step_runs_in_the_forked_child",
            "--exact",
            "--nocapture",
        ])
        .output()
        .expect("run the pre-exec containment test as a subprocess");
    let stdout = String::from_utf8_lossy(&helper.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&helper.stderr).into_owned();
    assert!(
        helper.status.success(),
        "the pre-exec containment test failed:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed"),
        "the filter matched no test, so nothing was proved:\n{stdout}"
    );
    assert!(
        stdout.contains(GROUP_OBSERVATION_MARKER),
        "a passing grid printed no observation, so the promised measurement \
         cannot happen as described (PR173-LIVE-RECORD-ANSWER-NEVER-PRINTED):\n\
         --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn the_ambient_job_step_does_nothing_on_unix_and_records_nothing() {
    let harness = Arc::new(Mutex::new(HookHarness::new()));
    let runner = HostRunner::new().with_hooks(Box::new(HarnessHooks::new(Arc::clone(&harness))));
    runner
        .start_write_command()
        .expect("Unix containment is the reaper, not a job object");
    let harness = harness.lock().expect("harness");
    for mode in [InjectionMode::Kill, InjectionMode::ErrorReturn] {
        assert!(
            !harness.reached_point(SPAWN_SITE, SubEffectPoint::AmbientJobJoined, mode),
            "a Unix host recorded a Windows containment point as executed"
        );
    }
}

#[test]
fn a_containment_proof_exists_only_where_containment_was_established() {
    let before = containment_establishments();
    let _first = contain_write_command(&mut NoHooks).expect("containment establishes on this host");
    assert_eq!(
        containment_establishments(),
        before + 1,
        "the step ran and the count did not move"
    );
    let _second =
        contain_write_command(&mut NoHooks).expect("the second establishment is the first");
    assert_eq!(
        containment_establishments(),
        before + 2,
        "each establishment is counted, so a census can say which call established"
    );
    #[cfg(windows)]
    assert!(
        proc::ambient_job_established(),
        "establishing twice left the process outside its ambient job"
    );

    #[cfg(windows)]
    {
        let refusing = HostRunner::new().with_hooks(Box::new(RefuseAmbientJoin::default()));
        let mark = containment_establishments();
        refusing
            .start_write_command()
            .expect_err("the observer refuses the join");
        assert_eq!(
            containment_establishments(),
            mark,
            "a refused establishment produced a containment proof"
        );
    }
}

#[cfg(windows)]
#[derive(Debug, Default)]
struct RefuseAmbientJoin {
    children: Vec<u32>,
    points: Vec<SubEffectPoint>,
}

#[cfg(windows)]
impl proc::SpawnHooks for RefuseAmbientJoin {
    fn point(&mut self, point: SubEffectPoint) -> crate::topology::effects::Injection {
        self.points.push(point);
        if point == SubEffectPoint::AmbientJobJoined {
            crate::topology::effects::Injection::Error
        } else {
            crate::topology::effects::Injection::Proceed
        }
    }

    fn child_created(&mut self, pid: u32) {
        self.children.push(pid);
    }
}

#[cfg(windows)]
#[test]
fn windows_ambient_job_unavailable_refuses_before_effects() {
    use std::sync::mpsc;

    #[derive(Debug)]
    struct Reporting {
        inner: RefuseAmbientJoin,
        tx: mpsc::Sender<(Vec<u32>, Vec<SubEffectPoint>)>,
    }
    impl proc::SpawnHooks for Reporting {
        fn point(&mut self, point: SubEffectPoint) -> crate::topology::effects::Injection {
            let answer = self.inner.point(point);
            let _ = self
                .tx
                .send((self.inner.children.clone(), self.inner.points.clone()));
            answer
        }
        fn child_created(&mut self, pid: u32) {
            self.inner.child_created(pid);
            let _ = self
                .tx
                .send((self.inner.children.clone(), self.inner.points.clone()));
        }
    }

    let (tx, rx) = mpsc::channel();
    let runner = HostRunner::new().with_hooks(Box::new(Reporting {
        inner: RefuseAmbientJoin::default(),
        tx,
    }));
    let established_before = containment_establishments();
    let error = runner
        .start_write_command()
        .expect_err("a write command whose ambient job cannot be established must refuse");
    let message = error.to_string();
    assert!(
        message.contains("ambient"),
        "the refusal must diagnose the ambient job: {message}"
    );
    assert!(
        message.contains("INV-18"),
        "the refusal must say which invariant it enforces: {message}"
    );
    assert!(
        message.contains("No process was spawned"),
        "the refusal must say the run performed no effect: {message}"
    );

    let (children, points) = rx.try_iter().last().expect("the hook was consulted");
    assert!(
        children.is_empty(),
        "a refused write command created a process: {children:?}"
    );
    assert_eq!(
        points,
        vec![SubEffectPoint::AmbientJobJoined],
        "the refusal reached a containment step past the ambient join"
    );
    assert_eq!(
        containment_establishments(),
        established_before,
        "a simulated join failure minted a containment proof"
    );
}

#[cfg(windows)]
#[test]
fn the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing() {
    let mut refusing = RefuseAmbientJoin::default();
    let before = containment_establishments();
    let error = contain_write_command(&mut refusing)
        .expect_err("a write command whose ambient job cannot be established must refuse");

    let message = error.to_string();
    for fragment in ["ambient", "INV-18", "No process was spawned"] {
        assert!(
            message.contains(fragment),
            "the refusal must say `{fragment}`: {message}"
        );
    }
    assert_eq!(
        refusing.points,
        vec![SubEffectPoint::AmbientJobJoined],
        "the refusal reached a containment step past the ambient join"
    );
    assert!(
        refusing.children.is_empty(),
        "a refused write command created a process: {:?}",
        refusing.children
    );
    assert_eq!(
        containment_establishments(),
        before,
        "a refused join minted a containment proof"
    );

    let _proof = contain_write_command(&mut NoHooks).expect("the real join succeeds on this host");
    assert_eq!(
        containment_establishments(),
        before + 1,
        "the step ran and minted nothing"
    );

    let mut refusing = RefuseAmbientJoin::default();
    let mark = containment_establishments();
    let error =
        start_write_command(&mut refusing).expect_err("the CLI's containment step must refuse too");
    assert!(
        error.to_string().contains("INV-18"),
        "the CLI's refusal must say which invariant it enforces: {error}"
    );
    assert_eq!(
        containment_establishments(),
        mark,
        "the unit-returning entry point swallowed the refusal and established containment"
    );
    assert!(
        refusing.children.is_empty(),
        "a refused dispatch created a process: {:?}",
        refusing.children
    );
}

#[cfg(windows)]
const JOIN_RECORD: &str = "UPSTROKE_PR4_JOIN_RECORD";

#[cfg(windows)]
#[derive(Debug)]
struct WitnessAmbientJoin {
    record: PathBuf,
    lines: Vec<String>,
}

#[cfg(windows)]
impl proc::SpawnHooks for WitnessAmbientJoin {
    fn point(&mut self, _point: SubEffectPoint) -> crate::topology::effects::Injection {
        crate::topology::effects::Injection::Proceed
    }

    fn point_mode(
        &mut self,
        point: SubEffectPoint,
        mode: InjectionMode,
    ) -> crate::topology::effects::Injection {
        if point == SubEffectPoint::AmbientJobJoined {
            self.lines.push(format!(
                "{mode:?} {}",
                i32::from(proc::ambient_job_established())
            ));
            std::fs::write(&self.record, self.lines.join("\n")).expect("record the observation");
        }
        crate::topology::effects::Injection::Proceed
    }
}

#[cfg(windows)]
#[test]
#[ignore = "subprocess helper"]
fn windows_ambient_join_ordering_helper() {
    let Some(record) = std::env::var_os(JOIN_RECORD) else {
        return;
    };
    let runner = HostRunner::new().with_hooks(Box::new(WitnessAmbientJoin {
        record: PathBuf::from(record),
        lines: Vec::new(),
    }));
    runner
        .start_write_command()
        .expect("the helper must establish its ambient job");
}

#[cfg(windows)]
#[test]
fn windows_the_ambient_join_is_observed_on_both_sides_of_the_join() {
    let dir = scratch("ambient-order");
    let record = dir.join("order");
    let mut helper = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "windows_ambient_join_ordering_helper",
            "--ignored",
            "--nocapture",
        ])
        .env(JOIN_RECORD, &record)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn the join-ordering helper");
    let status = helper.wait().expect("reap the join-ordering helper");
    assert!(
        status.success(),
        "the helper could not establish its ambient job"
    );
    let written = std::fs::read_to_string(&record).expect("the helper recorded what it observed");
    let observed: Vec<&str> = written.lines().collect();
    assert_eq!(
        observed,
        vec!["ErrorReturn 0", "Kill 1"],
        "the ambient join must be consulted before the join for its error contract and \
         after it for its kill claim; got {observed:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
const POISON_AMBIENT: &str = "UPSTROKE_POISON_AMBIENT";

#[cfg(windows)]
#[test]
#[ignore = "subprocess helper"]
fn poisoned_ambient_helper() {
    const MESSAGE: &str = "it could not be created (simulated by the helper)";
    if std::env::var_os(POISON_AMBIENT).is_none() {
        return;
    }
    assert!(
        proc::poison_ambient_for_tests(MESSAGE),
        "the ambient cell was already spent, so this helper measures nothing"
    );

    let error = proc::join_ambient_job(&mut proc::NoHooks)
        .expect_err("a memoised failure is a failure for every later caller");
    let rendered = error.to_string();
    assert!(rendered.contains(MESSAGE), "the diagnostic: {rendered}");
    assert!(
        rendered.contains("No process was spawned"),
        "and it says nothing ran: {rendered}"
    );

    let before = containment_establishments();
    let error = contain_write_command(&mut proc::NoHooks)
        .err()
        .map(|error| error.to_string())
        .unwrap_or_else(|| panic!("a Contained was minted with no ambient job"));
    assert!(error.contains(MESSAGE), "the mint's diagnostic: {error}");
    assert_eq!(
        containment_establishments(),
        before,
        "an establishment was counted although the join failed"
    );

    start_write_command(&mut proc::NoHooks).expect_err("the CLI write path refuses too");

    println!("POISONED-AMBIENT-REFUSED");
}

#[cfg(windows)]
#[test]
fn a_real_memoised_ambient_failure_refuses_the_write_command() {
    let helper = format!(
        "{}::poisoned_ambient_helper",
        module_path!()
            .split_once("::")
            .expect("this module is not the crate root")
            .1
    );
    let output = Command::new(std::env::current_exe().expect("the test executable"))
        .args([helper.as_str(), "--ignored", "--exact", "--nocapture"])
        .env(POISON_AMBIENT, "1")
        .output()
        .expect("spawn the poisoned-ambient helper");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the helper failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("POISONED-AMBIENT-REFUSED"),
        "the helper never reached its own conclusion, so it asserted nothing:\n{stdout}"
    );
}

#[cfg(windows)]
const STUB_RECORD: &str = "UPSTROKE_PR4_STUB_RECORD";

#[cfg(windows)]
#[derive(Debug)]
struct DieAtCreatedSuspended {
    record: PathBuf,
}

#[cfg(windows)]
impl proc::SpawnHooks for DieAtCreatedSuspended {
    fn point(&mut self, point: SubEffectPoint) -> crate::topology::effects::Injection {
        if point == SubEffectPoint::CreatedSuspended {
            crate::topology::effects::Injection::Kill
        } else {
            crate::topology::effects::Injection::Proceed
        }
    }

    fn child_created(&mut self, pid: u32) {
        let created = proc::process_creation_time(pid).unwrap_or_default();
        let member = proc::child_in_ambient_job(pid);
        std::fs::write(
            &self.record,
            format!("{pid} {created} {}", i32::from(member == Some(true))),
        )
        .expect("record the stub identity before dying");
    }
}

#[cfg(windows)]
#[test]
#[ignore = "subprocess helper"]
fn windows_ambient_coordinator_helper() {
    let Some(record) = std::env::var_os(STUB_RECORD) else {
        return;
    };
    let runner = HostRunner::new().with_hooks(Box::new(DieAtCreatedSuspended {
        record: PathBuf::from(record),
    }));
    runner
        .start_write_command()
        .expect("the helper must establish its ambient job");
    let template = ShellKind::Cmd.command("exit 0");
    let request = RunnerRequest {
        command: CommandSpec {
            program: template.get_program().to_string_lossy().into_owned(),
            args: template
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            env: Vec::new(),
            stdin: Vec::new(),
        },
        workspace: std::env::temp_dir(),
        role: ExecutionRole::Gate,
        timeout: Duration::from_secs(60),
        agent: None,
        invocation: gate_invocation(),
    };
    let _ = runner.run(&request);
    unreachable!("the coordinator helper was supposed to die at CreatedSuspended");
}

#[cfg(windows)]
fn one_crash_cycle(tag: &str) -> (u32, u64, bool) {
    let dir = scratch(tag);
    let record = dir.join("stub");
    let mut coordinator = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "windows_ambient_coordinator_helper",
            "--ignored",
            "--nocapture",
        ])
        .env(STUB_RECORD, &record)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn the coordinator helper");
    let status = coordinator.wait().expect("reap the coordinator helper");
    assert!(
        !status.success(),
        "the coordinator helper exited normally instead of dying at CreatedSuspended"
    );
    let written =
        std::fs::read_to_string(&record).expect("the coordinator recorded its stub before dying");
    let mut parts = written.split_whitespace();
    let pid: u32 = parts.next().expect("pid").parse().expect("pid");
    let created: u64 = parts.next().expect("creation time").parse().expect("time");
    let member = parts.next().expect("membership") == "1";
    let _ = std::fs::remove_dir_all(&dir);
    (pid, created, member)
}

#[cfg(windows)]
fn wait_until_gone(pid: u32, created: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if !proc::process_alive(pid, created) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    !proc::process_alive(pid, created)
}

#[cfg(windows)]
#[test]
fn windows_ambient_job_terminates_suspended_stub_after_coordinator_death() {
    let (pid, created, member) = one_crash_cycle("ambient-stub");
    assert_ne!(created, 0, "the stub's creation time was not readable");
    assert!(
        member,
        "INV-18: the child must be an ambient-job member at creation"
    );
    assert!(
        wait_until_gone(pid, created),
        "a suspended stub (pid {pid}, created {created}) outlived its coordinator"
    );
}

#[cfg(windows)]
#[test]
fn windows_repeated_crashes_leave_no_stub() {
    let mut identities = Vec::new();
    for cycle in 0..3 {
        let (pid, created, member) = one_crash_cycle(&format!("ambient-repeat-{cycle}"));
        assert!(member, "cycle {cycle}: the stub was not an ambient member");
        identities.push((pid, created));
    }
    assert_eq!(identities.len(), 3, "three cycles");
    for (cycle, (pid, created)) in identities.into_iter().enumerate() {
        assert!(
            wait_until_gone(pid, created),
            "cycle {cycle}: stub (pid {pid}, created {created}) survived"
        );
    }
}

fn naming_grid() -> Vec<ProgramNaming> {
    let mut all = Vec::new();
    for naming in [ProgramNaming::Posix, ProgramNaming::Windows] {
        match naming {
            ProgramNaming::Posix | ProgramNaming::Windows => all.push(naming),
        }
    }
    all
}

fn composed(pairs: &[(&str, &OsStr)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(key, value)| (OsString::from(*key), (*value).to_os_string()))
        .collect()
}

fn path_of(dirs: &[&Path]) -> OsString {
    std::env::join_paths(dirs).expect("a synthetic PATH")
}

fn program_file(dir: &Path, file_name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create the directory");
    let path = dir.join(file_name);
    std::fs::write(&path, "").expect("write the candidate");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make the candidate executable");
    }
    path
}

#[cfg(unix)]
fn unexecutable_file(dir: &Path, file_name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create the directory");
    let path = dir.join(file_name);
    std::fs::write(&path, "").expect("write the candidate");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("clear the execute bit");
    }
    path
}

const REAL_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC;.CPL";

fn shim_file_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_owned()
    }
}

fn marker_shim(dir: &Path, file_name: &str, marker: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create the shim directory");
    let path = dir.join(file_name);
    let script = if cfg!(windows) {
        format!("@echo off\r\necho {marker}:%~1\r\n")
    } else {
        format!("#!/bin/sh\necho \"{marker}:$1\"\n")
    };
    write_shim(&path, &script);
    path
}

fn write_shim(path: &Path, script: &str) {
    if cfg!(windows) {
        std::fs::write(path, script).expect("write the batch shim");
    } else {
        let mut writer = Command::new("/bin/sh");
        writer
            .args(["-c", "printf '%s' \"$2\" > \"$1\"", "write-shim"])
            .arg(path)
            .arg(script);
        let written = proc::test_support::run_with_timeout(writer, "", Duration::from_secs(60))
            .expect("run the shell shim writer in its own process");
        assert_eq!(written.code, Some(0), "write the shell shim: {written:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
                .expect("make the shim executable");
        }
    }
}

#[cfg(target_os = "linux")]
mod inherited_writer {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::os::unix::net::UnixStream;
    use std::time::Instant;

    use super::*;

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let removed = std::fs::remove_dir_all(&self.0);
            if !std::thread::panicking() {
                removed.expect("remove the inherited-writer fixture");
            }
        }
    }

    struct HeldFork {
        pid: libc::pid_t,
        release: UnixStream,
    }

    impl HeldFork {
        fn assert_alive(&self) {
            let mut status = 0;
            // SAFETY: pid is this guard's unreaped child and status is writable.

            let waited = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
            assert_eq!(
                waited, 0,
                "the descriptor holder exited before the observation: {status}"
            );
        }
    }

    impl Drop for HeldFork {
        fn drop(&mut self) {
            let released = self.release.shutdown(std::net::Shutdown::Write);
            let mut status = 0;
            let waited = loop {
                // SAFETY: pid is our unreaped direct child; status is a live,

                let result = unsafe { libc::waitpid(self.pid, &mut status, 0) };
                if result < 0
                    && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                {
                    continue;
                }
                break result;
            };
            if !std::thread::panicking() {
                released.expect("release the inherited-descriptor holder");
                assert_eq!(waited, self.pid, "reap the inherited-descriptor holder");
                assert_eq!(status, 0, "the holder completed its socket protocol");
            }
        }
    }

    fn hold_inherited_descriptors() -> HeldFork {
        let (release, child_socket) = UnixStream::pair().expect("the fork handshake");
        release
            .set_read_timeout(Some(Duration::from_secs(60)))
            .expect("bound fork readiness");
        child_socket
            .set_read_timeout(Some(Duration::from_secs(60)))
            .expect("bound the held fork");
        let parent_fd = release.as_raw_fd();
        let pid = std::thread::spawn(move || {
            let child_fd = child_socket.as_raw_fd();
            // SAFETY: only the parent returns into Rust after fork. The child

            unsafe {
                let pid = libc::fork();
                if pid == 0 {
                    libc::close(parent_fd);
                    let ready = [1_u8];
                    if libc::write(child_fd, ready.as_ptr().cast(), 1) != 1 {
                        libc::_exit(1);
                    }
                    let mut release = [0_u8];
                    let read = libc::read(child_fd, release.as_mut_ptr().cast(), 1);
                    libc::_exit(if read == 0 { 0 } else { 1 });
                }
                pid
            }
        })
        .join()
        .expect("join the thread that forked with inherited descriptors");
        assert!(pid > 0, "fork the inherited-descriptor holder");
        let mut held = HeldFork { pid, release };
        let mut ready = [0_u8];
        held.release
            .read_exact(&mut ready)
            .expect("the forked holder is ready");
        assert_eq!(ready, [1]);
        held
    }

    fn read_fifo(reader: &mut std::fs::File, bytes: &mut [u8]) {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut rest = bytes;
        while !rest.is_empty() {
            assert!(
                Instant::now() < deadline,
                "the shim writer did not supply its complete script"
            );
            let mut ready = libc::pollfd {
                fd: reader.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: ready is one initialized pollfd and remains live for this

            let polled = unsafe { libc::poll(&mut ready, 1, 1000) };
            if polled < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            assert!(
                polled >= 0,
                "poll the FIFO: {}",
                std::io::Error::last_os_error()
            );
            if polled == 0 {
                continue;
            }
            match reader.read(rest) {
                Ok(0) => panic!("the shim writer closed before writing the complete script"),
                Ok(count) => rest = &mut rest[count..],
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => panic!("read the shim FIFO: {error}"),
            }
        }
    }

    #[test]
    fn marker_shims_do_not_leave_writers_in_another_threads_fork() {
        let mut helper = Command::new(std::env::current_exe().expect("the test executable"));
        helper.args([
            "runner::host::tests::inherited_writer::inherited_writer_helper",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ]);
        let output = proc::test_support::run_with_timeout(helper, "", Duration::from_secs(180))
            .expect("supervise the isolated inherited-writer witness");
        assert_eq!(output.code, Some(0), "{output:?}");
        assert!(
            output.stdout.contains("1 passed"),
            "the helper must run one test: {output:?}"
        );
    }

    #[test]
    #[ignore = "subprocess helper; holds forked descriptors away from other tests"]
    fn inherited_writer_helper() {
        let root = Scratch(scratch("inherited-writer"));
        let executable = root.0.join("regular");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").expect("write the regular-file control");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("make the regular-file control executable");
        let original = std::fs::OpenOptions::new()
            .write(true)
            .open(&executable)
            .expect("open the writer that another thread's fork will inherit");
        let held = hold_inherited_descriptors();
        drop(original);
        let error = Command::new(&executable)
            .output()
            .expect_err("the fork still owns a writer after the original closes");
        assert_eq!(error.raw_os_error(), Some(libc::ETXTBSY));
        drop(held);
        let output = proc::test_support::run_with_timeout(
            Command::new(&executable),
            "",
            Duration::from_secs(60),
        )
        .expect("the executable is available after the inherited writer exits");
        assert_eq!(output.code, Some(0), "{output:?}");

        let fifo_name = "shim fifo ' $name";
        let fifo = root.0.join(fifo_name);
        let name =
            std::ffi::CString::new(fifo.as_os_str().as_bytes()).expect("a NUL-free FIFO path");
        // SAFETY: name is a live NUL-terminated path in our private scratch

        assert_eq!(
            unsafe { libc::mkfifo(name.as_ptr(), 0o600) },
            0,
            "create the shim FIFO"
        );
        let mut reader = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .expect("open the FIFO reader before the shim writer");
        // SAFETY: reader owns a live FIFO descriptor. F_SETPIPE_SZ takes an

        let capacity = unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_SETPIPE_SZ, 4096) };
        assert_eq!(capacity, 4096, "bound the pipe so the script cannot fit");
        let marker = format!("{} ' $name `printf literal`", "x".repeat(16 * 1024));
        let expected = format!("#!/bin/sh\necho \"{marker}:$1\"\n").into_bytes();
        let mut actual = vec![0; expected.len()];
        std::thread::scope(|scope| {
            let writer = scope.spawn(|| marker_shim(&root.0, fifo_name, &marker));
            read_fifo(&mut reader, &mut actual[..1]);
            let held = hold_inherited_descriptors();
            read_fifo(&mut reader, &mut actual[1..]);
            assert_eq!(writer.join().expect("join the shim writer"), fifo);
            held.assert_alive();
            let eof = reader.read(&mut [0_u8]);
            held.assert_alive();
            drop(held);
            assert_eq!(
                actual, expected,
                "the actual marker helper wrote the complete script"
            );
            assert!(
                matches!(eof, Ok(0)),
                "the fork inherited a shim writer: {eof:?}"
            );
        });
    }
}

fn environment_on_path(dirs: &[&Path], pathext: Option<&str>) -> HostEnvironment {
    let case = KeyCase::current();
    let mut base: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| {
            !case.same_key(name, OsStr::new("PATH")) && !case.same_key(name, OsStr::new("PATHEXT"))
        })
        .collect();
    base.push((os("PATH"), path_of(dirs)));
    if let Some(value) = pathext {
        base.push((os("PATHEXT"), os(value)));
    }
    HostEnvironment::with_base(base, case)
}

fn named_request(program: &str, argument: &str, workspace: &Path) -> RunnerRequest {
    crate::runner::gate_request(
        CommandSpec {
            program: program.to_owned(),
            args: vec![argument.to_owned()],
            env: Vec::new(),
            stdin: Vec::new(),
        },
        workspace.to_path_buf(),
        Duration::from_secs(60),
        gate_invocation(),
    )
}

const NAME_TABLE: &[(&str, bool, bool)] = &[
    ("claude", true, true),
    ("codex", true, true),
    ("cmd", true, true),
    ("claude.cmd", true, true),
    ("/usr/local/bin/claude", false, false),
    ("./claude", false, false),
    ("sub/claude", false, false),
    (r"C:\Users\John Smith\npm\copilot.cmd", true, false),
    (r"sub\claude", true, false),
    ("C:claude", true, false),
    (r"\\?\C:\x\claude.exe", true, false),
    ("", false, false),
];

#[test]
fn a_program_that_names_a_location_is_used_as_given_and_only_a_name_is_searched() {
    let empty = composed(&[]);
    let mut disagreements = 0_usize;
    for (program, posix_is_name, windows_is_name) in NAME_TABLE {
        assert_eq!(
            ProgramNaming::Posix.is_bare_name(program),
            *posix_is_name,
            "Posix: `{program}`"
        );
        assert_eq!(
            ProgramNaming::Windows.is_bare_name(program),
            *windows_is_name,
            "Windows: `{program}`"
        );
        if posix_is_name != windows_is_name {
            disagreements += 1;
        }

        for naming in naming_grid() {
            let is_name = match naming {
                ProgramNaming::Posix => *posix_is_name,
                ProgramNaming::Windows => *windows_is_name,
            };
            let resolved = resolve_program(program, &empty, KeyCase::Sensitive, naming);
            if is_name {
                let error = resolved.expect_err(&format!(
                    "{naming:?}: `{program}` is a name and nothing is on PATH"
                ));
                assert!(
                    error.to_string().contains(program),
                    "{naming:?}: the refusal must name the program: {error}"
                );
            } else {
                assert_eq!(
                    resolved.unwrap_or_else(|error| panic!(
                        "{naming:?}: `{program}` is a location: {error}"
                    )),
                    PathBuf::from(program),
                    "{naming:?}: a location was rewritten"
                );
            }
        }
    }
    assert_eq!(
        disagreements, 4,
        "the two naming rules must disagree on the four rows only Windows treats as \
         locations; if they agree everywhere this grid measures one rule twice"
    );
}

#[test]
fn path_directory_order_decides_between_installations_and_pathext_only_within_one() {
    let root = scratch("resolve-order");
    let first = root.join("first");
    let second = root.join("second");
    let both = root.join("both");
    let first_cmd = program_file(&first, "x.cmd");
    let second_exe = program_file(&second, "x.exe");
    let both_com = program_file(&both, "x.com");
    let both_exe = program_file(&both, "x.exe");
    let both_cmd = program_file(&both, "x.cmd");
    program_file(&both, "x.bat");

    let cases: Vec<(&str, Vec<&Path>, &str, &PathBuf)> = vec![
        (
            "an earlier directory's .cmd beats a later directory's .exe",
            vec![&first, &second],
            REAL_PATHEXT,
            &first_cmd,
        ),
        (
            "reversing PATH reverses the answer",
            vec![&second, &first],
            REAL_PATHEXT,
            &second_exe,
        ),
        (
            "within one directory PATHEXT decides, and .COM is first",
            vec![&both],
            REAL_PATHEXT,
            &both_com,
        ),
        (
            "reversing PATHEXT reverses the answer within that directory",
            vec![&both],
            ".CMD;.BAT;.EXE;.COM",
            &both_cmd,
        ),
        (
            "a directory holding four candidates still loses to an earlier one",
            vec![&first, &both],
            REAL_PATHEXT,
            &first_cmd,
        ),
        (
            "a PATHEXT that omits .CMD picks the .EXE beside it",
            vec![&both, &first],
            ".EXE;.COM",
            &both_exe,
        ),
    ];

    let mut resolved = BTreeSet::new();
    for (what, dirs, pathext, expected) in cases {
        let path = path_of(&dirs);
        let env = composed(&[("PATH", &path), ("PATHEXT", OsStr::new(pathext))]);
        let actual = resolve_program("x", &env, KeyCase::Insensitive, ProgramNaming::Windows)
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        assert_eq!(&actual, expected, "{what}");
        resolved.insert(actual);
    }
    assert_eq!(
        resolved.len(),
        5,
        "the grid must reach five distinct files; fewer means an axis is not varying: \
         {resolved:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_windows_name_is_pathext_and_never_the_extensionless_file() {
    let root = scratch("resolve-pathext");
    let dir = root.join("bin");
    let bare = program_file(&dir, "x");
    let exe = program_file(&dir, "x.exe");
    let com = program_file(&dir, "x.com");
    let foo = program_file(&dir, "x.foo");
    let path = path_of(&[&dir]);

    let resolve = |pathext: Option<&str>, naming| {
        let mut pairs: Vec<(&str, &OsStr)> = vec![("PATH", &path)];
        let value;
        if let Some(text) = pathext {
            value = OsString::from(text);
            pairs.push(("PATHEXT", &value));
        }
        resolve_program("x", &composed(&pairs), KeyCase::Insensitive, naming)
    };

    for absent in [None, Some(""), Some(";;;"), Some("exe"), Some(".")] {
        assert_eq!(
            resolve(absent, ProgramNaming::Windows).expect("the default PATHEXT applies"),
            com,
            "PATHEXT={absent:?}: an unusable PATHEXT must fall back to the platform default, \
             not to \"no candidates\""
        );
    }
    assert_eq!(
        resolve(Some(".FOO"), ProgramNaming::Windows).expect("PATHEXT is honoured"),
        foo
    );
    assert_eq!(
        resolve(Some(".EXE"), ProgramNaming::Windows).expect("PATHEXT is honoured"),
        exe
    );
    assert!(
        bare.is_file(),
        "the premise: an extensionless file must be present, or this row proves nothing"
    );
    for pathext in [None, Some(REAL_PATHEXT), Some(".FOO")] {
        assert_ne!(
            resolve(pathext, ProgramNaming::Windows).expect("something resolves"),
            bare,
            "PATHEXT={pathext:?}: an extensionless file was treated as a Windows program"
        );
    }
    for pathext in [None, Some(REAL_PATHEXT), Some(".FOO")] {
        assert_eq!(
            resolve(pathext, ProgramNaming::Posix).expect("Unix resolves the name itself"),
            bare,
            "PATHEXT={pathext:?}: Unix consulted PATHEXT"
        );
    }
    let value = OsString::from(".FOO");
    let with_extension = resolve_program(
        "x.exe",
        &composed(&[("PATH", &path), ("PATHEXT", &value)]),
        KeyCase::Insensitive,
        ProgramNaming::Windows,
    )
    .expect("a name with an extension resolves");
    assert_eq!(with_extension, exe, "`x.exe` resolved to something else");
    let shadow = root.join("shadow");
    std::fs::create_dir_all(shadow.join("x.exe")).expect("a directory named like a program");
    let shadow_path = path_of(&[&shadow, &dir]);
    for naming in naming_grid() {
        let resolved = resolve_program(
            "x.exe",
            &composed(&[("PATH", &shadow_path)]),
            KeyCase::Insensitive,
            naming,
        )
        .unwrap_or_else(|error| panic!("{naming:?}: {error}"));
        assert_eq!(
            resolved, exe,
            "{naming:?}: a directory named `x.exe` was resolved as a program"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn a_candidate_without_the_execute_bit_is_skipped_the_way_execvp_skips_it() {
    let root = scratch("resolve-mode");
    let first = root.join("first");
    let second = root.join("second");
    let blocked = unexecutable_file(&first, "x");
    let usable = program_file(&second, "x");

    let both = path_of(&[&first, &second]);
    assert_eq!(
        resolve_program(
            "x",
            &composed(&[("PATH", &both)]),
            KeyCase::Sensitive,
            ProgramNaming::Posix
        )
        .expect("the executable one further along PATH"),
        usable
    );
    assert!(
        blocked.is_file(),
        "the premise: the skipped candidate must exist"
    );

    let only_blocked = path_of(&[&first]);
    resolve_program(
        "x",
        &composed(&[("PATH", &only_blocked)]),
        KeyCase::Sensitive,
        ProgramNaming::Posix,
    )
    .expect_err("a file with no execute bit is not a program");

    assert_eq!(
        resolve_program(
            "x",
            &composed(&[("PATH", &only_blocked), ("PATHEXT", OsStr::new(".EXE"))]),
            KeyCase::Insensitive,
            ProgramNaming::Windows,
        )
        .ok(),
        None,
        "Windows names `x` only through PATHEXT, so this row must refuse for the other reason"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_name_that_matches_nothing_is_refused_and_an_empty_path_entry_is_never_searched() {
    let root = scratch("resolve-refusal");
    let dir = root.join("bin");
    let found = program_file(&dir, &shim_file_name("x"));
    let empty_dir = root.join("empty");
    std::fs::create_dir_all(&empty_dir).expect("an empty directory");

    let naming = ProgramNaming::current();
    let refuse = |path: Option<&OsStr>| {
        let mut pairs: Vec<(&str, &OsStr)> = Vec::new();
        if let Some(value) = path {
            pairs.push(("PATH", value));
        }
        resolve_program(
            "upstroke-no-such-program",
            &composed(&pairs),
            KeyCase::current(),
            naming,
        )
        .expect_err("nothing of that name exists")
        .to_string()
    };

    let one_empty_entry = path_of(&[Path::new("")]);
    let with_dir = path_of(&[&empty_dir]);
    for (what, path, directories) in [
        ("PATH is absent entirely", None, "0 directories"),
        ("PATH is empty", Some(OsStr::new("")), "0 directories"),
        (
            "PATH is a single empty entry",
            Some(one_empty_entry.as_os_str()),
            "0 directories",
        ),
        (
            "PATH is one real but empty directory",
            Some(with_dir.as_os_str()),
            "1 directory",
        ),
    ] {
        let message = refuse(path);
        assert!(
            message.contains("upstroke-no-such-program"),
            "{what}: the refusal must name the program: {message}"
        );
        assert!(
            message.contains("host runner"),
            "{what}: the refusal must name the boundary: {message}"
        );
        assert!(
            message.contains(directories),
            "{what}: expected `{directories}` searched: {message}"
        );
    }

    let mixed = path_of(&[Path::new(""), &dir]);
    assert_eq!(
        resolve_program(
            "x",
            &composed(&[("PATH", &mixed)]),
            KeyCase::current(),
            naming
        )
        .expect("the real directory is still searched"),
        found
    );
    let message = resolve_program(
        "upstroke-no-such-program",
        &composed(&[("PATH", &mixed)]),
        KeyCase::current(),
        naming,
    )
    .expect_err("nothing of that name exists")
    .to_string();
    assert!(
        message.contains("1 directory searched"),
        "the empty entry was counted as a directory: {message}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn a_bare_name_that_only_pathext_resolves_runs_through_the_host_runner() {
    let root = scratch("pathext-spawn");
    let bin = root.join("bin");
    let workspace = root.join("ws");
    std::fs::create_dir_all(&workspace).expect("a workspace");

    let stem = format!("upstroke-d1-{}", crate::ulid::ulid());
    let cases = [
        (format!("{stem}-c"), "cmd", "CMDSHIM", "hello world"),
        (format!("{stem}-b"), "bat", "BATSHIM", "second argument"),
    ];
    for (name, extension, marker, _) in &cases {
        marker_shim(&bin, &format!("{name}.{extension}"), marker);
    }

    let environment = environment_on_path(&[&bin], Some(REAL_PATHEXT));
    let composed_env = environment
        .compose(&ExecutionRole::Gate, None, &[])
        .expect("compose the child environment");
    assert!(
        composed_env.iter().any(|(key, value)| key == "PATHEXT"
            && value.to_string_lossy().to_uppercase().contains(".CMD")),
        "the premise of this case: PATHEXT must list .CMD"
    );

    let runner = HostRunner::new().with_environment(environment);
    let mut markers = BTreeSet::new();
    let mut arguments = BTreeSet::new();
    for (name, extension, marker, argument) in &cases {
        let mut direct = Command::new(name);
        direct.env_clear();
        direct.envs(composed_env.clone());
        direct.current_dir(&workspace);
        direct.args([argument]);
        let error = match direct.output() {
            Ok(output) => panic!(
                ".{extension}: std spawned a bare name it cannot resolve, so this platform \
                 never had PR6D-001: {output:?}"
            ),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::NotFound,
            ".{extension}: the platform fact this test rests on has changed: {error}"
        );

        let output = runner
            .run(&named_request(name, argument, &workspace))
            .unwrap_or_else(|error| panic!(".{extension}: {error}"));
        assert_eq!(output.code, Some(0), ".{extension}: {output:?}");
        assert!(
            output.stdout.contains(&format!("{marker}:{argument}")),
            ".{extension}: the shim did not run with its argument: {:?}",
            output.stdout
        );
        markers.insert((*marker).to_owned());
        arguments.insert((*argument).to_owned());
    }
    assert_eq!(markers.len(), 2, "both shims must have run: {markers:?}");
    assert_eq!(
        arguments.len(),
        2,
        "each shim must have received its own argument: {arguments:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn two_runners_in_one_process_resolve_one_name_against_their_own_environments() {
    let root = scratch("resolve-two-runners");
    let workspace = root.join("ws");
    std::fs::create_dir_all(&workspace).expect("a workspace");
    let name = format!("upstroke-d1-{}", crate::ulid::ulid());
    let file = shim_file_name(&name);

    let left_dir = root.join("left");
    let right_dir = root.join("right");
    marker_shim(&left_dir, &file, "LEFT");
    marker_shim(&right_dir, &file, "RIGHT");
    let left =
        HostRunner::new().with_environment(environment_on_path(&[&left_dir], Some(REAL_PATHEXT)));
    let right =
        HostRunner::new().with_environment(environment_on_path(&[&right_dir], Some(REAL_PATHEXT)));

    let ran = |runner: &HostRunner, which: &str| -> String {
        let output = runner
            .run(&named_request(&name, "arg", &workspace))
            .unwrap_or_else(|error| panic!("{which}: {error}"));
        assert_eq!(output.code, Some(0), "{which}: {output:?}");
        output.stdout.trim().to_owned()
    };

    let left_first = [ran(&left, "left"), ran(&right, "right")];
    let right_first = [ran(&right, "right"), ran(&left, "left")];
    assert_eq!(left_first, ["LEFT:arg", "RIGHT:arg"]);
    assert_eq!(right_first, ["RIGHT:arg", "LEFT:arg"]);
    let seen: BTreeSet<&String> = left_first.iter().chain(right_first.iter()).collect();
    assert_eq!(
        seen.len(),
        2,
        "one name reached two boundaries and got one answer: {seen:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_absolute_program_is_spawned_as_given_even_when_path_holds_that_name() {
    let root = scratch("resolve-absolute");
    let workspace = root.join("ws");
    std::fs::create_dir_all(&workspace).expect("a workspace");
    let name = format!("upstroke-d1-{}", crate::ulid::ulid());
    let file = shim_file_name(&name);

    let on_path = root.join("on-path");
    marker_shim(&on_path, &file, "ONPATH");
    let installed = root.join(A_DIRECTORY_WITH_A_SPACE);
    let absolute = marker_shim(&installed, &file, "ABSOLUTE");
    assert!(absolute.is_absolute() && absolute.to_string_lossy().contains(' '));

    let runner =
        HostRunner::new().with_environment(environment_on_path(&[&on_path], Some(REAL_PATHEXT)));
    let program = absolute
        .to_str()
        .expect("a scratch path this crate can name")
        .to_owned();

    let by_path = runner
        .run(&named_request(&program, "arg", &workspace))
        .expect("an absolute shim spawns");
    assert_eq!(by_path.code, Some(0), "{by_path:?}");
    assert_eq!(
        by_path.stdout.trim(),
        "ABSOLUTE:arg",
        "an absolute program was re-resolved against PATH"
    );

    let by_name = runner
        .run(&named_request(&name, "arg", &workspace))
        .expect("the bare name resolves on PATH");
    assert_eq!(by_name.stdout.trim(), "ONPATH:arg");
    let _ = std::fs::remove_dir_all(&root);
}

struct ResolutionWitness {
    at_points: Arc<Mutex<Vec<(SubEffectPoint, u64)>>>,
}

impl ResolutionWitness {
    fn new() -> Self {
        Self {
            at_points: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn handle(&self) -> Self {
        Self {
            at_points: Arc::clone(&self.at_points),
        }
    }

    fn seen(&self) -> Vec<(SubEffectPoint, u64)> {
        self.at_points
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl SpawnHooks for ResolutionWitness {
    fn point(&mut self, point: SubEffectPoint) -> Injection {
        self.at_points
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((point, program_resolutions()));
        Injection::Proceed
    }
}

#[test]
fn a_program_is_resolved_once_per_spawn_before_any_of_it_and_never_before_compose_refuses() {
    let root = scratch("resolve-once");
    let workspace = root.join("ws");
    std::fs::create_dir_all(&workspace).expect("a workspace");
    let name = format!("upstroke-d1-{}", crate::ulid::ulid());
    let bin = root.join("bin");
    let shim = marker_shim(&bin, &shim_file_name(&name), "ONCE");
    let absolute = shim
        .to_str()
        .expect("a scratch path this crate can name")
        .to_owned();

    for (what, program) in [
        ("a bare name", name.clone()),
        ("an absolute path", absolute),
    ] {
        let witness = ResolutionWitness::new();
        let runner = HostRunner::new()
            .with_environment(environment_on_path(&[&bin], Some(REAL_PATHEXT)))
            .with_hooks(Box::new(witness.handle()));

        let before = program_resolutions();
        let output = runner
            .run(&named_request(&program, "arg", &workspace))
            .unwrap_or_else(|error| panic!("{what}: {error}"));
        assert_eq!(output.stdout.trim(), "ONCE:arg", "{what}");
        assert_eq!(
            program_resolutions(),
            before + 1,
            "{what}: one spawn resolved its program more than once, or not at all"
        );

        let seen = witness.seen();
        let points: Vec<SubEffectPoint> = seen.iter().map(|(point, _)| *point).collect();
        assert_eq!(
            points,
            per_spawn_points(),
            "{what}: a resolved program did not reach this platform's containment points \
             in order"
        );
        for (point, count) in seen {
            assert_eq!(
                count,
                before + 1,
                "{what}: at {point:?} the program had been resolved {count} times, not once \
                 — resolution must complete before any of the spawn"
            );
        }
    }

    let witness = ResolutionWitness::new();
    let runner = HostRunner::new()
        .with_environment(environment_on_path(&[&bin], Some(REAL_PATHEXT)))
        .with_hooks(Box::new(witness.handle()));
    let mut request = named_request(&name, "arg", &workspace);
    request.command.env = vec![("PATH".to_owned(), "/somewhere/else".to_owned())];
    let before = program_resolutions();
    let error = runner
        .run(&request)
        .expect_err("an overlay naming a reserved key is refused pre-flight");
    assert!(error.to_string().contains("reserved"), "{error}");
    assert_eq!(
        program_resolutions(),
        before,
        "a request refused by compose still resolved its program"
    );
    assert!(
        witness.seen().is_empty(),
        "a request refused by compose reached a containment point: {:?}",
        witness.seen()
    );

    let witness = ResolutionWitness::new();
    let runner = HostRunner::new()
        .with_environment(environment_on_path(&[&root.join("nothing-here")], None))
        .with_hooks(Box::new(witness.handle()));
    let before = program_resolutions();
    let error = runner
        .run(&named_request(&name, "arg", &workspace))
        .expect_err("a name that matches nothing is refused");
    assert!(error.to_string().contains(&name), "{error}");
    assert_eq!(
        program_resolutions(),
        before + 1,
        "the refusal did not come from a resolution"
    );
    assert!(
        witness.seen().is_empty(),
        "a refused name still reached a containment point: {:?}",
        witness.seen()
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn every_bare_program_this_crate_ships_goes_through_one_resolution_rule() {
    const NAMES: [&str; 8] = [
        "cmd",
        "sh",
        "bash",
        "powershell",
        "pwsh",
        "claude",
        "codex",
        "copilot",
    ];
    for shell in [
        ShellKind::Cmd,
        ShellKind::Sh,
        ShellKind::Bash,
        ShellKind::PowerShell,
        ShellKind::Pwsh,
    ] {
        match shell {
            ShellKind::Cmd
            | ShellKind::Sh
            | ShellKind::Bash
            | ShellKind::PowerShell
            | ShellKind::Pwsh => {}
        }
        assert!(
            NAMES.contains(&shell.spec("exit 0").program.as_str()),
            "a shell ships a program this grid does not carry: {shell:?}"
        );
    }

    let root = scratch("resolve-one-rule");
    let bin = root.join("bin");
    let path = path_of(&[&bin]);
    let naming = ProgramNaming::current();
    let mut resolved = BTreeSet::new();
    for name in NAMES {
        assert!(
            naming.is_bare_name(name),
            "`{name}` is what production ships and it is not a name"
        );
        let expected = program_file(&bin, &shim_file_name(name));
        let actual = resolve_program(
            name,
            &composed(&[("PATH", &path), ("PATHEXT", OsStr::new(REAL_PATHEXT))]),
            KeyCase::current(),
            naming,
        )
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(actual, expected, "{name}");
        resolved.insert(actual);
    }
    assert_eq!(
        resolved.len(),
        NAMES.len(),
        "eight names reached fewer than eight files: {resolved:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn a_resolved_cmd_keeps_the_raw_tail_rule() {
    let spec = ShellKind::Cmd.spec(r#"echo "quoted arg""#);
    assert_eq!(spec.program, "cmd", "the shell ships a bare name");

    let environment = HostEnvironment::from_process();
    let composed = environment
        .compose(&ExecutionRole::Gate, None, &[])
        .expect("compose this process's environment");
    let resolved = resolve_program(
        &spec.program,
        &composed,
        environment.case(),
        ProgramNaming::current(),
    )
    .expect("this runner resolves the recorded shell");
    assert_ne!(
        resolved,
        PathBuf::from("cmd"),
        "the resolved spelling must differ from the bare one, or this compares one thing \
         with itself"
    );
    assert!(resolved.is_absolute(), "{}", resolved.display());

    let mut stdouts = BTreeSet::new();
    for program in [PathBuf::from("cmd"), resolved.clone()] {
        let out = build_command_at(&spec, &program)
            .output()
            .unwrap_or_else(|error| panic!("{}: {error}", program.display()));
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        assert_eq!(
            stdout,
            r#""quoted arg""#,
            "{}: the /C tail was re-quoted",
            program.display()
        );
        stdouts.insert(stdout);
    }
    assert_eq!(
        stdouts.len(),
        1,
        "resolving the shell changed what the child saw: {stdouts:?}"
    );

    let workspace = scratch("raw-tail");
    let request = crate::runner::gate_request(
        spec.clone(),
        workspace.clone(),
        Duration::from_secs(60),
        gate_invocation(),
    );
    let out = HostRunner::new()
        .run(&request)
        .expect("a gate's shell runs through the runner");
    assert_eq!(
        out.stdout.trim(),
        r#""quoted arg""#,
        "the runner's own route re-quoted the tail"
    );

    let shim = build_command_at(&spec, Path::new(r"C:\npm\claude.cmd"));
    let args: Vec<String> = shim
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        vec!["/C".to_owned(), r#"echo "quoted arg""#.to_owned()],
        "the spec's arguments must reach a non-`cmd` program unchanged"
    );
    let _ = std::fs::remove_dir_all(&workspace);
}

fn role_request_for(
    role: &ExecutionRole,
    program: &str,
    argument: &str,
    workspace: &Path,
) -> Option<RunnerRequest> {
    let spec = CommandSpec {
        program: program.to_owned(),
        args: vec![argument.to_owned()],
        env: Vec::new(),
        stdin: Vec::new(),
    };
    let timeout = Duration::from_secs(60);
    match role {
        ExecutionRole::Probe(ProbeTarget::Shell) => None,
        ExecutionRole::Probe(ProbeTarget::Agent(agent)) => Some(
            crate::agent::probe_request(agent.as_str(), spec, 0, timeout)
                .expect("a shipped adapter id"),
        ),
        ExecutionRole::Implement => Some(crate::runner::worker_request(
            spec,
            workspace.to_path_buf(),
            fixture_agent(),
            timeout,
            worker_invocation(),
        )),
        ExecutionRole::Review => Some(crate::runner::review_request(
            spec,
            workspace.to_path_buf(),
            fixture_agent(),
            timeout,
            review_invocation(),
        )),
        ExecutionRole::Gate => Some(crate::runner::gate_request(
            spec,
            workspace.to_path_buf(),
            timeout,
            gate_invocation(),
        )),
    }
}

#[test]
fn one_boundary_executes_one_file_for_a_name_across_a_probe_and_the_attempt() {
    let root = scratch("one-executable");
    let workspace = root.join("ws");
    std::fs::create_dir_all(&workspace).expect("a workspace");
    let name = format!("upstroke-d2-{}", crate::ulid::ulid());
    let file = shim_file_name(&name);

    let first_dir = root.join("first");
    let second_dir = root.join("second");
    let first = marker_shim(&first_dir, &file, "FIRST");
    marker_shim(&second_dir, &file, "SECOND");
    let environment = || environment_on_path(&[&first_dir, &second_dir], Some(REAL_PATHEXT));

    let runner = HostRunner::new().with_environment(environment());
    let probe = role_request_for(
        &ExecutionRole::Probe(ProbeTarget::Agent(fixture_agent())),
        &name,
        "arg",
        &workspace,
    )
    .expect("an agent probe carries a chosen program");
    let attempt = role_request_for(&ExecutionRole::Implement, &name, "arg", &workspace)
        .expect("a worker carries a chosen program");

    let searches = program_searches();
    let certified = runner.run(&probe).expect("the probe runs the CLI");
    assert_eq!(
        certified.stdout.trim(),
        "FIRST:arg",
        "pre-flight did not certify the first installation"
    );

    std::fs::remove_file(&first).expect("remove the certified installation");
    assert!(!first.exists());

    let fresh = HostRunner::new().with_environment(environment());
    let moved = fresh
        .run(
            &role_request_for(&ExecutionRole::Implement, &name, "arg", &workspace)
                .expect("a worker carries a chosen program"),
        )
        .expect("the second installation is reachable");
    assert_eq!(
        moved.stdout.trim(),
        "SECOND:arg",
        "the fixture cannot tell the two installations apart, so it proves nothing"
    );

    let outcome = runner.run(&attempt);
    let (how, said) = match &outcome {
        Ok(output) => (
            format!("it ran and exited {:?}", output.code),
            format!("{}{}", output.stdout, output.stderr),
        ),
        Err(error) => ("it failed".to_owned(), error.to_string()),
    };
    assert!(
        !said.contains("SECOND"),
        "the attempt ran the other installation: {how}: {said}"
    );
    assert_ne!(
        outcome.as_ref().ok().and_then(|output| output.code),
        Some(0),
        "the attempt reported success without its certified executable: {said}"
    );
    assert!(
        said.contains(&first.to_string_lossy().into_owned()),
        "what came back must name the file pre-flight certified: {how}: {said}"
    );
    assert_eq!(
        program_searches(),
        searches + 2,
        "the memoised runner searched more than once, or the fresh one did not search"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn one_name_is_searched_once_for_a_boundary_and_asked_for_once_per_spawn() {
    let root = scratch("searched-once");
    let workspace = root.join("ws");
    std::fs::create_dir_all(&workspace).expect("a workspace");
    let name = format!("upstroke-d2-{}", crate::ulid::ulid());
    let bin = root.join("bin");
    marker_shim(&bin, &shim_file_name(&name), "ONE");

    let on_path = environment_on_path(&[&bin], Some(REAL_PATHEXT));
    let case = on_path.case();
    let mut base = on_path.base().to_vec();
    base.retain(|(key, _)| !case.same_key(key, OsStr::new("CLAUDE_CONFIG_DIR")));
    base.push((os("CLAUDE_CONFIG_DIR"), os("/home/upstroke/.claude")));
    let environment = HostEnvironment::with_base(base, case);

    let requests: Vec<(String, RunnerRequest)> = ExecutionRole::all()
        .iter()
        .filter_map(|role| {
            role_request_for(role, &name, "arg", &workspace).map(|request| (role.label(), request))
        })
        .collect();
    assert_eq!(
        requests.len(),
        4,
        "four of the five roles carry a caller's program: {:?}",
        requests.iter().map(|(role, _)| role).collect::<Vec<_>>()
    );

    let composed: BTreeSet<Vec<(OsString, OsString)>> = requests
        .iter()
        .map(|(_, request)| {
            environment
                .compose(&request.role, request.agent.as_ref(), &request.command.env)
                .expect("compose each role's environment")
        })
        .collect();
    assert_eq!(
        composed.len(),
        2,
        "the four roles composed {} distinct environments; host-v1 scopes credential \
         locations by role, so a gate's must differ from an agent's",
        composed.len()
    );

    let runner = HostRunner::new().with_environment(environment);
    let searches = program_searches();
    let resolutions = program_resolutions();
    let mut stdouts = BTreeSet::new();
    for (role, request) in &requests {
        let output = runner
            .run(request)
            .unwrap_or_else(|error| panic!("{role}: {error}"));
        assert_eq!(output.code, Some(0), "{role}: {output:?}");
        stdouts.insert(output.stdout.trim().to_owned());
    }
    assert_eq!(
        stdouts,
        BTreeSet::from(["ONE:arg".to_owned()]),
        "the roles did not all reach one file"
    );
    assert_eq!(
        program_resolutions(),
        resolutions + 4,
        "a program must be decided for every spawn, memo or no memo"
    );
    assert_eq!(
        program_searches(),
        searches + 1,
        "one boundary asked the filesystem more than once for one name"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_resolution_question_is_the_program_and_the_environment_that_answers_it() {
    let root = scratch("memo-key");
    let name = format!("upstroke-d2-{}", crate::ulid::ulid());
    let file = shim_file_name(&name);
    let left_dir = root.join("left");
    let right_dir = root.join("right");
    let left = marker_shim(&left_dir, &file, "LEFT");
    let right = marker_shim(&right_dir, &file, "RIGHT");

    let runner = HostRunner::new();
    let ask = |path: &Path| -> (PathBuf, u64) {
        let composed = composed(&[
            ("PATH", path_of(&[path]).as_os_str()),
            ("PATHEXT", OsStr::new(REAL_PATHEXT)),
        ]);
        let before = program_searches();
        let answer = runner
            .program_for(&name, &composed)
            .expect("the shim is on this PATH");
        (answer, program_searches() - before)
    };

    let (first, searched_first) = ask(&left_dir);
    let (second, searched_second) = ask(&right_dir);
    let (again, searched_again) = ask(&left_dir);
    assert_eq!(first, left, "the first environment's own file");
    assert_eq!(
        second, right,
        "one runner replayed one environment's answer for another's question"
    );
    assert_eq!(again, left, "the first question stopped being answered");
    assert_eq!(
        (searched_first, searched_second, searched_again),
        (1, 1, 0),
        "a different question must search and the same question must not"
    );
    let files: BTreeSet<PathBuf> = [first, second, again].into_iter().collect();
    assert_eq!(files.len(), 2, "two environments, one answer: {files:?}");

    #[cfg(windows)]
    {
        let both = root.join("both");
        let by_cmd = marker_shim(&both, &format!("{name}.cmd"), "CMDSHIM");
        let by_bat = marker_shim(&both, &format!("{name}.bat"), "BATSHIM");
        let path = path_of(&[&both]);
        let ask_ext = |pathext: &str| -> (PathBuf, u64) {
            let composed =
                composed(&[("PATH", path.as_os_str()), ("PATHEXT", OsStr::new(pathext))]);
            let before = program_searches();
            let answer = runner
                .program_for(&name, &composed)
                .expect("a shim resolves under either PATHEXT");
            (answer, program_searches() - before)
        };
        let (cmd_first, searched_cmd) = ask_ext(".CMD;.BAT");
        let (bat_first, searched_bat) = ask_ext(".BAT;.CMD");
        assert_eq!(cmd_first, by_cmd);
        assert_eq!(
            bat_first, by_bat,
            "the memo replayed one PATHEXT's answer under another"
        );
        assert_eq!((searched_cmd, searched_bat), (1, 1));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_refused_name_is_refused_identically_without_asking_the_filesystem_again() {
    let root = scratch("memo-refusal");
    let workspace = root.join("ws");
    let bin = root.join("bin");
    std::fs::create_dir_all(&workspace).expect("a workspace");
    std::fs::create_dir_all(&bin).expect("an empty installation directory");
    let name = format!("upstroke-d2-{}", crate::ulid::ulid());
    let environment = || environment_on_path(&[&bin], Some(REAL_PATHEXT));

    let runner = HostRunner::new().with_environment(environment());
    let searches = program_searches();
    let resolutions = program_resolutions();
    let first = runner
        .run(&named_request(&name, "arg", &workspace))
        .expect_err("nothing of that name is installed");
    assert!(
        matches!(first, UpstrokeError::Refused { .. }),
        "an unresolvable name is a refusal: {first:?}"
    );
    let first = first.to_string();
    assert!(first.contains(&name), "{first}");

    marker_shim(&bin, &shim_file_name(&name), "LATE");
    let again = runner
        .run(&named_request(&name, "arg", &workspace))
        .expect_err("this boundary already answered for that name")
        .to_string();
    assert_eq!(again, first, "the replayed refusal is not the first one");
    assert_eq!(
        program_searches(),
        searches + 1,
        "the refusal was not remembered, so the filesystem decided twice"
    );
    assert_eq!(
        program_resolutions(),
        resolutions + 2,
        "both spawns must have asked for a program"
    );

    let fresh = HostRunner::new().with_environment(environment());
    let output = fresh
        .run(&named_request(&name, "arg", &workspace))
        .expect("a boundary that had not answered yet finds it");
    assert_eq!(output.stdout.trim(), "LATE:arg");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn production_reaches_a_spawn_through_one_host_runner_per_run() {
    const SITES: [(&str, usize); 6] = [
        ("src/engine/mod.rs", 2),
        ("src/engine/coordinator.rs", 0),
        ("src/engine/resume.rs", 0),
        ("src/engine/attempt.rs", 0),
        ("src/engine/preflight.rs", 0),
        ("src/engine/options.rs", 0),
    ];
    let sources = [
        ("src/engine/mod.rs", include_str!("../../engine/mod.rs")),
        (
            "src/engine/coordinator.rs",
            include_str!("../../engine/coordinator.rs"),
        ),
        (
            "src/engine/resume.rs",
            include_str!("../../engine/resume.rs"),
        ),
        (
            "src/engine/attempt.rs",
            include_str!("../../engine/attempt.rs"),
        ),
        (
            "src/engine/preflight.rs",
            include_str!("../../engine/preflight.rs"),
        ),
        (
            "src/engine/options.rs",
            include_str!("../../engine/options.rs"),
        ),
    ];
    assert_eq!(
        sources.len(),
        SITES.len(),
        "the census and its expectation cover different files"
    );

    const CONTROL: &str = r##"
// STRIP-CONTROL: HostRunner::new();
/* HostRunner::new(); /* HostRunner::new(); */ */
const TEXT: &str = "HostRunner::new();";
const RAW: &str = r#"HostRunner::new();"#;
const BYTES: &[u8] = b"HostRunner::new();";
const RAW_BYTES: &[u8] = br#"HostRunner::new();"#;
const QUOTE: char = '"';
#[cfg(test)]
pub(super) fn excluded_control() -> Result<((), ()), ()> {
    let runner = HostRunner::new();
    Ok(((), ()))
}
fn production_control() { let runner = HostRunner::new(); }
"##;
    let count_constructions = |source: &str| {
        crate::effects::production_code(source)
            .matches("HostRunner::new(")
            .count()
    };
    assert_eq!(
        count_constructions(CONTROL),
        1,
        "the control must count only the production construction"
    );
    let mut counted: Vec<(&str, usize)> = Vec::new();
    for (name, source) in sources {
        let count = count_constructions(source);
        let with_control = format!("{source}\n{CONTROL}");
        assert_eq!(
            count_constructions(&with_control),
            count + 1,
            "{name}: the census must ignore prose and test items and count an appended production construction"
        );
        counted.push((name, count));
    }
    assert_eq!(
        counted,
        SITES.to_vec(),
        "the engine constructs its host runner somewhere this repair did not account for. \
         The memo behind `program_searches` is per runner, so a runner per attempt is a \
         resolution per attempt and DESIGN.md:612 is open again"
    );

    let engine = crate::effects::production_code(include_str!("../../engine/mod.rs"));
    for facade in ["fn run_harness(", "fn resume_harness("] {
        let after = engine
            .split_once(facade)
            .map(|(_, rest)| rest.lines().take(8).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        assert!(
            after.contains("HostRunner::new()"),
            "`{facade}` is not one of the two construction sites this census counted"
        );
    }
}

#[test]
fn an_npm_style_installation_runs_by_bare_name_exactly_as_it_runs_by_path() {
    const CLIS: [&str; 3] = ["claude", "codex", "copilot"];

    let root = scratch("npm-style-equivalence");
    let workspace = root.join("ws");
    std::fs::create_dir_all(&workspace).expect("a workspace");
    let bin = root.join("node_modules-bin");

    let mut installed = Vec::new();
    for cli in CLIS {
        let file = shim_file_name(cli);
        installed.push((
            cli,
            file.clone(),
            marker_shim(&bin, &file, &cli.to_uppercase()),
        ));
    }

    let mut contents: Vec<String> = std::fs::read_dir(&bin)
        .expect("read the installation directory")
        .map(|entry| {
            entry
                .expect("an installation entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    contents.sort();
    let mut expected: Vec<String> = installed.iter().map(|(_, file, _)| file.clone()).collect();
    expected.sort();
    assert_eq!(contents, expected, "one file per CLI, and nothing else");
    for (cli, file, _) in &installed {
        assert_eq!(
            Path::new(file).extension().map(OsStr::to_os_string),
            if cfg!(windows) {
                Some(OsString::from("cmd"))
            } else {
                None
            },
            "{cli}: this is not the shape npm installs an agent CLI in"
        );
    }

    let runner =
        HostRunner::new().with_environment(environment_on_path(&[&bin], Some(REAL_PATHEXT)));
    let mut markers = BTreeSet::new();
    for (cli, _, path) in &installed {
        let located = path
            .to_str()
            .expect("a scratch path this crate can name")
            .to_owned();
        assert_ne!(*cli, located, "{cli}: the two program strings must differ");

        let mut stdouts = BTreeSet::new();
        for (what, program) in [
            ("the bare name", (*cli).to_owned()),
            ("the resolved path", located),
        ] {
            let output = runner
                .run(&named_request(&program, "arg", &workspace))
                .unwrap_or_else(|error| panic!("{cli}: {what}: {error}"));
            assert_eq!(output.code, Some(0), "{cli}: {what}: {output:?}");
            assert_eq!(
                output.stdout.trim(),
                format!("{}:arg", cli.to_uppercase()),
                "{cli}: {what}: this did not run the installed shim"
            );
            stdouts.insert(output.stdout.trim().to_owned());
        }
        assert_eq!(
            stdouts.len(),
            1,
            "{cli}: the bare name and the resolved path are different programs: {stdouts:?}"
        );
        markers.extend(stdouts);
    }
    assert_eq!(
        markers.len(),
        CLIS.len(),
        "the three CLIs did not reach three files: {markers:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

const PATH_ENTRY_TABLE: &[(&str, bool, bool)] = &[
    ("", false, false),
    (".", false, false),
    ("..", false, false),
    ("bin", false, false),
    ("./bin", false, false),
    ("/usr/local/bin", true, false),
    (r"\Windows\System32", false, false),
    (r"\\server\share\bin", false, true),
    ("~/bin", false, false),
];

#[test]
fn every_path_entry_this_runner_searches_names_a_location_on_its_own() {
    let root = scratch("path-entry-rule");
    let naming = ProgramNaming::current();
    let mut located = 0_usize;
    let mut relative = 0_usize;
    for (entry, on_unix, on_windows) in PATH_ENTRY_TABLE {
        let expected = if cfg!(windows) { *on_windows } else { *on_unix };
        assert_eq!(
            Path::new(entry).is_absolute(),
            expected,
            "`{entry}`: this platform disagrees with the table"
        );
        let message = resolve_program(
            "upstroke-no-such-program",
            &composed(&[("PATH", OsStr::new(entry))]),
            KeyCase::current(),
            naming,
        )
        .expect_err("nothing of that name exists anywhere")
        .to_string();
        if expected {
            located += 1;
            assert!(
                message.contains("1 directory searched,"),
                "`{entry}` names a location and was not searched: {message}"
            );
            assert!(
                !message.contains("skipped"),
                "`{entry}` names a location and was skipped: {message}"
            );
        } else {
            relative += 1;
            assert!(
                message.contains(
                    "0 directories searched, 1 PATH entry skipped as not \
                                  absolute"
                ),
                "`{entry}` does not name a location and was searched: {message}"
            );
        }
    }
    assert_eq!(
        (located, relative),
        (1, 8),
        "the table lost its teeth: it must hold entries of both kinds on both platforms"
    );

    let bin = root.join("bin");
    let found = program_file(&bin, &shim_file_name("x"));
    assert_eq!(
        resolve_program(
            "x",
            &composed(&[("PATH", path_of(&[&bin]).as_os_str())]),
            KeyCase::current(),
            naming,
        )
        .expect("an absolute entry is searched"),
        found
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn an_empty_path_entry_never_reaches_the_workspaces_own_copy_of_a_bare_name() {
    let root = scratch("empty-path-entry");
    let workspace = root.join("ws");
    let installed_dir = root.join("installed");
    let empty_dir = root.join("empty");
    std::fs::create_dir_all(&empty_dir).expect("an installation directory with nothing in it");
    let name = format!("upstroke-d2-{}", crate::ulid::ulid());
    let file = shim_file_name(&name);
    marker_shim(&workspace, &file, "WORKSPACE");
    marker_shim(&installed_dir, &file, "INSTALLED");

    let installed = installed_dir.to_string_lossy().into_owned();
    let empty = empty_dir.to_string_lossy().into_owned();
    let rows: [(&str, String, &str, &str); 4] = [
        (
            "an empty entry before a real installation",
            format!(":{installed}"),
            "WORKSPACE",
            "INSTALLED",
        ),
        (
            "an empty entry after a real installation",
            format!("{installed}:"),
            "INSTALLED",
            "INSTALLED",
        ),
        (
            "nothing but empty entries",
            ":".to_owned(),
            "WORKSPACE",
            "<refused>",
        ),
        (
            "an empty entry and a directory holding nothing",
            format!(":{empty}"),
            "WORKSPACE",
            "<refused>",
        ),
    ];

    let mut raw_markers = BTreeSet::new();
    let mut divergent = 0_usize;
    for (what, path, raw_expected, runner_expected) in &rows {
        let environment = HostEnvironment::with_base(
            vec![(os("PATH"), os(path)), (os("HOME"), os("/home/upstroke"))],
            KeyCase::current(),
        );
        let composed_env = environment
            .compose(&ExecutionRole::Gate, None, &[])
            .expect("compose the child environment");

        let mut direct = Command::new(&name);
        direct.env_clear();
        direct.envs(composed_env.clone());
        direct.current_dir(&workspace);
        direct.arg("arg");
        let raw = direct
            .output()
            .unwrap_or_else(|error| panic!("{what}: a raw spawn: {error}"));
        let raw = String::from_utf8_lossy(&raw.stdout).trim().to_owned();
        assert_eq!(
            raw,
            format!("{raw_expected}:arg"),
            "{what}: the platform fact this row rests on has changed"
        );
        raw_markers.insert(raw.clone());

        let witness = ResolutionWitness::new();
        let runner = HostRunner::new()
            .with_environment(environment)
            .with_hooks(Box::new(witness.handle()));
        let outcome = runner.run(&named_request(&name, "arg", &workspace));
        if *runner_expected == "<refused>" {
            let error = outcome
                .as_ref()
                .err()
                .map(std::string::ToString::to_string)
                .unwrap_or_else(|| format!("{what}: it ran: {outcome:?}"));
            assert!(
                error.contains("host runner cannot execute"),
                "{what}: {error}"
            );
            assert!(
                witness.seen().is_empty(),
                "{what}: a refused name still reached a containment point: {:?}",
                witness.seen()
            );
        } else {
            let output = outcome.unwrap_or_else(|error| panic!("{what}: {error}"));
            assert_eq!(
                output.stdout.trim(),
                format!("{runner_expected}:arg"),
                "{what}: the runner did not reach the installed CLI"
            );
        }
        if raw != format!("{runner_expected}:arg") {
            divergent += 1;
        }
    }
    assert_eq!(
        raw_markers.len(),
        2,
        "the fixture cannot tell the workspace's copy from the installed one: {raw_markers:?}"
    );
    assert_eq!(
        divergent, 3,
        "on {divergent} rows a raw spawn and the runner disagreed; a fixture where they \
         always agree cannot see this finding at all"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn a_relative_path_entry_is_refused_even_when_it_names_a_real_directory() {
    let root = scratch("relative-path-entry");
    let bin = root.join("bin");
    let name = format!("upstroke-d2-{}", crate::ulid::ulid());
    let file = shim_file_name(&name);
    let absolute = marker_shim(&bin, &file, "RELATIVE");

    let here = std::env::current_dir().expect("the coordinator's working directory");
    let normal = |path: &Path| {
        path.components()
            .filter(|component| matches!(component, std::path::Component::Normal(_)))
            .count()
    };
    let ups = normal(&here);
    let mut workspace = root.clone();
    while normal(&workspace) <= ups {
        workspace = workspace.join("d");
    }
    std::fs::create_dir_all(&workspace).expect("a workspace");
    let down = bin
        .strip_prefix("/")
        .expect("a Unix scratch path is rooted")
        .to_string_lossy()
        .into_owned();
    let entry = format!("{}{down}", "../".repeat(ups));

    assert!(
        Path::new(&entry).is_relative(),
        "{entry} is not a relative entry, so this row is not about one"
    );
    assert!(
        Path::new(&entry).join(&file).is_file(),
        "{entry}/{file} does not resolve from the coordinator's working directory, so a \
         runner that searched it would find nothing and this row would prove nothing"
    );
    assert!(
        !workspace.join(&entry).join(&file).exists(),
        "the entry resolves to the same file from the workspace, so the two current \
         directories are not distinguishable here"
    );

    let witness = ResolutionWitness::new();
    let runner = HostRunner::new()
        .with_environment(HostEnvironment::with_base(
            vec![(os("PATH"), os(&entry))],
            KeyCase::current(),
        ))
        .with_hooks(Box::new(witness.handle()));
    let error = runner
        .run(&named_request(&name, "arg", &workspace))
        .expect_err("a relative PATH entry contributes no candidate")
        .to_string();
    assert!(
        error.contains("1 PATH entry skipped as not absolute"),
        "{error}"
    );
    assert!(
        witness.seen().is_empty(),
        "a refused name still reached a containment point: {:?}",
        witness.seen()
    );

    let reachable = HostRunner::new()
        .with_environment(environment_on_path(&[&bin], Some(REAL_PATHEXT)))
        .run(&named_request(&name, "arg", &workspace))
        .expect("the same installation, named absolutely");
    assert_eq!(reachable.stdout.trim(), "RELATIVE:arg");
    assert!(absolute.is_absolute());
    let _ = std::fs::remove_dir_all(&root);
}
