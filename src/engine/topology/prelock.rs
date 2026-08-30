//! O01 — the read-only pre-lock checks of a fresh schema-4 write command.
//!
//! `decisions.workspace_candidates.run_creation`, first clause:
//!
//! > every write command first performs its read-only pre-lock checks and only
//! > then takes the physical worktree lock … the pre-lock checks of a fresh run
//! > are: config validation (PR1); resolution of the `RunnerPolicy` from
//! > `[runner]` config **by read-only inspection** … with `runner_policy_sha256`
//! > computed over its canonical serialization — an unreachable runtime, an
//! > absent image reference, or an absent volume **refuses here, before any
//! > lock or other effect**; generation of the coordinator incarnation id
//! > (per-process ULID) and the run id (**no effect**).
//!
//! Three properties, and each is a separate thing to get wrong:
//!
//! * **Order.** Resolution precedes id generation, and both precede
//!   `Lock.AcquireWorktree`. This module cannot take a lock — it holds no
//!   repository path and calls nothing that opens one — so the ordering is a
//!   fact about what it can reach rather than about where its call site sits.
//! * **No residue.** Nothing here creates a directory, a file, a lock, a
//!   container or a process. The one filesystem call is
//!   [`std::fs::canonicalize`], which is read-only, and the one runtime
//!   interaction is [`resolve_container`]'s four inspections, every one of which
//!   is a read.
//! * **Unforgeable output.** [`PreLockChecked`] has private fields, lives alone
//!   in a module of its own, derives no `Clone`, `Copy` or `Default`, and has
//!   exactly one constructor — [`check`]. Every later prefix consumes it by
//!   value, so "the run id, the incarnation and the policy P0–P8 use are the
//!   ones resolved before the lock" is a type rather than a comment.
//!
//! # What this module deliberately does **not** do
//!
//! **Config validation.** `run_creation` names it first among the pre-lock
//! checks and it is PR1's: [`crate::engine::preflight::validate_inputs`] is
//! `config::load_captured` plus the plan analysis, and both write commands
//! already run it as their first statement. Re-running it here would perform a
//! second read of the plan and the config, which is exactly what
//! `preflight::Validated`'s doc calls out as proving nothing ("a snapshot beside
//! an independent read proves nothing about what was validated"). What this
//! module takes instead is config's own *product*, a [`RunnerSelection`], which
//! no caller can obtain without having parsed a configuration.
//!
//! **The worktree lock.** `startup.rs` owns O02. A module that both refused
//! before the lock and took it would make "before" unobservable.

use std::path::{Path, PathBuf};

use crate::config::RunnerSelection;
use crate::error::UpstrokeError;
use crate::runner::container::resolve::resolve_container;
use crate::runner::container::runtime::ContainerRuntime;
use crate::runner::policy::{resolve_host, runner_policy_sha256};
use crate::topology::events::{IncarnationId, RunnerKind, RunnerPolicy};

use super::seams::IdSource;

pub use checked::PreLockChecked;

/// The witness, alone in a module, the way [`crate::rundir`]'s `ownership` is.
///
/// Field privacy is what makes [`check`] the only constructor: no code inside
/// `prelock` or outside it can name these fields, so a `PreLockChecked` in
/// anybody's hand is one that came out of the pre-lock sequence. It derives no
/// `Clone` (two copies would be two runs claiming one id), no `Copy` and no
/// `Default`.
mod checked {
    use super::{IncarnationId, Path, PathBuf, RunnerPolicy};

    /// The read-only pre-lock checks, performed.
    ///
    /// Carries exactly what the publication prefixes need and nothing they can
    /// re-derive: the resolved policy (P3b's owner record and P6's
    /// `run_started`), its digest (P1's marker and every container intent), the
    /// run id and the incarnation (the public directory name, the marker, the
    /// owner record, `run_started`, and every container name), the pid the
    /// marker records, and **the authorized private root**, which is the
    /// containment boundary every later locator is checked against.
    #[derive(Debug)]
    pub struct PreLockChecked {
        run_id: String,
        incarnation: IncarnationId,
        pid: u32,
        policy: RunnerPolicy,
        policy_sha256: String,
        private_root: PathBuf,
    }

    impl PreLockChecked {
        /// The one constructor, callable only from [`super::check`]'s module.
        pub(super) fn new(
            run_id: String,
            incarnation: IncarnationId,
            pid: u32,
            policy: RunnerPolicy,
            policy_sha256: String,
            private_root: PathBuf,
        ) -> Self {
            Self {
                run_id,
                incarnation,
                pid,
                policy,
                policy_sha256,
                private_root,
            }
        }

        /// The run id this command will create.
        #[must_use]
        pub fn run_id(&self) -> &str {
            &self.run_id
        }

        /// This coordinator process's incarnation.
        #[must_use]
        pub fn incarnation(&self) -> &IncarnationId {
            &self.incarnation
        }

        /// The pid `.creating` records.
        #[must_use]
        pub const fn pid(&self) -> u32 {
            self.pid
        }

        /// The policy resolved **before the worktree lock**, in full.
        #[must_use]
        pub const fn runner_policy(&self) -> &RunnerPolicy {
            &self.policy
        }

        /// `runner_policy_sha256` of [`Self::runner_policy`].
        #[must_use]
        pub fn runner_policy_sha256(&self) -> &str {
            &self.policy_sha256
        }

        /// The authorized private root `R`, canonical.
        #[must_use]
        pub fn private_root(&self) -> &Path {
            &self.private_root
        }

        /// `<R>/runs/<run_id>` — the locator the marker records and the only
        /// private path this run is authorized to write.
        #[must_use]
        pub fn private_dir(&self) -> PathBuf {
            self.private_root.join("runs").join(&self.run_id)
        }
    }
}

/// What the pre-lock checks read. Every field is an input; none is a handle to
/// anything this module could write through.
pub struct PreLock<'a> {
    /// `[runner]`, as PR1's config parse produced it.
    pub selection: &'a RunnerSelection,
    /// The container runtime, for a `Container` selection.
    ///
    /// `None` is a machine with no runtime seam wired, and a `Container`
    /// selection against it refuses here rather than proceeding as if the
    /// question had been asked and answered.
    pub runtime: Option<&'a dyn ContainerRuntime>,
    /// The private root this command is configured with — explicit
    /// `--private-root`, else [`crate::rundir::default_private_root`].
    pub private_root: &'a Path,
    /// Where the run id, the incarnation and the pid come from.
    pub ids: &'a dyn IdSource,
}

/// Perform the read-only pre-lock checks, in the packet's order.
///
/// 1. The authorized private root: canonicalized, and refused when it is not an
///    existing real directory. `run_creation` records the marker's `private_dir`
///    "as a canonical path", and a root that is not there yet cannot be
///    canonicalized — so the requirement is stated here, read-only, rather than
///    discovered at P3a where the answer would already have cost a lock.
///    [`crate::workspace_manager::WorkspaceManager::derive`] requires the same
///    thing of the same directory.
/// 2. The `RunnerPolicy`, by read-only inspection, and its digest.
/// 3. The run id and the incarnation, which perform no effect at all.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] when the private root is not a real directory,
/// when a `Container` selection has no runtime seam, or when the container
/// inspection refuses — an unreachable runtime, an image reference absent from
/// the runtime (there is no implicit pull), or an absent credential volume.
/// [`UpstrokeError::Io`] when the root cannot be canonicalized for any other
/// reason.
pub fn check(inputs: &PreLock<'_>) -> Result<PreLockChecked, UpstrokeError> {
    let private_root = authorized_private_root(inputs.private_root)?;

    let policy = match inputs.selection.kind {
        RunnerKind::Host => resolve_host()?,
        RunnerKind::Container => {
            let runtime = inputs.runtime.ok_or_else(|| UpstrokeError::Refused {
                message: format!(
                    "`[runner] kind = \"container\"` selects the container boundary, and this \
                     command was given no container runtime to inspect. INV-23 resolves the \
                     policy by read-only inspection before any lock — the runtime must be \
                     reachable, the image reference `{}` must already be present in it, and each \
                     credential volume must exist — so a missing runtime refuses here rather \
                     than being assumed available later",
                    inputs.selection.image.as_deref().unwrap_or("<unset>")
                ),
            })?;
            resolve_container(runtime, inputs.selection)?
        }
    };
    let policy_sha256 = runner_policy_sha256(&policy);

    // Last, and only once every refusal above has passed: `run_creation` puts
    // id generation after resolution and annotates it "(no effect)".
    let run_id = inputs.ids.run_id();
    let incarnation = inputs.ids.incarnation();
    let pid = inputs.ids.pid();

    Ok(PreLockChecked::new(
        run_id,
        incarnation,
        pid,
        policy,
        policy_sha256,
        private_root,
    ))
}

/// `R`, canonical, or a refusal naming what is wrong with it.
///
/// Canonicalizing the **root** rather than `<root>/runs/<run_id>` is forced:
/// the run directory does not exist during the pre-lock checks and creating it
/// to canonicalize it would be the residue this phase is defined not to leave.
/// `rundir::prove_private_half_ownership` resolves the same way from the other
/// side — it canonicalizes `<R>/runs` and joins the basename — so the locator
/// this produces and the expectation the census computes are the same value.
fn authorized_private_root(configured: &Path) -> Result<PathBuf, UpstrokeError> {
    let metadata = match std::fs::symlink_metadata(configured) {
        Ok(metadata) => metadata,
        Err(source) => {
            return Err(UpstrokeError::Io {
                path: configured.to_path_buf(),
                source,
            });
        }
    };
    // `symlink_metadata` first, then canonicalize: a private root reached
    // through a link is legitimate (`/tmp` on macOS, a mounted home), and the
    // reparse-point refusal `startup_census` states is about the chain *below*
    // the runs directory. What is refused here is a root that is not a
    // directory at all — a file or a device would canonicalize happily and then
    // fail at P3a, after the lock.
    let canonical = std::fs::canonicalize(configured).map_err(|source| UpstrokeError::Io {
        path: configured.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() && !canonical.is_dir() {
        return Err(UpstrokeError::Refused {
            message: format!(
                "the authorized private root {} is not a directory; every run's private half \
                 lives at <root>/runs/<run_id> and the root is checked read-only before any lock",
                configured.display()
            ),
        });
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;
    use crate::rundir::{NoHooks, create_private_dir, remove_public_husk};
    use crate::runner::container::runtime::{
        ContainerExecution, CreateSpec, CreatedContainer, DiscoveredContainer, ImageInspection,
        Liveness, RuntimeError, RuntimeOp, StopMode,
    };
    use crate::topology::events::RunnerContract;

    const IMAGE_REFERENCE: &str = "ghcr.io/upstroke/sandbox:1";
    const IMAGE_ID: &str =
        "sha256:aaaabbbbccccddddeeeeffff0000111122223333444455556666777788889999";

    /// A runtime that answers from a fixed inventory and records every question.
    ///
    /// It performs no effect of its own: the four effectful methods return
    /// canned values, which is why a module that may not *call* a
    /// `ContainerRuntime` primitive may still implement the trait.
    #[derive(Debug, Default)]
    struct Inventory {
        reachable: bool,
        images: BTreeMap<String, (String, Option<String>)>,
        volumes: Vec<String>,
        asked: Mutex<Vec<String>>,
    }

    impl Inventory {
        fn reachable() -> Self {
            Self {
                reachable: true,
                ..Self::default()
            }
        }

        fn with_image(mut self, reference: &str, id: &str, digest: Option<&str>) -> Self {
            self.images.insert(
                reference.to_owned(),
                (id.to_owned(), digest.map(str::to_owned)),
            );
            self
        }

        fn with_volume(mut self, name: &str) -> Self {
            self.volumes.push(name.to_owned());
            self
        }

        fn asked(&self) -> Vec<String> {
            self.asked
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        fn record(&self, what: &str) {
            self.asked
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(what.to_owned());
        }

        fn unreachable(&self) -> RuntimeError {
            RuntimeError::Unreachable {
                operation: RuntimeOp::Probe,
                detail: "no runtime on this machine".to_owned(),
            }
        }
    }

    impl ContainerRuntime for Inventory {
        fn probe(&self) -> Result<(), RuntimeError> {
            self.record("probe");
            if self.reachable {
                Ok(())
            } else {
                Err(self.unreachable())
            }
        }

        fn image_by_reference(
            &self,
            reference: &str,
        ) -> Result<Option<ImageInspection>, RuntimeError> {
            self.record(&format!("image_by_reference {reference}"));
            Ok(self
                .images
                .get(reference)
                .map(|(id, digest)| ImageInspection {
                    id: id.clone(),
                    digest: digest.clone(),
                    references: vec![reference.to_owned()],
                }))
        }

        fn image_by_id(&self, id: &str) -> Result<Option<ImageInspection>, RuntimeError> {
            self.record(&format!("image_by_id {id}"));
            Ok(self
                .images
                .values()
                .find(|(known, _)| known == id)
                .map(|(known, digest)| ImageInspection {
                    id: known.clone(),
                    digest: digest.clone(),
                    references: Vec::new(),
                }))
        }

        fn volume_present(&self, name: &str) -> Result<bool, RuntimeError> {
            self.record(&format!("volume_present {name}"));
            Ok(self.volumes.iter().any(|known| known == name))
        }

        fn containers_with_label(
            &self,
            _key: &str,
            _value: &str,
        ) -> Result<Vec<DiscoveredContainer>, RuntimeError> {
            Ok(Vec::new())
        }

        fn observe(&self, _name: &str) -> Result<Liveness, RuntimeError> {
            Ok(Liveness::Gone)
        }

        fn collect(&self, _name: &str) -> Result<ContainerExecution, RuntimeError> {
            Ok(ContainerExecution {
                exit_code: Some(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }

        fn create(&self, spec: &CreateSpec) -> Result<CreatedContainer, RuntimeError> {
            Ok(CreatedContainer {
                name: spec.name.clone(),
                reported_image_id: spec.image_id.clone(),
            })
        }

        fn start(&self, _name: &str) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn stop(&self, _name: &str, _mode: StopMode) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn remove(&self, _name: &str) -> Result<(), RuntimeError> {
            Ok(())
        }
    }

    /// Fixed identities, so an assertion can name a literal.
    #[derive(Debug)]
    struct Ids;

    impl IdSource for Ids {
        fn question_id(&self) -> crate::ir::QuestionId {
            crate::ir::QuestionId("q-fixed".to_owned())
        }

        fn run_id(&self) -> String {
            "01KZTPR7B00000000000000001".to_owned()
        }

        fn incarnation(&self) -> IncarnationId {
            IncarnationId("01KZTINCB0000000000000001".to_owned())
        }

        fn pid(&self) -> u32 {
            4242
        }
    }

    /// A scratch directory that **owns** its tree.
    ///
    /// The predecessor was a `fn scratch(&str) -> PathBuf`: it created the
    /// directory and handed back a path nothing owned, so every invocation left
    /// its root in the temp directory forever — on the ordinary exit, on an
    /// early return, and on the unwind a failing assertion starts. On this
    /// project's build box a directory leaked per test is inode exhaustion,
    /// which `df -h` reports as 72% full while every write fails — and the leak
    /// is not hypothetical: 5050 `upstroke-prelock-*` roots had accumulated in
    /// the temp directory by 2026-08-30, and five runs of this module after the
    /// repair added none.
    ///
    /// Both ends go through the run-directory funnel because this file is a
    /// `TOPOLOGY_MODULE`: `std::fs::create_dir_all` and every `std::fs` removal
    /// are denied in it, tests included. `RunDir.RemovePublicHusk` is the one
    /// recursive delete a test here can reach — it removes a directory's
    /// children and then the directory — because `RunDir.RemovePrivateHusk`
    /// takes a [`crate::rundir::PrivateHalfProof`], and a pre-lock scratch root
    /// is not the two-halves shape that mints one.
    ///
    /// The naming is the predecessor's, unchanged: the pid and the thread id
    /// keep two live fixtures apart, and reclamation is what this type adds.
    struct Scratch {
        root: PathBuf,
    }

    impl Scratch {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "upstroke-prelock-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            create_private_dir(&root, &mut NoHooks).expect("scratch root");
            Self { root }
        }

        /// The authorized private root a test hands to [`check`].
        fn path(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let reclaimed = remove_public_husk(&self.root, &mut NoHooks);
            // A failed reclamation is the leak this type exists to prevent, so
            // it is reported rather than discarded — but never while a panic is
            // already travelling. A second panic out of a destructor aborts the
            // process, which would replace the test's own failure with an abort
            // and lose the report that says what actually broke.
            assert!(
                reclaimed.is_ok() || std::thread::panicking(),
                "the scratch root {} was not reclaimed: {reclaimed:?}",
                self.root.display()
            );
        }
    }

    fn container_selection(image: &str, volumes: &[(&str, &str)]) -> RunnerSelection {
        RunnerSelection {
            kind: RunnerKind::Container,
            image: Some(image.to_owned()),
            credential_volumes: volumes
                .iter()
                .map(|(agent, volume)| ((*agent).to_owned(), (*volume).to_owned()))
                .collect(),
            mounts: Vec::new(),
            from_config: true,
        }
    }

    /// A host run resolves `host-v1`, digests it, and mints its identities —
    /// and the digest is the one the marker will carry.
    #[test]
    fn a_host_selection_resolves_host_v1_and_carries_its_digest() {
        let root = Scratch::new("host");
        let selection = RunnerSelection::host_default();
        let checked = check(&PreLock {
            selection: &selection,
            runtime: None,
            private_root: root.path(),
            ids: &Ids,
        })
        .expect("the host runner resolves with nothing to inspect");

        assert_eq!(checked.runner_policy().kind, RunnerKind::Host);
        assert_eq!(checked.runner_policy().policy, RunnerContract::HostV1);
        assert!(checked.runner_policy().image.is_none());
        assert_eq!(
            checked.runner_policy_sha256(),
            runner_policy_sha256(checked.runner_policy()),
            "the carried digest is the digest of the carried policy"
        );
        assert_eq!(checked.run_id(), "01KZTPR7B00000000000000001");
        assert_eq!(checked.incarnation().0, "01KZTINCB0000000000000001");
        assert_eq!(checked.pid(), 4242);
        assert_eq!(
            checked.private_dir(),
            checked.private_root().join("runs").join(checked.run_id()),
            "the locator is <R>/runs/<run_id> and nothing else"
        );
    }

    /// The pre-lock checks leave nothing behind — not the run directory, not
    /// the private half, not a lock file, not a container.
    #[test]
    fn the_pre_lock_checks_leave_no_residue() {
        let root = Scratch::new("residue");
        let before = names_under(root.path());
        let selection = container_selection(IMAGE_REFERENCE, &[("codex", "upstroke-codex")]);
        let runtime = Inventory::reachable()
            .with_image(IMAGE_REFERENCE, IMAGE_ID, Some("sha256:manifest"))
            .with_volume("upstroke-codex");

        let checked = check(&PreLock {
            selection: &selection,
            runtime: Some(&runtime),
            private_root: root.path(),
            ids: &Ids,
        })
        .expect("a reachable runtime holding the image and the volume resolves");

        assert_eq!(
            names_under(root.path()),
            before,
            "the pre-lock phase created something under the private root"
        );
        assert!(
            !checked.private_dir().exists(),
            "the private half must not exist before P3a"
        );
        // Every runtime interaction was a read.
        for question in runtime.asked() {
            assert!(
                question.starts_with("probe")
                    || question.starts_with("image_by_")
                    || question.starts_with("volume_present"),
                "`{question}` is not a read-only inspection"
            );
        }
    }

    fn names_under(dir: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The four container inspections happen in `run_creation`'s order, and the
    /// first failure ends it.
    #[test]
    fn the_container_inspections_run_in_order_and_the_first_failure_ends_them() {
        let root = Scratch::new("order");
        let selection = container_selection(IMAGE_REFERENCE, &[("codex", "upstroke-codex")]);
        let runtime = Inventory::reachable()
            .with_image(IMAGE_REFERENCE, IMAGE_ID, None)
            .with_volume("upstroke-codex");
        check(&PreLock {
            selection: &selection,
            runtime: Some(&runtime),
            private_root: root.path(),
            ids: &Ids,
        })
        .expect("resolves");
        assert_eq!(
            runtime.asked(),
            vec![
                "probe".to_owned(),
                format!("image_by_reference {IMAGE_REFERENCE}"),
                "volume_present upstroke-codex".to_owned(),
            ],
            "reachability, then the image, then the volumes"
        );

        // An unreachable runtime never reaches the image question.
        let unreachable = Inventory::default();
        let refusal = check(&PreLock {
            selection: &selection,
            runtime: Some(&unreachable),
            private_root: root.path(),
            ids: &Ids,
        })
        .expect_err("an unreachable runtime refuses");
        assert!(
            refusal.to_string().contains("runtime"),
            "the refusal names the runtime: {refusal}"
        );
        assert_eq!(
            unreachable.asked(),
            vec!["probe".to_owned()],
            "the image and the volumes were not asked about after the runtime refused"
        );
    }

    /// An absent credential volume refuses, and the digest is never computed.
    #[test]
    fn an_absent_credential_volume_refuses() {
        let root = Scratch::new("volume");
        let selection = container_selection(IMAGE_REFERENCE, &[("codex", "upstroke-codex")]);
        let runtime = Inventory::reachable().with_image(IMAGE_REFERENCE, IMAGE_ID, None);
        let refusal = check(&PreLock {
            selection: &selection,
            runtime: Some(&runtime),
            private_root: root.path(),
            ids: &Ids,
        })
        .expect_err("an absent volume refuses");
        assert!(
            refusal.to_string().contains("upstroke-codex"),
            "the refusal names the volume: {refusal}"
        );
    }

    /// A `Container` selection with no runtime seam refuses rather than
    /// silently proceeding as though the inspection had passed.
    #[test]
    fn a_container_selection_without_a_runtime_refuses() {
        let root = Scratch::new("noruntime");
        let selection = container_selection(IMAGE_REFERENCE, &[]);
        let refusal = check(&PreLock {
            selection: &selection,
            runtime: None,
            private_root: root.path(),
            ids: &Ids,
        })
        .expect_err("no runtime to inspect");
        assert!(
            refusal.to_string().contains(IMAGE_REFERENCE),
            "the refusal names the image it could not look for: {refusal}"
        );
    }

    /// A private root that is not there refuses read-only, before anything
    /// else is asked.
    #[test]
    fn a_private_root_that_is_not_a_real_directory_refuses_before_any_inspection() {
        let root = Scratch::new("root");
        // The root the check is given is a child that was never created; the
        // guard still owns the scratch tree that child was named under.
        let absent = root.path().join("absent");
        let selection = container_selection(IMAGE_REFERENCE, &[]);
        let runtime = Inventory::reachable().with_image(IMAGE_REFERENCE, IMAGE_ID, None);
        let refusal = check(&PreLock {
            selection: &selection,
            runtime: Some(&runtime),
            private_root: &absent,
            ids: &Ids,
        })
        .expect_err("an absent private root refuses");
        assert!(
            refusal.to_string().contains("absent"),
            "the refusal names the root: {refusal}"
        );
        assert!(
            runtime.asked().is_empty(),
            "the runtime was inspected before the root was even checked"
        );
    }

    /// The recorded root is **canonical**, so the locator the marker carries and
    /// the expectation the census computes are the same value.
    #[test]
    fn the_authorized_private_root_is_canonical() {
        let root = Scratch::new("canonical");
        let indirect = root.path().join(".").join("..").join(
            root.path()
                .file_name()
                .expect("the scratch root has a basename")
                .to_string_lossy()
                .as_ref(),
        );
        let selection = RunnerSelection::host_default();
        let checked = check(&PreLock {
            selection: &selection,
            runtime: None,
            private_root: &indirect,
            ids: &Ids,
        })
        .expect("a root reachable through `.`/`..` is the same root");
        assert_eq!(
            checked.private_root(),
            std::fs::canonicalize(root.path())
                .expect("the scratch root canonicalizes")
                .as_path(),
            "the witness carries the canonical root, not the spelling it was given"
        );
    }

    /// Every exit reclaims the scratch tree — the ordinary one and the unwind,
    /// which is the exit a failing assertion in any test above takes.
    ///
    /// The panic hook is deliberately **not** silenced for the second half.
    /// The hook is process-global and this suite runs in parallel, so a test
    /// that takes it, installs a no-op and restores it can interleave with
    /// another doing the same and leave the process with a no-op hook for good
    /// — every later panic anywhere in the suite losing its message and
    /// backtrace. The few lines this prints cost less than that.
    #[test]
    fn a_scratch_root_is_reclaimed_on_every_exit_including_an_unwind() {
        let ordinary = {
            let root = Scratch::new("raii-ordinary");
            let path = root.path().to_path_buf();
            // A tree rather than a bare directory: the guard reclaims what a
            // test left under its root as well as the root itself.
            create_private_dir(&path.join("nested"), &mut NoHooks).expect("a child of the root");
            assert!(path.join("nested").is_dir(), "the child was not created");
            path
        };
        assert!(
            !ordinary.exists(),
            "the scratch root {} outlived its guard on the ordinary exit",
            ordinary.display()
        );

        // The path is recorded from inside the closure rather than re-derived
        // here: re-deriving it would copy `Scratch::new`'s naming rule, and a
        // witness that agrees with a rule it restates proves nothing about it.
        let recorded = Mutex::new(None);
        let unwound = std::panic::catch_unwind(|| {
            let root = Scratch::new("raii-unwind");
            let path = root.path().to_path_buf();
            *recorded
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path.clone());
            // The shape of a real failure: an assertion about the run that does
            // not hold, raised with the guard still in scope.
            assert!(!path.is_dir(), "a deliberate failure, mid-test");
        });
        assert!(
            unwound.is_err(),
            "the closure was supposed to unwind, so nothing about the panic path was measured"
        );
        let path = recorded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("the closure recorded its root before it panicked");
        assert!(
            !path.exists(),
            "the scratch root {} survived the unwind",
            path.display()
        );
    }

    /// A reclamation that fails is **reported**, not discarded.
    ///
    /// `Drop` cannot return, so the alternative to reporting is silence — and
    /// silence here is the same leak the guard exists to close, with nothing to
    /// say it happened. The tree is reclaimed out from under the guard through
    /// the very funnel the guard would use, so the removal it then attempts
    /// fails for a real reason rather than an injected one, and the panic that
    /// carries the report is caught here rather than failing this test.
    #[test]
    fn a_scratch_root_that_cannot_be_reclaimed_is_reported_rather_than_discarded() {
        let reported = std::panic::catch_unwind(|| {
            let root = Scratch::new("raii-reported");
            remove_public_husk(root.path(), &mut NoHooks).expect("the tree reclaims early");
        })
        .expect_err("the guard discarded a failed reclamation");

        let message = reported
            .downcast_ref::<String>()
            .map_or_else(String::new, Clone::clone);
        assert!(
            message.contains("was not reclaimed") && message.contains("raii-reported"),
            "the report must name the root it could not reclaim: {message}"
        );
    }

    /// The child half of
    /// [`a_failed_reclamation_during_an_unwind_does_not_abort_the_process`].
    ///
    /// It drives the one corner of the guard's cross-product the two witnesses
    /// above cannot reach: a reclamation that **fails** while a panic is
    /// **already travelling**. `raii-reported` covers failure without an
    /// unwind and `raii-unwind` covers an unwind without a failure; only both
    /// at once reaches the `std::thread::panicking()` half of the assertion,
    /// and only there does the alternative — a second panic out of `Drop` —
    /// abort the process rather than fail a test.
    ///
    /// Everything asserted here is asserted **in this process**, so the parent
    /// needs no channel back: reaching the end of this body at all is the
    /// claim, and the harness's own result line is how the parent reads it.
    #[test]
    #[ignore = "spawned by a_failed_reclamation_during_an_unwind_does_not_abort_the_process"]
    fn scratch_unwind_with_a_failed_reclamation_child() {
        const PRIMARY: &str = "the primary failure this witness keeps observable";

        let caught = std::panic::catch_unwind(|| {
            let root = Scratch::new("raii-unwind-unreclaimable");
            // Reclaimed out from under the live guard, through the very funnel
            // the guard will use, so the removal it attempts while unwinding
            // fails with `NotFound` for a real reason rather than an injected
            // one — no fault hook, no permission trick, no timing.
            remove_public_husk(root.path(), &mut NoHooks).expect("the tree reclaims early");
            assert!(
                !root.path().exists(),
                "the guard's own removal has to be the one that fails, and it would succeed \
                 against a root that is still there"
            );
            panic!("{PRIMARY}");
        })
        .expect_err("the closure was supposed to unwind");

        // Reached at all only because the destructor did not panic a second
        // time: a panic out of `Drop` during this unwind aborts, and an
        // aborted process runs no assertion and prints no result line.
        let message = caught
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| caught.downcast_ref::<&str>().map(|m| (*m).to_owned()))
            .unwrap_or_default();
        assert_eq!(
            message, PRIMARY,
            "the destructor's own report displaced the primary panic, so the failure a test \
             would be diagnosing is the one that got lost"
        );
    }

    /// A reclamation that fails **while a panic is already travelling** does
    /// not panic a second time: the process survives it, and the primary panic
    /// is still the one that arrives.
    ///
    /// Measured **from outside the process that makes the observation**, which
    /// is forced. A second panic out of a destructor mid-unwind aborts, and an
    /// abort takes the whole test binary — so an in-process witness for this
    /// corner would have to survive its own subject. The child is the witness;
    /// this is the frame that reads its exit.
    ///
    /// The child is spawned **through the host Runner**, not through
    /// `std::process::Command`: `std::process::Command` is on the effect
    /// denylist and `src/engine/topology/**` may not reach it even in tests.
    /// The Runner is the funnel that owns `Process.Spawn`, which is exactly the
    /// rule — the same spawn `recover::tests::kill_during_recovery_repeats_recovery`
    /// and `create::tests::spawn_and_wait` already use.
    ///
    /// **Both assertions are load-bearing, and neither alone is enough.**
    /// `abort()` takes the process before the harness prints anything about the
    /// test, so an aborted child emits no `test result:` line — but a child
    /// whose filter matched *nothing* also exits 0 and prints `ok. 0 passed`,
    /// which a bare exit-code assertion would read as success. Requiring the
    /// zero exit **and** `ok. 1 passed` separates the three outcomes: aborted,
    /// selected-and-passed, and selected-nothing-at-all.
    #[test]
    fn a_failed_reclamation_during_an_unwind_does_not_abort_the_process() {
        use crate::runner::host::HostRunner;
        use crate::runner::{
            CommandSpec, ExecutionRole, InvocationId, ProbeTarget, Runner, RunnerRequest,
        };

        let workspace = Scratch::new("raii-abort-isolation");
        let exe = std::env::current_exe().expect("the test binary knows where it is");
        let request = RunnerRequest {
            command: CommandSpec {
                program: exe.display().to_string(),
                args: vec![
                    "--exact".to_owned(),
                    "engine::topology::prelock::tests::scratch_unwind_with_a_failed_reclamation_child"
                        .to_owned(),
                    "--ignored".to_owned(),
                    "--test-threads".to_owned(),
                    "1".to_owned(),
                ],
                // Nothing to pass: the child derives its own scratch root from
                // the temp directory and its own pid, so the two processes
                // cannot collide and there is no state to hand over.
                env: Vec::new(),
                stdin: Vec::new(),
            },
            workspace: workspace.path().to_path_buf(),
            role: ExecutionRole::Gate,
            timeout: std::time::Duration::from_secs(120),
            agent: None,
            invocation: InvocationId::probe(ProbeTarget::Shell, 11)
                .expect("a probe identity for the spawned child"),
        };
        let output = HostRunner::new().run(&request).expect("the child runs");

        assert!(
            !output.timed_out,
            "the child never finished, so nothing about the unwind was observed"
        );
        // `stderr` rather than the whole `ProcessOutput`: the child's stdout
        // carries its backtrace, and a failure report that buries the one line
        // that names the cause — `panic in a destructor during cleanup` — under
        // fifty frames of it is a report nobody reads.
        assert_eq!(
            output.code,
            Some(0),
            "the child did not complete as designed — a destructor that panicked into the live \
             unwind aborts it, and a process killed by that abort reports no exit code at all. \
             Its stderr: {}",
            output.stderr
        );
        assert!(
            output.stdout.contains("test result: ok. 1 passed"),
            "the harness printed no passing result for exactly one selected test, so the child \
             either aborted before printing or selected nothing at all: {}",
            output.stdout
        );
    }
}
