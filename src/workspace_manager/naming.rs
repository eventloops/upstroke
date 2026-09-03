//! The slot names, the snapshot names, and the intent record they are written
//! into.
//!
//! `decisions.workspace_candidates.manager` names two of the three namespaces
//! literally -- "detached linked worktrees with durable synced intents
//! (`tasks/k<key>-g<gen>`, `merge/s<seq>`)" -- and
//! `decisions.workspace_candidates.snapshots` requires the third's members to be
//! "never reused across roles or attempts".
//!
//! **Containment is structural; uniqueness is not, and the difference is worth
//! stating exactly.** [`safe_component`] is what makes a [`Slot`] path
//! containment-by-construction — a name that could escape the execution root is
//! refused, whatever produced it. Uniqueness across roles and attempts is a
//! weaker guarantee: [`SnapshotName`]'s three constructors encode the role, the
//! generation and the attempt into the name, so a caller that goes through them
//! cannot collide — but they are not the only way to obtain one.
//! [`Slot::from_intent_name`] reconstructs a [`Slot::Snapshot`] straight from an
//! on-disk intent filename, so that reclaim never has to trust a path stored
//! inside a record, and a name reconstructed that way carries whatever the
//! filename carried. **So "never reused across roles or attempts" rests on
//! caller discipline rather than on the type**, and this module documents that
//! rather than claiming a guarantee it does not provide.
//!
//! Pure string and path arithmetic. Nothing in this module reads or writes the
//! filesystem; the funnels that act on the paths it names are the parent's.
//!
//! **[`Slot`]'s five effect-site accessors are deliberately not here.** `row`,
//! `add_site`, `write_intent_site`, `remove_site` and `remove_intent_site` map a
//! slot to the [`EffectSiteId`](crate::topology::effects::EffectSiteId) its
//! funnel runs under, which is effect-site vocabulary rather than naming, and
//! `effects::tests::every_site_the_inventory_declares_has_a_funnel_that_names_
//! it_or_is_recorded_absent` reads `src/workspace_manager.rs` **by path** for
//! exactly those eleven variant literals. They stay in the parent, in a second
//! `impl Slot` block beside the module declaration, so that census keeps
//! measuring what it measured before the split.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree rather than by the file, so an out-of-line
// child of `src/workspace_manager.rs` inherits that file's inner
// `#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`
// unless it says otherwise -- `PR6-LANEF-004`, and the mistake two W1 pull
// requests then made independently (#100 and #102). Nothing here reaches a
// governed primitive, so all three are DENIED and this module takes no
// `effects/allowlist.toml` row: a row records an allowance, and this module
// takes none.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::Refusal;

/// The three worktree namespaces of an execution root.
///
/// `decisions.workspace_candidates.manager` names two of them literally —
/// "detached linked worktrees with durable synced intents (`tasks/k<key>-g<gen>`,
/// `merge/s<seq>`)" — and `snapshots` names the third, whose members
/// `decisions.workspace_candidates.snapshots` requires to be "never reused
/// across roles or attempts".
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    /// `tasks/k<key>-g<gen>` — a task worktree, R9.
    Task {
        /// The task key.
        key: String,
        /// The generation number.
        generation: u32,
    },
    /// `merge/s<seq>` — a staging worktree, R10. Never created for an
    /// exact-base fast sequence.
    Staging {
        /// The merge sequence number.
        sequence: u64,
    },
    /// `snapshots/<name>` — an exact gate or review snapshot, R24.
    Snapshot {
        /// The snapshot's name. Built through one of [`SnapshotName`]'s three
        /// constructors it encodes the role, the generation and the attempt, so
        /// callers going through them cannot collide; see that type for why
        /// that is not a property of every `SnapshotName`.
        name: SnapshotName,
    },
}

/// A snapshot's name.
///
/// The three constructors below encode the role, the generation and the attempt
/// into the string, which is how a caller that uses them satisfies
/// `decisions.workspace_candidates.snapshots`' "never reused across roles or
/// attempts". It is **not** a property of the type: `Slot::from_intent_name`
/// rebuilds one straight from an on-disk intent filename, so a reconstructed
/// name carries whatever the filename carried. The module doc says where that
/// leaves the guarantee.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotName(String);

impl SnapshotName {
    /// The one snapshot the whole gate set runs on.
    #[must_use]
    pub fn gates(generation: u32, attempt: u32) -> Self {
        Self(format!("g{generation}-a{attempt}-gates"))
    }

    /// One fresh snapshot per reviewer.
    #[must_use]
    pub fn review(generation: u32, attempt: u32, reviewer: u32) -> Self {
        Self(format!("g{generation}-a{attempt}-review{reviewer}"))
    }

    /// The snapshot an integration transaction judges its proposal on.
    #[must_use]
    pub fn integration(sequence: u64) -> Self {
        Self(format!("s{sequence}-integration"))
    }

    /// The name as a directory component.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SnapshotName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Check that `name` is safe as a single path component, or say why it is not.
///
/// The grammar is one non-empty run of ASCII alphanumerics, `-` and `_` that
/// does not start with `-`. Everything containment needs follows from it: a
/// separator, `..`, a drive or UNC prefix and a non-UTF-8 byte cannot occur
/// at all, and `.` is excluded so that [`Slot::intent_name`]'s `.` joins stay
/// unambiguous. The objection is the `why` of [`Refusal::SlotName`], and the
/// verdict is a `Result` so that a caller cannot drop it unread.
///
/// # Errors
///
/// The first objection, as a sentence fragment that completes "refusing the
/// slot name `x`: ...".
pub(super) fn safe_component(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("it is empty");
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("only ASCII alphanumerics, `-`, and `_` are legal in a slot component");
    }
    if name.starts_with('-') {
        return Err("a leading `-` would be read as an option by the Git commands the funnels run");
    }
    Ok(())
}

impl Slot {
    /// The slot's namespace directory and its component within it: the one
    /// rendering that [`Self::relative`], [`Self::git_path`] and
    /// [`Self::intent_name`] are three spellings of.
    fn parts(&self) -> (&'static str, Cow<'_, str>) {
        match self {
            Self::Task { key, generation } => {
                ("tasks", Cow::Owned(format!("k{key}-g{generation}")))
            }
            Self::Staging { sequence } => ("merge", Cow::Owned(format!("s{sequence}"))),
            Self::Snapshot { name } => ("snapshots", Cow::Borrowed(name.as_str())),
        }
    }

    /// The slot's path relative to the execution root.
    #[must_use]
    pub fn relative(&self) -> PathBuf {
        let (namespace, component) = self.parts();
        PathBuf::from(namespace).join(&*component)
    }

    /// The slot's relative path as the intent record spells it: a
    /// [`SlotPath`], `<namespace>/<component>` with a forward slash on every
    /// platform.
    ///
    /// String arithmetic over the same two parts as [`Self::relative`], not a
    /// conversion of that path, so no OS path is rendered lossily on the way
    /// into the record. The slot is validated first, which is what lets the
    /// result be a `SlotPath` by construction: [`SlotPath`]'s grammar is
    /// exactly the two parts of a slot that passes [`Self::validate`].
    ///
    /// # Errors
    ///
    /// [`Refusal::SlotName`], from [`Self::validate`].
    pub fn git_path(&self) -> Result<SlotPath, Refusal> {
        self.validate()?;
        let (namespace, component) = self.parts();
        Ok(SlotPath(format!("{namespace}/{component}")))
    }

    /// The intent file's name, injective over slots: the two parts are joined
    /// by `.`, which [`safe_component`] forbids inside either.
    #[must_use]
    pub fn intent_name(&self) -> String {
        let (namespace, component) = self.parts();
        format!("{namespace}.{component}.intent")
    }

    /// What the intent record calls this kind.
    #[must_use]
    pub fn intent_kind(&self) -> IntentKind {
        match self {
            Self::Task { .. } => IntentKind::Task,
            Self::Staging { .. } => IntentKind::Staging,
            Self::Snapshot { .. } => IntentKind::Snapshot,
        }
    }

    /// [`Self::intent_kind`] as the word the record and the refusals use.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        self.intent_kind().as_str()
    }

    /// Refuse a slot whose components could escape the execution root.
    ///
    /// # Errors
    ///
    /// [`Refusal::SlotName`], naming the kind, the name and
    /// [`safe_component`]'s objection.
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        let name = match self {
            Self::Task { key, .. } => key.as_str(),
            // A sequence number renders as decimal digits, which is a safe
            // component by construction.
            Self::Staging { .. } => return Ok(()),
            Self::Snapshot { name } => name.as_str(),
        };
        safe_component(name).map_err(|why| Refusal::SlotName {
            kind: self.kind(),
            name: name.to_owned(),
            why,
        })
    }

    /// Rebuild a slot from an intent file name, so reclaim never has to trust
    /// a path stored inside a record.
    ///
    /// `Some` exactly on [`Self::intent_name`]'s image. The name is parsed and
    /// then re-rendered, and only a name equal to its own rendering is an
    /// intent name. A name that merely *reads* as one — `tasks.kalpha-g03.intent`
    /// or `merge.s+7.intent`, both of which the integer parser accepts — is
    /// refused, because the slot it would produce renders to a different file:
    /// reclaim would remove that file's intent and leave this one for every
    /// later start to enumerate, report as reclaimed, and leave again.
    ///
    /// `None` is this parser's whole verdict. Five `?` reach it, one per
    /// clause of the grammar, and the round-trip comparison is a sixth exit.
    /// Each is dispositioned here in terms of what the verdict means to the
    /// one caller, the intents directory walk:
    ///
    /// - `strip_suffix(".intent")?`: no `.intent` suffix. **Not an intent
    ///   name** at all: a staging `.tmp`, an editor's backup, a stray file.
    /// - `rsplit_once("-g")?`: a task name with no generation separator.
    ///   **Malformed.**
    /// - the two `parse().ok()?`: a generation that is not a `u32`, or a
    ///   sequence that is not a `u64`. **Malformed.** The `ParseIntError` is
    ///   discarded because which way the digits failed adds nothing the
    ///   file's name, which the caller reports, does not already say.
    /// - `strip_prefix("snapshots.")?`, reached once `tasks.k` and `merge.s`
    ///   have not matched: the suffix is there and the namespace is not one
    ///   this version writes. **Malformed**, or another version's.
    /// - `then_some`: the name parses but does not re-render to itself.
    ///   **Malformed by canon** rather than by shape; the `g03` case above.
    ///
    /// "Not an intent name" and "malformed intent name" are told apart here
    /// for the reader and folded into one `None` for the caller, deliberately:
    /// the walk has one action for both. It refuses the reclaim and names the
    /// file, because it may delete only what it can prove it owns and may
    /// skip nothing that might be an intent another version wrote. The name
    /// in the refusal carries the distinction to the operator; a second
    /// variant would carry it to a caller with no second action to take. The
    /// parser itself does not know it is reading the intents directory, which
    /// is why the refusal, and its context, are the caller's.
    ///
    /// Grammar here, containment in [`Self::validate`]: a well-formed name
    /// whose component is not a [`safe_component`] — an empty key, a snapshot
    /// name carrying `.` — is returned, and `validate` refuses it.
    #[must_use]
    pub(super) fn from_intent_name(name: &str) -> Option<Self> {
        let stem = name.strip_suffix(".intent")?;
        let slot = if let Some(rest) = stem.strip_prefix("tasks.k") {
            let (key, generation) = rest.rsplit_once("-g")?;
            Self::Task {
                key: key.to_owned(),
                generation: generation.parse().ok()?,
            }
        } else if let Some(rest) = stem.strip_prefix("merge.s") {
            Self::Staging {
                sequence: rest.parse().ok()?,
            }
        } else {
            let rest = stem.strip_prefix("snapshots.")?;
            Self::Snapshot {
                name: SnapshotName(rest.to_owned()),
            }
        };
        (slot.intent_name() == name).then_some(slot)
    }
}

/// The durable per-owner recovery record `resource_accounting` requires of
/// every worktree, staging, and snapshot slot.
///
/// `enforcement_domains.external_physical`: "every worktree, staging, snapshot,
/// and container intent is a durable per-owner recovery record in its row,
/// reclaimed at process start (never 'empty')".
///
/// The worktree path is **not** a field. Reclaim derives it from the intent's
/// own name and the execution root, so a record cannot name a path outside the
/// root it lives in — the containment `cleanup` requires ("expected-path,
/// contained, idempotent, and never establishes authority") is then structural
/// rather than checked.
///
/// **This is a persisted schema.** The wire format is four JSON strings in
/// this order — `kind`, `slot`, `run_id`, `incarnation` — with no defaults,
/// no aliases and unknown fields refused; the test on this type pins the
/// exact bytes. Two of the fields are typed on the way in as well as on the
/// way out: `kind` is an [`IntentKind`], so a record whose kind is not one
/// of the three words is refused on read, and `slot` is a [`SlotPath`], so a
/// record whose slot is not `<namespace>/<safe component>` is refused on read.
/// Reclaim still trusts the file's name and nothing inside the file; the
/// typing keeps a record honest for whoever reads it, it grants nothing.
///
/// Written by [`WorkspaceManager::write_intent`] from a slot that passed
/// [`Slot::validate`]: `kind` from [`Slot::intent_kind`], `slot` from
/// [`Slot::git_path`].
///
/// [`WorkspaceManager::write_intent`]: super::WorkspaceManager::write_intent
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentRecord {
    /// `task`, `staging`, or `snapshot`.
    pub kind: IntentKind,
    /// The slot's path relative to the execution root as the record spells
    /// it, from [`Slot::git_path`].
    pub slot: SlotPath,
    /// The run that owns it.
    pub run_id: String,
    /// The coordinator incarnation that wrote it, so a later incarnation of the
    /// same run can tell its own residue from a live sibling's.
    pub incarnation: String,
}

/// What the intent record calls a slot's kind. The wire spelling is the
/// lowercase variant name, and nothing else deserializes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntentKind {
    /// `tasks/k<key>-g<gen>`.
    Task,
    /// `merge/s<seq>`.
    Staging,
    /// `snapshots/<name>`.
    Snapshot,
}

impl IntentKind {
    /// The word the record and the refusals use.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Staging => "staging",
            Self::Snapshot => "snapshot",
        }
    }
}

impl fmt::Display for IntentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A slot's path relative to the execution root as the intent record spells
/// it: `<namespace>/<component>`, the namespace one of `tasks`, `merge` or
/// `snapshots`, the component a [`safe_component`], the separator `/` on
/// every platform.
///
/// **This is the documented exception to "all paths through `std::path`".**
/// The record is platform-independent on-disk text by design: a `/` here is
/// part of the schema, not a separator this process chose, and nothing ever
/// joins a `SlotPath` to a root or opens it. It is produced by
/// [`Slot::git_path`] as string arithmetic over the slot's validated parts,
/// so no OS path is converted on the way in, and it is validated by
/// [`TryFrom<String>`] on the way out of JSON, so a record cannot carry
/// `..`, a leading `/`, a backslash or an empty component past a reader.
/// The private field is the invariant: the only way to hold one is through
/// one of those two doors.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SlotPath(String);

impl SlotPath {
    /// The path as the record spells it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The grammar, as a verdict over the text.
    fn objection(value: &str) -> Result<(), &'static str> {
        let (namespace, component) = value
            .split_once('/')
            .ok_or("it has no `/` between the namespace and the component")?;
        if !matches!(namespace, "tasks" | "merge" | "snapshots") {
            return Err("the namespace is not `tasks`, `merge` or `snapshots`");
        }
        safe_component(component)
    }
}

impl TryFrom<String> for SlotPath {
    type Error = SlotPathError;

    /// The grammar is checked on the whole value; a second `/` is refused
    /// because `/` is not legal inside a component.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match Self::objection(&value) {
            Ok(()) => Ok(Self(value)),
            Err(why) => Err(SlotPathError { value, why }),
        }
    }
}

impl From<SlotPath> for String {
    fn from(path: SlotPath) -> Self {
        path.0
    }
}

impl fmt::Display for SlotPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A string offered as a [`SlotPath`] that is not one.
#[derive(Debug, thiserror::Error)]
#[error("`{value}` is not a slot path: {why}")]
pub struct SlotPathError {
    /// The value as it was offered.
    value: String,
    /// The first objection, from [`SlotPath`]'s grammar.
    why: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One slot per shape the grammar has to tell apart: a key that contains,
    /// ends with, or is the `-g` separator's letter; the generation and
    /// sequence extremes; and all three snapshot constructors.
    fn every_shape() -> Vec<Slot> {
        vec![
            Slot::Task {
                key: "alpha".to_owned(),
                generation: 0,
            },
            Slot::Task {
                key: "alpha-g2".to_owned(),
                generation: 3,
            },
            Slot::Task {
                key: "alpha-g".to_owned(),
                generation: 3,
            },
            Slot::Task {
                key: "g".to_owned(),
                generation: u32::MAX,
            },
            Slot::Task {
                key: "0123456789abcdef".to_owned(),
                generation: 1,
            },
            Slot::Staging { sequence: 0 },
            Slot::Staging { sequence: u64::MAX },
            Slot::Snapshot {
                name: SnapshotName::gates(1, 2),
            },
            Slot::Snapshot {
                name: SnapshotName::review(1, 2, 3),
            },
            Slot::Snapshot {
                name: SnapshotName::integration(4),
            },
        ]
    }

    #[test]
    fn every_slot_shape_survives_the_intent_name_round_trip() {
        for slot in every_shape() {
            slot.validate().expect("every fixture slot is a valid one");
            let name = slot.intent_name();
            assert_eq!(
                Slot::from_intent_name(&name).as_ref(),
                Some(&slot),
                "`{name}` did not come back as the slot that rendered it"
            );
            assert_eq!(
                name,
                format!("{}.{}.intent", slot.kind_namespace(), slot.component()),
                "the intent name is the namespace, the component and the suffix"
            );
        }
    }

    #[test]
    fn a_name_intent_name_did_not_produce_is_not_an_intent_name() {
        for name in [
            // The integer parser accepts these; the round trip does not,
            // because each re-renders to a different file name.
            "tasks.kalpha-g03.intent",
            "tasks.kalpha-g+3.intent",
            "merge.s007.intent",
            "merge.s+7.intent",
            // Malformed bodies under a known namespace.
            "tasks.kalpha.intent",
            "tasks.kalpha-g.intent",
            "tasks.kalpha-gx.intent",
            "tasks.kalpha-g4294967296.intent",
            "tasks.kalpha-g-1.intent",
            "merge.s.intent",
            "merge.s-1.intent",
            "merge.sx.intent",
            // Unknown or misspelled namespaces, and no namespace.
            "snapshot.g1-a1-gates.intent",
            "task.kalpha-g1.intent",
            "intents.x.intent",
            ".intent",
            "",
            // Not the suffix. The first is the name `write_synced` stages an
            // intent under before its rename; the parser refuses it like any
            // other stray file, and what the directory walk should make of
            // that residue is the parent's decision, not this parser's.
            "tasks.kalpha-g1.tmp",
            "tasks.kalpha-g1.intent.bak",
            "tasks.kalpha-g1",
            "tasks.kalpha-g1.intent ",
        ] {
            assert_eq!(
                Slot::from_intent_name(name),
                None,
                "`{name}` was accepted as an intent name"
            );
        }
    }

    #[test]
    fn the_parser_reads_the_grammar_and_validate_reads_containment() {
        // Well-formed under the grammar, refused by containment: the parser
        // returns the slot and `validate` is where it is refused, so the
        // directory walk sees the refusal that names the component.
        for (name, objection) in [
            ("tasks.k-g3.intent", "it is empty"),
            ("snapshots..intent", "it is empty"),
            ("snapshots.a.b.intent", "only ASCII alphanumerics"),
            ("tasks.ka/b-g1.intent", "only ASCII alphanumerics"),
            ("snapshots.-x.intent", "a leading `-`"),
        ] {
            let slot = Slot::from_intent_name(name)
                .unwrap_or_else(|| panic!("`{name}` is well-formed under the grammar"));
            assert_eq!(
                slot.intent_name(),
                name,
                "the round trip holds for `{name}`"
            );
            let refusal = slot
                .validate()
                .expect_err("containment refuses what the grammar admitted");
            assert!(
                slot.git_path().is_err(),
                "and `git_path` refuses it too, so no record can spell it"
            );
            assert!(
                matches!(&refusal, Refusal::SlotName { why, .. } if why.starts_with(objection)),
                "`{name}` was refused for the wrong reason: {refusal}"
            );
        }
    }

    #[test]
    fn safe_component_names_the_first_objection() {
        for name in ["a", "A-1_b", "0", "a-", "_"] {
            assert_eq!(safe_component(name), Ok(()), "`{name}` is a safe component");
        }
        assert_eq!(safe_component(""), Err("it is empty"));
        for name in ["a/b", "a\\b", "a.b", "..", "a b", "\u{e9}", "a\0"] {
            assert!(
                safe_component(name).is_err_and(|why| why.starts_with("only ASCII")),
                "`{name}` was accepted"
            );
        }
        assert!(
            safe_component("-x").is_err_and(|why| why.starts_with("a leading `-`")),
            "a leading `-` is refused after the character-set check"
        );
        // A staging slot's component is decimal digits by construction and is
        // the one `validate` never checks; the claim is kept honest here.
        for sequence in [0, 1, u64::MAX] {
            let slot = Slot::Staging { sequence };
            assert_eq!(safe_component(&slot.component()), Ok(()));
            slot.validate()
                .expect("a staging slot is valid by construction");
        }
    }

    /// What this pins: the exact bytes, so the field names and their order;
    /// that the four names deserialize; that no field has a default; that a
    /// field under another name is refused, the same value included; and
    /// that an unknown field is refused. Values are pinned by the two typed
    /// fields' own tests below.
    #[test]
    fn the_intent_record_schema_is_pinned() {
        let record = IntentRecord {
            kind: IntentKind::Task,
            slot: SlotPath::try_from("tasks/kalpha-g1".to_owned()).expect("a slot path"),
            run_id: "run".to_owned(),
            incarnation: "01".to_owned(),
        };
        let json = serde_json::to_string(&record).expect("a record serializes");
        assert_eq!(
            json, r#"{"kind":"task","slot":"tasks/kalpha-g1","run_id":"run","incarnation":"01"}"#,
            "the persisted field names and order are the on-disk schema; a change here is a \
             compatibility decision, not a refactor"
        );
        let back: IntentRecord = serde_json::from_str(&json).expect("the record reads back");
        assert_eq!(back, record);

        // The field list is derived from the serialized record, not written
        // out again here, so a field added to the type is dropped below like
        // the others. Each field is dropped on its own: `#[serde(default)]`
        // on any one of them would turn that absence into a silently accepted
        // record, and only a per-field drop sees which one.
        let object = || -> serde_json::Map<String, serde_json::Value> {
            serde_json::from_str(&json).expect("the record is a JSON object")
        };
        // `serde_json::Map` sorts its keys; the order is pinned by the exact
        // string above, the set by this. The map is consumed for its keys.
        let fields: Vec<String> = object().into_iter().map(|(key, _)| key).collect();
        assert_eq!(
            fields,
            ["incarnation", "kind", "run_id", "slot"],
            "the field list this test varies is the type's"
        );
        for field in &fields {
            let mut without = object();
            without.remove(field);
            assert!(
                serde_json::from_value::<IntentRecord>(serde_json::Value::Object(without)).is_err(),
                "a record without `{field}` was accepted: no field has a default"
            );
        }
        // The key set from the deserializing side: the four names read back,
        // and a field offered under another name, its value unchanged, is an
        // unknown field plus a missing one and refuses. `#[serde(alias =
        // "legacy_kind")]` on `kind` would accept the first spelling below.
        let read: IntentRecord = serde_json::from_value(serde_json::Value::Object(object()))
            .expect("the four names deserialize");
        assert_eq!(read, record);
        for field in &fields {
            for other in [
                format!("legacy_{field}"),
                format!("{field}_"),
                "x".to_owned(),
            ] {
                let mut renamed = object();
                let value = renamed
                    .remove(field)
                    .expect("the field list came from this object");
                let accepted =
                    format!("`{field}` was accepted as `{other}`: no field has an alias");
                renamed.insert(other, value);
                assert!(
                    serde_json::from_value::<IntentRecord>(serde_json::Value::Object(renamed))
                        .is_err(),
                    "{accepted}"
                );
            }
        }
        let mut extra = object();
        extra.insert(
            "path".to_owned(),
            serde_json::Value::String("/x".to_owned()),
        );
        assert!(
            serde_json::from_value::<IntentRecord>(serde_json::Value::Object(extra)).is_err(),
            "an unknown field is refused: a record must not smuggle a path into reclaim"
        );
    }

    #[test]
    fn the_record_kind_is_one_of_three_words() {
        let with_kind = |kind: &str| {
            serde_json::from_str::<IntentRecord>(&format!(
                r#"{{"kind":{kind},"slot":"tasks/kalpha-g1","run_id":"run","incarnation":"01"}}"#
            ))
        };
        for (word, kind) in [
            ("task", IntentKind::Task),
            ("staging", IntentKind::Staging),
            ("snapshot", IntentKind::Snapshot),
        ] {
            let record = with_kind(&format!("\"{word}\"")).expect("a known kind reads");
            assert_eq!(record.kind, kind);
            assert_eq!(kind.as_str(), word, "and renders back to the same word");
        }
        for bogus in ["\"bogus\"", "\"Task\"", "\"\"", "0", "null", "[\"task\"]"] {
            assert!(
                with_kind(bogus).is_err(),
                "kind {bogus} was accepted: the record's kind is one of three words"
            );
        }
        for slot in every_shape() {
            assert_eq!(slot.intent_kind().as_str(), slot.kind());
        }
    }

    #[test]
    fn the_record_slot_is_refused_on_read_outside_its_grammar() {
        let with_slot = |slot: &str| {
            serde_json::from_str::<IntentRecord>(&format!(
                r#"{{"kind":"task","slot":"{slot}","run_id":"run","incarnation":"01"}}"#
            ))
        };
        for good in ["tasks/kalpha-g1", "merge/s1", "snapshots/g1-a1-gates"] {
            let record = with_slot(good).expect("a slot path in the grammar reads");
            assert_eq!(record.slot.as_str(), good);
            assert_eq!(
                String::from(record.slot),
                good,
                "and converts back to the same text"
            );
        }
        // JSON spelling on the left, so a backslash is `\\\\` here and one
        // backslash in the value the record reads.
        for (bad, objection) in [
            ("tasks/..", "only ASCII"),
            ("../tasks/kalpha-g1", "the namespace"),
            ("/tasks/kalpha-g1", "the namespace"),
            ("tasks\\\\kalpha-g1", "it has no `/`"),
            ("tasks/kalpha\\\\g1", "only ASCII"),
            ("tasks/", "it is empty"),
            ("tasks//kalpha-g1", "only ASCII"),
            ("tasks/kalpha-g1/", "only ASCII"),
            ("/", "the namespace"),
            ("", "it has no `/`"),
            ("kalpha-g1", "it has no `/`"),
            ("worktrees/kalpha-g1", "the namespace"),
            ("tasks/-g1", "a leading `-`"),
        ] {
            let error = with_slot(bad).expect_err("a slot path outside the grammar is refused");
            assert!(
                error.to_string().contains(objection),
                "`{bad}` was refused for the wrong reason: {error}"
            );
            let direct = SlotPath::try_from(bad.replace("\\\\", "\\"))
                .expect_err("and refused by the constructor itself");
            assert!(direct.to_string().contains(objection), "{direct}");
        }
    }

    #[test]
    fn the_record_slot_is_the_relative_path_spelled_with_forward_slashes() {
        for slot in every_shape() {
            let git_path = slot.git_path().expect("every fixture slot is a valid one");
            let components: Vec<String> = slot
                .relative()
                .components()
                .map(|component| {
                    component
                        .as_os_str()
                        .to_str()
                        .expect(
                            "every slot path component is UTF-8: the fixtures are built from &str",
                        )
                        .to_owned()
                })
                .collect();
            assert_eq!(
                git_path.as_str(),
                components.join("/"),
                "`git_path` and `relative` are two spellings of the same parts"
            );
            assert!(
                !git_path.as_str().contains('\\'),
                "the schema string has no OS separator on any platform: {git_path}"
            );
            assert_eq!(
                slot.intent_name(),
                format!("{}.intent", git_path.as_str().replace('/', ".")),
                "and the intent name is the third spelling"
            );
            let again = SlotPath::try_from(String::from(git_path)).expect("round trip");
            assert_eq!(
                again.as_str(),
                components.join("/"),
                "what `git_path` builds, the grammar admits"
            );
        }
    }

    impl Slot {
        /// The namespace half of the intent name, for the tests' own oracle.
        fn kind_namespace(&self) -> &'static str {
            match self {
                Self::Task { .. } => "tasks",
                Self::Staging { .. } => "merge",
                Self::Snapshot { .. } => "snapshots",
            }
        }

        /// The component half: the last path component of [`Self::relative`].
        fn component(&self) -> String {
            self.relative()
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .expect(
                    "every slot path ends in a UTF-8 component: the fixtures are built from &str",
                )
        }
    }
}
