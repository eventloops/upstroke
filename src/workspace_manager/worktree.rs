//! What Git reports about a linked worktree, and what
//! [`WorkspaceManager::verify_worktree`](super::WorkspaceManager::verify_worktree)
//! demands of one before it is reused.
//!
//! `decisions.workspace_candidates.generation` (its substance is `DESIGN.md`
//! §26 since the record was retired): "a worktree is reused across a process
//! boundary or after an interrupted Git command … only after Worktree.Verify".
//! These are the three values that conversation is held in -- the record Git
//! hands back, the quiescence the caller asks for, and the reasons the answer
//! can be no. The verification itself runs a Git child and is the parent's; the
//! record is produced by `parsers.rs`, which reads the `--porcelain -z` framing,
//! and this module holds the grammar of the record's own attributes.

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
/// The value holds the grammar of its structural attributes: a `HEAD` is a
/// full hexadecimal object id and a `branch` is inside the refname byte set.
/// Both are applied once, in [`WorktreeRecord::from_porcelain`], which is the
/// one way to build a record; the fields are private so that nothing
/// constructs one around it (`locked` is readable by the parent module for
/// now, and says why). The path and the two reasons are verbatim.
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
    /// `pub(super)` for one reader and for now: `residue.rs` (queue row 6,
    /// PR #128 in flight) compares it with `initializing` at two sites, and
    /// that file is not this sweep's to edit. Its two lines move to
    /// [`WorktreeRecord::is_initializing`] and this field goes private with
    /// the base merge-in that follows #128; every other reader asks the
    /// accessor.
    pub(super) locked: Option<String>,
    /// Git's own prunable reason, when the worktree is prunable; empty when
    /// Git gave none.
    prunable: Option<String>,
}

/// One record's attributes as `git worktree list --porcelain -z` printed them,
/// before the grammar is applied: the parser's accumulator, and
/// [`WorktreeRecord::from_porcelain`]'s input.
///
/// The path is already decoded, because which bytes can be a path is the
/// platform's question and the parser answers it per platform. Every other
/// attribute is the bytes after the label's one space, borrowed from the
/// answer; `Some(b"")` is a label Git printed with no value, which is how a
/// lock or a prunable entry without a reason is listed (measured, Git 2.43.0:
/// `git worktree lock <path>` lists as `locked`, and `--reason "why  "` as
/// `locked why`, Git having trimmed its own file). `None` is an attribute the
/// record does not carry.
#[derive(Debug)]
pub(super) struct Attributes<'a> {
    /// `worktree <path>`, decoded.
    pub(super) path: PathBuf,
    /// `HEAD <sha>`.
    pub(super) head: Option<&'a [u8]>,
    /// `branch <refname>`.
    pub(super) branch: Option<&'a [u8]>,
    /// `locked` or `locked <reason>`.
    pub(super) locked: Option<&'a [u8]>,
    /// `prunable` or `prunable <reason>`.
    pub(super) prunable: Option<&'a [u8]>,
}

/// Why [`WorktreeRecord::from_porcelain`] refused a record: the attribute
/// that is outside its grammar.
///
/// The text is a predicate of the record, for the parser to join after the
/// record's number: "record 0 has a HEAD that is not one object id".
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum MalformedAttribute {
    /// `HEAD` is not forty or sixty-four hexadecimal digits.
    #[error("has a HEAD that is not one object id")]
    Head,
    /// `branch` is empty or carries a byte no refname can.
    #[error("has a branch that is not one refname")]
    Branch,
}

impl WorktreeRecord {
    /// Apply the grammar to one record's attributes.
    ///
    /// `HEAD` is a full hexadecimal object id of either hash length, decided
    /// by the same predicate the ref transitions apply
    /// ([`is_object_id`](super::is_object_id)); an object id is ASCII, so a
    /// value that is not UTF-8 fails that same test and the offset at which
    /// it stopped being UTF-8 would add nothing. `branch` is kept as the bytes
    /// Git printed, which [`can_be_refname`] holds to the byte set of
    /// `git check-ref-format`: a refname is bytes to Git and need not be UTF-8
    /// on Unix, and [`WorktreeRecord::has_checked_out`] compares those bytes,
    /// so no spelling of one refname is mistaken for another. The two reasons
    /// are decoded lossily: they are consulted for the one word
    /// `initializing` and shown to an operator, never used as identity (§8).
    ///
    /// # Errors
    ///
    /// [`MalformedAttribute`] naming the attribute outside its grammar.
    pub(super) fn from_porcelain(attributes: Attributes<'_>) -> Result<Self, MalformedAttribute> {
        let Attributes {
            path,
            head,
            branch,
            locked,
            prunable,
        } = attributes;
        let head = match head {
            None => None,
            Some(value) => match std::str::from_utf8(value) {
                Ok(text) if is_object_id(text) => Some(text.to_owned()),
                Ok(_) | Err(_) => return Err(MalformedAttribute::Head),
            },
        };
        let branch = match branch {
            None => None,
            Some(value) if can_be_refname(value) => Some(value.to_vec()),
            Some(_) => return Err(MalformedAttribute::Branch),
        };
        Ok(Self {
            path,
            head,
            branch,
            locked: locked.map(reason),
            prunable: prunable.map(reason),
        })
    }

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
    /// full refname (`refs/heads/…`), as Git printed them. Ask
    /// [`Self::has_checked_out`] rather than spelling the comparison.
    #[must_use]
    pub fn branch(&self) -> Option<&[u8]> {
        self.branch.as_deref()
    }

    /// Whether `refname` is the branch this worktree has checked out.
    ///
    /// Byte equality with the full refname: a short name is not the branch,
    /// and a branch Git spells with bytes that are not UTF-8 equals no `&str`
    /// at all rather than the `U+FFFD` spelling a lossy decode would give it.
    /// A detached worktree has checked out nothing.
    #[must_use]
    pub fn has_checked_out(&self, refname: &str) -> bool {
        self.branch.as_deref() == Some(refname.as_bytes())
    }

    /// Git's own lock reason: `None` when the worktree is not locked, `Some("")`
    /// for a lock taken without a reason, otherwise the reason as Git printed
    /// it (Git trims its own `locked` file, so the reason has no trailing
    /// whitespace; an embedded newline survives, which is why the parent asks
    /// for `-z`).
    #[must_use]
    pub fn lock_reason(&self) -> Option<&str> {
        self.locked.as_deref()
    }

    /// Whether `git worktree add` is populating this worktree, or died before
    /// it had.
    ///
    /// `git worktree add` holds the lock reason `initializing` for the whole of
    /// its run and releases it only once the checkout is populated, so this is
    /// how a registered-but-unpopulated worktree announces itself, and it is
    /// exactly that word: a lock without a reason is somebody's lock on a
    /// populated worktree, and `git worktree lock --reason` on one is theirs
    /// too.
    #[must_use]
    pub fn is_initializing(&self) -> bool {
        self.locked.as_deref() == Some("initializing")
    }

    /// Git's own prunable reason: `None` when Git would not prune the entry,
    /// `Some("")` when it would and printed no reason, otherwise the reason as
    /// Git printed it.
    #[must_use]
    pub fn prunable_reason(&self) -> Option<&str> {
        self.prunable.as_deref()
    }
}

/// A reason as Git printed it, for showing and for the one word the parent
/// looks for; never identity, so the lossy decode is the right one (§8).
fn reason(value: &[u8]) -> String {
    String::from_utf8_lossy(value).into_owned()
}

/// Whether `value` can be a refname at all: `git check-ref-format` forbids
/// ASCII control characters, space, DEL and the seven bytes `~ ^ : ? * [ \`
/// anywhere in one, and a refname is never empty. Not the whole rule (the
/// `..`, `@{` and `.lock` clauses are not applied), but everything a stray or
/// hostile byte could add to a name Git itself checked out.
fn can_be_refname(value: &[u8]) -> bool {
    !value.is_empty()
        && !value
            .iter()
            .any(|byte| *byte <= b' ' || *byte == 0x7f || b"~^:?*[\\".contains(byte))
}

/// Why [`WorkspaceManager::verify_worktree`](super::WorkspaceManager::verify_worktree)
/// refused to reuse a worktree.
///
/// `decisions.workspace_candidates.generation`: "a worktree is reused across a
/// process boundary or after an interrupted Git command … only after
/// Worktree.Verify: the recorded path is a linked worktree of this repository,
/// HEAD equals the recorded base (or, for RetainedIdle, the worktree holds the
/// retained cumulative tree), the index is unlocked, and no
/// cherry-pick/merge/revert/sequencer/rebase state exists".
///
/// The caller's action is decided by the generation's class, not by the
/// variant: an open generation is removed with force and re-added, a retained
/// one is closed. The variant and its `Display` are what an operator is told
/// afterwards, so each names the observation and carries what it compared.
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
    /// Registered, and `git worktree add` never finished populating it — the
    /// `registered-but-unpopulated` residue element.
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
                "the worktree is registered and was never populated: `git worktree add` still \
                 holds its `initializing` lock",
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
    /// The ordinary case: HEAD equals the recorded base.
    AtBase(String),
    /// `RetainedIdle`: "the worktree holds the retained cumulative tree".
    HoldsTree(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA1: &str = "88663d58b63b0acaf3c31e98aa723336b24f1510";
    const SHA256: &str = "88663d58b63b0acaf3c31e98aa723336b24f151088663d58b63b0acaf3c31e98";

    fn attributes<'a>(
        head: Option<&'a [u8]>,
        branch: Option<&'a [u8]>,
        locked: Option<&'a [u8]>,
        prunable: Option<&'a [u8]>,
    ) -> Attributes<'a> {
        Attributes {
            path: PathBuf::from("slot"),
            head,
            branch,
            locked,
            prunable,
        }
    }

    fn record(attributes: Attributes<'_>) -> WorktreeRecord {
        WorktreeRecord::from_porcelain(attributes).expect("a record inside the grammar")
    }

    /// The path is the parser's decoding, taken as given and handed back
    /// owned for the refusal that names it.
    #[test]
    fn the_path_is_taken_as_decoded_and_handed_back_owned() {
        let path = PathBuf::from("root").join("tasks").join("kalpha-g1");
        let bare = record(Attributes {
            path: path.clone(),
            head: None,
            branch: None,
            locked: None,
            prunable: None,
        });
        assert_eq!(bare.path(), path.as_path());
        assert_eq!(bare.head(), None);
        assert_eq!(bare.branch(), None);
        assert_eq!(bare.lock_reason(), None);
        assert_eq!(bare.prunable_reason(), None);
        assert_eq!(bare.into_path(), path);
    }

    /// A `HEAD` is forty or sixty-four hexadecimal digits in either case and
    /// nothing else: the record refuses what `is_object_id` refuses, and a
    /// value that is not UTF-8 is refused by the same test.
    #[test]
    fn a_head_is_a_full_hexadecimal_object_id_of_either_length() {
        let sha1_upper = SHA1.to_ascii_uppercase();
        let accepted: &[(&str, &[u8])] = &[
            ("a SHA-1", SHA1.as_bytes()),
            ("a SHA-256", SHA256.as_bytes()),
            ("an uppercase SHA-1", sha1_upper.as_bytes()),
        ];
        for (name, head) in accepted {
            let kept = record(attributes(Some(head), None, None, None));
            assert_eq!(
                kept.head().map(str::as_bytes),
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
            assert_eq!(
                WorktreeRecord::from_porcelain(attributes(Some(head), None, None, None)),
                Err(MalformedAttribute::Head),
                "{name} is not one object id"
            );
        }
        assert_eq!(refused.len(), 8, "eight independent refusals");
    }

    /// A `branch` is inside the byte set `git check-ref-format` allows and is
    /// kept as the bytes Git printed, not a spelling of them.
    #[test]
    fn a_branch_is_inside_the_refname_byte_set_and_kept_as_bytes() {
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
            assert_eq!(
                WorktreeRecord::from_porcelain(attributes(None, Some(&branch), None, None)),
                Err(MalformedAttribute::Branch),
                "byte {byte:#04x} cannot be in a refname"
            );
        }
        assert_eq!(
            WorktreeRecord::from_porcelain(attributes(None, Some(b""), None, None)),
            Err(MalformedAttribute::Branch),
            "a refname is never empty"
        );

        let latin1: &[u8] = b"refs/heads/caf\xe9";
        let kept = record(attributes(None, Some(latin1), None, None));
        assert_eq!(
            kept.branch(),
            Some(latin1),
            "bytes that are not UTF-8 are a refname to Git and are kept as printed"
        );
        let utf8 = "refs/heads/café".as_bytes();
        let kept = record(attributes(None, Some(utf8), None, None));
        assert_eq!(kept.branch(), Some(utf8));
    }

    /// `has_checked_out` is byte equality with the full refname: no short
    /// name, no trailing byte, no lossy spelling, and nothing on a detached
    /// worktree.
    #[test]
    fn checked_out_is_byte_equality_with_the_full_refname() {
        let main = record(attributes(None, Some(b"refs/heads/main"), None, None));
        assert!(main.has_checked_out("refs/heads/main"));
        assert!(
            !main.has_checked_out("main"),
            "a short name is not the branch"
        );
        assert!(!main.has_checked_out("refs/heads/main "));
        assert!(!main.has_checked_out("refs/heads/mai"));
        assert!(!main.has_checked_out(""));

        let latin1 = record(attributes(None, Some(b"refs/heads/caf\xe9"), None, None));
        assert!(
            !latin1.has_checked_out("refs/heads/caf\u{FFFD}"),
            "the lossy spelling of a branch is not the branch"
        );
        assert!(
            !latin1.has_checked_out("refs/heads/café"),
            "the UTF-8 spelling of the same letters is a different refname to Git"
        );
        let utf8 = record(attributes(
            None,
            Some("refs/heads/café".as_bytes()),
            None,
            None,
        ));
        assert!(utf8.has_checked_out("refs/heads/café"));

        let detached = record(attributes(Some(SHA1.as_bytes()), None, None, None));
        assert!(!detached.has_checked_out("refs/heads/main"));
        assert!(!detached.has_checked_out(""));
    }

    /// A lock without a reason is a lock (`Some("")`), not an absent
    /// attribute (`None`); only the one word `initializing` is `git worktree
    /// add`'s lock; and the prunable reason is read the same way and never
    /// mistaken for the lock.
    #[test]
    fn a_bare_lock_is_a_lock_without_a_reason_and_only_initializing_is_initializing() {
        let unlocked = record(attributes(None, None, None, None));
        assert_eq!(unlocked.lock_reason(), None);
        assert!(!unlocked.is_initializing());

        let bare = record(attributes(None, None, Some(b""), None));
        assert_eq!(bare.lock_reason(), Some(""), "locked, with no reason");
        assert!(
            !bare.is_initializing(),
            "somebody's lock on a populated worktree"
        );

        let initializing = record(attributes(None, None, Some(b"initializing"), None));
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
            let locked = record(attributes(None, None, Some(reason), None));
            assert!(!locked.is_initializing(), "{name} is not Git's own lock");
            assert_eq!(
                locked.lock_reason().map(str::as_bytes),
                Some(*reason),
                "{name} is kept verbatim"
            );
        }

        let verbatim = record(attributes(None, None, Some(b"why\nnot"), Some(b"")));
        assert_eq!(verbatim.lock_reason(), Some("why\nnot"));
        assert_eq!(
            verbatim.prunable_reason(),
            Some(""),
            "prunable, with no reason"
        );

        let lossy = record(attributes(None, None, Some(b"caf\xe9"), Some(b"caf\xe9")));
        assert_eq!(
            lossy.lock_reason(),
            Some("caf\u{FFFD}"),
            "a reason is shown, never identity"
        );
        assert_eq!(lossy.prunable_reason(), Some("caf\u{FFFD}"));

        let prunable = record(attributes(None, None, None, Some(b"initializing")));
        assert_eq!(prunable.prunable_reason(), Some("initializing"));
        assert!(
            !prunable.is_initializing(),
            "the prunable reason is not the lock"
        );
    }

    /// The refusal's text is the predicate the parser joins after the
    /// record's number, which its tests assert by that spelling.
    #[test]
    fn a_malformed_attribute_reads_as_a_predicate_of_the_record() {
        assert_eq!(
            MalformedAttribute::Head.to_string(),
            "has a HEAD that is not one object id"
        );
        assert_eq!(
            MalformedAttribute::Branch.to_string(),
            "has a branch that is not one refname"
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
                VerifyFailure::Unpopulated => seen[1] = true,
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
