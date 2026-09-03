//! `host-v1`'s environment contract: the platform's name rule, and the
//! composition of base, reserved values and overlay (DESIGN.md:258-264).
//!
//! The vocabulary this composes over -- `RESERVED_ALWAYS`,
//! `CREDENTIAL_LOCATIONS`, [`reserved_keys`] and [`supplies_credentials`] --
//! stays in the parent, which is "the one list" `runner::container::env` reads.
//! What lives here is the rule that decides *which* variables a request gets
//! and in what order, and it performs no effect: the base is handed in.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree and not by the file, so an out-of-line
// child of `src/runner/host.rs` would otherwise inherit that file's inner
// `#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`
// -- `PR6-LANEF-004`, and the mistake two W1 pull requests each made
// independently. Nothing here reaches a governed primitive, so all three are
// DENIED rather than allowed, and this module takes no `effects/allowlist.toml`
// row: an allowance is what that file records, and this module takes none.
// `runner::container::tests::every_child_module_of_the_container_funnel_states_\
// its_own_lint_level` already walks `src/runner/host/`, so this file was graded
// against all three from its first commit.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::ffi::{OsStr, OsString};

use crate::error::UpstrokeError;
use crate::runner::{AgentId, ExecutionRole};

use super::{RESERVED_ALWAYS, credential_location, reserved_keys, supplies_credentials};

/// How the platform compares environment variable names.
///
/// A type rather than a `cfg!` at each comparison. `cfg!(windows)` is false on
/// a Linux developer box and on the Linux CI cell, so a rule written as a
/// `cfg!` is a rule whose Windows arm no test on those machines can reach —
/// both sides of the pin move together. [`Self::ALL`] is what the grids run
/// over; [`Self::current`] is what production selects. The same shape
/// [`crate::topology::effects::Host`] uses, for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyCase {
    /// Unix: `Path` and `PATH` are two variables.
    Sensitive,
    /// Windows: `Path` and `PATH` are one variable, and a child that received
    /// both would receive whichever the block happened to list last.
    Insensitive,
}

impl KeyCase {
    /// Both rules. Every grid runs over this, not over [`Self::current`].
    pub const ALL: &'static [Self] = &[Self::Sensitive, Self::Insensitive];

    /// The rule this machine's process environment obeys.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Insensitive
        } else {
            Self::Sensitive
        }
    }

    /// Whether these two names are the same variable under this rule.
    #[must_use]
    pub fn same_key(self, left: &OsStr, right: &OsStr) -> bool {
        match self {
            Self::Sensitive => left == right,
            Self::Insensitive => left
                .to_string_lossy()
                .eq_ignore_ascii_case(&right.to_string_lossy()),
        }
    }
}

/// `host-v1`'s environment contract.
///
/// Holds its base explicitly so a test can compose against a base it wrote
/// rather than against whatever variables happen to be set on the machine
/// running the suite.
#[derive(Debug, Clone)]
pub struct HostEnvironment {
    base: Vec<(OsString, OsString)>,
    case: KeyCase,
}

impl HostEnvironment {
    /// The Upstroke process environment, under this platform's name rule.
    #[must_use]
    pub fn from_process() -> Self {
        Self {
            base: std::env::vars_os().collect(),
            case: KeyCase::current(),
        }
    }

    /// An explicit base, for grids that must cover both name rules.
    #[must_use]
    pub fn with_base(base: Vec<(OsString, OsString)>, case: KeyCase) -> Self {
        Self { base, case }
    }

    /// The base this runner composes from.
    #[must_use]
    pub fn base(&self) -> &[(OsString, OsString)] {
        &self.base
    }

    /// The name rule in force.
    #[must_use]
    pub const fn case(&self) -> KeyCase {
        self.case
    }

    /// The reserved values the runner supplies for this request.
    ///
    /// A reserved key the base does not carry is **not** supplied: setting an
    /// absent variable to the empty string is a different environment from not
    /// setting it, and several CLIs read "set but empty" as an instruction.
    ///
    /// DESIGN.md:259-262 — "the host runner starts from the Upstroke environment
    /// and the container runner from the image environment; **each** supplies
    /// role-scoped `HOME`, `PATH`, and credential locations" — resolved for
    /// `host-v1` as follows, and the split is deliberate:
    ///
    /// * **credential locations are role-scoped**, by
    ///   [`supplies_credentials`]. A gate is repository-controlled code and the
    ///   shell probe is a shell; neither runs an agent CLI, so neither is told
    ///   where an agent's credentials live, whatever agent the request happens
    ///   to name. This is the sentence's own word "role-scoped" doing work.
    /// * **`HOME`, `PATH` and `USERPROFILE` are supplied to every role at the
    ///   host boundary's own value.** That is a boundary, and "one machine,
    ///   one user" is a rationale rather than a basis for it, so it is drawn
    ///   from live passages — three of them, each forbidding a different part
    ///   of a per-role value:
    ///
    ///   1. DESIGN.md:263 — "Probe and execution compose the **same** base,
    ///      mounts, reserved values, and overlay, so pre-flight certifies the
    ///      environment that will actually spend." `probe(<agent>)`,
    ///      `implement` and `review` are the probe and the execution that
    ///      sentence pairs; a `HOME` differing across them would make
    ///      pre-flight certify an environment the attempt never runs in.
    ///   2. `decisions/2026-08-12-merge-queue-execution-topology.md:331-333` —
    ///      "gate-shell/program availability is checked inside the same
    ///      boundary." The shell probe certifies the shell a gate will run; a
    ///      `PATH` differing between `probe(shell)` and `gate` would certify a
    ///      different program from the one that runs.
    ///   3. The same decision, :341-342 — "Host runner behavior remains
    ///      available and honestly provides **no OS boundary** around gate
    ///      code." Handing gate code a different `HOME` on this host would
    ///      assert an isolation the host does not have: repository-controlled
    ///      code reads the real home directory by absolute path either way.
    ///      What the host *can* honestly do is not disclose a location it
    ///      would otherwise hand over, and that is [`supplies_credentials`].
    ///
    ///   The value comes from the base rather than from anything this runner
    ///   invents, because the same decision says where the base is (:321-322):
    ///   "**The host base starts from the Upstroke process environment**, while
    ///   the container base starts from the image environment." A process
    ///   environment carries one value per key under [`KeyCase`] — so one
    ///   value is what a correct `host-v1` *produces*, not a narrowing this
    ///   slice chose. The container runner differs not because its `HOME`
    ///   string differs per role but because each role's container is its own
    ///   filesystem; PR4's `production_effect` is "same behavior plus stronger
    ///   Windows crash containment", and no passage describes a per-role home
    ///   directory on the host for it to grow into.
    ///
    ///   Asserted from those passages, not commented, by
    ///   `the_reserved_values_every_role_gets_are_the_host_boundarys_own` — so
    ///   a `host-v1` that ever does scope `HOME` has to change a passage
    ///   first, rather than a count.
    ///
    /// A reserved key the base does not carry is **not** supplied: setting an
    /// absent variable to the empty string is a different environment from not
    /// setting it, and several CLIs read "set but empty" as an instruction.
    #[must_use]
    pub fn reserved_values(
        &self,
        role: &ExecutionRole,
        agent: Option<&AgentId>,
    ) -> Vec<(&'static str, OsString)> {
        let mut supplied = Vec::new();
        for key in RESERVED_ALWAYS {
            if let Some(value) = self.lookup(key) {
                supplied.push((*key, value));
            }
        }
        if supplies_credentials(role) {
            if let Some(key) = agent.and_then(credential_location) {
                if let Some(value) = self.lookup(key) {
                    supplied.push((key, value));
                }
            }
        }
        supplied
    }

    /// Base, then reserved values, then overlay — DESIGN.md:263's own order
    /// ("the same base, mounts, reserved values, and overlay").
    ///
    /// The base's own copies of the **reserved** keys are dropped before the
    /// runner supplies them, and that is what makes "role-scoped" a property
    /// of the child's environment rather than of a vector nothing reads.
    /// Cloning the base and then upserting would leave every credential
    /// location the Upstroke process happens to carry in a gate's environment —
    /// a gate is repository-controlled code, and `CODEX_HOME` reaching it is
    /// exactly the thing [`supplies_credentials`] exists to prevent. It would
    /// also make this step *output-equivalent to deleting it*, because
    /// [`Self::reserved_values`] reads the values back out of the same base.
    /// So the reserved keys arrive from one place — this function's supply
    /// step, which is role-scoped — or not at all.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] naming the key when the overlay names a
    /// reserved one. That is the contract's `expected_failures_refusals[0]`,
    /// "reserved env conflict -> pre-flight error", and it is refused by
    /// **key**: `invariants_introduced[0]` says "reserved keys refused
    /// pre-flight", and an overlay permitted to restate `PATH` today because
    /// the value happens to match is an overlay that breaks silently the day
    /// the runner's value changes.
    pub fn compose(
        &self,
        role: &ExecutionRole,
        agent: Option<&AgentId>,
        overlay: &[(String, String)],
    ) -> Result<Vec<(OsString, OsString)>, UpstrokeError> {
        self.preflight(overlay)?;
        let mut composed = self.base.clone();
        for reserved in reserved_keys() {
            composed.retain(|(name, _)| !self.case.same_key(name, OsStr::new(reserved)));
        }
        for (key, value) in self.reserved_values(role, agent) {
            upsert(&mut composed, self.case, OsString::from(key), value);
        }
        for (key, value) in overlay {
            upsert(
                &mut composed,
                self.case,
                OsString::from(key),
                OsString::from(value),
            );
        }
        Ok(composed)
    }

    /// The reserved-key refusal on its own, so a caller can certify an overlay
    /// without building an environment.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] naming the offending key and the reserved key
    /// it collides with.
    pub fn preflight(&self, overlay: &[(String, String)]) -> Result<(), UpstrokeError> {
        for (key, _) in overlay {
            if let Some(reserved) = reserved_keys()
                .into_iter()
                .find(|reserved| self.case.same_key(OsStr::new(key), OsStr::new(reserved)))
            {
                return Err(UpstrokeError::Refused {
                    message: format!(
                        "the command overlay sets `{key}`, which is reserved by the host runner \
                         (`{reserved}`). An adapter may select a profile or change CLI behaviour, \
                         but the runner owns the environment the process executes in \
                         (DESIGN.md:258-264)"
                    ),
                });
            }
        }
        Ok(())
    }

    pub(super) fn lookup(&self, key: &str) -> Option<OsString> {
        self.base
            .iter()
            .find(|(name, _)| self.case.same_key(name, OsStr::new(key)))
            .map(|(_, value)| value.clone())
    }
}

fn upsert(into: &mut Vec<(OsString, OsString)>, case: KeyCase, key: OsString, value: OsString) {
    if let Some(slot) = into.iter_mut().find(|(name, _)| case.same_key(name, &key)) {
        slot.1 = value;
        return;
    }
    into.push((key, value));
}
