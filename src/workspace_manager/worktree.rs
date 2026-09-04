//! What Git reports about a linked worktree, and what
//! [`WorkspaceManager::verify_worktree`](super::WorkspaceManager::verify_worktree)
//! demands of one before it is reused.
//!
//! These are the three values that conversation is held in -- the record Git
//! hands back, the quiescence the caller asks for, and the reasons the answer
//! can be no. The verification itself runs a Git child and is the parent's; the
//! record is produced by `parsers.rs`, which reads the `--porcelain -z` framing
//! and feeds each attribute to an [`OpenRecord`], and this module holds the
//! grammar of the record itself.
//!
//! **What this module does not state.** Which conditions a reuse must meet is
//! the parent's `verify_worktree`, and where that rule comes from is
//! `DESIGN.md`'s to say. The tree cites a decision record retired on
//! 2026-09-03 for it, at 51 sites, and `DESIGN.md`'s retired-records table
//! maps no such record to a section (`SWEEP-WORKTREE-014`, for the owner).
//! Until it does, nothing here quotes that record: these types document what
//! they are and what the code does with them, and no more.

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

use std::fmt;
use std::path::{Path, PathBuf};

use super::is_object_id;
use crate::topology::effects::ResidueElement;

/// A registered worktree of the managed repository, as
/// `git worktree list --porcelain -z` reports it.
///
/// The value holds the grammar of its attributes -- a `HEAD` is a full
/// hexadecimal object id, a `branch` is inside the refname byte set, and no
/// attribute is read twice -- and the shape of the record as a whole: a
/// worktree is either bare, with no HEAD and no branch, or it names a HEAD
/// and exactly one of `branch` and `detached`. Each attribute rule is applied
/// by [`OpenRecord`] as the attribute arrives and the shape by
/// [`OpenRecord::close`], which is the one way to build a record; the fields
/// are private so that nothing constructs one around them (`locked` is
/// readable by the parent module for now, and says why).
///
/// So `branch()` answering `None` means this worktree has checked out no
/// branch, never that Git's answer did not say. `detached` and `bare` are not
/// stored, because the shape rule makes them derivable: a record with a HEAD
/// and no branch is the detached one, and a record with no HEAD is the bare
/// one.
///
/// The path is the parser's decoding. The two reasons are **not** verbatim:
/// they are decoded with replacement characters, which
/// [`WorktreeRecord::lock_reason`] states, because they are shown and
/// compared with one ASCII word, never used as identity (§8).
///
/// No `Clone`: nothing copies a record. The parent iterates the list Git
/// returned by value and moves the path into the refusal that names it
/// ([`WorktreeRecord::into_path`]); a consumer that wants a copy asks for the
/// field it wants. `PartialEq` is the parser's oracle, comparing a list read
/// from bytes with the records those bytes spell.
#[derive(Debug, PartialEq, Eq)]
pub struct WorktreeRecord {
    /// The checkout path, decoded byte-safely by the parser.
    path: PathBuf,
    /// A full hexadecimal object id, when the worktree has a HEAD.
    head: Option<String>,
    /// A full refname's bytes, when the worktree is not detached.
    branch: Option<Vec<u8>>,
    /// Git's own lock reason, when the worktree is locked; empty for a lock
    /// taken without one.
    ///
    /// `pub(super)` for one reader and for now: `residue.rs` compares it with
    /// `initializing` directly, at one site since its own sweep merged
    /// (PR #128, `95c5bd3`). That line becomes
    /// [`WorktreeRecord::is_initializing`] and this field goes private when a
    /// change touches it -- the parent's sweep, queue row 11, reads every one
    /// of these call sites (`SWEEP-WORKTREE-004`). Every other reader asks the
    /// accessor.
    pub(super) locked: Option<String>,
    /// Git's own prunable reason, when the worktree is prunable; empty when
    /// Git gave none.
    prunable: Option<String>,
}

/// One record of `git worktree list --porcelain -z` while its attributes are
/// being read: from its `worktree` line to the empty attribute that closes it.
///
/// The parser feeds each attribute as it arrives and each attribute's own
/// rule is applied here, in attribute order, so a refusal names the first
/// attribute outside the grammar, exactly as the parser did when it held the
/// rules itself. [`OpenRecord::close`] applies the rule no single attribute
/// can carry -- which combinations of `HEAD`, `branch`, `bare` and `detached`
/// are a worktree -- and is the one way to make a [`WorktreeRecord`].
///
/// The path is already decoded, because which bytes can be a path is the
/// platform's question and the parser answers it per platform. Every other
/// attribute is the bytes after the label's one space, borrowed from the
/// answer; the empty slice is a label Git printed with no value, which is how
/// a lock or a prunable entry without a reason is listed (measured, Git
/// 2.43.0: `git worktree lock <path>` lists as `locked`, and
/// `--reason "why  "` as `locked why`, Git having trimmed its own file).
#[derive(Debug)]
pub(super) struct OpenRecord<'a> {
    /// `worktree <path>`, decoded.
    path: PathBuf,
    /// `HEAD <sha>`, already an object id.
    head: Option<&'a str>,
    /// `branch <refname>`, already inside the byte set.
    branch: Option<&'a [u8]>,
    /// Whether `detached` was read.
    detached: bool,
    /// Whether `bare` was read.
    bare: bool,
    /// `locked` or `locked <reason>`.
    locked: Option<&'a [u8]>,
    /// `prunable` or `prunable <reason>`.
    prunable: Option<&'a [u8]>,
}

/// Why an [`OpenRecord`] refused a record: an attribute outside its grammar or
/// read twice, or a set of attributes that is not a worktree.
///
/// The text is a predicate of the record, for the parser to join after the
/// record's number: "record 0 has a HEAD that is not one object id".
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum MalformedRecord {
    /// `HEAD` is not forty or sixty-four hexadecimal digits, or is a second
    /// `HEAD`: either way the record does not have one object id.
    #[error("has a HEAD that is not one object id")]
    Head,
    /// `branch` is empty or carries a byte no refname can, or is a second
    /// `branch`.
    #[error("has a branch that is not one refname")]
    Branch,
    /// `detached` or `bare` a second time.
    #[error("has a boolean attribute twice")]
    BooleanTwice,
    /// `locked` or `prunable` a second time.
    #[error("has a reason attribute twice")]
    ReasonTwice,
    /// `bare` alongside a `HEAD`, a `branch` or `detached`.
    #[error("is bare and also names a HEAD, a branch or a detached checkout")]
    BareWithCheckout,
    /// Neither `bare` nor a `HEAD`.
    #[error("names no HEAD and is not bare")]
    NoHead,
    /// Both `branch` and `detached`: a checkout is one or the other.
    #[error("is on a branch and detached at once")]
    BranchAndDetached,
    /// Neither `branch` nor `detached` on a worktree that has a HEAD, so
    /// nothing in the record says whether a branch is checked out there.
    #[error("names a HEAD but neither a branch nor a detached checkout")]
    NeitherBranchNorDetached,
}

impl<'a> OpenRecord<'a> {
    /// Open the record whose `worktree` line named `path`.
    pub(super) fn at(path: PathBuf) -> Self {
        Self {
            path,
            head: None,
            branch: None,
            detached: false,
            bare: false,
            locked: None,
            prunable: None,
        }
    }

    /// `HEAD <value>`: a full hexadecimal object id of either hash length,
    /// decided by the same predicate the ref transitions apply
    /// ([`is_object_id`](super::is_object_id)); an object id is ASCII, so a
    /// value that is not UTF-8 fails that same test and the offset at which
    /// it stopped being UTF-8 would add nothing.
    ///
    /// # Errors
    ///
    /// [`MalformedRecord::Head`] for a value outside the grammar or a
    /// second `HEAD`.
    pub(super) fn head(&mut self, value: &'a [u8]) -> Result<(), MalformedRecord> {
        if self.head.is_some() {
            return Err(MalformedRecord::Head);
        }
        match std::str::from_utf8(value) {
            Ok(text) if is_object_id(text) => {
                self.head = Some(text);
                Ok(())
            }
            Ok(_) | Err(_) => Err(MalformedRecord::Head),
        }
    }

    /// `branch <value>`: the bytes Git printed, held by [`can_be_refname`] to
    /// the byte set of `git check-ref-format`. A refname is bytes to Git and
    /// need not be UTF-8 on Unix, and [`WorktreeRecord::has_checked_out`]
    /// compares those bytes, so no spelling of one refname is mistaken for
    /// another.
    ///
    /// # Errors
    ///
    /// [`MalformedRecord::Branch`] for a value outside the byte set or a
    /// second `branch`.
    pub(super) fn branch(&mut self, value: &'a [u8]) -> Result<(), MalformedRecord> {
        if self.branch.is_some() || !can_be_refname(value) {
            return Err(MalformedRecord::Branch);
        }
        self.branch = Some(value);
        Ok(())
    }

    /// `detached`, a label with no value.
    ///
    /// # Errors
    ///
    /// [`MalformedRecord::BooleanTwice`] for a second `detached`.
    pub(super) fn detached(&mut self) -> Result<(), MalformedRecord> {
        if self.detached {
            return Err(MalformedRecord::BooleanTwice);
        }
        self.detached = true;
        Ok(())
    }

    /// `bare`, a label with no value.
    ///
    /// # Errors
    ///
    /// [`MalformedRecord::BooleanTwice`] for a second `bare`.
    pub(super) fn bare(&mut self) -> Result<(), MalformedRecord> {
        if self.bare {
            return Err(MalformedRecord::BooleanTwice);
        }
        self.bare = true;
        Ok(())
    }

    /// `locked` or `locked <reason>`, the reason's bytes and the empty slice
    /// for the bare label.
    ///
    /// # Errors
    ///
    /// [`MalformedRecord::ReasonTwice`] for a second `locked`.
    pub(super) fn locked(&mut self, reason: &'a [u8]) -> Result<(), MalformedRecord> {
        if self.locked.is_some() {
            return Err(MalformedRecord::ReasonTwice);
        }
        self.locked = Some(reason);
        Ok(())
    }

    /// `prunable` or `prunable <reason>`, the reason's bytes and the empty
    /// slice for the bare label.
    ///
    /// # Errors
    ///
    /// [`MalformedRecord::ReasonTwice`] for a second `prunable`.
    pub(super) fn prunable(&mut self, reason: &'a [u8]) -> Result<(), MalformedRecord> {
        if self.prunable.is_some() {
            return Err(MalformedRecord::ReasonTwice);
        }
        self.prunable = Some(reason);
        Ok(())
    }

    /// The empty attribute: the record is complete, if its attributes are a
    /// worktree.
    ///
    /// **The shape rule, measured on the box (Git 2.43.0) over every shape a
    /// repository could be put into**: a bare repository's own worktree lists
    /// as `bare` and nothing else; every other worktree lists a `HEAD` and
    /// then exactly one of `branch <refname>` and `detached`. That holds for
    /// the main worktree and a linked one, for `--detach`, `--no-checkout` and
    /// `-b`, for a repository with no commits at all (`HEAD` is the null id
    /// and the unborn branch is still named), and for a registration whose
    /// checkout is gone (`HEAD` null, `detached`, `prunable …`); `locked` and
    /// `prunable` are orthogonal to it.
    ///
    /// Anything else is refused rather than read, because the alternative is
    /// to answer a question about a worktree from evidence that does not say:
    /// a record with a `HEAD` and no `branch` would otherwise reach
    /// `assert_publishable` as "this worktree has no branch checked out", and
    /// a `bare` record carrying a `branch` as a checkout that is not one.
    /// §14: malformed or contradictory external evidence fails closed.
    ///
    /// The borrowed attributes become the owned record here, the one copy
    /// this module makes. The two reasons are decoded with replacement
    /// characters: they are compared with one ASCII word and shown, never used
    /// as identity (§8).
    ///
    /// # Errors
    ///
    /// [`MalformedRecord::BareWithCheckout`], [`MalformedRecord::NoHead`],
    /// [`MalformedRecord::BranchAndDetached`] or
    /// [`MalformedRecord::NeitherBranchNorDetached`].
    pub(super) fn close(self) -> Result<WorktreeRecord, MalformedRecord> {
        if self.bare {
            if self.head.is_some() || self.branch.is_some() || self.detached {
                return Err(MalformedRecord::BareWithCheckout);
            }
        } else if self.head.is_none() {
            return Err(MalformedRecord::NoHead);
        } else if self.branch.is_some() && self.detached {
            return Err(MalformedRecord::BranchAndDetached);
        } else if self.branch.is_none() && !self.detached {
            return Err(MalformedRecord::NeitherBranchNorDetached);
        }
        Ok(WorktreeRecord {
            path: self.path,
            head: self.head.map(str::to_owned),
            branch: self.branch.map(<[u8]>::to_vec),
            locked: self.locked.map(reason),
            prunable: self.prunable.map(reason),
        })
    }
}

impl WorktreeRecord {
    /// The checkout path, as the parser decoded it.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The checkout path, owned: for a refusal that carries it.
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }

    /// The commit its HEAD names, when it has one: a full hexadecimal object
    /// id, in the case Git printed it.
    #[must_use]
    pub fn head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    /// The branch it has checked out, when it is not detached: the bytes of a
    /// full refname (`refs/heads/…`), as Git printed them. `None` is a
    /// detached or bare worktree, which the shape rule makes a fact about the
    /// worktree rather than about Git's answer. Ask
    /// [`Self::has_checked_out`] rather than spelling the comparison.
    #[must_use]
    pub fn branch(&self) -> Option<&[u8]> {
        self.branch.as_deref()
    }

    /// Whether `refname` is, byte for byte, the branch this worktree has
    /// checked out.
    ///
    /// A short name is not the branch, and a branch Git spells with bytes that
    /// are not UTF-8 equals no `&str` at all rather than the `U+FFFD` spelling
    /// a lossy decode would give it. A detached worktree has checked out
    /// nothing.
    ///
    /// **`false` is not proof that this worktree holds a different ref.**
    /// Whether two spellings name one ref is the ref store's question, not
    /// this value's: with the files backend on a case-insensitive filesystem
    /// (Git documents Windows and macOS), `refs/heads/x` and `refs/heads/X`
    /// can be one loose ref file while comparing unequal here, and Git prints
    /// whichever spelling the worktree's symbolic HEAD carries. So a caller
    /// refusing an action because a ref is checked out somewhere gets a
    /// **necessary** condition from this predicate and not a sufficient one.
    /// Answering it properly needs the repository and its backend, so it
    /// belongs to the parent (`SWEEP-WORKTREE-015`); no case folding is done
    /// here, because which spellings a store treats as one depends on the
    /// backend and the filesystem and a guess would break publication in the
    /// other direction.
    #[must_use]
    pub fn has_checked_out(&self, refname: &str) -> bool {
        self.branch.as_deref() == Some(refname.as_bytes())
    }

    /// Git's own lock reason: `None` when the worktree is not locked, `Some("")`
    /// for a lock taken without a reason, otherwise the reason Git printed,
    /// decoded with replacement characters where it is not UTF-8 (Git trims
    /// its own `locked` file, so the reason has no trailing whitespace; an
    /// embedded newline survives, which is why the parent asks for `-z`). It
    /// is a diagnostic and the one ASCII word [`Self::is_initializing`] looks
    /// for, never identity, so the lossy decode is the right one (§8).
    #[must_use]
    pub fn lock_reason(&self) -> Option<&str> {
        self.locked.as_deref()
    }

    /// Whether the lock reason is Git's word `initializing`, compared exactly.
    ///
    /// `git worktree add` writes that reason to the lock for the whole of its
    /// run and removes the lock (or, under `--lock`, replaces the reason) only
    /// once the checkout is populated, so a surviving `initializing` is how a
    /// registered-but-unpopulated worktree announces itself; a lock without a
    /// reason, or with any other, is somebody's lock on a worktree Git
    /// finished, and the parent reads it as populated.
    ///
    /// This is not provenance, and nothing downstream may say it is. The
    /// record cannot tell Git's `initializing` from the same word a repository
    /// writer puts there with `git worktree lock --reason initializing` on a
    /// populated worktree, which Git permits: both read `true`, and the
    /// parent's verification answers [`VerifyFailure::Unpopulated`] for both,
    /// whose text therefore reports the lock and not a history. The engine
    /// writes no marker of its own at the add funnel
    /// (`worktree add --detach --quiet`), so a marker only it produces is the
    /// parent's to add, and whether one is owed -- which turns on whether a
    /// writer to the execution root is inside this engine's trust boundary, a
    /// question no `DESIGN.md` section settles -- is
    /// `SWEEP-WORKTREE-013`, for the owner rather than for this file.
    #[must_use]
    pub fn is_initializing(&self) -> bool {
        self.locked.as_deref() == Some("initializing")
    }

    /// Git's own prunable reason: `None` when Git would not prune the entry,
    /// `Some("")` when it would and printed no reason, otherwise the reason
    /// Git printed, decoded like [`Self::lock_reason`].
    #[must_use]
    pub fn prunable_reason(&self) -> Option<&str> {
        self.prunable.as_deref()
    }
}

/// A reason for showing and for the one word the parent looks for, decoded
/// with `U+FFFD` where Git's bytes are not UTF-8; never identity, so the lossy
/// decode is the right one (§8).
fn reason(value: &[u8]) -> String {
    String::from_utf8_lossy(value).into_owned()
}

/// Whether `value` is a full refname, by every rule `git check-ref-format`
/// documents for one.
///
/// The rules, in the documentation's own order: a slash-separated component
/// may not begin with `.` or end with `.lock`; the name has at least one `/`
/// (this is a full refname, not a one-level one); no `..`; no ASCII control
/// character, space, DEL, `~`, `^` or `:`; no `?`, `*` or `[`; no leading or
/// trailing `/` and no `//` (each is an empty component); no trailing `.`; no
/// `@{`; not the single character `@` (subsumed by the `/` rule, and checked
/// anyway); no `\`.
///
/// That is the whole documented list for a full refname, so a `branch`
/// attribute this accepts is one Git would accept — which is what the
/// publication check reads it as. It is a rule about the *spelling*: whether
/// two well-formed spellings name one ref is the ref store's question, which
/// [`WorktreeRecord::has_checked_out`] says this module does not answer.
fn can_be_refname(value: &[u8]) -> bool {
    if value.is_empty() || value == b"@" || !value.contains(&b'/') {
        return false;
    }
    if value
        .iter()
        .any(|byte| *byte <= b' ' || *byte == 0x7f || b"~^:?*[\\".contains(byte))
    {
        return false;
    }
    if value.windows(2).any(|pair| pair == b".." || pair == b"@{") {
        return false;
    }
    if value.last() == Some(&b'.') {
        return false;
    }
    value.split(|byte| *byte == b'/').all(|component| {
        !component.is_empty() && component.first() != Some(&b'.') && !component.ends_with(b".lock")
    })
}

/// Why [`WorkspaceManager::verify_worktree`](super::WorkspaceManager::verify_worktree)
/// refused to reuse a worktree.
///
/// One variant per observation that stops a reuse. Which observations are made,
/// and in which order, is `verify_worktree`'s and not stated here twice; this
/// enum is the vocabulary its answer is given in, and the module documentation
/// says why it quotes no rule.
///
/// The caller's action is decided by the generation's class, not by the
/// variant: an open generation is removed with force and re-added, a retained
/// one is closed. The variant is what a caller reads today
/// (`Reuse::Recreated`, `RetryOutcome::Close`); nothing renders the `Display`
/// to an operator yet (`SWEEP-WORKTREE-007`, the engine's), and each arm
/// carries what it compared so that a renderer has it.
///
/// `Clone` because [`Reuse`](crate::engine::topology::dispatch::Reuse) derives
/// it and carries a failure, and the engine's test doubles answer one recorded
/// failure to every caller; `PartialEq` because those doubles and this
/// module's own tests compare the failure they were given with the one that
/// came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyFailure {
    /// Nothing is registered at the recorded path.
    NotRegistered,
    /// Registered, and locked with Git's word `initializing`.
    ///
    /// That is the lock `git worktree add` holds while it populates a
    /// checkout, so the reuse path treats it as the
    /// `registered-but-unpopulated` residue element. It is not proof of one:
    /// a repository writer may lock a populated worktree with the same word,
    /// and the record cannot tell ([`WorktreeRecord::is_initializing`]), which
    /// is why this variant's text reports the lock rather than a history.
    Unpopulated,
    /// Registered at the path but belonging to a different repository.
    ForeignRepository,
    /// The checkout directory is gone.
    Missing,
    /// HEAD is not the recorded base.
    HeadMismatch {
        /// The recorded base.
        expected: String,
        /// What HEAD actually is.
        actual: String,
    },
    /// The retained cumulative tree is not the one the worktree holds.
    TreeMismatch {
        /// The recorded tree.
        expected: String,
        /// Why the index does not hold it: the paths that differ, or the reason
        /// the comparison could not be made against that tree at all.
        ///
        /// This was the tree the index writes out as, and obtaining it meant
        /// running `git write-tree`, which **writes** (`PR5-CONF-002`). A
        /// read-only observation cannot name a tree object that does not exist
        /// yet, so it names the difference instead — which is the more useful
        /// half of that diagnostic anyway.
        difference: String,
    },
    /// Administrative residue of an interrupted command.
    Residue(ResidueElement),
}

impl fmt::Display for VerifyFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRegistered => f.write_str("no worktree is registered at the recorded path"),
            Self::Unpopulated => f.write_str(
                "the worktree is registered and holds an `initializing` lock, the lock \
                 `git worktree add` writes while it populates a checkout",
            ),
            Self::ForeignRepository => {
                f.write_str("the worktree at the recorded path belongs to another repository")
            }
            Self::Missing => f.write_str("the worktree's checkout directory is gone"),
            Self::HeadMismatch { expected, actual } => {
                write!(
                    f,
                    "the worktree's HEAD is {actual}, not the recorded base {expected}"
                )
            }
            Self::TreeMismatch {
                expected,
                difference,
            } => write!(
                f,
                "the worktree does not hold the retained cumulative tree {expected}: {difference}"
            ),
            Self::Residue(element) => write!(
                f,
                "administrative residue of an interrupted command is present: {element:?}"
            ),
        }
    }
}

/// What a worktree has to hold for
/// [`WorkspaceManager::verify_worktree`](super::WorkspaceManager::verify_worktree)
/// to pass it.
///
/// `Clone` because the engine's test doubles record every question they were
/// asked; production passes a quiescence by reference and never copies one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Quiescence {
    /// HEAD must equal this commit: the ordinary case, an open generation at
    /// its recorded base.
    AtBase(String),
    /// The worktree's index must hold this tree: a retained generation, whose
    /// cumulative work no base can be re-cut into.
    HoldsTree(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA1: &str = "88663d58b63b0acaf3c31e98aa723336b24f1510";
    const SHA256: &str = "88663d58b63b0acaf3c31e98aa723336b24f151088663d58b63b0acaf3c31e98";

    /// An open record with nothing read yet.
    fn open() -> OpenRecord<'static> {
        OpenRecord::at(PathBuf::from("slot"))
    }

    /// An open record of the shape Git prints for a detached worktree, which
    /// [`OpenRecord::close`] accepts: a HEAD and `detached`.
    fn detached() -> OpenRecord<'static> {
        let mut open = open();
        open.head(SHA1.as_bytes()).expect("one HEAD");
        open.detached().expect("detached once");
        open
    }

    /// The record `open` spells, which must be a shape Git prints.
    fn close(open: OpenRecord<'_>) -> WorktreeRecord {
        open.close().expect("a shape Git prints")
    }

    /// A detached record with one lock reason and, optionally, one prunable
    /// reason.
    fn locked_with(lock: Option<&[u8]>, prunable: Option<&[u8]>) -> WorktreeRecord {
        let mut open = detached();
        if let Some(reason) = lock {
            open.locked(reason).expect("one lock");
        }
        if let Some(reason) = prunable {
            open.prunable(reason).expect("one prunable");
        }
        close(open)
    }

    /// A record checking out `branch`.
    fn on_branch(branch: &[u8]) -> WorktreeRecord {
        let mut open = open();
        open.head(SHA1.as_bytes()).expect("one HEAD");
        open.branch(branch).expect("a branch inside the byte set");
        close(open)
    }

    /// The path is the parser's decoding, taken as given and handed back
    /// owned for the refusal that names it; a bare worktree has nothing else.
    #[test]
    fn the_path_is_taken_as_decoded_and_handed_back_owned() {
        let path = PathBuf::from("root").join("tasks").join("kalpha-g1");
        let mut open = OpenRecord::at(path.clone());
        open.bare().expect("bare once");
        let bare = close(open);
        assert_eq!(bare.path(), path.as_path());
        assert_eq!(bare.head(), None);
        assert_eq!(bare.branch(), None);
        assert_eq!(bare.lock_reason(), None);
        assert_eq!(bare.prunable_reason(), None);
        assert!(!bare.is_initializing());
        assert_eq!(bare.into_path(), path);
    }

    /// The shape rule: a worktree is bare, or names a HEAD and exactly one of
    /// `branch` and `detached`. Every combination Git 2.43.0 was measured
    /// printing is accepted and every other refused, so `branch()` answering
    /// `None` is a fact about the worktree and not about Git's answer.
    #[test]
    fn a_record_is_bare_or_a_head_with_exactly_one_of_branch_and_detached() {
        let head = || {
            let mut open = open();
            open.head(SHA1.as_bytes()).expect("one HEAD");
            open
        };
        let bare = || {
            let mut open = open();
            open.bare().expect("bare once");
            open
        };

        let mut on_branch = head();
        on_branch.branch(b"refs/heads/main").expect("one branch");
        assert!(on_branch.close().is_ok(), "a worktree on a branch");
        let mut apart = head();
        apart.detached().expect("detached once");
        assert!(apart.close().is_ok(), "a detached worktree");
        let mut locked = bare();
        locked.locked(b"").expect("one lock");
        assert!(locked.close().is_ok(), "a lock is orthogonal to the shape");
        assert!(bare().close().is_ok(), "a bare repository");

        assert_eq!(
            head().close(),
            Err(MalformedRecord::NeitherBranchNorDetached),
            "a HEAD says nothing about a branch on its own"
        );
        let mut both = head();
        both.branch(b"refs/heads/main").expect("one branch");
        both.detached().expect("detached once");
        assert_eq!(both.close(), Err(MalformedRecord::BranchAndDetached));
        let mut bare_head = bare();
        bare_head.head(SHA1.as_bytes()).expect("one HEAD");
        assert_eq!(bare_head.close(), Err(MalformedRecord::BareWithCheckout));
        let mut bare_branch = bare();
        bare_branch.branch(b"refs/heads/main").expect("one branch");
        assert_eq!(bare_branch.close(), Err(MalformedRecord::BareWithCheckout));
        let mut bare_apart = bare();
        bare_apart.detached().expect("detached once");
        assert_eq!(bare_apart.close(), Err(MalformedRecord::BareWithCheckout));
        assert_eq!(
            open().close(),
            Err(MalformedRecord::NoHead),
            "a record naming nothing is not a worktree"
        );
        let mut only_branch = open();
        only_branch.branch(b"refs/heads/main").expect("one branch");
        assert_eq!(only_branch.close(), Err(MalformedRecord::NoHead));
    }

    /// A `HEAD` is forty or sixty-four hexadecimal digits in either case and
    /// nothing else, read once: the record refuses what `is_object_id`
    /// refuses, a value that is not UTF-8 by the same test, and a second
    /// `HEAD` by the same name.
    #[test]
    fn a_head_is_a_full_hexadecimal_object_id_of_either_length_read_once() {
        let sha1_upper = SHA1.to_ascii_uppercase();
        let accepted: &[(&str, &[u8])] = &[
            ("a SHA-1", SHA1.as_bytes()),
            ("a SHA-256", SHA256.as_bytes()),
            ("an uppercase SHA-1", sha1_upper.as_bytes()),
        ];
        for (name, head) in accepted {
            let mut open = open();
            assert_eq!(open.head(head), Ok(()), "{name} is one object id");
            assert_eq!(
                open.head(SHA1.as_bytes()),
                Err(MalformedRecord::Head),
                "{name}: a second HEAD is not one object id"
            );
            open.detached().expect("detached once");
            assert_eq!(
                close(open).head().map(str::as_bytes),
                Some(*head),
                "{name} is kept as printed"
            );
        }

        let thirty_nine = &SHA1[..39];
        let forty_one = format!("{SHA1}0");
        let sixty_three = &SHA256[..63];
        let sixty_five = format!("{SHA256}0");
        let not_hex = format!("{}g", &SHA1[..39]);
        let trailing_space = format!("{SHA1} ");
        let not_utf8 = [0xffu8; 40];
        let refused: &[(&str, &[u8])] = &[
            ("thirty-nine digits", thirty_nine.as_bytes()),
            ("forty-one digits", forty_one.as_bytes()),
            ("sixty-three digits", sixty_three.as_bytes()),
            ("sixty-five digits", sixty_five.as_bytes()),
            ("a byte outside hexadecimal", not_hex.as_bytes()),
            ("a trailing space", trailing_space.as_bytes()),
            ("an empty value", b""),
            ("forty bytes that are not UTF-8", &not_utf8),
        ];
        for (name, head) in refused {
            let mut open = open();
            assert_eq!(
                open.head(head),
                Err(MalformedRecord::Head),
                "{name} is not one object id"
            );
            assert_eq!(
                open.close(),
                Err(MalformedRecord::NoHead),
                "{name} stored nothing"
            );
        }
        assert_eq!(refused.len(), 8, "eight independent refusals");
    }

    /// Every rule `git check-ref-format` documents for a full refname is
    /// applied, one case per rule from that list, and every `branch` value
    /// measured on Git 2.43.0 is still accepted.
    #[test]
    fn a_branch_is_a_full_refname_by_every_documented_rule() {
        let refused: &[(&str, &[u8])] = &[
            ("a component beginning with a dot", b"refs/heads/.hidden"),
            ("a component ending in .lock", b"refs/heads/main.lock"),
            (
                "a middle component ending in .lock",
                b"refs/heads.lock/main",
            ),
            ("no slash at all", b"main"),
            ("two dots", b"refs/heads/a..b"),
            ("a leading slash", b"/refs/heads/main"),
            ("a trailing slash", b"refs/heads/main/"),
            ("two slashes", b"refs//heads/main"),
            ("a trailing dot", b"refs/heads/main."),
            ("an at-brace", b"refs/heads/ma@{in"),
            ("the single character at", b"@"),
            ("a backslash", b"refs/heads/ma\\in"),
            ("a tilde", b"refs/heads/ma~in"),
            ("a caret", b"refs/heads/ma^in"),
            ("a colon", b"refs/heads/ma:in"),
            ("a question mark", b"refs/heads/ma?in"),
            ("an asterisk", b"refs/heads/ma*in"),
            ("an open bracket", b"refs/heads/ma[in"),
            ("a space", b"refs/heads/ma in"),
            ("DEL", b"refs/heads/ma\x7fin"),
            ("a control byte", b"refs/heads/ma\x01in"),
            ("nothing at all", b""),
        ];
        for (name, branch) in refused {
            let mut open = detached();
            assert_eq!(
                open.branch(branch),
                Err(MalformedRecord::Branch),
                "{name} is not a full refname"
            );
        }
        assert_eq!(refused.len(), 22, "one case per documented rule");

        // Every `branch` value the twelve measured shapes printed on Git
        // 2.43.0, so the rule cannot over-refuse what Git itself writes.
        let accepted: &[(&str, &[u8])] = &[
            ("the main worktree", b"refs/heads/master"),
            ("a linked worktree", b"refs/heads/wt-branch"),
            (
                "an unborn branch in a fresh repository",
                b"refs/heads/master",
            ),
            ("`worktree add -b`", b"refs/heads/newbr"),
            (
                "this engine's run branch",
                b"refs/heads/upstroke/run-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            ),
            ("a branch with a dot inside a component", b"refs/heads/v1.2"),
            ("a deep namespace", b"refs/remotes/origin/feature/x"),
        ];
        for (name, branch) in accepted {
            let mut open = detached();
            assert_eq!(open.branch(branch), Ok(()), "{name} is a full refname");
        }
        assert_eq!(accepted.len(), 7, "seven measured or engine-written names");
    }

    /// A `branch` is read once and kept as the bytes Git printed, not a
    /// spelling of them.
    #[test]
    fn a_branch_is_read_once_and_kept_as_bytes() {
        let mut forbidden: Vec<u8> = (0..=b' ').collect();
        forbidden.push(0x7f);
        forbidden.extend_from_slice(b"~^:?*[\\");
        assert_eq!(
            forbidden.len(),
            41,
            "33 control bytes and space, DEL, seven bytes"
        );
        for byte in forbidden {
            let mut branch = b"refs/heads/ma".to_vec();
            branch.push(byte);
            branch.extend_from_slice(b"in");
            let mut open = detached();
            assert_eq!(
                open.branch(&branch),
                Err(MalformedRecord::Branch),
                "byte {byte:#04x} cannot be in a refname"
            );
            assert_eq!(
                close(open).branch(),
                None,
                "byte {byte:#04x} stored nothing"
            );
        }

        let mut twice = open();
        twice.head(SHA1.as_bytes()).expect("one HEAD");
        assert_eq!(twice.branch(b"refs/heads/main"), Ok(()));
        assert_eq!(
            twice.branch(b"refs/heads/other"),
            Err(MalformedRecord::Branch),
            "a second branch is not one refname"
        );
        assert_eq!(close(twice).branch(), Some(&b"refs/heads/main"[..]));

        let latin1: &[u8] = b"refs/heads/caf\xe9";
        assert_eq!(
            on_branch(latin1).branch(),
            Some(latin1),
            "bytes that are not UTF-8 are a refname to Git and are kept as printed"
        );
        let utf8 = "refs/heads/café".as_bytes();
        assert_eq!(on_branch(utf8).branch(), Some(utf8));
    }

    /// `detached` and `bare` are labels read once each; a second is refused
    /// by name, as the other attributes are.
    #[test]
    fn a_boolean_attribute_is_read_once() {
        let mut open = open();
        assert_eq!(open.detached(), Ok(()));
        assert_eq!(
            open.detached(),
            Err(MalformedRecord::BooleanTwice),
            "detached twice"
        );
        assert_eq!(open.bare(), Ok(()), "bare is a different label");
        assert_eq!(
            open.bare(),
            Err(MalformedRecord::BooleanTwice),
            "bare twice"
        );
    }

    /// `has_checked_out` is byte equality with the full refname: no short
    /// name, no trailing byte, no lossy spelling, and nothing on a detached
    /// worktree.
    #[test]
    fn checked_out_is_byte_equality_with_the_full_refname() {
        let main = on_branch(b"refs/heads/main");
        assert!(main.has_checked_out("refs/heads/main"));
        assert!(
            !main.has_checked_out("main"),
            "a short name is not the branch"
        );
        assert!(!main.has_checked_out("refs/heads/main "));
        assert!(!main.has_checked_out("refs/heads/mai"));
        assert!(!main.has_checked_out(""));

        let latin1 = on_branch(b"refs/heads/caf\xe9");
        assert!(
            !latin1.has_checked_out("refs/heads/caf\u{FFFD}"),
            "the lossy spelling of a branch is not the branch"
        );
        assert!(
            !latin1.has_checked_out("refs/heads/café"),
            "the UTF-8 spelling of the same letters is a different refname to Git"
        );
        assert!(on_branch("refs/heads/café".as_bytes()).has_checked_out("refs/heads/café"));

        let apart = close(detached());
        assert!(!apart.has_checked_out("refs/heads/main"));
        assert!(!apart.has_checked_out(""));
    }

    /// A lock without a reason is a lock (`Some("")`), not an absent
    /// attribute (`None`); only the one word `initializing` is read as
    /// initializing, and it is read as such whoever wrote it, which the doc
    /// says the record cannot tell; the prunable reason is read the same way
    /// and never mistaken for the lock; neither is read twice.
    #[test]
    fn a_bare_lock_is_a_lock_without_a_reason_and_only_initializing_is_initializing() {
        let unlocked = locked_with(None, None);
        assert_eq!(unlocked.lock_reason(), None);
        assert!(!unlocked.is_initializing());

        let bare = locked_with(Some(b""), None);
        assert_eq!(bare.lock_reason(), Some(""), "locked, with no reason");
        assert!(
            !bare.is_initializing(),
            "somebody's lock on a populated worktree"
        );

        // Git's own `worktree add` lock, and a writer's
        // `git worktree lock --reason initializing` on a populated worktree,
        // print the same attribute: the record reads both as initializing.
        let initializing = locked_with(Some(b"initializing"), None);
        assert_eq!(initializing.lock_reason(), Some("initializing"));
        assert!(initializing.is_initializing());

        let not_quite: &[(&str, &[u8])] = &[
            ("a trailing space", b"initializing "),
            ("a leading space", b" initializing"),
            ("a prefix", b"initializing the checkout"),
            ("a different case", b"Initializing"),
            ("a suffix", b"still initializing"),
        ];
        for (name, reason) in not_quite {
            let locked = locked_with(Some(reason), None);
            assert!(!locked.is_initializing(), "{name} is not Git's own lock");
            assert_eq!(
                locked.lock_reason().map(str::as_bytes),
                Some(*reason),
                "{name} keeps its bytes"
            );
        }

        let kept = locked_with(Some(b"why\nnot"), Some(b""));
        assert_eq!(kept.lock_reason(), Some("why\nnot"));
        assert_eq!(kept.prunable_reason(), Some(""), "prunable, with no reason");

        let lossy = locked_with(Some(b"caf\xe9"), Some(b"caf\xe9"));
        assert_eq!(
            lossy.lock_reason(),
            Some("caf\u{FFFD}"),
            "a reason is shown, never identity"
        );
        assert_eq!(lossy.prunable_reason(), Some("caf\u{FFFD}"));

        let prunable = locked_with(None, Some(b"initializing"));
        assert_eq!(prunable.prunable_reason(), Some("initializing"));
        assert!(
            !prunable.is_initializing(),
            "the prunable reason is not the lock"
        );

        let mut twice = detached();
        assert_eq!(twice.locked(b""), Ok(()));
        assert_eq!(
            twice.locked(b"again"),
            Err(MalformedRecord::ReasonTwice),
            "locked twice"
        );
        assert_eq!(twice.prunable(b""), Ok(()), "prunable is a different label");
        assert_eq!(
            twice.prunable(b"again"),
            Err(MalformedRecord::ReasonTwice),
            "prunable twice"
        );
        let twice = close(twice);
        assert_eq!(twice.lock_reason(), Some(""), "the first reason stands");
        assert_eq!(twice.prunable_reason(), Some(""));
    }

    /// The refusal's text is the predicate the parser joins after the
    /// record's number, which its tests assert by that spelling.
    #[test]
    fn a_malformed_attribute_reads_as_a_predicate_of_the_record() {
        assert_eq!(
            MalformedRecord::Head.to_string(),
            "has a HEAD that is not one object id"
        );
        assert_eq!(
            MalformedRecord::Branch.to_string(),
            "has a branch that is not one refname"
        );
        assert_eq!(
            MalformedRecord::BooleanTwice.to_string(),
            "has a boolean attribute twice"
        );
        assert_eq!(
            MalformedRecord::ReasonTwice.to_string(),
            "has a reason attribute twice"
        );
        assert_eq!(
            MalformedRecord::BareWithCheckout.to_string(),
            "is bare and also names a HEAD, a branch or a detached checkout"
        );
        assert_eq!(
            MalformedRecord::NoHead.to_string(),
            "names no HEAD and is not bare"
        );
        assert_eq!(
            MalformedRecord::BranchAndDetached.to_string(),
            "is on a branch and detached at once"
        );
        assert_eq!(
            MalformedRecord::NeitherBranchNorDetached.to_string(),
            "names a HEAD but neither a branch nor a detached checkout"
        );
    }

    /// Every failure displays as a fragment (lowercase start, no trailing
    /// period, so a report chain can join it after `": "`) that carries what
    /// it compared; the match below is exhaustive, so a new variant must be
    /// added to the cases before the crate compiles.
    #[test]
    fn every_verify_failure_displays_as_a_lowercase_fragment_carrying_its_fields() {
        let mut cases = vec![
            VerifyFailure::NotRegistered,
            VerifyFailure::Unpopulated,
            VerifyFailure::ForeignRepository,
            VerifyFailure::Missing,
            VerifyFailure::HeadMismatch {
                expected: "expected-base".to_owned(),
                actual: "actual-head".to_owned(),
            },
            VerifyFailure::TreeMismatch {
                expected: "expected-tree".to_owned(),
                difference: "the-difference".to_owned(),
            },
        ];
        cases.extend(
            ResidueElement::ALL
                .iter()
                .map(|element| VerifyFailure::Residue(*element)),
        );
        let mut seen = [false; 7];
        for failure in &cases {
            let text = failure.to_string();
            assert!(
                text.starts_with(|c: char| c.is_ascii_lowercase()),
                "{failure:?} starts lowercase: {text:?}"
            );
            assert!(
                !text.ends_with('.'),
                "{failure:?} has no trailing period: {text:?}"
            );
            match failure {
                VerifyFailure::NotRegistered => seen[0] = true,
                VerifyFailure::Unpopulated => {
                    seen[1] = true;
                    // What was observed, not a history the record cannot
                    // know: a writer's `--reason initializing` on a populated
                    // worktree reaches this variant too.
                    assert!(
                        text.contains("holds an `initializing` lock"),
                        "{text:?} names the lock it saw"
                    );
                    assert!(
                        !text.contains("never populated"),
                        "{text:?} does not claim a history"
                    );
                }
                VerifyFailure::ForeignRepository => seen[2] = true,
                VerifyFailure::Missing => seen[3] = true,
                VerifyFailure::HeadMismatch { expected, actual } => {
                    seen[4] = true;
                    assert_eq!(
                        text,
                        format!(
                            "the worktree's HEAD is {actual}, not the recorded base {expected}"
                        )
                    );
                }
                VerifyFailure::TreeMismatch {
                    expected,
                    difference,
                } => {
                    seen[5] = true;
                    assert_eq!(
                        text,
                        format!(
                            "the worktree does not hold the retained cumulative tree {expected}: \
                             {difference}"
                        )
                    );
                }
                VerifyFailure::Residue(element) => {
                    seen[6] = true;
                    assert!(
                        text.ends_with(&format!("{element:?}")),
                        "{text:?} names {element:?}"
                    );
                }
            }
        }
        assert!(
            seen.iter().all(|seen| *seen),
            "every variant is displayed: {seen:?}"
        );
        assert_eq!(cases.len(), 6 + ResidueElement::ALL.len());
    }
}
