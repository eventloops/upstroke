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
    use crate::rundir::{NoHooks, create_private_dir};
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

    /// A scratch directory, created through the run-directory funnel: this file
    /// is a `TOPOLOGY_MODULE` and `std::fs::create_dir_all` is denied in it,
    /// tests included.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "upstroke-prelock-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        create_private_dir(&dir, &mut NoHooks).expect("scratch root");
        dir
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
        let root = scratch("host");
        let selection = RunnerSelection::host_default();
        let checked = check(&PreLock {
            selection: &selection,
            runtime: None,
            private_root: &root,
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
        let root = scratch("residue");
        let before = names_under(&root);
        let selection = container_selection(IMAGE_REFERENCE, &[("codex", "upstroke-codex")]);
        let runtime = Inventory::reachable()
            .with_image(IMAGE_REFERENCE, IMAGE_ID, Some("sha256:manifest"))
            .with_volume("upstroke-codex");

        let checked = check(&PreLock {
            selection: &selection,
            runtime: Some(&runtime),
            private_root: &root,
            ids: &Ids,
        })
        .expect("a reachable runtime holding the image and the volume resolves");

        assert_eq!(
            names_under(&root),
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
        let root = scratch("order");
        let selection = container_selection(IMAGE_REFERENCE, &[("codex", "upstroke-codex")]);
        let runtime = Inventory::reachable()
            .with_image(IMAGE_REFERENCE, IMAGE_ID, None)
            .with_volume("upstroke-codex");
        check(&PreLock {
            selection: &selection,
            runtime: Some(&runtime),
            private_root: &root,
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
            private_root: &root,
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
        let root = scratch("volume");
        let selection = container_selection(IMAGE_REFERENCE, &[("codex", "upstroke-codex")]);
        let runtime = Inventory::reachable().with_image(IMAGE_REFERENCE, IMAGE_ID, None);
        let refusal = check(&PreLock {
            selection: &selection,
            runtime: Some(&runtime),
            private_root: &root,
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
        let root = scratch("noruntime");
        let selection = container_selection(IMAGE_REFERENCE, &[]);
        let refusal = check(&PreLock {
            selection: &selection,
            runtime: None,
            private_root: &root,
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
        let root = scratch("root").join("absent");
        let selection = container_selection(IMAGE_REFERENCE, &[]);
        let runtime = Inventory::reachable().with_image(IMAGE_REFERENCE, IMAGE_ID, None);
        let refusal = check(&PreLock {
            selection: &selection,
            runtime: Some(&runtime),
            private_root: &root,
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
        let root = scratch("canonical");
        let indirect = root.join(".").join("..").join(
            root.file_name()
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
            std::fs::canonicalize(&root)
                .expect("the scratch root canonicalizes")
                .as_path(),
            "the witness carries the canonical root, not the spelling it was given"
        );
    }
}
