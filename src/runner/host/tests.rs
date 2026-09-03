// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, which
// carries this file's own review clause. This is the extracted test module of
// the Process funnel in `src/runner/host.rs` -- the region that lived inline
// there, under that file's own inner allow of these same three lints, moved out
// unchanged. The set of calls permitted here is unchanged by the move: the
// fixtures build and tear down scratch trees, mark programs executable, and
// spawn real child processes, exactly as they did inline. What moved is where
// the permission is stated, not what it permits.
//
// `PR6-LANEF-004`: it states that level **of its own** rather than inheriting
// one. A lint level is scoped by the MODULE TREE and not by the file, so an
// out-of-line child of a funnel is covered by the parent's allow unless it says
// otherwise, and the funnel's child-module census requires each child to say so.
// All three are needed here and each was measured at extraction; the counts are
// in the review clause.
// `decisions.effect_site_inventory.mechanism` (2).
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use super::*;
use crate::runner::invocation::{AttemptRole, InvocationId};
use crate::runner::policy::resolve_host;
use crate::runner::{HarnessHooks, ProbeTarget, SPAWN_SITE};
use crate::topology::effects::{HookHarness, Injection, InjectionMode, Platform, SubEffectPoint};
use crate::topology::events::{AttemptNumber, GenerationId};
use crate::topology::registry::TaskKey;

// -----------------------------------------------------------------------
// helpers
// -----------------------------------------------------------------------

fn os(value: &str) -> OsString {
    OsString::from(value)
}

/// A gate identity, for the tests whose subject is not the identity.
fn gate_invocation() -> InvocationId {
    InvocationId::attempt(
        TaskKey(0),
        GenerationId(0),
        AttemptNumber(1),
        AttemptRole::Gate(0),
        0,
    )
}

/// The pre-flight shell probe's identity: the packet's third form, target
/// `Shell`.
fn shell_probe_invocation() -> InvocationId {
    InvocationId::probe(ProbeTarget::Shell, 0).expect("the shell probe identity")
}

/// One attempt's worker identity — the packet's first form, role `worker`.
fn worker_invocation() -> InvocationId {
    InvocationId::attempt(
        TaskKey(0),
        GenerationId(0),
        AttemptNumber(1),
        AttemptRole::Worker,
        0,
    )
}

/// One review pass's identity — the same form, role `review_pass0`.
fn review_invocation() -> InvocationId {
    InvocationId::attempt(
        TaskKey(0),
        GenerationId(0),
        AttemptNumber(1),
        AttemptRole::ReviewPass(0),
        0,
    )
}

/// The agent a worker or a reviewer is bound to.
///
/// `ExecutionRole::all` names the same adapter for its agent-probe target,
/// so the grid's three slotted roles all carry a real, shipped id — which
/// is what `host-v1` looks up a credential location by.
fn fixture_agent() -> AgentId {
    AgentId::new(claude::ADAPTER_ID)
}

/// A base written by the test rather than read from the machine, so a
/// composition fixture asserts the same thing on every developer box.
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

/// The shell every platform has, invoked exactly as `gates` invokes it.
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

// -----------------------------------------------------------------------
// policy
// -----------------------------------------------------------------------

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

// -----------------------------------------------------------------------
// environment composition fixtures  (proof test: "environment composition
// fixtures")
// -----------------------------------------------------------------------

#[test]
fn environment_composition_fixtures() {
    // Every fixture is (role, agent, overlay, what the composition must
    // say). The expected values are written from DESIGN.md:258-264, not
    // read back out of `compose`.
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
                // The base carries all three; a gate is repository code
                // and receives none of them.
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
                // A claude-code worker is not told where codex's or
                // copilot's credentials live.
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
                // A reviewer runs an agent CLI, so it is supplied the
                // unscoped values too.
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
                // The host knows no credential location for this agent, so
                // it supplies none — and the base's three do not leak in
                // its place.
                ("CLAUDE_CONFIG_DIR", None),
                ("COPILOT_HOME", None),
                ("CODEX_HOME", None),
            ],
        },
    ];

    // Fixture hostility as distinct-value counts, not as a comment.
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
            // One variable, one entry: a duplicated key is an environment
            // whose meaning depends on which end the child's runtime reads.
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
    // "set but empty" and "unset" are different environments, and CLIs
    // read them differently.
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

/// The refusal is `expected_failures_refusals[0]`: "reserved env conflict
/// -> pre-flight error".
#[test]
fn a_reserved_key_in_the_overlay_is_a_preflight_error() {
    // Written out from DESIGN.md:260-262 ("role-scoped HOME, PATH, and
    // credential locations") and capacity.rs:36-37, not read from
    // `reserved_keys()`.
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

/// Refused **wherever it sits in the overlay**.
///
/// Every reserved-key fixture in this suite hands `compose` a one-pair
/// overlay, so the conflicting pair is always `overlay.first()` — and a
/// scan that stopped after the first pair is indistinguishable from a full
/// one given those inputs. An adapter's overlay is not one pair:
/// `invariants_introduced[0]` says "reserved keys refused pre-flight",
/// which is a claim quantified over the whole vector.
///
/// The grid is the positions a conflict can occupy in an overlay of four:
/// first, two interior, and last.
#[test]
fn a_reserved_key_is_refused_at_every_position_in_the_overlay() {
    const HARMLESS: [&str; 3] = ["UPSTROKE_ALPHA", "UPSTROKE_BETA", "UPSTROKE_GAMMA"];
    let agent = AgentId::new(claude::ADAPTER_ID);
    let mut refusals = 0_usize;
    for case in KeyCase::ALL {
        let environment = HostEnvironment::with_base(synthetic_base(), *case);
        // The control: four harmless pairs compose, so a refusal below is
        // the reserved key and not the shape of the overlay.
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
    // Six keys x four positions x two key cases, counted so a grid that
    // quietly stopped traversing fails here rather than passing smaller.
    assert_eq!(refusals, 6 * 4 * KeyCase::ALL.len());
}

/// Refused by **key**, whatever the value — including the runner's own.
///
/// `invariants_introduced[0]` is "reserved keys refused pre-flight" and
/// `expected_failures_refusals[0]` is "reserved env conflict -> pre-flight
/// error naming the key". Neither says "unless the adapter agrees with the
/// runner today": an overlay allowed to restate `PATH` because the value
/// happens to match is an overlay that has taken ownership of the key, and
/// it breaks silently the day the runner's value changes. Every other
/// reserved-key fixture supplies a *different* value (`/tmp/hijacked`,
/// `/nowhere`, `C:\hijack`), so equality is the one case they cannot see.
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
    // The platform axis is a value here, not a `cfg!`. Both arms are
    // reached on every host, so a Linux cell proves the Windows rule.
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
    // And the same-key rule is what decides an upsert, not only a refusal.
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

/// DESIGN.md:263: "Probe and execution compose the same base, mounts,
/// reserved values, and overlay, so pre-flight certifies the environment
/// that will actually spend."
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

/// Prints this process's whole environment between two markers.
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

/// One run of the dump helper, as the environment its child actually
/// carried.
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

/// A probe child and an execution child of one adapter, held side by side.
///
/// DESIGN.md:263 — "Probe and execution compose the same base, mounts,
/// reserved values, and overlay, so pre-flight certifies the environment
/// that will actually spend." Every probe fixture in this suite probes one
/// agent in isolation and asserts only that the probe succeeded, and the
/// one composition comparison compares two *maps* rather than two children
/// — so the probe's base and the probe's overlay could each be replaced on
/// the way to the process and nothing would notice.
///
/// Two children, one adapter, one runner, at the same time. The probe half
/// is built by production (`agent::probe_request`), not by this test, so a
/// substitution made there is inside the comparison. The two sentinels
/// exist because equality between two empty environments is also equality:
/// one comes only from the base, the other only from the overlay, and both
/// must arrive.
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
    // The worker half comes from production's builder, so the request this
    // compares against the probe is the request an attempt sends: the
    // bound agent and the worker identity, not a hand-written pair.
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

/// The same claim, over **every** role production binds to an agent and
/// **every** shipped binding — in the children, not in a map.
///
/// DESIGN.md:258-264: "each supplies role-scoped `HOME`, `PATH`, and
/// credential locations", and "Probe and execution compose the **same**
/// base, mounts, reserved values, and overlay, so pre-flight certifies the
/// environment that will actually spend."
///
/// The test above holds one pair — `Probe(Agent(claude-code))` against
/// `Implement` — and `supplies_credentials` names **three** roles. So a
/// forwarding site that dropped the binding for `Review` alone left every
/// child-level comparison green: direct `compose` tests bypass the
/// forwarding site entirely, the actual-child parity compares Probe with
/// Implement, the credential-child test compares Gate with Implement, and
/// the Review cells of the role grid never look at their environment
/// (`PR5-CORRECTNESS-009`). The domain is therefore taken from
/// `ExecutionRole::all()` and filtered by `supplies_credentials`, so a role
/// added later is covered or fails here.
///
/// Three sentinels rather than equality alone, because two identical
/// *absences* are also equal: one value that can only have come from the
/// base, one that can only have come from the overlay, and the credential
/// location, which can only have come from the **binding** — which is the
/// one the finding is about, and the one a stripped binding removes.
#[test]
fn every_credential_supplied_role_composes_one_environment_per_binding() {
    let workspace = scratch("binding-parity");
    let mut base = synthetic_base();
    base.push((os("UPSTROKE_BASE_SENTINEL"), os("base-only-value")));
    // A credential location already in the base, which is the failure
    // sequence's own starting state: composition strips reserved keys, and
    // only the agent binding can put the value back.
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
            // The binding's own contribution. `key` is a reserved key, so a
            // child carrying it carries a value composition put there —
            // and a role whose binding was dropped on the way to `compose`
            // carries nothing under this name at all.
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

/// What `host-v1` supplies for `HOME`, `PATH` and `USERPROFILE`, asserted
/// from the passages that decide it.
///
/// DESIGN.md:260 names three things role-scoped — `HOME`, `PATH`, and
/// credential locations — and `host-v1` scopes one of them. That is a
/// boundary, so it is asserted here from the sentences that draw it and
/// not as a count of distinct values. The difference is the whole point of
/// this test's shape: the form it replaces asserted "`PATH` took exactly
/// **one** value across the five roles", a claim no passage makes, which
/// would fail the container runner and which made *implementing*
/// DESIGN.md:260's plainest reading a test failure. A repair round that
/// encodes a narrower boundary as the expected result is the shape this
/// project fears most, and a count cannot tell the two apart.
///
/// Three claims, one per passage:
///
/// * **Every role is supplied every one of the three keys the base
///   carries.** DESIGN.md:260 says the runner *supplies* them; a role
///   handed none at all satisfies any count of distinct values.
/// * **The roles the packet pairs are supplied identical reserved sets.**
///   DESIGN.md:263 ("probe and execution compose the same base, mounts,
///   reserved values, and overlay") over `probe(<agent>)`, `implement`,
///   `review`; `decisions/2026-08-12-…:331-333` ("gate-shell/program
///   availability is checked inside the same boundary") over
///   `probe(shell)` and `gate`.
/// * **The value is the host boundary's own.** `decisions/2026-08-12-…:321`
///   — "the host base starts from the Upstroke process environment". The
///   expected values are read out of the base vector *this fixture wrote*,
///   never out of [`HostEnvironment::lookup`]: `reserved_values` calls
///   `lookup` too, and a function used as its own oracle asserts nothing.
///
/// What this test could get wrong: the two groups could stop covering the
/// five roles — a role dropped from both would be unasserted while every
/// remaining assertion passed — so their union is compared against
/// `ExecutionRole::all()` as a set, and not merely counted.
#[test]
fn the_reserved_values_every_role_gets_are_the_host_boundarys_own() {
    let base = synthetic_base();
    let environment = HostEnvironment::with_base(base.clone(), KeyCase::current());
    // The agent `ExecutionRole::all()` names, so the groups below can be
    // compared against it as sets rather than by length alone.
    let agent = AgentId::new(claude::ADAPTER_ID);
    let roles = ExecutionRole::all();
    assert_eq!(roles.len(), 5, "the grid covers every role");

    // Written here, not read from RESERVED_ALWAYS: the set is the claim.
    let from_the_boundary = ["PATH", "HOME", "USERPROFILE"];
    assert_eq!(from_the_boundary.len(), 3);
    // The independent oracle: the base this fixture wrote.
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

    // The roles each passage holds together, and the passage, so a failure
    // names the sentence that was broken.
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

/// The half `host-v1` **does** scope: credential locations.
///
/// The expected split is written from what each role executes — a worker, a
/// review and an agent probe run an agent CLI; a gate is repository code
/// and the shell probe is a shell — and not computed from
/// `supplies_credentials` or from `is_slotted`.
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

/// The same split, asserted about the **environment the child receives**
/// rather than about the list the runner assembled.
///
/// `reserved_values` is what the runner *supplies*; `compose` is what the
/// process *gets*, and until this test the two were only related by
/// inspection: `compose` cloned the whole Upstroke base first, so a
/// `CODEX_HOME` in the coordinator's own environment reached a gate
/// whatever `supplies_credentials` said. DESIGN.md:258-264 scopes the
/// credential location by role, and the role a variable is scoped to is
/// the one the process runs under.
///
/// The grid crosses every role with every agent — including the one the
/// host knows no location for — under both name rules, and the expected
/// value of each of the three credential keys is written from the rule
/// ("the bound agent's location, and only for a role that runs an agent
/// CLI"), not read from `supplies_credentials` or `reserved_values`.
#[test]
fn compose_gives_a_child_the_credential_location_of_its_own_agent_and_no_other() {
    let all_credentials = [
        ("claude-code", "CLAUDE_CONFIG_DIR", "/home/upstroke/.claude"),
        ("copilot", "COPILOT_HOME", "/home/upstroke/.copilot"),
        ("codex", "CODEX_HOME", "/home/upstroke/.codex"),
    ];
    // Written from what each role executes, exactly as
    // `credential_locations_are_role_scoped` writes it.
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
                // And the keys that come from the host boundary itself are
                // still there, for every role: DESIGN.md:262 names HOME
                // and PATH beside the credential locations, and `host-v1`
                // supplies the boundary's own value of each to all five
                // roles (see
                // `the_reserved_values_every_role_gets_are_the_host_boundarys_own`
                // for the passages that decide that).
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
                // Nothing else was dropped on the way through.
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
    // Hostility as counts: 2 rules x 5 roles x 5 agents x 3 keys = 150
    // decisions, and both answers really occur.
    assert_eq!(supplied_count + denied_count, 2 * 5 * 5 * 3);
    assert_eq!(
        supplied_count,
        2 * 3 * 3,
        "two name rules x three agent-CLI roles x the one bound agent each"
    );
}

/// The composed base **is** the Upstroke process environment, entry for
/// entry.
///
/// DESIGN.md:258-259: "`CommandSpec.env` overlays a runner-owned base
/// rather than replacing it. The host runner starts from the Upstroke
/// environment". `run` calls `env_clear()` before installing the composed
/// block, so anything `from_process` fails to collect is not merely
/// unscoped — it is *gone* from every child, and nothing else in the suite
/// would notice. That includes the entries only one platform has: Windows'
/// `=C:`-style per-drive working directories are yielded by `vars_os`
/// (Rust 1.85 keeps keys beginning with `=`), and this asserts they
/// survive collection on the platform that has them rather than assuming
/// it.
/// The variables the subprocess witness below inherits, and their values.
///
/// Values are deliberately awkward — a non-ASCII character, an `=`, and
/// the encoding's own separator — so a collector that split or filtered on
/// any of them loses the entry.
///
/// The second name is not decoration. `PR4-CORRECTNESS-006`'s surviving
/// mutation drops exactly that key inside `from_process`, and a filter
/// keyed on a name **no test process carries** removes nothing: it is
/// inert, and no oracle can observe an inert edit. Setting the name here is
/// what makes it a variable that exists and can therefore be lost. The
/// entry-for-entry equality below is the general statement; this is the
/// named guard for the one mutation that motivated it.
const BASE_WITNESS: &[(&str, &str)] = &[
    ("UPSTROKE_PR4_BASE_WITNESS", "café=;value"),
    ("UPSTROKE_PR4_DRIVE_CWD_SENTINEL", "=D:=D:\\base;café"),
];

/// The child half of [`the_base_of_a_process_environment_is_the_process_environment`].
///
/// A subprocess rather than `set_var`: the suite is multi-threaded and
/// mutating this process's environment while another test reads it is the
/// race `std::env::set_var` is `unsafe` for. The same shape
/// `proc::tests::sigchld_reaper_host_helper` uses, for the same reason.
#[test]
#[ignore = "subprocess helper"]
fn base_witness_helper() {
    if std::env::var_os(BASE_WITNESS[0].0).is_none() {
        return;
    }
    let base = HostEnvironment::from_process();

    // The general statement, made where the environment is *known* to
    // carry awkward entries rather than wherever the machine left it: any
    // entry `from_process` drops, filters or reorders shows up here,
    // whatever it is named.
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

    // And each injected entry reaches a composed child environment, for a
    // role that is handed nothing else.
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

/// The composed base **is** the Upstroke process environment, entry for
/// entry.
///
/// DESIGN.md:258-259: "`CommandSpec.env` overlays a runner-owned base
/// rather than replacing it. The host runner starts from the Upstroke
/// environment". `run` calls `env_clear()` before installing the composed
/// block, so anything `from_process` fails to collect is not merely
/// unscoped — it is *gone* from every child, and until this test nothing
/// in the suite would notice: every composition fixture supplies its own
/// base through `with_base`.
///
/// Two halves, because one alone is not enough. The equality is over
/// whatever this machine happens to carry — on Windows that includes the
/// `=C:`-style per-drive working directories, which Rust 1.85's `vars_os`
/// deliberately yields. The subprocess adds an entry chosen to be
/// awkward, so the equality is not satisfied merely by an environment with
/// nothing interesting in it.
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
        // The **full path**. `--exact` matches the whole test name, so a
        // bare `base_witness_helper` filters all 932 tests out and the
        // child exits 0 having run nothing — a subprocess witness that
        // witnesses nothing, and a green one.
        "runner::host::tests::base_witness_helper",
        "--ignored",
        "--exact",
    ]);
    for (key, value) in BASE_WITNESS {
        child.env(key, value);
    }
    let out = child.output().expect("spawn the base-witness helper");
    let report = String::from_utf8_lossy(&out.stdout);
    // Assert the **count**, never a bare `ok`: `ok. 0 passed` is what a
    // filter that matched nothing prints, and it is indistinguishable from
    // success at the exit code.
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
    // An independent table: adapter id -> the vendor's config-directory
    // variable (capacity.rs:36-37 for two of the three).
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

// -----------------------------------------------------------------------
// supervision parity  (proof test: "supervision parity tests")
// -----------------------------------------------------------------------

struct ParityFixture {
    name: &'static str,
    script: &'static str,
    stdin: &'static str,
    timeout: Duration,
    /// The shortest this child can possibly have taken, as a fact about the
    /// child rather than about the machine that runs it: a sleeper cannot
    /// finish before it has slept, and a child killed for exceeding its
    /// timeout ran at least that long. `None` where the fixture finishes as
    /// fast as the machine can run it and no floor is stateable.
    ///
    /// This is the lower half of the duration pin. The upper half is wall
    /// clock the test measures around each call, and neither is the
    /// runner's own arithmetic.
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
            // `1>&2` redirects the same way in `cmd` and in `sh`.
            script: "echo problem 1>&2",
            stdin: "",
            timeout: Duration::from_secs(30),
            floor: None,
        },
        ParityFixture {
            // The regression test for `build_command`'s `cmd.exe` rule:
            // std would re-quote this tail as `\"quoted arg\"`, which
            // cmd.exe does not un-escape, so the child would print
            // something else entirely. On Unix nothing special happens and
            // the fixture still has to agree.
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
            // The fixture that pins duration **from below** without naming
            // the timeout. Every other fixture finishes as fast as the
            // machine can run it, so a duration reported too *large* is
            // caught by the elapsed bound while one reported too *small* is
            // caught by nothing: `> 0` admits a nanosecond.
            //
            // A sleeping child gives the test a floor it can state in
            // advance and that holds on any machine — a loaded one only
            // makes the interval wider, never narrower. One second nominal,
            // asserted at half, because `ping -n 2` is "two pings a second
            // apart" rather than a sleep of exactly a second.
            name: "a sleeping child is measured, not the timeout",
            script: if cfg!(windows) {
                // `cmd` has no `sleep`; this is the shape the timeout
                // fixture below already relies on.
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
            // The timeout, written again rather than read from the field
            // beside it: a child killed for exceeding its timeout ran at
            // least that long, and the two values are the same number for
            // that reason rather than by construction.
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
        // Wall clock around the call, measured by the test. It is an upper
        // bound on any honest `duration` *by construction*: the funnel
        // starts its own clock after this instant and stops it before the
        // drain, so `duration <= elapsed` holds however slow or loaded the
        // machine is. See the duration assertions below.
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
        // Byte for byte, including the line endings. Both sides run the
        // same script through the same recorded shell on the same machine,
        // so there is nothing legitimate for a normalization to absorb —
        // and normalizing *both* sides is how a runner that rewrote CRLF to
        // LF would have gone unnoticed on the only platform that produces
        // CRLF at all. `invariants_preserved[0]` says output capture is
        // unchanged, and rewriting it is a change.
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
        // Duration is part of the record an attempt keeps
        // (`Outcome.duration`), so "supervision unchanged" includes
        // measuring **this child** rather than reporting some other true
        // fact. Not compared to the other supervisor's duration for
        // equality: two runs of one script take two times, and asserting
        // they match would be asserting the machine is idle.
        //
        // Pinned from both sides instead, against two oracles that are not
        // the runner's own arithmetic:
        //
        // * above, wall clock the *test* measured around the call — an
        //   upper bound by construction; and
        // * `fixture.floor`, the child's own behaviour, where the fixture
        //   has one to state.
        //
        // Positivity alone is what let `.map(|mut output| { output.duration
        // = request.timeout; output })` survive a whole review round
        // (`PR4-CONF-013`): every ordinary fixture has a 30s timeout and
        // reports `> 0` under it, and the timeout fixture reports exactly
        // its timeout, which its own floor admits. The elapsed bound is
        // what that mutation cannot satisfy — a child that echoes and exits
        // has not taken thirty seconds.
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
    // Hostility as counts: a parity suite whose fixtures all produce the
    // same output proves that two functions agree about nothing.
    assert!(codes.len() >= 3, "distinct exit codes: {codes:?}");
    assert!(stdouts.len() >= 3, "distinct stdout values: {stdouts:?}");
    assert!(stderr_nonempty >= 1, "no fixture wrote to stderr");
    assert_eq!(timed_out.len(), 2, "both timeout outcomes are exercised");
    // And the duration pin is counted rather than hoped for: two fixtures
    // state a floor, one from a sleep and one from a timeout kill. A grid
    // that lost them would still assert `> 0` on every row and read green.
    assert_eq!(
        parity_fixtures()
            .iter()
            .filter(|fixture| fixture.floor.is_some())
            .count(),
        2,
        "the fixtures that pin duration from below"
    );
}

// -----------------------------------------------------------------------
// output transparency  (`invariants_preserved[0]`: "output capture …
// unchanged")
// -----------------------------------------------------------------------

/// What the transparency shim prints on stdout: JSON Lines, because that is
/// the shape whose *first* lines carry the meaning — a Codex transcript's
/// `thread.started` (the session) and its `item.completed` (the verdict)
/// both precede the final `turn.completed`.
const TRANSPARENT_STDOUT: &[&str] = &[
    r#"{"type":"thread.started","thread_id":"th-transparency"}"#,
    r#"{"type":"item.completed","item":{"type":"agent_message","text":"the verdict"}}"#,
    r#"{"type":"turn.completed","usage":{"input_tokens":11,"output_tokens":7}}"#,
];

/// And on stderr, which is captured by the same funnel and is where a Codex
/// run puts its tracing log.
const TRANSPARENT_STDERR: &[&str] = &["tracing line one", "tracing line two"];

/// A launcher that ignores its arguments and prints
/// [`TRANSPARENT_STDOUT`] and [`TRANSPARENT_STDERR`].
///
/// Not [`forwarding_shim`]: that one forwards to this test binary, whose
/// `libtest` output is what it is. This child's output is chosen by the
/// test, so "what the child produced" and "what the runner returned" are
/// two things that can be compared.
///
/// Every payload byte is `echo`-safe in both dialects — JSON carries none
/// of `&`, `<`, `>`, `|`, `^`, and a `"` is printed literally by `cmd`'s
/// `echo`.
///
/// **The redirection goes first on Windows.** `cmd`'s `echo` prints
/// *everything* between the command and the redirection operator, so
/// `echo foo 1>&2` emits `foo` followed by a **trailing space** — measured
/// on the guest, where the first run of this grid failed with
/// `["tracing line one "]` against `["tracing line one"]`. `1>&2 echo foo`
/// has no such gap. A test that trimmed instead would have stopped being
/// able to see a runner that trimmed.
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
    std::fs::write(&path, script).expect("write the transparency shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make the shim executable");
    }
    path.to_str()
        .expect("a scratch path this crate can name")
        .to_owned()
}

/// A captured stream as lines, with the platform's terminator folded away.
fn captured_lines(stream: &str) -> Vec<String> {
    stream
        .replace("\r\n", "\n")
        .lines()
        .map(str::to_owned)
        .collect()
}

/// One `TaskRun`, so each adapter's own `build_args` can be asked what
/// production's argument vector for it is.
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

/// `HostRunner::run` hands back **the child's whole output**, for every
/// request shape production sends.
///
/// `invariants_preserved[0]` is "process supervision, timeout, **output
/// capture**, adapter parsing unchanged", and `PR5-CORRECTNESS-012` is a
/// runner that keeps only the last stdout line when the role is
/// `Implement`/`Review`, the agent is `codex` and the first argument is
/// `exec` — after which a successful review loses its session and its
/// verdict and is re-asked, and can end as `ReviewFailed`.
///
/// Three axes, varied independently, because a suppression can key on any
/// of them and the existing grids hold two of them fixed:
///
/// * **role** — built by production's own builder, never by this fixture;
/// * **agent binding** — all three shipped ids, not one;
/// * **the argument vector** — each adapter's real one, from its own `pub
///   fn build_args`, so `exec`, `-p` and the bare-prompt form all appear.
///   *Every* existing grid in this file sends `["--exact", NO_SUCH_TEST]`,
///   which is why an `args[0]`-keyed edit had nowhere to fail. That is
///   `PR4-CONF-006`'s class one field further over, and this is the field.
///
/// The resumed shape is carried too: `codex exec resume <id>` moves the
/// subcommand's position, so a check on `args[0]` and one on "is this an
/// exec" are different predicates.
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

    // The probe role too: pre-flight reads a CLI's own answer, and a
    // truncated one is a capability read wrong rather than lost work.
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

    // And the unbound role, whose program is the recorded shell rather than
    // a located CLI — the other half of the program-shape partition.
    let script = if cfg!(windows) {
        TRANSPARENT_STDOUT
            .iter()
            .map(|line| format!("echo {line}"))
            .chain(
                // Redirection first, for the trailing-space reason on
                // `transparency_shim`.
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

    // Hostility as counts, not prose.
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

/// Each adapter's production argument vector, from the adapter itself.
fn build_args_for(id: &str, resume: Option<&str>) -> Vec<String> {
    let run = transparency_run(id, resume);
    match id {
        "claude-code" => crate::agent::claude::build_args(&run),
        "codex" => crate::agent::codex::build_args(&run),
        "copilot" => crate::agent::copilot::build_args(&run),
        other => panic!("an adapter shipped without an entry here: {other}"),
    }
}

/// Every line the child wrote came back, in order, on the stream it was
/// written to.
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
    // Named separately, because "the last line survived" is the assertion a
    // truncating runner would still pass.
    assert!(
        output.stdout.contains("thread.started"),
        "{cell}: the *first* line is gone, which is the session and the \
         verdict: {:?}",
        output.stdout
    );
}

/// The bounded capture allowance is the same one on both public funnel
/// entries, and it is the real one.
///
/// `invariants_preserved[0]`: "process supervision, timeout, output
/// capture and adapter parsing unchanged". `HostRunner` reaches the funnel
/// only through `run_with_timeout_hooked`, so a limit passed there and
/// nowhere else is a limit no parity fixture sees: every existing
/// output-limit test calls the private `run_with_timeout_and_limit` with a
/// 64 KiB test value. This one drives the *real* constant through both
/// public entries with a child that never stops writing.
///
/// The expected bound is written here (16 MiB per stream) rather than read
/// from `proc`, so raising the constant to `usize::MAX` — or to anything
/// else — fails here rather than agreeing with itself.
#[test]
fn the_runner_bounds_output_at_the_same_allowance_the_direct_funnel_does() {
    /// `proc::OUTPUT_LIMIT_BYTES`, transcribed. Per stream.
    const EXPECTED_LIMIT: usize = 16 * 1024 * 1024;
    /// How much the helper writes before exiting, as a decimal byte count.
    ///
    /// Four times the allowance, and **finite**. A funnel that bounds
    /// correctly kills the child while it is blocked on a full pipe, far
    /// short of this, so the passing case is unchanged. A funnel that does
    /// not bound captures 64 MiB and the child exits 0 — which fails
    /// `output_limited` below by name, instead of running the parent out
    /// of memory and taking the whole test binary with it. A budget set
    /// *below* `EXPECTED_LIMIT` would fail this test's own
    /// `output_limited` assertion, so it cannot drift silently.
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

    // (a) The direct entry point, which is what the legacy engine used
    // before this slice.
    let mut direct = Command::new(&exe);
    direct
        .args(helper_args)
        .env("UPSTROKE_EXCESSIVE_OUTPUT_HELPER", &budget);
    let started = std::time::Instant::now();
    let direct = proc::test_support::run_with_timeout(direct, "", Duration::from_secs(120))
        .expect("direct supervision of a noisy child");
    let direct_elapsed = started.elapsed();

    // (b) The same child through the Runner.
    let workspace = scratch("output-limit");
    let request = RunnerRequest {
        command: CommandSpec {
            program: exe.to_string_lossy().into_owned(),
            args: helper_args.iter().map(|arg| (*arg).to_owned()).collect(),
            // Not a reserved key: the helper reads it to decide it is the
            // helper rather than an ordinary test run, and to size its
            // output.
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
        // And it really did have to *reach* the allowance: a limit of zero
        // would also satisfy the bound above.
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
    // A relative redirect target resolves against the child's cwd in both
    // shells, which is the point of the fixture.
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

/// The role scoping is in the child's own environment block, not only in
/// the vector the runner assembled.
///
/// A gate is repository-controlled code — "the one thing on the host that
/// no agent permission surface bounds" — and DESIGN.md:262 scopes the
/// credential location by role. This runs a real process under a base that
/// carries all three locations and asks the child what it received.
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
    // A base that carries every credential location, so "absent from the
    // child" cannot be an artefact of this machine.
    let runner = HostRunner::new().with_environment(HostEnvironment::with_base(
        synthetic_base()
            .into_iter()
            // The shell must still be findable, so the real PATH wins over
            // the synthetic one.
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
    // Windows `echo` prints the literal `%VAR%` when the variable is unset;
    // `sh` prints nothing. Both are "not set to a credential directory",
    // and neither is the value the base carries.
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

    // The same base, the role that runs codex's CLI: its own location
    // arrives and the other two still do not. Without this half the
    // assertion above would be satisfied by supplying nothing at all.
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

// -----------------------------------------------------------------------
// the shell probe
// -----------------------------------------------------------------------

/// A runner that never spawns, so the probe's *classification* of a
/// failure can be tested on a machine where every `ShellKind` happens to
/// be installed.
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

/// Every way a `ProcessOutput` can say the probe did not succeed.
///
/// `expected_failures_refusals[3]` is "a failing shell probe -> returned
/// pre-flight error to the caller", and the funnel has three independent
/// ways to report one: the exit code, `timed_out`, and `output_limited`.
/// Two of them can arrive **with** `code: Some(0)` — the limit is observed
/// during the final drain, and a signal-killed child reports `code: None`
/// with `timed_out: false` — so a probe that reads only one field
/// certifies a shell that did not run `exit 0`.
///
/// The grid is written from those three fields rather than from
/// `run_shell_probe`'s branches, so a field it stops reading fails here.
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
            // Killed by a signal, or by anything else that leaves no exit
            // code: `None` is not `Some(0)` and must not be read as one.
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
            // The bounded-output contract terminated the owned tree; the
            // funnel can still report the code the leader exited with,
            // because the limit is observed during the final drain.
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

/// The recorded shell the missing-shell case probes for.
///
/// `pwsh` rather than one of the other four because it is the one
/// [`ShellKind`] that is **not** reachable from any directory the child's
/// program search consults once `PATH` has been replaced: `cmd` is in the
/// Windows system directory always, `powershell` is in a subdirectory of it
/// that is on every Windows `PATH`, `sh` and `bash` are `/bin` programs on
/// Unix and Git-for-Windows programs on the runners. PowerShell 7 installs
/// outside all of those on every platform — and the helper asserts that,
/// rather than assuming it.
const MISSING_SHELL: ShellKind = ShellKind::Pwsh;

/// Set by the parent test on the helper it spawns, so the helper is inert
/// when `cargo test -- --ignored` runs it directly.
const MISSING_SHELL_MARKER: &str = "UPSTROKE_MISSING_SHELL_PROBE";

/// Printed by the helper after it has asserted, so the parent can tell "the
/// helper ran and refused" from "the helper never ran".
const MISSING_SHELL_OK: &str = "<<MISSING-SHELL-REFUSED";

/// The directories a child's program search consults **besides** `PATH`,
/// on the platform that has any.
///
/// std resolves a bare Windows program name against the child `PATH`, the
/// application directory, the system directory, the Windows directory and
/// then the **parent's** `PATH` (`library/std/src/sys/.../windows/process.rs`,
/// `search_paths`). The helper controls both `PATH`s by construction; these
/// three it can only *check*, which is what makes the check the premise.
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

/// The missing-shell half of
/// [`host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing`],
/// in a child process because one of the two `PATH`s that decide the answer
/// belongs to the **process**, not to the request.
///
/// The previous version of this case hid `pwsh.exe` by composing a base
/// environment with `PATH` removed and ran in-process. Windows CI proved
/// that oracle invalid — `CreateProcess` also searches the *parent's*
/// `PATH`, so an emptied child `PATH` hides nothing the runner itself can
/// see, and the guest passed only because that machine has no `pwsh` at
/// all. A process cannot rewrite its own `PATH` for one test without racing
/// every other test in the binary, so the absence is constructed where it
/// can be: in a child whose entire `PATH` is one directory this suite
/// created and asserts is empty.
///
/// Everything here is a **premise check followed by the claim**. If the
/// construction ever stops constructing the absence — a `PATH` that is not
/// the empty directory, a directory that is not empty, a `pwsh.exe` that
/// has appeared in one of the three directories the search reaches
/// regardless — this fails on the premise and says which one, rather than
/// passing for the wrong reason.
#[test]
#[ignore = "subprocess helper"]
fn shell_probe_missing_shell_helper() {
    if std::env::var_os(MISSING_SHELL_MARKER).is_none() {
        return;
    }

    // Premise 1: this process's `PATH` is exactly one directory, and that
    // directory is empty. On Unix this is the whole search — `execvp`
    // consults `PATH` and nothing else, and an *absent* `PATH` would be
    // worse than a controlled one, because then `execvp` falls back to the
    // confstr default `/bin:/usr/bin`, where the CI image really does ship
    // `/usr/bin/pwsh`.
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

    // Premise 2: the directories Windows searches whatever `PATH` says do
    // not hold this shell either.
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

    // Premise 3: the workspace **exists**. The three conditions the
    // contract's proof test composes are an existing workspace, an absent
    // shell and `HostRunner::shell_probe`; a probe that failed because its
    // directory was missing would prove the first of them false and test
    // something else. `HostRunner::run` hands every child an absolute
    // `current_dir`, so this is the difference between "the shell is not
    // there" and "the directory is not there".
    let workspace = scratch("missing-shell");
    assert!(
        workspace.is_dir(),
        "the premise: {} must exist",
        workspace.display()
    );

    // The claim. `HostRunner::new()` composes from *this* process's
    // environment, so the child inherits the same one-empty-directory
    // `PATH` — production's own composition, not a substituted one.
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

/// `decisions.pr_sequence[5].slice_contract.proof_tests[8]`, by name.
///
/// The contract names one test for the whole shell probe and it composes
/// three things: the **recorded shell** succeeding, a shell that is
/// **missing** failing, and both going through
/// [`HostRunner::shell_probe`] — the `RunnerPreflight` entry point — rather
/// than through the free [`run_shell_probe`] or through `Runner::run`.
/// Decomposing it into separately-tested layers loses exactly that
/// composition: with the missing-shell case gone, a `shell_probe` body of
///
/// ```text
/// match run_shell_probe(self, shell, workspace.to_path_buf(), invocation) {
///     Err(error) if workspace.exists() && error.to_string().contains("os error 2") => Ok(()),
///     outcome => outcome,
/// }
/// ```
///
/// survives every remaining case — (a) succeeds, (c) has no workspace, (d)
/// does not use the method, and (e) does not spawn.
#[test]
fn host_shell_probe_succeeds_with_recorded_shell_and_fails_when_shell_missing() {
    let workspace = scratch("shell-probe");
    let runner = HostRunner::new();

    // (a) The recorded shell, actually spawned. `gates::shell_available`
    // is a PATH check; this is a spawn, which is the only thing that
    // establishes availability (packet finding F-43 / V14-VERIFY-004).
    runner
        .shell_probe(native(), &workspace, shell_probe_invocation())
        .expect("the platform's native shell runs `exit 0`");

    // (b) A recorded shell that is **missing**, with the workspace that
    // (a) just used still in place, through the same method — the
    // contract's own composition. It runs in a child because the absence
    // has to be constructed out of both `PATH`s and one of them is the
    // process's; see [`shell_probe_missing_shell_helper`], which holds the
    // assertions.
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

    // (c) and (d): the other two ways the host can fail to complete a
    // probe, neither of which is a claim about what happens to be
    // installed. Both fail because of something this test constructs and
    // then checks, so they fail identically on a machine with every shell
    // in existence installed.

    // (c) The recorded shell, asked to run in a directory that is not
    // there. `HostRunner::run` gives every child an absolute
    // `current_dir`, and starting a process in a directory that does not
    // exist is refused by the kernel everywhere this crate runs: `chdir`
    // answers `ENOENT` on Unix — whether std reaches it through `fork` or
    // through `posix_spawn_file_actions_addchdir_np` — and
    // `CreateProcessW` fails with `ERROR_DIRECTORY` on Windows. Both
    // surface as `Err` from `Command::spawn`, which is the same production
    // path an absent shell binary takes, and the caller must be handed a
    // pre-flight error naming the shell rather than an `Ok`.
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

    // (d) The missing-program fault at the `Runner::run` layer, expressed
    // so that no installed program can satisfy it: an absolute path inside
    // the directory (c) just established does not exist. It contains a path
    // separator, so it is looked up verbatim — neither `execvp`'s `PATH`
    // walk nor std's Windows search of the system directories and the
    // parent `PATH` is consulted for a name like this one, and there is
    // nothing at the name. (b) is the same fault one layer up, through the
    // method and against the recorded shell, which is what the contract
    // names; this one pins the layer beneath it.
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

    // (e) A shell that runs and refuses, and one that hangs. Both are
    // pre-flight errors and neither is a `ProcessOutput` the caller has to
    // interpret.
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

/// The probe spells every shell exactly as `gates::ShellGate` does, so it
/// certifies the invocation the gates will use.
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

// -----------------------------------------------------------------------
// containment sub-effect hook points (ST-07 subset)
// -----------------------------------------------------------------------

/// The eight points and the host each belongs to, transcribed from
/// `decisions.effect_site_inventory.containment_sub_effects`:
///
/// > Windows: Spawn.AmbientJobJoined …, Spawn.CreatedSuspended …,
/// > Spawn.PrivateJobAssigned, Spawn.Resumed … ; Unix: Spawn.ReaperStarted
/// > …, Spawn.PreExecPgidAndRegister …, Spawn.Exec, Spawn.Registered
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

/// Run one trivial command through the runner and report which containment
/// points its funnel reached.
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

/// Records the points a spawn reaches **in the order it reaches them**.
struct OrderedHooks(Arc<Mutex<Vec<SubEffectPoint>>>);

impl SpawnHooks for OrderedHooks {
    fn point(&mut self, point: SubEffectPoint) -> crate::topology::effects::Injection {
        self.0.lock().expect("order").push(point);
        crate::topology::effects::Injection::Proceed
    }
}

/// The containment points of one spawn, in order.
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

/// The points fire in the packet's own order, and each label sits at the
/// coordinate it names.
///
/// `containment_sub_effects` lists them as a sequence — "Windows:
/// Spawn.AmbientJobJoined …, Spawn.CreatedSuspended …,
/// Spawn.PrivateJobAssigned, Spawn.Resumed …; Unix: Spawn.ReaperStarted …,
/// Spawn.PreExecPgidAndRegister …, Spawn.Exec, Spawn.Registered" — and the
/// order is the whole content of two of them: `PrivateJobAssigned`
/// promises the child is *still suspended* and `Resumed` promises it is
/// not. A set-valued observation cannot tell those two apart, so swapping
/// the two labels in production would leave the ST-07 evidence claiming a
/// pre-resume coordinate for a hook that fires after the child can execute.
///
/// The expected sequence is transcribed from that sentence, not read back
/// from the funnel.
#[test]
fn the_containment_points_of_a_spawn_fire_in_the_packets_order() {
    let observed = point_order();
    let expected: Vec<SubEffectPoint> = if cfg!(windows) {
        // `AmbientJobJoined` is a write-command startup step rather than a
        // per-spawn one, so a spawn reaches the other three.
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
    // Each point once: a point consulted twice at one spawn would make the
    // sequence above ambiguous about which coordinate it names.
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
    // `AmbientJobJoined` is a write-command startup step, not a per-spawn
    // one, so it is not reached by running a command; it is exercised by
    // `windows_ambient_job_unavailable_refuses_before_effects` and by the
    // coordinator-death tests.
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

/// Where each containment point's **operation** happens, and where its
/// **injection** is controlled. Transcribed from
/// `decisions.effect_site_inventory.containment_sub_effects`, which
/// annotates the coordinate of exactly one point ("Spawn.PreExecPgidAndRegister
/// (in the child before exec)") and one other as parent-side
/// ("Spawn.Registered (parent-side registration)").
///
/// Two of the eight perform their operation outside the parent, and both
/// are injected in the parent. That is not a coordinate the implementation
/// chose for convenience:
///
/// * their only declared mode is `Kill`, a **coordinator** death, and a
///   coordinator is the parent — a kill inside the forked child would end
///   the fork, not the coordinator, and the packet's claim for these points
///   ("a coordinator kill after any of these leaves a group the reaper
///   settles while holding R28") needs a group that exists;
/// * an observer cannot run between `fork` and `exec` at all: only
///   async-signal-safe calls are permitted there and every real observer
///   locks and allocates.
///
/// The packet contemplates both: "ST-07 evidence executes each point on its
/// platform (these are parent-side **or** pre-exec points the harness
/// controls)", and `InjectionMode` is documented as "how a fault is
/// introduced at a **parent-side** sub-effect point". The boundary is
/// counted here so it cannot grow: a third point that stops executing at
/// its own coordinate fails this test.
#[test]
fn the_two_points_whose_operation_is_not_parent_side_are_named_and_counted() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Coordinate {
        /// In the coordinator process.
        Parent,
        /// In the forked child, before `exec`.
        ForkedChildBeforeExec,
        /// In the child, at `exec` itself.
        Child,
    }
    // (point, where the operation happens, where the injection is controlled)
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
    // The reason, asserted rather than described: both declare `Kill`
    // alone, and `Kill` is a coordinator death. The expected modes are the
    // packet's own split — "Kill is universal … error-return is narrower …
    // Windows' AmbientJobJoined has one".
    for point in &elsewhere {
        assert_eq!(point.modes(), &[InjectionMode::Kill], "{point}");
    }
    assert_eq!(
        SubEffectPoint::AmbientJobJoined.modes(),
        InjectionMode::ALL,
        "the ambient join is the one containment point with an error contract"
    );
}

// -----------------------------------------------------------------------
// the containment points, observed at runtime on **every** role
// -----------------------------------------------------------------------

/// **Every** containment point this host declares, read out of the frozen
/// inventory rather than transcribed from it.
///
/// `SPAWN_SITE.sub_effects()` is `Process.Spawn`'s own list and
/// `SubEffectPoint::platform()` is the point's own host, both in the frozen
/// `topology::effects` inventory — `sites.rs` and `vocab.rs` respectively since
/// that module was split into per-concern children — so a point added to the
/// site later is in this domain the moment it exists. That is not tidiness: the hand-written
/// Windows list this replaced named `CreatedSuspended`, `PrivateJobAssigned`
/// and `Resumed` and silently omitted `AmbientJobJoined`, so the kill grid
/// iterated three of the four points the platform has and six guest runs
/// reported covering a point none of them had executed (`PR5-RD-002`). A
/// domain that can omit a point is a domain whose coverage claim is a
/// coincidence.
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

/// The containment points one **spawn** reaches on this platform: every
/// point of [`containment_points`] that is not a startup step.
///
/// `AmbientJobJoined` is a write-command *startup* step rather than a
/// per-spawn one — it is reached by [`HostRunner::start_write_command`],
/// not by running a command. It is role-free by construction (the ambient
/// job is a property of the process, established once before anything can
/// spawn) and is witnessed for error-return by
/// `windows_ambient_job_unavailable_refuses_before_effects` and for kill by
/// `a_kill_armed_at_any_containment_point_actually_kills`, which iterates
/// the *whole* domain rather than this subset.
fn per_spawn_points() -> Vec<SubEffectPoint> {
    containment_points()
        .into_iter()
        .filter(|point| !STARTUP_POINTS.contains(point))
        .collect()
}

/// The containment points a write command reaches at **startup**, before it
/// has spawned anything.
const STARTUP_POINTS: &[SubEffectPoint] = &[SubEffectPoint::AmbientJobJoined];

/// The two domains partition the platform's points, and neither is empty.
///
/// The partition is the property that makes `per_spawn_points` safe to
/// derive by subtraction: a point added to `Process.Spawn` later lands in
/// one of the two by construction, and cannot land in neither. Asserted
/// against `containment_points`, which is itself derived from the frozen
/// inventory, so the only way to lose a point from both is to delete it
/// from the `topology::effects` inventory (`vocab.rs` holds the points since
/// that module was split).
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
    // Derived, not transcribed: the domain agrees with the packet's own
    // platform split for this host.
    let expected: &[SubEffectPoint] = if cfg!(windows) {
        WINDOWS_POINTS
    } else {
        UNIX_POINTS
    };
    assert_eq!(all, expected, "the derived domain left the packet's split");
}

/// The child of a role whose production program is **the recorded shell**:
/// the gate (`gates::ShellGate::check` sends `ShellKind::spec`) and the
/// shell probe ([`shell_probe_request`] sends the same). Built by
/// [`ShellKind::spec`] rather than round-tripped through a `Command`, so
/// the program string is the one [`shell_probe_request`] itself carries.
///
/// No overlay and no stdin, because that is what production's two shell
/// specs carry: `gates::ShellKind::spec` writes `env: Vec::new(), stdin:
/// Vec::new()` and `gates` asserts it ("a gate carries no overlay and no
/// stdin").
fn shell_command() -> CommandSpec {
    native().spec(SHELL_PROBE_COMMAND)
}

/// A filter that matches no test in this binary, so the child below runs
/// nothing and exits 0.
const NO_SUCH_TEST: &str = "upstroke_pr4_role_grid_matches_no_test";

/// The child of a role whose production program is **an agent CLI**: the
/// worker, the reviewer and the agent probe, all of which execute the
/// located binary `bin::Invocation::spec` names and never a shell.
///
/// This test binary is that program. It is the one executable this suite
/// knows exists on both platforms at an absolute path, which is the shape
/// `bin::locate` produces, and with a filter that matches nothing it runs
/// no test and exits 0.
///
/// **Why the grid may not run one child for every role.** `HostRunner::run`
/// chooses the observer it hands the funnel, and a grid whose every child
/// was the recorded shell left
///
/// ```text
/// let selected = if is_a_shell(&request.command.program) { hooks } else { &mut NoHooks };
/// ```
///
/// green while every real worker, reviewer and agent probe — the three
/// roles that execute a CLI — ran with no containment hooks and no fault
/// injection. Same defect, same shape, one field over from the stdin one
/// below.
fn agent_cli_command(stdin: &[u8]) -> CommandSpec {
    agent_cli_command_at(&this_test_binary(), stdin)
}

/// This test binary's own path, as the `String` a [`CommandSpec`] carries.
fn this_test_binary() -> String {
    std::env::current_exe()
        .expect("this test binary's own path")
        .to_str()
        .expect("a target directory this crate can name")
        .to_owned()
}

/// [`agent_cli_command`] against an arbitrary launcher for this binary, so
/// the *program shape* can vary while everything else stays production's.
fn agent_cli_command_at(program: &str, stdin: &[u8]) -> CommandSpec {
    CommandSpec {
        program: program.to_owned(),
        args: vec!["--exact".to_owned(), NO_SUCH_TEST.to_owned()],
        env: Vec::new(),
        stdin: stdin.to_vec(),
    }
}

/// One shape a production `CommandSpec.program` can take on this platform.
struct ProgramShape {
    /// What production produces it.
    what: &'static str,
    command: CommandSpec,
    /// Whether the child ends up being this test binary, so its `libtest`
    /// report is readable on stdout. False for the recorded shell, which
    /// answers `exit 0` and prints nothing.
    reports: bool,
}

/// Write a launcher into `dir` that forwards its arguments and its stdin to
/// this test binary, and return its absolute path.
///
/// The two spellings are the two an installer actually produces: on Windows
/// npm writes a `.cmd` (or `.bat`) batch shim beside the package, and on
/// Unix it writes an extensionless script with a shebang. Neither is a
/// native executable, and `CreateProcessW`/`execve` reach both only through
/// an interpreter — which is precisely why a runner that treats them as a
/// different kind of program is a defect this suite has to be able to see.
fn forwarding_shim(dir: &Path, name: &str) -> String {
    std::fs::create_dir_all(dir).expect("create the shim directory");
    let path = dir.join(name);
    let exe = this_test_binary();
    if cfg!(windows) {
        // `@echo off` so the batch text itself does not reach stdout, and
        // the target quoted because its own path may contain a space.
        std::fs::write(&path, format!("@echo off\r\n\"{exe}\" %*\r\n"))
            .expect("write the batch shim");
    } else {
        std::fs::write(&path, format!("#!/bin/sh\nexec \"{exe}\" \"$@\"\n"))
            .expect("write the shell shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("make the shim executable");
        }
    }
    path.to_str()
        .expect("a scratch path this crate can name")
        .to_owned()
}

/// A directory name with a space in it, which is what makes the path it
/// contains one std must quote. `bin.rs`'s own fixture is the production
/// value this transcribes: `C:\Users\John Smith\npm\copilot.cmd`.
const A_DIRECTORY_WITH_A_SPACE: &str = "John Smith";

/// Every **program shape** production can hand the runner on this platform,
/// materialised under `root`.
///
/// The list is derived from what actually reaches `CommandSpec.program` in
/// this crate, not from intuition. There are two producers —
/// `bin::Invocation::spec`, which carries the absolute path
/// `agent::bin::locate` resolved, and `gates::ShellKind::spec`, which
/// carries the recorded shell's **bare name** — and the first of them can
/// carry three different kinds of file, because `locate` accepts whatever
/// the installation is: a native executable, a batch shim, or (on Unix) a
/// shebang script. `bin::locate`'s own candidate list names `.cmd`
/// explicitly, and npm-installed agent CLIs on Windows *are*
/// `claude.cmd`, `codex.cmd`, `copilot.cmd`.
///
/// Two axes, varied independently, because a suppression can key on either:
/// the **kind of file** (native / batch / script / bare name) and whether
/// the **path needs quoting** (a directory with a space in it).
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

/// What production writes to a **worker's** stdin: the materialized task
/// prompt, which `engine::attempt::run_attempt` puts on the spec with
/// `.stdin(cx.adapter.stdin_payload(&task_run).as_bytes().to_vec())`.
///
/// Non-empty, and that is the whole point of it being here — see
/// [`agent_cli_command`] and the grid below.
const WORKER_STDIN: &str = "## Task one\n\nthe materialized worker prompt, delivered on \
                            stdin the way `AgentAdapter::stdin_payload` says\n";

/// What production writes to a **reviewer's** stdin, from the same seam at
/// `review::run_review`. A different payload from the worker's, so the grid
/// carries two distinct non-empty ones rather than one repeated.
const REVIEW_STDIN: &str = "review the candidate diff and answer with the structured \
                            verdict\n";

/// Every adapter's `PROBE_TIMEOUT`, transcribed: `claude.rs`, `copilot.rs`
/// and `codex.rs` each declare `const PROBE_TIMEOUT: Duration =
/// Duration::from_secs(60)`, private to their own module.
const AGENT_PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// The request **production** sends for `role`, from production's own
/// builder — five roles, five builders, and this fixture writes none of
/// them:
///
/// * `Probe(Shell)` — [`shell_probe_request`], which is what
///   [`run_shell_probe`] (and therefore [`HostRunner::shell_probe`], the
///   `RunnerPreflight` entry point ordered at P4 by
///   `decisions.workspace_candidates.run_creation`) builds;
/// * `Probe(Agent)` — [`crate::agent::probe_request`], the builder every
///   adapter's `probe` calls;
/// * `Implement` — [`crate::runner::worker_request`], which
///   `engine::attempt::run_attempt` calls;
/// * `Gate` — [`crate::runner::gate_request`], which `gates::ShellGate::
///   check` calls;
/// * `Review` — [`crate::runner::review_request`], which
///   `review::run_review` calls.
///
/// This is the repair for a real hole, not tidiness. The three in-attempt
/// roles used to be hand-built here with `agent: None` and a *gate*
/// identity, while production sends `agent: Some(<the adapter>)` and a
/// worker/review identity. A `HostRunner::run` that selected [`NoHooks`]
/// for `role in {Implement, Review}` **and** `agent.is_some()` — the
/// production shape, and only it — therefore ran every real worker and
/// reviewer with no containment hooks and no fault injection while the
/// whole suite stayed green.
///
/// **The builders are only half of it.** A builder fixes the role, the
/// binding and the identity; everything it is *handed* — the program, the
/// arguments, the overlay, the stdin payload and the timeout — is still
/// this fixture's choice, and each of those is a field `HostRunner::run`
/// can key an observer selection on. So each is given production's own
/// value for that role rather than one convenient constant shared by all
/// five:
///
/// * **program and args** — [`agent_cli_command`] for the three roles that
///   execute a located CLI, [`shell_command`] for the two that execute the
///   recorded shell;
/// * **stdin** — the adapter's prompt for the worker and the reviewer
///   (`AgentAdapter::stdin_payload`, delivered at
///   `engine::attempt::run_attempt` and `review::run_review`), empty for
///   the gate and both probes, which is what their specs carry;
/// * **env** — empty for all five, because that *is* production's only
///   value: `ShellKind::spec` and `bin::Invocation::spec` are the only two
///   spec constructors this crate has and both write `env: Vec::new()`,
///   and no call site adds an overlay entry (asserted by
///   `runner::tests::every_production_command_spec_payload_is_classified`);
/// * **timeout** — each role's own production default, five distinct
///   values.
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
            // A probe asks a CLI about itself: an agent binary, and no
            // prompt on stdin.
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

/// Start one child in `role`, through the entry point production uses for
/// that role, and wait for it.
///
/// `Probe(Shell)` goes through [`HostRunner::shell_probe`] rather than
/// through `run` directly, because that entry point is what pre-flight
/// calls and it adds the probe's own refusals; it builds its request with
/// [`shell_probe_request`], which is the value [`production_request`]
/// returns for that role. Every other role is [`production_request`]
/// executed through `run`, which is exactly how production sends it.
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

/// Everything one spawn's containment observers can see, for one role.
///
/// `point` delegates to [`HarnessHooks`] — the production wiring onto
/// PR3's [`HookHarness`] — so the evidence lands where ST-07 reads it, and
/// records the order beside it, because a set cannot tell
/// `PrivateJobAssigned` (the child is still suspended) from `Resumed` (it
/// is not). `child_created` is the funnel's other observation, and on Unix
/// it asks the kernel whether the containment *operation* happened for
/// this role rather than only its hook.
struct RoleWitness {
    harness: Arc<Mutex<HookHarness>>,
    order: Arc<Mutex<Vec<SubEffectPoint>>>,
    children: Arc<Mutex<Vec<u32>>>,
    #[cfg(unix)]
    led_own_group: Arc<Mutex<Vec<bool>>>,
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

    /// A second handle on the same recordings, for the runner to own.
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
            .push(proc::child_leads_its_own_group(pid));
    }

    #[cfg(windows)]
    fn child_created(&mut self, pid: u32) {
        self.children.lock().expect("children").push(pid);
    }
}

/// The grid's fixtures vary every independently meaningful field
/// independently, and the counts say so.
///
/// The standing guard in `reviews/FINDINGS.md`: "fixtures must vary every
/// independently meaningful field independently; assert hostility as
/// distinct-value **counts**, not prose." A grid that moved two fields
/// together would prove only that *some* combination reaches the funnel.
///
/// **The field list is taken from the type, not from intuition**, because
/// three repairs in a row swept the fields their author thought of and the
/// next confirmation found the one nobody listed. [`RunnerRequest`] has six
/// fields and its `command` has four, so this asserts all nine that can
/// vary: role, agent binding, invocation identity, workspace, timeout, and
/// the spec's program, args, env and stdin.
///
/// Two of them used to be constants here (`agent: None`, a gate identity),
/// which is precisely the shape production never sends. Three more were:
/// every request carried `stdin: Vec::new()` while production's worker and
/// reviewer always carry a prompt, every request ran the recorded shell
/// while production's three agent-bound roles always run a CLI, and every
/// request carried `SHELL_PROBE_TIMEOUT` while production gives each role
/// its own. Each of those left a one-line observer suppression in
/// `HostRunner::run` — keyed on `stdin.is_empty()`, on the program, or on
/// the timeout — passing this whole file.
///
/// Nothing asserted below is read back out of the fixture: the bindings are
/// R3's own predicate, the payload split is `AgentAdapter::stdin_payload`'s
/// rule, the program split is "a bound process runs its agent's CLI", and
/// the timeouts are production's own public constants.
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

    // Field 1: the role. Five distinct values, one per member.
    let labels: BTreeSet<String> = requests.iter().map(|r| r.role.label()).collect();
    assert_eq!(labels.len(), 5, "five distinct roles: {labels:?}");
    // Field 2: the agent binding. Three bound, two not — and *which* three
    // is R3's rule, not this fixture's.
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
    // Field 3: the identity. Five distinct renderings, and the form
    // follows the role: the two probes carry probe identities, the three
    // in-attempt roles carry attempt identities with three distinct role
    // tokens.
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
    // And the pairing, so no two fields can be swapped without notice.
    let paired: BTreeSet<(String, bool, String)> = requests
        .iter()
        .map(|r| (r.role.label(), r.agent.is_some(), r.invocation.render()))
        .collect();
    assert_eq!(paired.len(), 5, "five distinct (role, binding, identity)");

    // Field 4: the stdin payload. `AgentAdapter::stdin_payload` is
    // delivered by `engine::attempt::run_attempt` and `review::run_review`
    // and by nothing else, so exactly the worker and the reviewer carry
    // bytes; a gate's spec carries none (`gates::ShellKind::spec`, and
    // `gates` asserts it), and neither probe does.
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

    // Field 5: the program (and with it the args). Production's two
    // spec constructors are `gates::ShellKind::spec` — the recorded shell —
    // and `bin::Invocation::spec` — a located agent binary at an absolute
    // path; which one a role gets is decided by whether it runs an agent
    // CLI.
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
    // The program and the binding do move together, and that is
    // production's rule rather than this fixture's shortcut — a bound
    // process is one that executes its agent's CLI, an unbound one executes
    // the recorded shell. Asserted, so a fixture that broke the
    // correspondence would have to say why.
    for request in &requests {
        assert_eq!(
            request.command.program != shell_program,
            request.agent.is_some(),
            "{}: a request runs a CLI without a binding, or is bound and runs a shell",
            request.role
        );
    }
    // The payload split is *not* the binding split, so the two cannot be
    // mistaken for one field: the agent probe is bound and runs a CLI and
    // still carries no prompt.
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

    // Field 6: the timeout. Each role's own production default — five
    // constants, five distinct values, none of them this fixture's.
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

    // Field 7: the overlay. Empty everywhere, because that is production's
    // only value — both spec constructors write `env: Vec::new()` and no
    // call site adds an entry
    // (`runner::tests::every_production_command_spec_payload_is_classified`
    // is the tripwire for a call site that starts to). Stated as an
    // assertion rather than left to silence: the day production carries an
    // overlay, this row has to become a varying dimension like the four
    // above, and this is what says so.
    assert!(
        requests.iter().all(|r| r.command.env.is_empty()),
        "production composes an overlay now; the grid has to carry one too"
    );

    // Field 8: the workspace. Two distinct values, and which is which is
    // production's: a probe has no workspace of its own and runs in the
    // coordinator's directory (`agent::probe_workspace`), everything else
    // runs in the run's worktree.
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

/// The **identity** dimension, over the whole space production builds —
/// not the five the role grid happens to carry.
///
/// The role grid carries one identity per role, so five, and each is the
/// first of its kind: `worker`, `gate0`, `review_pass0`, and the two probes
/// at ordinal 0. Production builds more than that, and every one of them is
/// a value `HostRunner::run` could key an observer selection on exactly the
/// way `PR4-CONF-006`'s mutation keyed on an empty stdin. Enumerated from
/// `AttemptRole`'s own variants and from the call sites that build them:
///
/// * `AttemptRole::ReviewReask(n)` — `review::run_review`'s re-ask, a
///   second reviewer process inside one pass and the one attempt role the
///   five-role grid has no slot for;
/// * non-zero role indices — `engine::attempt` numbers gates by position
///   (`Gate(index)`) and review passes by pass (`ReviewPass(pass)`), so the
///   grid's `0` is the first of several, not the only one;
/// * non-zero probe ordinals — each adapter fixes one per pre-flight step
///   (`claude::probe_ordinal`: version 0, help 1, auth status 2).
///
/// Two shapes are deliberately absent because production does not build
/// them. `InvocationId::Sequence` has no production call site in this slice
/// — integration transactions are a later PR — and the `Attempt` form's own
/// `ordinal` is always 0, because `engine::attempt::AttemptCx::invocation`
/// says why: "nothing inside one attempt runs a given role twice".
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
    // Every identity here renders differently from every identity the role
    // grid sends, or this test would be re-proving the grid.
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

/// The **agent binding** dimension, over every shipped adapter rather than
/// the one the role grid names.
///
/// `ExecutionRole::all` binds `claude-code` to its agent-probe target and
/// `fixture_agent` binds the same id to the worker and the reviewer, so the
/// whole containment grid runs on one of the three ids this crate ships.
/// `agent` is a field of every request and `host-v1` already branches on it
/// (`HostEnvironment::compose` gives each agent its own credential
/// location), so `if request.agent == Some(AgentId::new("copilot")) {
/// NoHooks }` is a suppression that runs one third of every real run
/// unobserved and leaves the five-role grid green.
///
/// The roster is [`CREDENTIAL_LOCATIONS`]' own, not a list written here, so
/// a fourth adapter has to appear in this test the moment `host-v1` learns
/// where its credentials live.
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

/// The **program shape** dimension, over every shape production can hand
/// the runner rather than the one the role grid happens to carry.
///
/// Every agent role in the five-role grid runs `std::env::current_exe()` —
/// a native `.exe` on Windows — and the only `.cmd` this suite ever
/// executes is `agent::bin::tests::a_batch_shim_runs_and_receives_its_argument`,
/// which calls `build_command(&spec).output()` and so bypasses `HostRunner`
/// and its hooks entirely. That left
///
/// ```text
/// let mut no_hooks = NoHooks;
/// … if request.command.program.to_ascii_lowercase().ends_with(".cmd") {
///        &mut no_hooks as &mut dyn SpawnHooks
///    } else { &mut **hooks }
/// ```
///
/// green across the whole suite while **every real Windows agent CLI** ran
/// with no containment observation and no fault injection — because
/// npm-installed agent CLIs on Windows are exactly `claude.cmd`,
/// `codex.cmd` and `copilot.cmd`. That is the production shape, not an
/// exotic one, and repair round 6 named this mutation in its own report and
/// neither repaired it nor carried it to `reviews/FINDINGS.md`.
///
/// Same two claims as every other axis in this file — the points are
/// reached, and the observer's answer is honoured — so a shape that is
/// merely *observed* and not *injectable* fails here too.
#[test]
fn every_production_program_shape_reaches_the_containment_points() {
    let points = per_spawn_points();
    let root = scratch("program-shapes");
    let shapes = program_shapes(&root, WORKER_STDIN.as_bytes());

    // The axes, as counts, so the list cannot shrink in silence.
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

    // And the observer's answer is honoured for every shape, at every
    // point — observation and injection are two claims.
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

/// And the shim shape through **every role that runs a CLI**, built by that
/// role's own production builder.
///
/// [`every_production_program_shape_reaches_the_containment_points`] varies
/// the shape against one role, the way
/// [`every_shipped_agent_binding_reaches_the_containment_points`] varies the
/// binding against one role. This is the other half: the role grid itself
/// carrying a batch-shim program, so a suppression keyed on the *pair*
/// — a `.cmd` in the reviewer's hands, say — has nowhere left to be green.
///
/// Three roles and not five: `gate` and `probe(shell)` run the recorded
/// shell by production's own rule, which
/// `the_role_grid_sends_the_shapes_production_sends` asserts, and a fixture
/// that handed a gate an agent shim would be asserting something production
/// never does.
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
        // A probe chooses its own workspace, so the run has to happen where
        // the request says rather than where this test would prefer.
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

/// The grid's non-shell child really does run nothing.
///
/// What the round-6 repair could get wrong: [`agent_cli_command`] executes
/// **this test binary**, so a filter that ever matched something would have
/// the role grid running tests inside its own fixtures — silently, three
/// times per grid, with whatever those tests do to the filesystem — and a
/// filter that stopped exiting 0 would fail the grid for a reason that has
/// nothing to do with containment. Neither is visible from the grid itself,
/// which only reads the exit code.
///
/// So the child's own report is read: `libtest` prints the count it ran,
/// and it must be none.
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

/// Every role's spawn is observed at the containment points — not only the
/// gate's.
///
/// `scope`: "HostRunner wraps proc supervision … through the process
/// funnel with containment sub-effect hook points" and "**probes**,
/// workers, gates, reviews go through the Runner"; `gating`: "process
/// funnel sites recorded"; `proof_tests[3]`: "containment sub-effect hook
/// tests (ST-07 subset)".
///
/// Until this fixture existed every runtime containment observer in this
/// file built its request with `ExecutionRole::Gate`, so a `run` that
/// passed [`NoHooks`] for any *other* role emitted no hook evidence at all
/// and the whole suite stayed green. The probe roles are what that costs
/// most: `decisions.workspace_candidates.run_creation` orders P4
/// `RunnerPreflight` before P6 `run_started`, so their spawns are the
/// prefixes ST-07 evidence over `Process.Spawn` is read as covering.
///
/// Three observations per role, not one — the points reached, the order
/// they were reached in, and (on Unix) the kernel's answer that the
/// containment *operation* ran for this role's child. A funnel that fired
/// the hooks for a probe while skipping the operation would pass the first
/// and fail the third.
///
/// `runner::tests::the_spawn_site_files_every_role_under_one_context_and_the_count_says_which`
/// does **not** discharge this: counting that two roles fall outside the
/// site's declared context proves the mismatch exists; it does not prove
/// the hooks execute on those roles. A counted admission is not runtime
/// proof. This is the runtime proof, and it is asserted for all five roles
/// rather than for the two the count names, because a suppression keyed on
/// any single role is the same defect.
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
        // The same points, in the packet's order and each exactly once —
        // so a role whose funnel reached them in another order, or twice,
        // fails here too and not only the set above.
        assert_eq!(
            *witness.order.lock().expect("order"),
            points.to_vec(),
            "{role}: the funnel's containment order"
        );
        // The funnel's other observation, and the evidence a fault
        // injected at `child_created` would need.
        assert_eq!(
            witness.children.lock().expect("children").len(),
            1,
            "{role}: one spawn, one child"
        );
        // Unix: the containment *operation* — not only its hook — ran for
        // this role. The witness is the kernel.
        #[cfg(unix)]
        assert_eq!(
            *witness.led_own_group.lock().expect("groups"),
            vec![true],
            "{role}: the child did not lead its own process group, so the \
             pre-exec containment step did not run for this role"
        );
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

// -----------------------------------------------------------------------
// Kill mode, actually executed
// -----------------------------------------------------------------------

/// Which containment point the kill helper is to die at.
const SPAWN_KILL_POINT: &str = "UPSTROKE_SPAWN_KILL_POINT";

/// A hook that kills the funnel at one named point and nowhere else.
struct KillAtPoint(SubEffectPoint);

impl SpawnHooks for KillAtPoint {
    fn point(&mut self, point: SubEffectPoint) -> crate::topology::effects::Injection {
        if point == self.0 {
            crate::topology::effects::Injection::Kill
        } else {
            crate::topology::effects::Injection::Proceed
        }
    }

    /// A point consulted at **two** coordinates is killed at the one the
    /// kill mode belongs at, and not at the other one.
    ///
    /// `Spawn.AmbientJobJoined` is that point: its error-return coordinate
    /// is *before* the join and its kill coordinate is *after* it. The
    /// inherited default answers `point()` to both, so a hook armed for a
    /// kill would abort at the earlier, error-return coordinate — before
    /// there is an ambient handle to close, which is the state the point's
    /// kill claim says there is not. The grid would still see an abort and
    /// would still pass, while witnessing a coordinate the packet does not
    /// name. That is the same shape of false witness as the omitted point
    /// itself (`PR5-RD-002`), one layer in.
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

/// The child half of [`a_kill_armed_at_any_containment_point_actually_kills`].
///
/// A kill is `std::process::abort` for the reason [`proc::apply`] gives: the
/// claim under test is what a coordinator that dies **without running any
/// cleanup** does, and both `panic!` and `exit` run destructors — including
/// the one that closes the very job handle whose close-on-death is the
/// mechanism. So it needs a process of its own.
///
/// It **establishes containment first**, which is not decoration. On Windows
/// a kill at `CreatedSuspended` leaves a suspended stub by construction —
/// that is the state INV-18 exists for — and the only thing that reaps it is
/// the ambient job's handle closing when this process dies. A helper that
/// skipped the step would leak one suspended `cmd.exe` per point onto the
/// guest, **measured**: the first run of this grid left three of them and a
/// hung parent. On Unix the step is a no-op and the per-invocation reaper
/// settles the group instead.
///
/// **The startup step is where the arming happens for a startup point.**
/// This used to run `start_write_command(&mut NoHooks)` unconditionally and
/// only then install `KillAtPoint`, so `Spawn.AmbientJobJoined` — which is
/// reached by that call and by nothing later — could not receive a kill at
/// all, and six guest runs of a grid that claimed to cover it executed it
/// zero times (`PR5-RD-002`). A startup point is now armed *on the startup
/// call*, which is the only place it is consulted.
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
        // The point's kill coordinate is *inside* this call, after the real
        // ambient join. Reaching the line after it means the kill never
        // fired, and the parent reads a clean exit as exactly that.
        let _ = start_write_command(&mut KillAtPoint(point));
        std::process::exit(0);
    }
    start_write_command(&mut proc::NoHooks)
        .expect("the helper establishes containment before it spawns anything");
    let runner = HostRunner::new().with_hooks(Box::new(KillAtPoint(point)));
    // Every point this platform declares is reached by an ordinary spawn,
    // which is what `every_role_reaches_the_containment_points_of_this_
    // platform` establishes; the gate role is the cheapest of the five.
    let _ = runner.run(&crate::runner::gate_request(
        shell_command(),
        workspace.clone(),
        crate::config::DEFAULT_GATE_TIMEOUT,
        gate_invocation(),
    ));
    let _ = std::fs::remove_dir_all(&workspace);
    // Reached only if the kill did not fire, which the parent detects as a
    // clean exit.
    std::process::exit(0);
}

/// `Injection::Kill` **aborts**, at every containment point that declares
/// the mode on this platform.
///
/// `decisions.effect_site_inventory.scope` requires "every parent-side
/// sub-effect point observed **executed** at least once by the suite in
/// every injection mode the point supports", and every containment point
/// declares `Kill` (`SubEffectPoint::modes`). Nothing had ever let one
/// fire: the runtime reach tests arm nothing, and the fault grid injects
/// `Injection::Error` — deliberately, because an abort would take the test
/// binary with it. So `Injection::Kill => Ok(())` in `proc::apply` passed
/// the whole suite (`PR5-SEAMS-001`), and with it every ST-07 kill-mode
/// claim about `Process.Spawn`.
///
/// This is the sibling of `events::log::tests::a_kill_at_each_append_point_
/// leaves_the_shape_the_packet_tables`, and the same idiom: a subprocess
/// helper, and the child's death **checked** rather than assumed — not a
/// clean exit, no `panicked at` on stderr, and on Unix the signal is
/// `SIGABRT` and not some other way of dying.
///
/// The domain is [`containment_points()`] — **every** point this platform
/// declares, read out of `Process.Spawn`'s own `sub_effects()` and each
/// point's own `platform()`, so a point added later is covered by
/// construction rather than by someone remembering to add it here. It was a
/// hand-written three-element list, which omitted `AmbientJobJoined`
/// (`PR5-RD-002`): the grid, its helper and this doc comment all agreed
/// that the Windows ambient join was covered in kill mode, and it had never
/// once executed in that mode on the guest.
///
/// Both of the funnel's two appliers are inside the domain: on Unix all
/// four points go through `proc::apply`, and on Windows `AmbientJobJoined`
/// goes through `apply` while the three per-spawn points go through
/// `apply_io`. That is the second reason the omission mattered — with
/// `AmbientJobJoined` absent, no Windows run of this grid touched `apply`
/// at all.
///
/// **The helper's output goes to files, and this waits on the process
/// rather than on its pipes.** `Command::output()` returns when the pipe
/// write ends close, not when the child exits, and on Windows
/// `CreateProcessW` inherits handles — so the grandchild this helper leaves
/// **suspended by design** holds a duplicate of the pipe and `output()`
/// blocks for ever. Measured on the guest, where it hung the whole run.
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
    // The count, so a domain that shrank would fail here rather than pass
    // vacuously — four on **both** platforms, which is what the frozen
    // inventory declares and what the omitted `AmbientJobJoined` made read
    // as three.
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

/// A hook that fails the funnel at one named point and nowhere else.
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

/// The hooks are not merely *called* on every role — their answer is
/// honoured, so every role is fault-injectable at every containment point
/// this platform reaches.
///
/// Observation and injection are two claims, and the second is the one
/// ST-07 spends: a funnel that consulted the observer and then ignored
/// what it said would satisfy
/// `every_role_reaches_the_containment_points_of_this_platform` and inject
/// nothing. The armed point is named in the failure the caller receives,
/// so this also pins *which* coordinate refused — a funnel that collapsed
/// four points into one arming site would fail here.
///
/// `Injection::Error` rather than `Injection::Kill` because `Kill` aborts
/// the process: it is exercised by the Windows coordinator-death tests,
/// which need a subprocess to survive it. The packet gives only
/// `AmbientJobJoined` an error contract, so an `Error` at these points can
/// come only from a hand-written observer — which is exactly what a fault
/// injection is, and `apply`/`apply_io` surface it rather than dropping it.
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

/// The operation of `Spawn.PreExecPgidAndRegister` really did happen in the
/// forked child, and the funnel really did reach the four Unix points in
/// the packet's order.
///
/// The witness is the kernel, not this crate: `getpgid(pid) == pid` is true
/// exactly when the closure's `setpgid(0, 0)` ran, and it is asked at
/// `child_created` — the first instant the parent knows the pid.
#[cfg(unix)]
#[test]
fn the_pre_exec_containment_step_runs_in_the_forked_child() {
    #[derive(Clone, Default)]
    struct Witness {
        led_own_group: Arc<Mutex<Vec<bool>>>,
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
                .push(proc::child_leads_its_own_group(pid));
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

    assert_eq!(
        *witness.led_own_group.lock().expect("groups"),
        vec![true],
        "the child did not lead its own process group, so the pre-exec \
         closure did not run in it"
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

/// A containment proof exists exactly when containment was established,
/// and establishing it twice is establishing it once.
///
/// [`Contained`] is what the engine's write coordinator now requires, and
/// [`containment_establishments`] is what an entry-point census reads, so
/// both are only worth anything if a token cannot appear without the step
/// having run. The count is incremented by the token's own constructor, so
/// this is the assertion that the constructor is not reachable another way
/// — including through the refusal path, which returns before it.
///
/// Idempotence is not decoration either: the CLI establishes containment
/// at dispatch and then calls `engine::run`, which establishes it again.
/// The ambient job is a process-wide singleton (`join_ambient` memoises),
/// so the second call must be a no-op that still hands back a proof — and
/// on the platform where it does something, the process must still be a
/// member afterwards.
#[test]
fn a_containment_proof_exists_only_where_containment_was_established() {
    let before = containment_establishments();
    // The tokens are held rather than discarded: two live proofs at once is
    // what a coordinator entered from an already-contained CLI has.
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

    // A refused establishment mints nothing. Only Windows can refuse — on
    // Unix the step is the reaper and the isolated process group, and
    // there is nothing that can fail — so the negative half is asserted
    // where it exists.
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

// -----------------------------------------------------------------------
// Windows ambient job (INV-18)
// -----------------------------------------------------------------------

/// An observer that refuses the ambient join, for
/// `windows_ambient_job_unavailable_refuses_before_effects`.
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

    // The observer has to be readable after the run, and `with_hooks`
    // takes ownership, so it reports through a channel.
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
    // "The simulated failure left no real ambient job behind" — asserted
    // on the *count* rather than on `ambient_job_established`, which is a
    // process-wide latch and no longer a valid oracle here: the library's
    // write coordinator establishes containment now
    // (`engine::tests::every_public_write_coordinator_entry_point_establishes_containment`),
    // so other tests in this binary legitimately join the real ambient job
    // and the latch may already be true when this test runs. The count is
    // per-thread and per-call, so it still says exactly what this test
    // means: *this* refusal established nothing.
    assert_eq!(
        containment_establishments(),
        established_before,
        "a simulated join failure minted a containment proof"
    );
}

/// The production containment mint propagates a join refusal, and a
/// refused join mints nothing.
///
/// `invariants_introduced[3]`: "INV-18 host portion: … refusal before any
/// effect if the ambient job cannot be established", and
/// `expected_failures_refusals[1]`: "ambient job cannot be created or
/// joined (Windows) → write command refuses at startup with a diagnostic".
///
/// The subject is [`contain_write_command`] **itself** — the function
/// `engine::run_harness`, `engine::resume_harness` and `src/main.rs`'s
/// dispatch all reach, and the only place in the crate that mints a
/// [`Contained`]. Every other simulated ambient failure in this suite goes
/// through [`HostRunner::start_write_command`] or through a closure injected
/// at `engine::run_contained`, so
///
/// ```text
/// let _join_outcome = proc::join_ambient_job(hooks);
/// Ok(Contained::new())
/// ```
///
/// left the whole suite green while every facade run and every `upstroke run`
/// on Windows dispatched with **no ambient job** — and a coordinator killed
/// between `CreateProcessW` and private-job assignment then leaves a
/// suspended stub with no owner, which is the one thing the ambient job
/// exists to prevent.
///
/// Windows only, and that is the invariant rather than a limitation of the
/// test: on Unix [`proc::join_ambient_job`] returns `Ok` unconditionally and
/// does not consult the observer at all — deliberately, so a Linux cell
/// cannot record a Windows containment point as executed — so there is no
/// failure on that platform for anything to propagate. The Linux suite
/// cannot kill that mutation and does not claim to.
#[cfg(windows)]
#[test]
fn the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing() {
    // The observer is borrowed rather than owned here, so it can simply be
    // read afterwards — `with_hooks` takes ownership and needs a channel,
    // this does not.
    let mut refusing = RefuseAmbientJoin::default();
    let before = containment_establishments();
    let error = contain_write_command(&mut refusing)
        .expect_err("a write command whose ambient job cannot be established must refuse");

    // The diagnostic reaches the caller. The three fragments are
    // `proc::AMBIENT_REFUSAL_PREFIX` and `proc::AMBIENT_REFUSAL_SIMULATED`,
    // named rather than matched whole: what the operator has to be told is
    // that it is the ambient job, which invariant it enforces, and that
    // nothing ran.
    let message = error.to_string();
    for fragment in ["ambient", "INV-18", "No process was spawned"] {
        assert!(
            message.contains(fragment),
            "the refusal must say `{fragment}`: {message}"
        );
    }
    // No effect precedes it: the funnel reached the join's own coordinate
    // and nothing past it, and no child exists.
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
    // And no proof was minted — which is the half the mutation above
    // breaks. The count is per-thread and per-call, so it says exactly that
    // *this* call established nothing, where `proc::ambient_job_established`
    // is a process-wide latch other tests in this binary legitimately set.
    assert_eq!(
        containment_establishments(),
        before,
        "a refused join minted a containment proof"
    );

    // The success direction, so the assertion above is about the refusal
    // and not about a function that never mints at all.
    let _proof = contain_write_command(&mut NoHooks).expect("the real join succeeds on this host");
    assert_eq!(
        containment_establishments(),
        before + 1,
        "the step ran and minted nothing"
    );

    // `start_write_command` is the same step for the caller with nothing to
    // prove it to — `src/main.rs`'s dispatch, which is the CLI's whole
    // write side — and it has a body of its own to drop the refusal in.
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

/// Where the join-ordering helper writes what it saw.
#[cfg(windows)]
const JOIN_RECORD: &str = "UPSTROKE_PR4_JOIN_RECORD";

/// Records, at each consultation of `Spawn.AmbientJobJoined`, whether the
/// ambient job existed *at that instant*. The kernel is the oracle: the
/// singleton is set only by a successful `AssignProcessToJobObject`.
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

/// The point named `AmbientJobJoined` is observed on the side of the join
/// its own contract needs.
///
/// `containment_sub_effects` gives it both an error contract ("failure
/// refuses the write command", which stands in place of establishing the
/// job) and a kill claim ("a coordinator kill after any of these leaves no
/// host process — the ambient handle closes"), and those are opposite sides
/// of one operation. So the error-return coordinate must see no job and the
/// kill coordinate must see one.
///
/// In a subprocess because the ambient job is a process-wide singleton:
/// "not yet established" is observable once per process, and a test that
/// depended on being the first in its binary would be a test whose meaning
/// depended on test order.
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

/// Set for the child that is to carry a **real** memoised ambient failure.
#[cfg(windows)]
const POISON_AMBIENT: &str = "UPSTROKE_POISON_AMBIENT";

/// The child half of
/// [`a_real_memoised_ambient_failure_refuses_the_write_command`].
///
/// It spends this process's one ambient cell, which is why it needs a
/// process of its own: `AMBIENT` is a `OnceLock` and the test binary's
/// other tests need it unspent.
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

    // (1) The funnel's own entry point reports the remembered failure.
    let error = proc::join_ambient_job(&mut proc::NoHooks)
        .expect_err("a memoised failure is a failure for every later caller");
    let rendered = error.to_string();
    assert!(rendered.contains(MESSAGE), "the diagnostic: {rendered}");
    assert!(
        rendered.contains("No process was spawned"),
        "and it says nothing ran: {rendered}"
    );

    // (2) The production mint refuses, and mints nothing.
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

    // (3) And the CLI's unit-returning entry, which is what `src/main.rs`
    // calls before any dispatch arm.
    start_write_command(&mut proc::NoHooks).expect_err("the CLI write path refuses too");

    println!("POISONED-AMBIENT-REFUSED");
}

/// A **real** memoised ambient failure refuses the write command.
///
/// `crash_reconstruction`: "if the ambient job cannot be created or joined
/// the write command refuses at startup with a diagnostic before any
/// workspace effect (no degraded mode; deferred)", and
/// `expected_failures_refusals[1]` requires the same refusal.
///
/// `PR4-CONF-005` closed the *injected* half — an observer refusing at
/// `Spawn.AmbientJobJoined`, which fires strictly **before** the memo is
/// consulted. `PR5-CORRECTNESS-010` is the half beyond it: no test had ever
/// carried an actual memoised `Err` through `join_ambient`'s match, so
/// `Err(_) => Ok(())` there left `join_ambient_job` reporting success,
/// `contain_write_command` minting `Contained`, and `run`/`resume` taking
/// workspace effects with no ambient kill-on-close job.
///
/// Windows only, and that is the invariant rather than a gap:
/// `join_ambient_job` is a no-op on Unix and has no memo to poison. The
/// platform-independent half of the same claim — that a remembered failure
/// comes back as that failure — is `agent::proc::tests::
/// a_memoised_establishment_failure_reaches_every_later_caller`, which runs
/// everywhere.
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

/// Where the coordinator helper writes the identity of the stub it created
/// before it dies.
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
            // The kill hook: after `CreateProcess` and before private-job
            // assignment. `apply_io` aborts, so no destructor runs and the
            // ambient handle is closed only by the kernel.
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
    // Aborts inside `run`, at `CreatedSuspended`.
    let _ = runner.run(&request);
    unreachable!("the coordinator helper was supposed to die at CreatedSuspended");
}

/// Run one coordinator-death cycle and return the stub's identity and
/// whether it was an ambient-job member at creation.
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

// -----------------------------------------------------------------------
// program resolution (PR6D-001)
// -----------------------------------------------------------------------

/// Both naming rules, exhaustively.
///
/// The `match` is what makes it exhaustive: a variant added later fails to
/// compile here rather than quietly leaving every grid below, which is the
/// failure `PR5-RD-002` recorded one level out — a hand-written domain that
/// omitted a point while six guest runs reported covering it.
fn naming_grid() -> Vec<ProgramNaming> {
    let mut all = Vec::new();
    for naming in [ProgramNaming::Posix, ProgramNaming::Windows] {
        match naming {
            ProgramNaming::Posix | ProgramNaming::Windows => all.push(naming),
        }
    }
    all
}

/// A composed environment holding exactly the pairs given.
fn composed(pairs: &[(&str, &OsStr)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(key, value)| (OsString::from(*key), (*value).to_os_string()))
        .collect()
}

/// `PATH` as an `OsString`, from directories.
fn path_of(dirs: &[&Path]) -> OsString {
    std::env::join_paths(dirs).expect("a synthetic PATH")
}

/// An empty file the platform would accept as a program: on Unix the
/// execute bit is set, because `execvp` requires it and
/// [`ProgramNaming::Posix`] therefore checks it.
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

/// A file that exists and is **not** a program: no execute bit.
///
/// Unix only, because it is only there that a file's mode decides: Windows
/// carries no execute bit, so "exists but is not executable" is not a state
/// a fixture can construct on that platform at all.
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

/// The `PATHEXT` a Windows machine ships, in its order.
const REAL_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC;.CPL";

/// The file name a bare `name` is installed under here: npm's two
/// spellings, a batch shim on Windows and an extensionless script on Unix.
fn shim_file_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_owned()
    }
}

/// A runnable shim that prints `marker` and its first argument.
///
/// The **file name is the caller's**, so the extension is a field a test
/// varies while the content is held constant, and the marker is the
/// caller's so "a shim ran" and "*this* shim ran" are different
/// observations.
fn marker_shim(dir: &Path, file_name: &str, marker: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create the shim directory");
    let path = dir.join(file_name);
    if cfg!(windows) {
        std::fs::write(&path, format!("@echo off\r\necho {marker}:%~1\r\n"))
            .expect("write the batch shim");
    } else {
        std::fs::write(&path, format!("#!/bin/sh\necho \"{marker}:$1\"\n"))
            .expect("write the shell shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("make the shim executable");
        }
    }
    path
}

/// This process's environment with `PATH` — and `PATHEXT` — replaced.
///
/// The rest of the process environment is kept rather than emptied, because
/// spawning a batch shim on Windows goes through `cmd.exe` and a child
/// stripped to two variables would be testing something else. Only the two
/// fields under test move.
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

/// A gate request for `program` with one argument.
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

/// A program string, and whether each naming rule calls it a **name** (to
/// search for) rather than a **location** (to use as given).
///
/// Written here from the two platforms' own rules, not read from the code
/// under test. The four rows the two rules disagree on are the point: on
/// Unix a backslash and a colon are ordinary characters in a file name, and
/// a rule that ignored its `self` would agree with itself on every row that
/// only contains `/`.
const NAME_TABLE: &[(&str, bool, bool)] = &[
    // (program, is a name under Posix, is a name under Windows)
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

/// A program that names a **location** is handed to `Command` as given, and
/// only a bare name is searched for.
///
/// This is the constraint the repair had to hold: "an absolute `program`
/// must spawn exactly as today". It is asserted by construction rather than
/// by a spawn — the environment carries no `PATH` at all, so a resolution
/// that searched would have nothing to find and would refuse, and every
/// location row instead comes back byte for byte.
///
/// The second field held constant is the environment (empty, for every
/// row); what varies is the program shape and the naming rule, and the four
/// rows on which the two rules must **disagree** are counted so that a
/// `is_bare_name` which ignored its platform reports it.
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

/// `PATH` order decides between installations; `PATHEXT` order decides only
/// **within** one directory.
///
/// The intersection this repair is most exposed to, and the one a shell and
/// `std::process::Command` answer differently: std appends `.exe` and only
/// `.exe`, so an earlier directory's `claude.cmd` is invisible to it and a
/// later directory's `claude.exe` wins. A shell — and now this runner —
/// takes the earlier directory. Both axes vary independently and the
/// resolved files are counted as distinct values, so a nesting that swapped
/// the loops collapses the count.
///
/// Driven under [`ProgramNaming::Windows`] on **both** platforms, which is
/// the whole reason that type exists: `PR6D-001` is a Windows rule, and a
/// Windows rule only the guest can execute is a rule that ships untested
/// six days out of seven.
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
    // `first/x.cmd`, `second/x.exe`, `both/x.com`, `both/x.cmd`,
    // `both/x.exe` — one per way the two axes can decide, and a grid that
    // reaches fewer has an axis that is not varying the answer.
    assert_eq!(
        resolved.len(),
        5,
        "the grid must reach five distinct files; fewer means an axis is not varying: \
         {resolved:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// What a Windows name may be is `PATHEXT`, and never the extensionless
/// file beside it.
///
/// Three things a resolution can get wrong here, each with its own row:
/// widening (an extensionless `claude` in a `PATH` directory shadowing the
/// real `claude.exe` — `CreateProcessW` appends `.exe` and `cmd.exe`
/// appends `PATHEXT`, and neither would run it); not reading `PATHEXT` at
/// all (a hard-coded list agrees with the default and diverges the moment
/// an operator sets one); and treating an unusable `PATHEXT` as "this
/// machine has no programs" rather than as a malformed variable.
///
/// Every row holds the directory and the file set constant and varies only
/// `PATHEXT` and the naming rule.
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

    // The default is the platform's, in the platform's order: `.COM` first.
    for absent in [None, Some(""), Some(";;;"), Some("exe"), Some(".")] {
        assert_eq!(
            resolve(absent, ProgramNaming::Windows).expect("the default PATHEXT applies"),
            com,
            "PATHEXT={absent:?}: an unusable PATHEXT must fall back to the platform default, \
             not to \"no candidates\""
        );
    }
    // And it really is read, not assumed: a PATHEXT naming one extension
    // nobody ships picks that file over the `.exe` sitting beside it.
    assert_eq!(
        resolve(Some(".FOO"), ProgramNaming::Windows).expect("PATHEXT is honoured"),
        foo
    );
    assert_eq!(
        resolve(Some(".EXE"), ProgramNaming::Windows).expect("PATHEXT is honoured"),
        exe
    );
    // The widening that must not happen: `x` exists and is never chosen.
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
    // Unix has no extensions: the bare file is the only answer, and
    // `PATHEXT` changes nothing.
    for pathext in [None, Some(REAL_PATHEXT), Some(".FOO")] {
        assert_eq!(
            resolve(pathext, ProgramNaming::Posix).expect("Unix resolves the name itself"),
            bare,
            "PATHEXT={pathext:?}: Unix consulted PATHEXT"
        );
    }
    // A name that carries an extension is tried verbatim first, under a
    // PATHEXT that would otherwise send it elsewhere.
    let value = OsString::from(".FOO");
    let with_extension = resolve_program(
        "x.exe",
        &composed(&[("PATH", &path), ("PATHEXT", &value)]),
        KeyCase::Insensitive,
        ProgramNaming::Windows,
    )
    .expect("a name with an extension resolves");
    assert_eq!(with_extension, exe, "`x.exe` resolved to something else");
    // And a directory is not a program, on either rule.
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

/// A candidate without the execute bit is skipped, the way `execvp` skips
/// it.
///
/// The regression this guards is silent and platform-specific: `execvp`
/// walks past a non-executable file and finds the real installation further
/// along `PATH`, so a resolution that stopped at the first *existing* file
/// would refuse — or spawn `EACCES` — where the code it replaced ran. Two
/// directories, same name, and the answer must be the second.
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

    // Windows has no such bit, so the same file is a program there — the
    // two rules must not be the same rule.
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

/// A name that matches nothing is refused, naming the name and the
/// boundary — and an empty `PATH` entry is never searched.
///
/// Fail-closed is the choice the repair round is held to: the alternative
/// is handing the name to `Command` anyway and taking a `NotFound` that
/// names no boundary, which on Windows is the failure an operator could not
/// diagnose. The empty-entry rule is here because this is the site where it
/// would actually execute: an empty `PATH` entry means "the current
/// directory" to some shells, and this runner's current directory is the
/// workspace — repository content, under automation.
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

    // The empty entry is skipped rather than treated as "here", and a real
    // directory beside it is still searched: the count is the observable.
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

/// **`PR6D-001`.** A bare name that only `PATHEXT` resolves is executed by
/// the host runner — the npm-installed agent CLI, spawned the way
/// production now spawns it.
///
/// `PATHEXT` lists `.CMD`; `CreateProcessW` appends `.exe` and nothing
/// else, and neither does Rust's `Command`. So a `CommandSpec.program` of
/// `claude` with `claude.cmd` on `PATH` failed with `NotFound` on every
/// Windows host, for the probe, the worker, the gate, the review and the
/// re-ask. The suite could not see it: every `.cmd` fixture in this crate
/// used an **absolute** path, which is a different property, and the guest
/// has none of the three CLIs installed.
///
/// The platform fact is asserted **in this test**, not cited: the same
/// bare name is handed to `std::process::Command` under the same composed
/// environment first, and must fail with `NotFound`. Without that row the
/// claim below would pass on a platform where the bug never existed. Two
/// shims, two extensions, two markers and two arguments, counted as
/// distinct values so a fixture that ran one shim twice reports it; what is
/// held constant is the `PATH` directory, the runner and the composed
/// environment.
#[cfg(windows)]
#[test]
fn a_bare_name_that_only_pathext_resolves_runs_through_the_host_runner() {
    let root = scratch("pathext-spawn");
    let bin = root.join("bin");
    let workspace = root.join("ws");
    std::fs::create_dir_all(&workspace).expect("a workspace");

    // Unique per run, so nothing on this machine's real PATH, in the
    // application directory or in the system directories can satisfy it.
    //
    // Both arguments are **benign**, for `bin.rs`'s own reason: `%~1`
    // strips the quotes the child received, so a `&` in the value would be
    // re-parsed by `cmd.exe` as a command separator *inside the shim* and
    // this case would be measuring batch re-parsing instead of resolution.
    // Argument escaping through a batch target is
    // `agent::bin::tests::arguments_reach_the_command_untouched`'s subject
    // and is unaffected by which file a name resolved to. Measured on the
    // guest: with `second & argument` the `.bat` shim exits 1 with
    // "'argument' is not recognized", and the `.cmd` shim beside it — same
    // resolution, benign argument — passes.
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
        // The platform fact, executed. `std` searches the child PATH, the
        // application directory, the system directories and the parent
        // PATH, appending `.exe` to each — so a `.cmd`/`.bat` on PATH is
        // invisible to it, and this is `PR6D-001` itself.
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

        // The claim: the same name, through the runner.
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

/// Two runners in one process resolve one name against **their own**
/// environments.
///
/// The hazard the container runner introduces and this repair must not
/// reintroduce: a resolution remembered anywhere — a `OnceLock`, a field, a
/// process-wide cache — hands the first boundary's answer to the second,
/// and a value that is correct on first use and wrong on the second is
/// invisible to any test that constructs one runner.
/// `agent::built_program_tests` holds that for the adapters; this holds it
/// for the boundary, with real spawns.
///
/// Both orders, because "the first caller wins" is a property of order, and
/// the markers are counted as distinct values.
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

/// An absolute program is spawned as given, even when a `PATH` directory
/// holds a different file of the same name.
///
/// "An absolute `program` must spawn exactly as today" is the constraint
/// this repair was given, and a resolution that re-resolved one would be
/// invisible to every existing fixture: they all put the *only* copy of the
/// program at that path. Here there are two copies, so "used as given" and
/// "searched for" produce different output. The space in the directory name
/// is `bin.rs`'s own production shape, `C:\Users\John Smith\npm\`.
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

    // The control: the same runner, the same name without its directory,
    // reaches the other file — so the two really are distinguishable and
    // the row above is not passing by coincidence.
    let by_name = runner
        .run(&named_request(&name, "arg", &workspace))
        .expect("the bare name resolves on PATH");
    assert_eq!(by_name.stdout.trim(), "ONPATH:arg");
    let _ = std::fs::remove_dir_all(&root);
}

/// Everything one spawn's containment observers can see about resolution.
struct ResolutionWitness {
    /// `program_resolutions()` as each containment point saw it, in order.
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

/// One program name is resolved **once per spawn**, **before** any of the
/// spawn, and **not at all** when the request is refused earlier.
///
/// Ordering is a set of independently droppable predicates and a suite that
/// proves only "the right file ran" holds none of them. The observable is
/// the sequence: [`program_resolutions`] read at every containment point,
/// which are the coordinates the funnel passes through between
/// `CreateProcess`/`fork` and the running child. A resolution that happened
/// twice shows a second increment; one that happened lazily at spawn time
/// shows a count that is still at the baseline when the first point fires;
/// one that happened before `compose` shows an increment on the request
/// `compose` refuses.
///
/// Both program shapes, because "once" must hold on both branches of the
/// resolution — a bare name that is searched for, and an absolute path that
/// is not — and the bare name is a `PATHEXT`-resolved batch shim, which is
/// the intersection {a name to resolve} x {a file `CreateProcessW` reaches
/// only through an interpreter} that no fixture in this crate had.
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

    // A request the environment refuses is refused **before** anything is
    // resolved: `compose` runs first, so a reserved-key overlay never
    // reaches the filesystem and never reaches a containment point.
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

    // And a name that resolves to nothing is refused **after** resolution
    // and **before** any of the spawn: the count moves, the points do not.
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

/// Every bare program this crate ships goes through **one** resolution
/// rule.
///
/// Two rules is how `PR6D-001` happened: the shells' bare names worked
/// because a shell is an `.exe`, so nothing noticed that the agent CLIs'
/// bare names — the same shape, one field over — did not. The names are
/// written here rather than read from `ShellKind::program` and the adapter
/// constants, and the exhaustive `match` below ties the written table to the
/// enum so a sixth shell fails to compile rather than silently leaving the
/// grid.
///
/// What is held constant is the directory, the shim content and the naming
/// rule; what varies is the name, and the resolved files are counted so a
/// rule that special-cased one name collapses the count.
#[test]
fn every_bare_program_this_crate_ships_goes_through_one_resolution_rule() {
    // The five shells `gates::ShellKind::spec` can put in a spec, and the
    // three agent CLIs `bin::Invocation::named` can.
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

/// Resolving `cmd` does not change `cmd.exe`'s raw-tail rule.
///
/// `build_command`'s one Windows rule is keyed on the program, and the
/// program the runner hands it is now the **resolved** one — an absolute
/// path where the spec carried a bare name. A gate whose command line
/// changed meaning depending on whether its shell had been resolved is not
/// "adapter parsing unchanged"; `gates::ShellKind::command` says why the
/// tail must reach the child un-re-quoted, and this is the half of that
/// rule which the repair could have broken.
///
/// **This has to spawn.** `Command::get_args` yields the same sequence for
/// `arg` and for `raw_arg`, so an assertion over the built `Command` cannot
/// tell the two apart — measured: the first version of this test was green
/// under the mutation it was written for. What distinguishes them is the
/// command line `CreateProcessW` receives, and the only oracle for that is
/// the child's own output: std escapes an embedded quote as `\"`, which
/// `cmd.exe` does not un-escape, so a re-quoted tail echoes `\"quoted`.
///
/// The resolved path is the one **this runner** resolves, not a transcribed
/// `C:\Windows\System32\cmd.exe`, so the case holds on a machine whose
/// shell lives elsewhere; that it differs from `cmd` is asserted first, or
/// the two spellings would be one.
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

    // Both spellings, spawned. The quotes must arrive as the operator wrote
    // them, from the resolved path exactly as from the bare name.
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

    // And through the production route, where the bare name is what the
    // gate ships and the runner does the resolving.
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

    // The control: a program that is not `cmd` does not get the rule at
    // all, so the rows above are not true of everything. An npm shim named
    // `claude.cmd` has the file stem `claude`, and a rule keyed on the
    // extension rather than the stem would hand it a raw tail.
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

// -----------------------------------------------------------------------
// PR6-LANED-001: one boundary, one executable
// -----------------------------------------------------------------------

/// The request **production** sends for `role`, carrying **this** program.
///
/// [`production_request`] fixes the program per role because there the role
/// is the subject; here the program is the subject and the role is what
/// varies, so each role's own production builder is handed the same spec.
/// `Probe(Shell)` returns `None` because its program is not a caller's to
/// choose — [`shell_probe_request`] writes the recorded shell — and the
/// `match` is exhaustive, so a role added later has to be classified here
/// rather than silently leaving the grid.
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

/// **`PR6-LANED-001`.** One boundary executes **one** file for a name, even
/// when the filesystem moves between pre-flight and the attempt.
///
/// DESIGN.md:612 — "Probes run through that same runner, **or pre-flight
/// could certify a host CLI/version different from the one the attempt
/// executes**". Routing the probe through the runner is necessary and is
/// not sufficient, and this is the fixture that says so: `PATH=first:second`
/// with the same name in both directories, the probe certifies
/// `first/<name>`, `first/<name>` is then removed, and a runner that
/// re-searched per spawn hands the attempt `second/<name>` — a different
/// executable under an unchanged `CommandSpec.program`. A test asserting
/// that the two *program strings* agree passes throughout, which is why the
/// claim it supported was wrong.
///
/// **The control is the oracle**: a *fresh* runner over the same
/// environment, after the removal, does reach `second/<name>` — so the two
/// files are genuinely distinguishable, the removal genuinely changes the
/// answer, and the memoised runner's refusal is the memo rather than a
/// fixture that could not tell them apart.
///
/// The second field held constant is the environment — one `HostEnvironment`
/// for every row, composed the same way — and what varies is the role
/// (`Probe(Agent)` then `Implement`, which is the pair the passage names)
/// and the state of the filesystem between them.
///
/// **Fail-closed on purpose.** The attempt does not get `second/<name>`; it
/// gets a spawn failure naming the file pre-flight certified. An operator
/// reading it learns that the CLI moved under a running run, which is true,
/// instead of a `Caps.version` that quietly stopped describing anything.
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

    // The CLI moves under the run: the file pre-flight executed is gone,
    // and the other one — same name, same PATH, different executable — is
    // still there.
    std::fs::remove_file(&first).expect("remove the certified installation");
    assert!(!first.exists());

    // The oracle. A boundary that had not decided yet reaches `second`, so
    // "resolve per spawn" really does change the answer here.
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

    // The claim. The runner that certified `first` does not silently run
    // `second`.
    //
    // *What* the failure looks like is the platform's, and the two differ:
    // on Unix the spawn of a vanished file is `ENOENT` and the runner
    // returns an error naming it, while on Windows `std` runs a `.cmd`
    // through `cmd.exe`, so the spawn succeeds and the interpreter exits
    // non-zero. The claim is neither of those spellings — it is that the
    // attempt did not run the *other* installation and did not report
    // success — so it is asserted over everything the boundary handed back,
    // whichever shape it came in.
    //
    // The text is the child's own, not a `{:?}` of it: a debug rendering
    // escapes every backslash, and a Windows path searched for inside one
    // would never be found however right the runner was.
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

/// One name is **searched once** for a whole run, and asked for once per
/// spawn.
///
/// The identity predicate and the ordering predicate are independently
/// droppable, so they have two counters: [`program_resolutions`] moves once
/// per spawn (D1's `a_program_is_resolved_once_per_spawn_…` holds that and
/// its position in the spawn) and [`program_searches`] moves once per
/// boundary. A memo that never hits satisfies the first and reopens
/// DESIGN.md:612; this is the fixture for the second.
///
/// **Across roles, and that is the point.** `host-v1` supplies credential
/// locations *role-scoped* ([`supplies_credentials`]), so a probe's
/// composed environment and a gate's differ — asserted here as a
/// distinct-value count before anything else, because a memo keyed on the
/// composed environment would miss on exactly the pre-flight/attempt pair
/// :612 requires to agree, and this test would then be reporting that four
/// spawns searched four times for a good reason. The key is the three
/// fields that decide the answer, not the environment.
///
/// `Probe(Shell)` is the one role absent: its program is the recorded
/// shell, not a caller's choice. [`role_request_for`] is exhaustive over
/// [`ExecutionRole`], so it is absent by classification rather than by
/// omission.
#[test]
fn one_name_is_searched_once_for_a_boundary_and_asked_for_once_per_spawn() {
    let root = scratch("searched-once");
    let workspace = root.join("ws");
    std::fs::create_dir_all(&workspace).expect("a workspace");
    let name = format!("upstroke-d2-{}", crate::ulid::ulid());
    let bin = root.join("bin");
    marker_shim(&bin, &shim_file_name(&name), "ONE");

    // `host-v1` supplies a credential location only when the *base* carries
    // it, so the base has to carry one for the roles to differ at all —
    // this machine's own environment has no `CLAUDE_CONFIG_DIR` and every
    // role would otherwise compose the same thing.
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

    // The premise: these roles really do compose different environments, so
    // "one answer for all of them" is a claim and not a tautology.
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

/// What the memo is keyed on: the program **and** the environment that
/// answers for it.
///
/// The hazard the repair introduces. A memo keyed on the program name alone
/// is a new way to certify the wrong executable — the same defect one layer
/// in — and it is invisible to every fixture above, because in production
/// `PATH` is reserved and constant for a run. So this asks the boundary the
/// same name under two environments and requires two answers.
///
/// `program_for` directly rather than through `run`, because the only
/// composed value a caller can vary within one runner *through* `run` is
/// `PATHEXT` (an overlay may not name `PATH`, which is reserved), and
/// `PATHEXT` decides nothing on Unix. Both fields of the key are then
/// exercised on both platforms, which is the property D1's `ProgramNaming`
/// exists to preserve.
///
/// Held constant: the runner, the name, and the naming rule. Varied: the
/// composed `PATH`, and — on Windows, where it decides anything — the
/// composed `PATHEXT`. The resolved files are counted as distinct values.
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

    // The other field of the key, where it decides anything.
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

/// A name this boundary refused stays refused, in the same words, without
/// asking the filesystem again.
///
/// The failure branch of the memo, and the one where fail-open would be
/// easy: not remembering a refusal means a run whose pre-flight could not
/// find `claude` silently finds one at the third attempt because something
/// installed it meanwhile — pre-flight certifying an absence the attempt
/// does not honour, which is DESIGN.md:612 with the polarity flipped.
///
/// **The control is the oracle.** After the CLI appears, a *fresh* boundary
/// does run it. So the second refusal is the memo holding, not a fixture in
/// which nothing changed.
///
/// The replayed error is required to be the first one **byte for byte**,
/// which is what makes storing the refusal as its message safe:
/// [`UpstrokeError::Refused`] displays as exactly its message, and if that
/// ever stops being true this row says so.
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

    // The CLI appears under the run.
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

    // The oracle: it really is installed now.
    let fresh = HostRunner::new().with_environment(environment());
    let output = fresh
        .run(&named_request(&name, "arg", &workspace))
        .expect("a boundary that had not answered yet finds it");
    assert_eq!(output.stdout.trim(), "LATE:arg");
    let _ = std::fs::remove_dir_all(&root);
}

/// Production reaches every spawn of a run through **one** `HostRunner`.
///
/// The memo is per boundary, so "the probe and the attempts agree" is only
/// true while the probe and the attempts share a runner. Nothing in the
/// type system says so: `run_harness_on` takes `&dyn Runner`, and an engine
/// that constructed one per attempt would leave the memo correct, the suite
/// green, and DESIGN.md:612 reopened. This is the census that fails first.
///
/// Structural rather than behavioural, for
/// `the_adapters_hold_no_process_wide_resolution_state`'s reason: a runner
/// constructed per attempt is indistinguishable from one constructed per run
/// in every observation except how many times the filesystem was asked, and
/// that observation belongs to a fixture that would have to drive a whole
/// engine run against a moving filesystem.
///
/// The expectation is written out — two construction sites, both in
/// `src/engine/mod.rs`, being `run_harness` and `resume_harness` — rather
/// than counted from the tree, because a count read from the tree grows
/// with it.
#[test]
fn production_reaches_a_spawn_through_one_host_runner_per_run() {
    /// Where a `HostRunner` is constructed in the engine's production code,
    /// and how many times in each file.
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

    let mut counted: Vec<(&str, usize)> = Vec::new();
    let mut stripped = 0_usize;
    for (name, source) in sources {
        let production = crate::effects::production_region(source);
        let kept: Vec<&str> = production
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        stripped += production.lines().count() - kept.len();
        counted.push((name, kept.join("\n").matches("HostRunner::new(").count()));
    }
    assert!(
        stripped > 100,
        "the comment strip removed {stripped} lines, so this census is reading prose"
    );
    // The control: the pattern matches when it is present, so a zero means
    // absence rather than a broken search.
    assert_eq!(
        "let r = HostRunner::new();"
            .matches("HostRunner::new(")
            .count(),
        1,
        "the census pattern matches nothing at all"
    );
    assert_eq!(
        counted,
        SITES.to_vec(),
        "the engine constructs its host runner somewhere this repair did not account for. \
         The memo behind `program_searches` is per runner, so a runner per attempt is a \
         resolution per attempt and DESIGN.md:612 is open again"
    );
    // And the two are the run and the resume facade, each of which then
    // borrows that one runner for pre-flight and every attempt.
    let engine = crate::effects::production_region(include_str!("../../engine/mod.rs"));
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

/// **`PR6-LANED-002` / refuted claim 3.** An **npm-style** installation of
/// each of the three agent CLIs runs by bare name exactly as it runs by
/// path.
///
/// The equivalence `agent::built_program_tests::the_host_runner_executes_a_
/// bare_program_name_as_it_executes_the_resolved_path` claims, over the
/// installation shape that one cannot express. That row uses `git` — a
/// native `.exe`, which `CreateProcessW` reaches from a bare name whether or
/// not this runner resolves anything — so it was green on Windows while
/// `PR6D-001` was live. The three agent CLIs are not installed that way:
/// `npm install -g` writes `claude.cmd`, `codex.cmd`, `copilot.cmd`, and a
/// `.cmd` is reachable only through `PATHEXT`. A fixture that cannot hold
/// the failing installation is a correlated fixture, whatever it asserts.
///
/// **All three names, because the behaviour that was dropped was not one
/// adapter's.** `PR6D-CODEX-STORE-ALIAS-WALK-DROPPED` records the deletion
/// as codex's Windows-Store-alias walk; the same deletion took `.cmd`/`.bat`
/// selection and `PATHEXT` away from `claude` and `copilot`, where a plain
/// npm install fails with no Store alias and no competing `PATH` entry in
/// sight. So the grid is the three names, each with its own marker, and the
/// markers are counted — a rule that special-cased one name collapses the
/// count.
///
/// The installation is `<name>.cmd` on Windows and an extensionless script
/// on Unix, with **no `.exe` beside it**, on a `PATH` this test wrote so
/// that nothing installed on the machine can satisfy it. Both spellings —
/// the bare name, and the absolute path of the file it must resolve to — go
/// through the production route, `HostRunner::run`, and must produce the
/// same output; the two program strings are asserted to differ first, or
/// this would compare a thing with itself.
///
/// It runs on **both** platforms. On Unix the property holds because
/// `execvp` would have satisfied it anyway, which is precisely why the
/// Windows arm needs a fixture that can fail rather than a `#[cfg(windows)]`
/// afterthought; D1's `a_bare_name_that_only_pathext_resolves_…` holds the
/// `NotFound` platform fact on the guest, and this holds the equivalence
/// everywhere.
///
/// What varies: the CLI name and the spelling of the program. Held constant:
/// the runner, the environment, the argument and the files on disk — so a
/// difference in output can only be a difference in which file ran.
#[test]
fn an_npm_style_installation_runs_by_bare_name_exactly_as_it_runs_by_path() {
    // The three names `bin::Invocation::named` ships, written here rather
    // than read from the adapters' private `CLI` constants;
    // `agent::built_program_tests::an_adapters_program_is_the_boundarys_…`
    // is what ties each adapter to its own name.
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

    // The installation really is the failing shape: one file per CLI, and
    // on Windows not one of them is an `.exe`.
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

// -----------------------------------------------------------------------
// PR6-LANED-003: the workspace is not a PATH directory
// -----------------------------------------------------------------------

/// A `PATH` entry, and whether each platform calls it a location on its
/// own.
///
/// Written from the two platforms' rules rather than read from
/// `Path::is_absolute`, and then checked against it for the platform this
/// is running on — the Windows column on the guest, the Unix column here.
/// Every entry is free of both `PATH` separators, so one entry stays one
/// entry under `std::env::split_paths` on either platform.
const PATH_ENTRY_TABLE: &[(&str, bool, bool)] = &[
    // (entry, is a location under Unix, is a location under Windows)
    //
    // The empty entry: `PR6-LANED-003` itself. POSIX gives a null prefix
    // the meaning "the current directory".
    ("", false, false),
    (".", false, false),
    ("..", false, false),
    ("bin", false, false),
    ("./bin", false, false),
    // Rooted on Unix; on Windows a leading separator is relative to the
    // *current drive*, so it is still a current-directory question.
    ("/usr/local/bin", true, false),
    (r"\Windows\System32", false, false),
    // A UNC share names a location on Windows and is an ordinary file name
    // on Unix.
    (r"\\server\share\bin", false, true),
    ("~/bin", false, false),
];

/// Every `PATH` entry this runner searches names a location on its own.
///
/// **`PR6-LANED-003`**, as a rule rather than as one vector. A `PATH` entry
/// that is not absolute is resolved against *a* current directory, and this
/// boundary has two of them: the coordinator's, which is what
/// `ProgramNaming::is_program` inspects, and the workspace, which is what
/// the child actually runs in. So such an entry does not merely widen the
/// search — it lets the runner certify one file and execute another, and
/// the file it executes is repository content under automation
/// (DESIGN.md:398-402).
///
/// The written table is checked against `Path::is_absolute` first, so the
/// row below is a claim about `resolve_program` and not about `std`. What
/// varies is the entry; what is held constant is the program name, the
/// candidates and the composed environment.
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

    // And "searched" means searched: the one kind of entry that is not
    // skipped does find a program in it.
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

/// **`PR6-LANED-003`.** An empty `PATH` entry never reaches the
/// workspace's own copy of a bare name.
///
/// The finding's vector, executed. A coordinator whose `PATH` holds an
/// empty segment — `:/usr/bin`, which is what a shell profile that appends
/// to an unset `PATH` produces — and a request workspace containing an
/// executable called `claude`. POSIX gives the null prefix the meaning "the
/// current directory", and this runner's current directory *is* the
/// workspace, so a bare name handed to `Command` runs repository content
/// with the coordinator's authority as the agent (DESIGN.md:398-402).
///
/// **The platform fact is executed, not cited**: every row spawns the same
/// bare name through `std::process::Command` under the same composed
/// environment and the same working directory first, and the outcome is
/// compared against a written expectation. Three of the four rows are ones
/// where the raw spawn reaches the workspace and the runner must not, and
/// that count is asserted — a fixture in which the two agree everywhere
/// proves nothing and says so.
///
/// Unix only, and deliberately: the empty-entry-means-here rule is POSIX's,
/// and the fixture's installations are shell scripts. The rule that closes
/// it is not Unix-only —
/// `every_path_entry_this_runner_searches_names_a_location_on_its_own`
/// executes it on both platforms.
///
/// What varies: where the empty entry sits, and whether a real installation
/// is on the `PATH` at all. Held constant: the workspace, its planted
/// executable, the name and the argument.
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
    // Repository content, under automation: the workspace's own copy.
    marker_shim(&workspace, &file, "WORKSPACE");
    marker_shim(&installed_dir, &file, "INSTALLED");

    let installed = installed_dir.to_string_lossy().into_owned();
    let empty = empty_dir.to_string_lossy().into_owned();
    // (what, PATH, what a raw spawn reaches, what the runner must reach)
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

        // The platform fact. `execvp` searches the child's PATH from the
        // child's working directory, so the empty entry is the workspace.
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

        // The claim, through the boundary, with an observer so that a
        // refusal is a refusal *before* any spawn rather than a failed one.
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

/// A relative `PATH` entry that really does name a directory is refused
/// rather than searched.
///
/// The empty entry of `PR6-LANED-003` is the degenerate case of this one,
/// and this is the case where the two current directories the boundary has
/// are visibly different: the entry resolves from the **coordinator's**
/// working directory — asserted here, so the row is hostile — while the
/// child would resolve it from the **workspace**. A runner that searched it
/// would hand `Command` a relative program and certify a file that is not
/// the one that runs.
///
/// The entry is built out of `..` back to the root and down again, so it is
/// genuinely relative and genuinely resolvable without anything being
/// written inside the repository. Unix only: the same construction on
/// Windows depends on the temporary directory and the working directory
/// sharing a drive, and a row that silently stops applying is worse than
/// one that is not there.
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
    // Deeper than the coordinator's own directory, so that the same
    // relative entry names a *different* place from each of them — which is
    // the whole hazard, and a workspace that happened to sit at the same
    // depth would hide it.
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

    // The row's own premise, both halves.
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

    // The oracle: the same directory, named as a location, does run.
    let reachable = HostRunner::new()
        .with_environment(environment_on_path(&[&bin], Some(REAL_PATHEXT)))
        .run(&named_request(&name, "arg", &workspace))
        .expect("the same installation, named absolutely");
    assert_eq!(reachable.stdout.trim(), "RELATIVE:arg");
    assert!(absolute.is_absolute());
    let _ = std::fs::remove_dir_all(&root);
}
