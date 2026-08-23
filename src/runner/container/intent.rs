//! The global container intent — its six fields, its name, and its five
//! labels.
//!
//! `decisions.admission_and_leases.permits.crash_reconstruction`, which is the
//! authority for every sentence in this module:
//!
//! > every container invocation writes a synced intent in the global namespace
//! > `<R>/containers/<container-name>.intent` (R = the run's private root, the
//! > one recorded in `run_started.private_dir`) recording owner run id, run
//! > directory (public path), coordinator incarnation id, repo key, invocation
//! > id, and `runner_policy_sha256`; the coordinator incarnation id is a
//! > per-process ULID recorded in `run_started(4)`/`run_resumed(4)` and is never
//! > read from lock-file contents …; the container name is
//! > `tactus-<repo_key>-<run_id>-<incarnation>-<invocation-hash>`, so
//! > deterministic `InvocationId`s never collide across incarnations and no
//! > earlier ownership evidence is overwritten; labels `tactus.private_root`,
//! > `tactus.run`, `tactus.run_dir`, `tactus.incarnation`, `tactus.invocation`
//!
//! Nothing here performs an effect. The write and the removal are funnel APIs
//! in `src/runner/container.rs`, under `ContainerSite::WriteIntent` and
//! `ContainerSite::RemoveIntent`.
//!
//! ## The one thing here that is not data: [`IntentWritten`]
//!
//! `expected_failures_refusals[6]` is "container start without an intent is
//! **impossible by construction**". Before repair R1 it was impossible by
//! nobody having written it: `create_container` and `start_container` were
//! public, took a bare [`ContainerName`], performed no inspection, and a
//! `start_existing(name)` added tomorrow would have compiled
//! (`PR6-CORRECTNESS-012` / `PR6-ENUM-001`, and catalogue survivor
//! `PR6-INTENT-020` reached the same clause from the other side).
//!
//! [`IntentWritten`] is the capability, in the idiom `PR4-CONF-003` established
//! for `Contained`: the two funnel APIs that create and start a container take
//! one, and there are exactly two ways to obtain it — `container::write_intent`,
//! which writes the record, and [`IntentWritten::certify`], which **reads the
//! published record back and parses it**. A private field alone would not do
//! it: Rust privacy makes a private item of `runner::container` visible to
//! every child module of `runner::container`, which is where the lanes are, so
//! the proof is grounded in the filesystem instead of in visibility. Forging
//! one therefore requires writing a well-formed intent record at
//! `<R>/containers/<name>.intent` — which is writing the intent.

// `PR6-LANEF-004`: the Container funnel's module-level allow is an INNER
// attribute, and a Rust lint level is scoped by the MODULE TREE rather than by
// the file, so every out-of-line child of `runner::container` inherited it --
// measured, a `ContainerRuntime::start` planted in a child module passed
// `cargo clippy --all-targets --all-features -- -D warnings`. Re-denying here
// is what makes `decisions.effect_site_inventory.mechanism` (1)'s BUILD error
// true of a lane's module, which is the leg the source census cannot supply.
// Enforced for every file in this directory by `runner::container::tests::
// every_child_module_of_the_container_funnel_states_its_own_lint_level`.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::TactusError;
use crate::runner::InvocationId;

/// The directory of the global namespace, under the run's recorded private
/// root.
pub const CONTAINERS_DIR: &str = "containers";

/// The suffix of one intent record.
pub const INTENT_SUFFIX: &str = ".intent";

/// The staging suffix. The record is published by rename, like every other
/// durable record this engine writes.
pub const INTENT_STAGED_SUFFIX: &str = ".intent.tmp";

/// The name's fixed prefix.
pub const NAME_PREFIX: &str = "tactus";

/// The separator between the name's components.
///
/// A component may not contain it — [`validate_component`] refuses one that
/// does — which is what makes [`ContainerName::parse`] injective.
pub const NAME_SEPARATOR: char = '-';

/// The domain tag of the invocation hash.
///
/// Domain-separated so the same bytes hashed for another purpose are a
/// different value, in the idiom of `workspace_manager::repo_key_v1` and
/// `runner::policy::CANONICAL_VERSION`.
pub const INVOCATION_HASH_DOMAIN: &str = "tactus.container-invocation.v1";

/// How many hex characters of the invocation digest the name carries.
///
/// Named for the character count it produces, which is this project's
/// convention (`workspace_manager`'s `REPO_KEY_HEX_CHARS` says so in as many
/// words).
pub const INVOCATION_HASH_HEX_CHARS: usize = 16;

/// The longest a single name component may be.
///
/// A run id and an incarnation are 26-character ULIDs and a repo key is 16 hex
/// characters, so this is slack rather than a constraint on anything the engine
/// produces — it exists so a hostile value cannot push the whole name past what
/// a container runtime accepts.
pub const MAX_COMPONENT_LEN: usize = 64;

/// The longest whole name.
///
/// Docker's own limit is far higher; the engine's own longest name is
/// `tactus`(6) + 4 separators + 16 + 26 + 26 + 16 = 94.
pub const MAX_NAME_LEN: usize = 200;

// ---------------------------------------------------------------------------
// The five labels
// ---------------------------------------------------------------------------

/// `tactus.private_root` — the canonical path of `<R>`. Discovery is `docker
/// ps` by this label.
pub const LABEL_PRIVATE_ROOT: &str = "tactus.private_root";
/// `tactus.run` — the owner run id.
pub const LABEL_RUN: &str = "tactus.run";
/// `tactus.run_dir` — the owner's **public** run directory.
pub const LABEL_RUN_DIR: &str = "tactus.run_dir";
/// `tactus.incarnation` — the owning coordinator incarnation.
pub const LABEL_INCARNATION: &str = "tactus.incarnation";
/// `tactus.invocation` — the rendered [`InvocationId`], in full.
pub const LABEL_INVOCATION: &str = "tactus.invocation";

/// The five labels, in the packet's order.
///
/// Written out rather than derived from the map a container carries, so a
/// label dropped from the map is a disagreement with this list rather than a
/// shorter map nobody compares to anything.
pub const LABELS: &[&str] = &[
    LABEL_PRIVATE_ROOT,
    LABEL_RUN,
    LABEL_RUN_DIR,
    LABEL_INCARNATION,
    LABEL_INVOCATION,
];

/// The bytes [`path_label`] passes through unescaped.
///
/// `/` is here because it is a path separator on both platforms and keeping it
/// literal is what makes a label readable; `:` because a Windows drive letter
/// is not ambiguous with anything. Nothing else structural survives: `%` is the
/// escape and must itself escape, and `,`, `=` and the line terminators are the
/// bytes that would end a `docker --filter label=…` argument or start another
/// one, so a root carrying them cannot widen a filter.
const LABEL_UNRESERVED: &[u8] = b"/:.-_";

/// The hex digits an escape is written with, and the only ones
/// [`decode_path_label`] accepts.
const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// A path as a label value: percent-encoded, injective, and ASCII.
///
/// **Injective, and two things in this module have to be.** [`ContainerName`]
/// was designed for injectivity — its components are `[0-9A-Za-z_]` only, so
/// the parse on `-` is unambiguous — because `crash_reconstruction` says
/// "different private roots are **disjoint worlds**". A label is the other half
/// of that sentence: `tactus.private_root` is the value
/// `docker ps --filter label=tactus.private_root=…` selects on, so two distinct
/// roots that render to one label are one world, and a census authorized for
/// either queries — and reclaims — the containers of the other.
///
/// **`tactus.run_dir` is the same function for a sharper reason.** It is the
/// owner's public run directory, and arm (ii) of the liveness rule *probes that
/// directory's `run.lock`*: "free -> dead owner -> reclaim; held -> live owner
/// -> never touched". A rendering that maps two directories onto one string
/// sends the probe to a **different, real** directory, finds no lock there, and
/// classifies a **live** owner as dead — which kills a running coordinator's
/// container. `PR6-RECOV-001` is that entry.
///
/// The rendering that shipped was `to_string_lossy().replace('\\', "/")`, which
/// collides two ways. `<R>/a\b` and `<R>/a/b` are **different directories on
/// Unix**, where a backslash is an ordinary filename byte, and they rendered to
/// one label; and `to_string_lossy` maps every ill-formed byte sequence to
/// `U+FFFD`, so two distinct non-UTF-8 roots rendered to one label as well.
/// Substituting one ambiguity for another — rewriting `:` as well, say — keeps
/// every existing fixture green, which is why the property asserted is a
/// **colliding pair**, not a round trip.
///
/// So: percent-encode the path's own bytes. Every byte outside
/// [`LABEL_UNRESERVED`] and the ASCII alphanumerics becomes `%XX`, upper-case
/// hex, which is injective because `%` is itself escaped and every escape is a
/// fixed three bytes. [`decode_path_label`] is the inverse and exists: an
/// encoding the census cannot undo would be injective and useless, because the
/// probe needs the *path* back.
///
/// **The one byte that is platform-shaped is `\`, and it is not an exception to
/// injectivity.** On Windows `\` and `/` are both path separators and `<R>\a`
/// and `<R>/a` name the same directory, so rendering `\` as `/` maps *equal*
/// roots to one label — canonicalization, which injectivity over paths asks
/// for. On Unix `\` names a different directory and is escaped like any other
/// byte. The `cfg` and this sentence are asserted against each other by
/// `census::tests::the_private_root_label_is_injective_over_hostile_roots`.
#[must_use]
pub fn path_label(path: &Path) -> String {
    let bytes = path.as_os_str().as_encoded_bytes();
    let mut label = String::with_capacity(bytes.len());
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || LABEL_UNRESERVED.contains(&byte) {
            label.push(char::from(byte));
        } else if byte == b'\\' && cfg!(windows) {
            label.push('/');
        } else {
            label.push('%');
            label.push(char::from(HEX[usize::from(byte >> 4)]));
            label.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    label
}

/// The value of the `tactus.private_root` label for a root.
///
/// A name for one use of [`path_label`], kept because the private root is the
/// value a `docker ps --filter` argument is built from and the call sites read
/// better for saying so.
#[must_use]
pub fn private_root_label(private_root: &Path) -> String {
    path_label(private_root)
}

/// [`path_label`]'s inverse: the path a label value was encoded from.
///
/// **Fail-closed, and deliberately strict.** A value this function cannot
/// decode is not a value any funnel wrote, and the census's answer to evidence
/// it cannot read is to refuse and block admission — never to guess a path and
/// then probe a lock there. `%` followed by anything but two upper-case hex
/// digits is the only malformed shape [`path_label`] cannot produce, so it is
/// the only one refused.
///
/// On Unix an `OsStr` is bytes and the decode is exact. On Windows an `OsStr`
/// is WTF-8 and only its UTF-8 subset can be rebuilt safely — the alternative
/// is `OsStr::from_encoded_bytes_unchecked`, whose contract a hostile label
/// value cannot be trusted to meet. A Windows path outside UTF-8 is an unpaired
/// surrogate in a file name; refusing it is the fail-closed side of that trade
/// and is stated here rather than discovered.
///
/// # Errors
///
/// [`TactusError::Refused`] when `value` carries a malformed escape, or when
/// the decoded bytes are not a path this platform can name.
pub fn decode_path_label(value: &str) -> Result<PathBuf, TactusError> {
    let refuse = |why: &str| TactusError::Refused {
        message: format!(
            "the label value `{value}` is not a tactus path label ({why}); a path label is its \
             bytes with everything outside `[0-9A-Za-z]` and `{}` percent-encoded, and a value \
             this engine could not have written is not evidence a census may probe a lock from",
            String::from_utf8_lossy(LABEL_UNRESERVED)
        ),
    };
    let raw = value.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        if raw[index] != b'%' {
            bytes.push(raw[index]);
            index += 1;
            continue;
        }
        let (Some(high), Some(low)) = (raw.get(index + 1), raw.get(index + 2)) else {
            return Err(refuse("a `%` with fewer than two digits after it"));
        };
        let (Some(high), Some(low)) = (
            HEX.iter().position(|digit| digit == high),
            HEX.iter().position(|digit| digit == low),
        ) else {
            return Err(refuse("a `%` not followed by two upper-case hex digits"));
        };
        // `as u8` cannot truncate: both indices are positions in a 16-byte
        // table, so the value is `0..=255` by construction.
        bytes.push(((high << 4) | low) as u8);
        index += 3;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
    }
    #[cfg(not(unix))]
    {
        match String::from_utf8(bytes) {
            Ok(text) => Ok(PathBuf::from(text)),
            Err(_) => Err(refuse("bytes that are not valid UTF-8 on this platform")),
        }
    }
}

// ---------------------------------------------------------------------------
// The record
// ---------------------------------------------------------------------------

/// `<R>/containers/<name>.intent` — the six fields, in the packet's order.
///
/// `deny_unknown_fields` for the same reason [`crate::rundir::CreatingMarker`]
/// carries it: a record that grew a seventh field somewhere else is a record
/// this process did not write, and reading it as if it had is how one engine
/// adopts another's ownership evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerIntent {
    /// Owner run id.
    pub run_id: String,
    /// Run directory — the **public** path, canonical, as a [`path_label`].
    ///
    /// **Encoded, and the record and the label carry one spelling.** The
    /// `tactus.run_dir` label is this field verbatim, so a second rendering
    /// would be a second thing to keep in step; and the census reaches the
    /// owner's `run.lock` through [`Self::run_dir_path`], which is
    /// [`decode_path_label`] and the rooted check in one place. Build the
    /// record with [`Self::new`] rather than filling this in by hand — a raw
    /// path whose bytes are all unreserved is its own encoding and survives
    /// either way, but one carrying a `%` does not.
    pub run_dir: String,
    /// Coordinator incarnation id: a per-process ULID, never read from a lock
    /// file.
    pub incarnation: String,
    /// Repo key.
    pub repo_key: String,
    /// The rendered [`InvocationId`], in full. The name carries a 16-character
    /// digest of it; this field carries the value, so ownership evidence is
    /// exact rather than collision-resistant.
    pub invocation: String,
    /// `runner_policy_sha256` — the digest of the run's `RunnerPolicy`, so
    /// "the census report names each reclaimed container's boundary from its
    /// `runner_policy_sha256`" can be answered from the record alone.
    pub runner_policy_sha256: String,
}

impl ContainerIntent {
    /// The six fields, with the run directory encoded on the way in.
    ///
    /// The one construction site production code uses, so "the record's run
    /// directory is a [`path_label`]" is true by construction rather than by
    /// every caller remembering.
    #[must_use]
    pub fn new(
        run_id: String,
        run_dir: &Path,
        incarnation: String,
        repo_key: String,
        invocation: String,
        runner_policy_sha256: String,
    ) -> Self {
        Self {
            run_id,
            run_dir: path_label(run_dir),
            incarnation,
            repo_key,
            invocation,
            runner_policy_sha256,
        }
    }

    /// The owner's public run directory, decoded and checked.
    ///
    /// # Errors
    ///
    /// As [`owner_run_dir`].
    pub fn run_dir_path(&self) -> Result<PathBuf, TactusError> {
        owner_run_dir(&self.run_dir, "intent record")
    }

    /// The five labels this intent's container carries, given the private root
    /// its record lives under.
    ///
    /// The labels are derived from the record rather than passed beside it, so
    /// a container whose labels and whose intent disagree is not constructible
    /// through this API — `labeled_orphan_without_intent_reclaimed` is about a
    /// container with **no** record, which is a different thing.
    ///
    /// `tactus.private_root` is the one label with no field of its own: the
    /// record's *location* is inside `<R>`, so the root is what the census
    /// already knows when it reads one.
    #[must_use]
    pub fn labels(&self, private_root: &Path) -> BTreeMap<String, String> {
        let mut labels = BTreeMap::new();
        labels.insert(
            LABEL_PRIVATE_ROOT.to_owned(),
            private_root_label(private_root),
        );
        labels.insert(LABEL_RUN.to_owned(), self.run_id.clone());
        labels.insert(LABEL_RUN_DIR.to_owned(), self.run_dir.clone());
        labels.insert(LABEL_INCARNATION.to_owned(), self.incarnation.clone());
        labels.insert(LABEL_INVOCATION.to_owned(), self.invocation.clone());
        labels
    }
}

/// The owner's public run directory, from a record field or a label value.
///
/// **The census probes `<run_dir>/run.lock` and acts on the answer**, so this
/// is the function that decides which lock the question "is that owner alive?"
/// is asked about. Two shapes must never reach it as a path:
///
/// * the **empty** value, which joins to `run.lock` — a path relative to
///   whatever directory this process happens to be in, where there is no lock,
///   so a live owner reads as dead and its running container is killed
///   (`PR6-CORRECTNESS-016`);
/// * any other **relative** value, for the same reason with more steps.
///
/// The predicate is [`Path::has_root`] and not `is_absolute`, deliberately. On
/// Windows `is_absolute` additionally requires a prefix, so `/srv/…` — the
/// shape every Unix-written record carries and every cross-platform fixture
/// uses — is *not* absolute there, and the check would refuse on one platform
/// what it accepts on the other. `has_root` is the property that actually
/// matters here: a rooted path does not depend on the process's working
/// directory. A drive-relative `C:dir` has no root and is refused.
///
/// Refusing is what `expected_failures_refusals[8]` asks for — "an unreclaimable
/// labeled container blocks admission" — and it is the fail-closed side: the
/// alternative to refusing unownable evidence is probing *something*, and every
/// wrong probe answers "free", which reclaims.
///
/// # Errors
///
/// [`TactusError::Refused`] when `value` does not decode, is empty, or is not
/// rooted.
pub fn owner_run_dir(value: &str, source: &str) -> Result<PathBuf, TactusError> {
    if value.is_empty() {
        return Err(TactusError::Refused {
            message: format!(
                "the {source} carries an empty `{LABEL_RUN_DIR}`; the liveness rule probes \
                 `<run_dir>/run.lock` non-blocking and an empty owner directory would probe \
                 `run.lock` relative to this process's working directory, find no lock, and \
                 classify a live owner as dead. Evidence that does not say where its owner's \
                 lock is cannot be reclaimed under the rule, and an unreclaimable labeled \
                 container blocks admission"
            ),
        });
    }
    // Wrapped, not propagated: a census refusal that did not name the field
    // would leave an operator reading a decoder's complaint with no way to know
    // which piece of evidence carried it.
    let path = decode_path_label(value).map_err(|error| TactusError::Refused {
        message: format!("the {source}'s `{LABEL_RUN_DIR}` is unreadable: {error}"),
    })?;
    if !path.has_root() {
        return Err(TactusError::Refused {
            message: format!(
                "the {source} carries `{LABEL_RUN_DIR}={value}`, which is a relative path; the \
                 owner's run directory is the **public** path and the liveness rule probes \
                 `<run_dir>/run.lock` from it, so a value resolved against this process's \
                 working directory asks about a lock that is not the owner's. An unreclaimable \
                 labeled container blocks admission"
            ),
        });
    }
    Ok(path)
}

// ---------------------------------------------------------------------------
// The capability: proof that a container's intent record is published
// ---------------------------------------------------------------------------

/// Evidence that `<R>/containers/<name>.intent` exists and is a
/// [`ContainerIntent`].
///
/// `expected_failures_refusals[6]`: "container start without an intent is
/// **impossible by construction**". `super::create_container` and
/// `super::start_container` take one of these by reference, so reaching
/// `Container.Create` or `Container.Start` without the record is not a thing a
/// caller can express.
///
/// **The proof is a filesystem observation, not a private field**, and that is
/// forced rather than chosen: `runner::container::exec`, `::census` and every
/// other lane module is a *descendant* of `runner::container`, and Rust makes
/// an ancestor's private items visible to its descendants — so a token minted
/// only inside `container.rs` would be forgeable from `exec.rs` by writing the
/// struct literal. Grounding the proof in `<R>/containers` closes that: the
/// only way to obtain one is to have the record on disk, which is the property
/// the clause is about. It is the tree's own "ground truth is the diff, not the
/// transcript" applied to ownership evidence.
///
/// The fields are private so the pair (name, path) cannot be recombined — a
/// proof for container A cannot be relabelled as a proof for container B.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentWritten {
    name: ContainerName,
    path: PathBuf,
    record: ContainerIntent,
}

impl IntentWritten {
    /// Read `<R>/containers/<name>.intent` back and certify it.
    ///
    /// The **only** constructor. `super::write_intent` calls it after
    /// publishing, which is what makes its return value a proof rather than a
    /// path; a census or a reclaimer that already holds a record calls it
    /// directly. Either way the record was on disk when the proof was made.
    ///
    /// # Errors
    ///
    /// [`TactusError::Io`] when the record is absent or unreadable —
    /// `ErrorKind::NotFound` is the "no intent" case and is a refusal, not an
    /// absence to tolerate — and [`TactusError::Refused`] when the bytes are
    /// not a [`ContainerIntent`].
    pub fn certify(private_root: &Path, name: &ContainerName) -> Result<Self, TactusError> {
        let path = name.intent_path(private_root);
        let bytes = fs::read(&path).map_err(|source| TactusError::Io {
            path: path.clone(),
            source,
        })?;
        let record: ContainerIntent =
            serde_json::from_slice(&bytes).map_err(|error| TactusError::Refused {
                message: format!(
                    "`{}` is not a container intent, so it is not evidence that `{name}` is \
                     owned: {error}",
                    path.display()
                ),
            })?;
        Ok(Self {
            name: name.clone(),
            path,
            record,
        })
    }

    /// The container this record owns.
    #[must_use]
    pub const fn name(&self) -> &ContainerName {
        &self.name
    }

    /// Where the record is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The six fields, as they were read back.
    #[must_use]
    pub const fn record(&self) -> &ContainerIntent {
        &self.record
    }
}

// ---------------------------------------------------------------------------
// The name
// ---------------------------------------------------------------------------

/// `tactus-<repo_key>-<run_id>-<incarnation>-<invocation-hash>`.
///
/// **Injective by construction.** No component may contain
/// [`NAME_SEPARATOR`], so a rendered name splits into exactly five fields and
/// two distinct tuples differ in some field and therefore in the rendering.
/// [`ContainerName::parse`] is the inverse and refuses anything else.
///
/// The **incarnation** component is the one carrying the packet's stated
/// purpose: "deterministic `InvocationId`s never collide across incarnations
/// and no earlier ownership evidence is overwritten". A probe identity repeats
/// across incarnations by construction (`InvocationId::Probe`'s own doc says
/// so), so without that component a resuming incarnation would write its intent
/// over the dead one's — destroying the evidence the census needs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainerName(String);

/// A name taken back apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerNameParts {
    pub repo_key: String,
    pub run_id: String,
    pub incarnation: String,
    pub invocation_hash: String,
}

impl ContainerName {
    /// Build the name from its four components.
    ///
    /// # Errors
    ///
    /// [`TactusError::Refused`] when a component is empty, carries a character
    /// outside `[0-9A-Za-z_]`, is longer than [`MAX_COMPONENT_LEN`], or when
    /// the whole name would exceed [`MAX_NAME_LEN`].
    pub fn new(
        repo_key: &str,
        run_id: &str,
        incarnation: &str,
        invocation: &InvocationId,
    ) -> Result<Self, TactusError> {
        Self::from_parts(repo_key, run_id, incarnation, &invocation_hash(invocation))
    }

    /// Build the name from four already-rendered components.
    ///
    /// Separate from [`Self::new`] so a test can construct a name whose
    /// invocation component is *not* the hash of any invocation — which is what
    /// makes the parse's injectivity testable over the whole component domain
    /// rather than over the digests one function happens to produce.
    ///
    /// # Errors
    ///
    /// As [`Self::new`].
    pub fn from_parts(
        repo_key: &str,
        run_id: &str,
        incarnation: &str,
        invocation_hash: &str,
    ) -> Result<Self, TactusError> {
        validate_component("repo key", repo_key)?;
        validate_component("run id", run_id)?;
        validate_component("incarnation", incarnation)?;
        validate_component("invocation hash", invocation_hash)?;
        let rendered = format!("{NAME_PREFIX}-{repo_key}-{run_id}-{incarnation}-{invocation_hash}");
        if rendered.len() > MAX_NAME_LEN {
            return Err(TactusError::Refused {
                message: format!(
                    "the container name `{rendered}` is {} bytes; the limit is {MAX_NAME_LEN}",
                    rendered.len()
                ),
            });
        }
        Ok(Self(rendered))
    }

    /// The name as the runtime sees it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The file name of this container's intent record.
    #[must_use]
    pub fn intent_file_name(&self) -> String {
        format!("{}{INTENT_SUFFIX}", self.0)
    }

    /// `<R>/containers/<name>.intent`.
    #[must_use]
    pub fn intent_path(&self, private_root: &Path) -> PathBuf {
        containers_dir(private_root).join(self.intent_file_name())
    }

    /// Take a rendered name apart.
    ///
    /// # Errors
    ///
    /// [`TactusError::Refused`] when `value` is not `tactus-` followed by
    /// exactly four separator-free components.
    pub fn parse(value: &str) -> Result<ContainerNameParts, TactusError> {
        let refuse = || TactusError::Refused {
            message: format!(
                "`{value}` is not a tactus container name: the name is \
                 `{NAME_PREFIX}{NAME_SEPARATOR}<repo_key>{NAME_SEPARATOR}<run_id>\
                 {NAME_SEPARATOR}<incarnation>{NAME_SEPARATOR}<invocation-hash>` \
                 (decisions.admission_and_leases.permits.crash_reconstruction)"
            ),
        };
        let parts: Vec<&str> = value.split(NAME_SEPARATOR).collect();
        let [prefix, repo_key, run_id, incarnation, invocation_hash] = parts.as_slice() else {
            return Err(refuse());
        };
        if *prefix != NAME_PREFIX {
            return Err(refuse());
        }
        for component in [repo_key, run_id, incarnation, invocation_hash] {
            if component.is_empty() {
                return Err(refuse());
            }
        }
        Ok(ContainerNameParts {
            repo_key: (*repo_key).to_owned(),
            run_id: (*run_id).to_owned(),
            incarnation: (*incarnation).to_owned(),
            invocation_hash: (*invocation_hash).to_owned(),
        })
    }

    /// Rebuild a name from a rendered value, refusing one no funnel could have
    /// written.
    ///
    /// # Errors
    ///
    /// As [`Self::parse`] and [`Self::from_parts`].
    pub fn rebuild(value: &str) -> Result<Self, TactusError> {
        let parts = Self::parse(value)?;
        Self::from_parts(
            &parts.repo_key,
            &parts.run_id,
            &parts.incarnation,
            &parts.invocation_hash,
        )
    }

    /// The name of the container whose intent record this file name belongs to,
    /// or `None`.
    ///
    /// # Errors
    ///
    /// [`TactusError::Refused`] when the stem is not a well-formed name.
    pub fn from_intent_file_name(file_name: &str) -> Result<Option<Self>, TactusError> {
        match file_name.strip_suffix(INTENT_SUFFIX) {
            Some(stem) => Self::rebuild(stem).map(Some),
            None => Ok(None),
        }
    }
}

impl std::fmt::Display for ContainerName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `<R>/containers`.
#[must_use]
pub fn containers_dir(private_root: &Path) -> PathBuf {
    private_root.join(CONTAINERS_DIR)
}

/// `hex16(sha256(domain || 0x00 || rendered invocation id))`.
///
/// A digest and not the value itself, because the packet says
/// `<invocation-hash>`: a rendered [`InvocationId`] is up to
/// [`crate::runner::invocation::MAX_LEN`] bytes and carries `.` separators,
/// and the name already has four components. The **record** carries the
/// invocation in full, so ownership evidence stays exact — a 64-bit digest is
/// collision-resistant, not injective, and the difference matters for evidence
/// even though it does not matter for a name.
#[must_use]
pub fn invocation_hash(invocation: &InvocationId) -> String {
    let mut hasher = Sha256::new();
    hasher.update(INVOCATION_HASH_DOMAIN.as_bytes());
    hasher.update([0u8]);
    hasher.update(invocation.render().as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(INVOCATION_HASH_HEX_CHARS);
    for byte in digest.iter().take(INVOCATION_HASH_HEX_CHARS.div_ceil(2)) {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex.truncate(INVOCATION_HASH_HEX_CHARS);
    hex
}

/// Refuse a component that would make the name ambiguous or unusable.
///
/// The charset excludes [`NAME_SEPARATOR`] — which is what
/// [`ContainerName::parse`]'s injectivity rests on — and `.`, which is the
/// separator inside a rendered [`InvocationId`] and the boundary of the
/// `.intent` suffix.
fn validate_component(what: &str, value: &str) -> Result<(), TactusError> {
    if value.is_empty() {
        return Err(TactusError::Refused {
            message: format!("a container name's {what} component is never empty"),
        });
    }
    if value.len() > MAX_COMPONENT_LEN {
        return Err(TactusError::Refused {
            message: format!(
                "a container name's {what} component is {} bytes; the limit is \
                 {MAX_COMPONENT_LEN}",
                value.len()
            ),
        });
    }
    if let Some(bad) = value
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '_'))
    {
        return Err(TactusError::Refused {
            message: format!(
                "a container name's {what} component carries `{bad}`, which is outside \
                 [0-9A-Za-z_]; the name joins four components with `{NAME_SEPARATOR}` and \
                 names a file `<name>{INTENT_SUFFIX}`, so a component carrying the separator, \
                 a `.`, or a path separator would name a different container than the record \
                 says"
            ),
        });
    }
    Ok(())
}
