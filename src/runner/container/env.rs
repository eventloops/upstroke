//! Extended notes: `docs/internals/runner/container/env.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::BTreeMap;

use crate::error::UpstrokeError;
use crate::runner::host::{KeyCase, credential_location, reserved_keys};
use crate::runner::{AgentId, ExecutionRole, ProbeTarget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryLayout {
    workspace: String,
    credentials: String,
    git_view: String,
    git_objects: String,
    scratch: String,
}

impl Default for BoundaryLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl BoundaryLayout {
    pub const DEFAULT_WORKSPACE: &'static str = "/upstroke/workspace";

    pub const DEFAULT_CREDENTIALS: &'static str = "/upstroke/credentials";

    pub const DEFAULT_GIT_VIEW: &'static str = "/upstroke/gitview";

    pub const DEFAULT_GIT_OBJECTS: &'static str = "/upstroke/gitobjects";

    pub const DEFAULT_SCRATCH: &'static str = "/tmp";

    #[must_use]
    pub fn new() -> Self {
        Self {
            workspace: Self::DEFAULT_WORKSPACE.to_owned(),
            credentials: Self::DEFAULT_CREDENTIALS.to_owned(),
            git_view: Self::DEFAULT_GIT_VIEW.to_owned(),
            git_objects: Self::DEFAULT_GIT_OBJECTS.to_owned(),
            scratch: Self::DEFAULT_SCRATCH.to_owned(),
        }
    }

    #[must_use]
    pub fn with_roots(
        workspace: impl Into<String>,
        credentials: impl Into<String>,
        git_view: impl Into<String>,
        git_objects: impl Into<String>,
        scratch: impl Into<String>,
    ) -> Self {
        Self {
            workspace: workspace.into(),
            credentials: credentials.into(),
            git_view: git_view.into(),
            git_objects: git_objects.into(),
            scratch: scratch.into(),
        }
    }

    #[must_use]
    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    #[must_use]
    pub fn git_view(&self) -> &str {
        &self.git_view
    }

    #[must_use]
    pub fn git_objects(&self) -> &str {
        &self.git_objects
    }

    #[must_use]
    pub fn scratch(&self) -> &str {
        &self.scratch
    }

    #[must_use]
    pub fn git_pointer(&self) -> String {
        format!("{}/.git", self.workspace)
    }

    #[must_use]
    pub fn credentials(&self, agent: &AgentId) -> String {
        format!("{}/{}", self.credentials, agent.as_str())
    }

    #[must_use]
    pub fn credential_root(&self) -> &str {
        &self.credentials
    }
}

#[must_use]
pub const fn supplies_credential_location(role: &ExecutionRole) -> bool {
    match role {
        ExecutionRole::Implement
        | ExecutionRole::Review
        | ExecutionRole::Probe(ProbeTarget::Agent(_)) => true,
        ExecutionRole::Gate | ExecutionRole::Probe(ProbeTarget::Shell) => false,
    }
}

pub const CONTAINER_KEY_CASE: KeyCase = KeyCase::Sensitive;

pub const CONTAINER_PATH_SEPARATOR: char = ':';

#[must_use]
pub fn cwd_dependent_path_components(value: &str) -> Vec<String> {
    value
        .split(CONTAINER_PATH_SEPARATOR)
        .filter(|component| !component.starts_with('/'))
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Clone, Copy)]
pub struct RoleScope<'a> {
    pub role: &'a ExecutionRole,
    pub agent: Option<&'a AgentId>,
    pub volumes: &'a BTreeMap<String, String>,
    pub layout: &'a BoundaryLayout,
}

#[derive(Debug, Clone)]
pub struct ContainerEnvironment {
    base: Vec<(String, String)>,
    case: KeyCase,
}

impl Default for ContainerEnvironment {
    fn default() -> Self {
        Self::inherited()
    }
}

impl ContainerEnvironment {
    #[must_use]
    pub fn from_image(base: Vec<(String, String)>) -> Self {
        Self {
            base,
            case: CONTAINER_KEY_CASE,
        }
    }

    #[must_use]
    pub fn inherited() -> Self {
        Self::from_image(Vec::new())
    }

    #[must_use]
    pub fn with_base(base: Vec<(String, String)>, case: KeyCase) -> Self {
        Self { base, case }
    }

    #[must_use]
    pub fn base(&self) -> &[(String, String)] {
        &self.base
    }

    #[must_use]
    pub const fn case(&self) -> KeyCase {
        self.case
    }

    #[must_use]
    pub fn reserved_values(&self, scope: &RoleScope<'_>) -> Vec<(String, String)> {
        let mut supplied = Vec::new();
        for key in crate::runner::host::RESERVED_ALWAYS {
            if let Some(value) = self.lookup(key) {
                supplied.push(((*key).to_owned(), value));
            }
        }
        if supplies_credential_location(scope.role) {
            if let Some(agent) = scope.agent {
                if scope.volumes.contains_key(agent.as_str()) {
                    if let Some(key) = credential_location(agent) {
                        supplied.push((key.to_owned(), scope.layout.credentials(agent)));
                    }
                }
            }
        }
        supplied
    }

    #[must_use]
    pub fn withheld_credential_locations(&self, scope: &RoleScope<'_>) -> Vec<(String, String)> {
        let supplied = self.reserved_values(scope);
        crate::runner::host::CREDENTIAL_LOCATIONS
            .iter()
            .map(|(_, key)| *key)
            .filter(|key| {
                !supplied
                    .iter()
                    .any(|(name, _)| self.case.same_key(name.as_ref(), key.as_ref()))
            })
            .map(|key| (key.to_owned(), String::new()))
            .collect()
    }

    pub fn compose(
        &self,
        scope: &RoleScope<'_>,
        overlay: &[(String, String)],
    ) -> Result<Vec<(String, String)>, UpstrokeError> {
        self.preflight(overlay)?;
        let mut composed = self.base.clone();
        for reserved in reserved_keys() {
            composed.retain(|(name, _)| !self.case.same_key(name.as_ref(), reserved.as_ref()));
        }
        for (key, value) in self.reserved_values(scope) {
            upsert(&mut composed, self.case, key, value);
        }
        for (key, value) in self.withheld_credential_locations(scope) {
            upsert(&mut composed, self.case, key, value);
        }
        for (key, value) in overlay {
            upsert(&mut composed, self.case, key.clone(), value.clone());
        }
        self.certify_path(&composed)?;
        Ok(composed)
    }

    pub fn certify_path(&self, composed: &[(String, String)]) -> Result<(), UpstrokeError> {
        let Some((_, value)) = composed
            .iter()
            .find(|(name, _)| self.case.same_key(name.as_ref(), "PATH".as_ref()))
        else {
            return Err(UpstrokeError::Refused {
                message: "the container runner composed an environment that names no `PATH`, so \
                          the recorded image's own would decide which binary every bare program \
                          name resolves to. DESIGN.md:260 has the runner supply role-scoped \
                          `HOME`, `PATH`, and credential locations, and DESIGN.md:263 has \
                          pre-flight certify the environment that will actually spend — neither \
                          holds for a base this runner never read"
                    .to_owned(),
            });
        };
        let relative = cwd_dependent_path_components(value);
        if relative.is_empty() {
            return Ok(());
        }
        Err(UpstrokeError::Refused {
            message: format!(
                "the container runner was given `PATH={value}`, whose component(s) {relative:?} \
                 resolve against the working directory. A probe has no worktree and an attempt \
                 has one, so the same bare program name would resolve to different binaries in \
                 the two — DESIGN.md:612: \"Probes run through that same runner, or pre-flight \
                 could certify a host CLI/version different from the one the attempt executes\". \
                 Every `PATH` component at this boundary must be absolute (an empty component is \
                 the working directory)"
            ),
        })
    }

    pub fn preflight(&self, overlay: &[(String, String)]) -> Result<(), UpstrokeError> {
        for (key, _) in overlay {
            if let Some(reserved) = reserved_keys()
                .into_iter()
                .find(|reserved| self.case.same_key(key.as_ref(), reserved.as_ref()))
            {
                return Err(UpstrokeError::Refused {
                    message: format!(
                        "the command overlay sets `{key}`, which is reserved by the container \
                         runner (`{reserved}`). An adapter may select a profile or change CLI \
                         behaviour, but the runner owns the environment the process executes in \
                         (DESIGN.md:258-264)"
                    ),
                });
            }
        }
        Ok(())
    }

    fn lookup(&self, key: &str) -> Option<String> {
        self.base
            .iter()
            .find(|(name, _)| self.case.same_key(name.as_ref(), key.as_ref()))
            .map(|(_, value)| value.clone())
    }
}

fn upsert(into: &mut Vec<(String, String)>, case: KeyCase, key: String, value: String) {
    if let Some(slot) = into
        .iter_mut()
        .find(|(name, _)| case.same_key(name.as_ref(), key.as_ref()))
    {
        slot.1 = value;
        return;
    }
    into.push((key, value));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::runner::host::{CREDENTIAL_LOCATIONS, HostEnvironment, RESERVED_ALWAYS};

    const VOLUMES: &[(&str, &str)] = &[
        ("claude-code", "upstroke-creds-claude"),
        ("copilot", "upstroke-creds-copilot"),
        ("codex", "upstroke-creds-codex"),
    ];

    fn volumes() -> BTreeMap<String, String> {
        VOLUMES
            .iter()
            .map(|(agent, volume)| ((*agent).to_owned(), (*volume).to_owned()))
            .collect()
    }

    fn image_base() -> Vec<(String, String)> {
        [
            ("PATH", "/usr/local/sbin:/usr/local/bin:/usr/bin:/bin"),
            ("HOME", "/root"),
            ("LANG", "C.UTF-8"),
            ("UPSTROKE_IMAGE_MARKER", "image-environment-v1"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
    }

    fn scope<'a>(
        role: &'a ExecutionRole,
        agent: Option<&'a AgentId>,
        volumes: &'a BTreeMap<String, String>,
        layout: &'a BoundaryLayout,
    ) -> RoleScope<'a> {
        RoleScope {
            role,
            agent,
            volumes,
            layout,
        }
    }

    fn binding(role: &ExecutionRole) -> Option<AgentId> {
        match role {
            ExecutionRole::Gate | ExecutionRole::Probe(ProbeTarget::Shell) => None,
            ExecutionRole::Probe(ProbeTarget::Agent(agent)) => Some(agent.clone()),
            ExecutionRole::Implement | ExecutionRole::Review => Some(AgentId::new("claude-code")),
        }
    }

    fn value<'a>(composed: &'a [(String, String)], key: &str) -> Option<&'a str> {
        composed
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn the_reserved_key_enumeration_is_the_hosts_and_not_a_second_list() {
        const EXPECTED: &[&str] = &[
            "PATH",
            "HOME",
            "USERPROFILE",
            "CLAUDE_CONFIG_DIR",
            "COPILOT_HOME",
            "CODEX_HOME",
        ];
        let keys = reserved_keys();
        assert_eq!(keys, EXPECTED, "the reserved enumeration moved");
        assert_eq!(RESERVED_ALWAYS.len() + CREDENTIAL_LOCATIONS.len(), 6);

        let environment = ContainerEnvironment::from_image(image_base());
        let host = HostEnvironment::with_base(Vec::new(), CONTAINER_KEY_CASE);
        for key in EXPECTED {
            let overlay = vec![((*key).to_owned(), "anything".to_owned())];
            let refusal = environment
                .preflight(&overlay)
                .expect_err("a reserved key in the overlay is refused");
            assert!(
                refusal.to_string().contains(key),
                "the refusal does not name `{key}`: {refusal}"
            );
            host.preflight(&overlay)
                .expect_err("and the host refuses the same key");
        }
        environment
            .preflight(&[(
                "CLAUDE_CODE_MAX_OUTPUT_TOKENS".to_owned(),
                "8000".to_owned(),
            )])
            .expect("an ordinary adapter override is not a reserved key");
    }

    #[test]
    fn an_overlay_naming_a_reserved_key_is_refused_by_key_across_every_role() {
        let environment = ContainerEnvironment::from_image(image_base());
        let volumes = volumes();
        let layout = BoundaryLayout::new();
        let mut refused = 0_usize;
        let mut allowed = 0_usize;
        for role in ExecutionRole::all() {
            let agent = binding(&role);
            let scope = scope(&role, agent.as_ref(), &volumes, &layout);
            for key in reserved_keys() {
                let value = if key == "PATH" {
                    "/usr/local/sbin:/usr/local/bin:/usr/bin:/bin".to_owned()
                } else {
                    layout.credentials(&AgentId::new("claude-code"))
                };
                environment
                    .compose(&scope, &[(key.to_owned(), value)])
                    .expect_err("a reserved key is refused whatever its value");
                refused += 1;
            }
            environment
                .compose(&scope, &[("UPSTROKE_OVERLAY".to_owned(), "1".to_owned())])
                .expect("a non-reserved overlay key composes");
            allowed += 1;
        }
        assert_eq!(refused, 5 * 6, "five roles crossed with six reserved keys");
        assert_eq!(allowed, 5);
        assert_eq!(ExecutionRole::all().len(), 5);
    }

    #[test]
    fn the_overlay_overlays_the_base_rather_than_replacing_it() {
        let environment = ContainerEnvironment::from_image(image_base());
        let volumes = volumes();
        let layout = BoundaryLayout::new();
        let role = ExecutionRole::Gate;
        let scope = scope(&role, None, &volumes, &layout);

        let composed = environment
            .compose(
                &scope,
                &[
                    ("UPSTROKE_NEW".to_owned(), "landed".to_owned()),
                    ("LANG".to_owned(), "en_GB.UTF-8".to_owned()),
                ],
            )
            .expect("composes");

        assert_eq!(
            value(&composed, "UPSTROKE_IMAGE_MARKER"),
            Some("image-environment-v1"),
            "the image environment is the base, and a key nobody touched survives it"
        );
        assert_eq!(value(&composed, "UPSTROKE_NEW"), Some("landed"));
        assert_eq!(value(&composed, "LANG"), Some("en_GB.UTF-8"));

        let mut keys: Vec<&str> = composed.iter().map(|(key, _)| key.as_str()).collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), total, "a key appears twice in the composed set");

        assert_eq!(
            environment
                .base()
                .iter()
                .find(|(key, _)| key == "LANG")
                .map(|(_, v)| v.as_str()),
            Some("C.UTF-8")
        );
    }

    #[test]
    fn the_credential_location_is_role_scoped_and_names_the_boundarys_own_path() {
        let environment = ContainerEnvironment::from_image(image_base());
        let layout = BoundaryLayout::new();
        let recorded = volumes();
        let empty = BTreeMap::new();

        let mut supplied = 0_usize;
        let mut withheld = 0_usize;
        let mut targets: Vec<String> = Vec::new();
        for role in ExecutionRole::all() {
            let agent = binding(&role);
            for (recorded_volumes, is_recorded) in [(&recorded, true), (&empty, false)] {
                let scope = scope(&role, agent.as_ref(), recorded_volumes, &layout);
                let composed = environment.compose(&scope, &[]).expect("composes");
                for (_, key) in CREDENTIAL_LOCATIONS {
                    assert!(
                        value(&composed, key).is_some(),
                        "{role} (volume recorded: {is_recorded}): `{key}` is not named at all, so \
                         the recorded image's own value reaches the container"
                    );
                }
                let key = agent.as_ref().and_then(credential_location);
                let found = key
                    .and_then(|key| value(&composed, key))
                    .filter(|value| !value.is_empty());
                let expected =
                    supplies_credential_location(&role) && agent.is_some() && is_recorded;
                assert_eq!(
                    found.is_some(),
                    expected,
                    "{role} (volume recorded: {is_recorded}) got {found:?}"
                );
                if let Some(found) = found {
                    let agent = agent.as_ref().expect("a location implies an agent");
                    assert_eq!(found, layout.credentials(agent));
                    assert!(
                        found.starts_with(layout.credential_root()),
                        "the location is the boundary's own path, not a host one: {found}"
                    );
                    targets.push(found.to_owned());
                    supplied += 1;
                } else {
                    if let Some(key) = key {
                        assert_eq!(
                            value(&composed, key),
                            Some(""),
                            "{role} (volume recorded: {is_recorded}): a withheld location must be \
                             named with nothing, not left to the image"
                        );
                    }
                    withheld += 1;
                }
            }
        }
        assert_eq!(supplied, 3, "three of the ten cells supply a location");
        assert_eq!(withheld, 7);

        let claude = AgentId::new("claude-code");
        let mut hostile = 0_usize;
        for role in [
            ExecutionRole::Gate,
            ExecutionRole::Probe(ProbeTarget::Shell),
        ] {
            let scope = scope(&role, Some(&claude), &recorded, &layout);
            let composed = environment.compose(&scope, &[]).expect("composes");
            assert_eq!(
                value(&composed, "CLAUDE_CONFIG_DIR"),
                Some(""),
                "{role} named an agent and was handed its credential location"
            );
            hostile += 1;
        }
        assert_eq!(hostile, 2);

        let per_agent: std::collections::BTreeSet<String> = VOLUMES
            .iter()
            .map(|(agent, _)| layout.credentials(&AgentId::new(*agent)))
            .collect();
        assert_eq!(per_agent.len(), VOLUMES.len(), "{per_agent:?}");
        assert!(!targets.is_empty());
    }

    #[test]
    fn the_container_boundary_is_case_sensitive_whatever_the_coordinator_is() {
        assert_eq!(
            CONTAINER_KEY_CASE,
            KeyCase::Sensitive,
            "a Linux image has two variables where Windows has one"
        );
        assert_eq!(KeyCase::ALL.len(), 2);

        let volumes = volumes();
        let layout = BoundaryLayout::new();
        let role = ExecutionRole::Gate;
        let scope = scope(&role, None, &volumes, &layout);
        let overlay = vec![("Path".to_owned(), "/opt/tools".to_owned())];

        let sensitive = ContainerEnvironment::with_base(image_base(), KeyCase::Sensitive);
        let composed = sensitive
            .compose(&scope, &overlay)
            .expect("`Path` is not `PATH` at a boundary that tells them apart");
        assert_eq!(value(&composed, "Path"), Some("/opt/tools"));
        assert_eq!(
            value(&composed, "PATH"),
            Some("/usr/local/sbin:/usr/local/bin:/usr/bin:/bin"),
            "both variables survive, which is what case-sensitive means"
        );

        let insensitive = ContainerEnvironment::with_base(image_base(), KeyCase::Insensitive);
        insensitive
            .compose(&scope, &overlay)
            .expect_err("under the other rule `Path` collides with the reserved `PATH`");

        assert_eq!(
            ContainerEnvironment::inherited().case(),
            KeyCase::Sensitive,
            "the coordinator's platform decided the container's name rule"
        );
    }

    #[test]
    fn an_image_credential_variable_does_not_survive_into_a_role_that_takes_none() {
        let volumes = volumes();
        let layout = BoundaryLayout::new();
        let codex = AgentId::new("codex");

        let with_image_value = {
            let mut base = image_base();
            base.push(("CODEX_HOME".to_owned(), "/image/codex".to_owned()));
            base.push(("GH_CONFIG_DIR".to_owned(), "/image/gh".to_owned()));
            ContainerEnvironment::from_image(base)
        };
        let without_image_value = {
            let mut base = image_base();
            base.push(("GH_CONFIG_DIR".to_owned(), "/image/gh".to_owned()));
            ContainerEnvironment::from_image(base)
        };

        let mut cells = 0_usize;
        for (image_label, environment) in [
            ("image sets CODEX_HOME", &with_image_value),
            ("image sets no CODEX_HOME", &without_image_value),
        ] {
            let composed = environment
                .compose(
                    &scope(&ExecutionRole::Gate, Some(&codex), &volumes, &layout),
                    &[],
                )
                .expect("composes");
            assert_eq!(
                value(&composed, "CODEX_HOME"),
                Some(""),
                "{image_label}: a gate is repository-controlled code, and a `CODEX_HOME` this \
                 vector does not name is a `CODEX_HOME` the image chooses"
            );
            assert_eq!(
                value(&composed, "GH_CONFIG_DIR"),
                Some("/image/gh"),
                "{image_label}: the image environment was wiped rather than overridden"
            );
            cells += 1;

            let composed = environment
                .compose(
                    &scope(&ExecutionRole::Implement, Some(&codex), &volumes, &layout),
                    &[],
                )
                .expect("composes");
            assert_eq!(
                value(&composed, "CODEX_HOME"),
                Some(layout.credentials(&codex).as_str()),
                "{image_label}: the value is the boundary's mount target, not the image's own path"
            );
            assert_ne!(
                value(&composed, "CODEX_HOME"),
                Some("/image/codex"),
                "{image_label}: the image's value survived the supply step"
            );
            cells += 1;
        }
        assert_eq!(cells, 4, "{{image sets the key}} x {{role receives it}}");

        let gate = scope(&ExecutionRole::Gate, Some(&codex), &volumes, &layout);
        let withheld: BTreeSet<String> = with_image_value
            .withheld_credential_locations(&gate)
            .into_iter()
            .map(|(key, value)| {
                assert_eq!(value, "", "a withheld location carries a value");
                key
            })
            .collect();
        assert_eq!(
            withheld,
            CREDENTIAL_LOCATIONS
                .iter()
                .map(|(_, key)| (*key).to_owned())
                .collect::<BTreeSet<_>>(),
            "a gate takes no credential location at all, so all three are withheld"
        );
        let implement = scope(&ExecutionRole::Implement, Some(&codex), &volumes, &layout);
        let withheld: BTreeSet<String> = with_image_value
            .withheld_credential_locations(&implement)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert!(
            !withheld.contains("CODEX_HOME") && withheld.len() == CREDENTIAL_LOCATIONS.len() - 1,
            "the one location this scope is given must not also be withheld: {withheld:?}"
        );
    }

    #[test]
    fn the_boundary_layout_derives_every_path_from_its_own_root() {
        let layout = BoundaryLayout::new();
        assert_eq!(layout.workspace(), "/upstroke/workspace");
        assert_eq!(layout.git_view(), "/upstroke/gitview");
        assert_eq!(layout.git_objects(), "/upstroke/gitobjects");
        assert_eq!(layout.git_pointer(), "/upstroke/workspace/.git");
        assert_eq!(
            layout.credentials(&AgentId::new("codex")),
            "/upstroke/credentials/codex"
        );
        assert!(layout.git_pointer().starts_with(layout.workspace()));
        assert!(!layout.git_view().starts_with(layout.workspace()));
        assert!(!layout.git_objects().starts_with(layout.git_view()));

        let moved = BoundaryLayout::with_roots(
            "/elsewhere/ws",
            "/elsewhere/creds",
            "/elsewhere/view",
            "/elsewhere/objects",
            "/elsewhere/scratch",
        );
        assert_eq!(moved.git_pointer(), "/elsewhere/ws/.git");
        assert_eq!(moved.git_view(), "/elsewhere/view");
        assert_eq!(moved.git_objects(), "/elsewhere/objects");
        assert_eq!(
            moved.credentials(&AgentId::new("codex")),
            "/elsewhere/creds/codex"
        );

        let before = [
            layout.workspace().to_owned(),
            layout.git_view().to_owned(),
            layout.git_objects().to_owned(),
            layout.git_pointer(),
            layout.credentials(&AgentId::new("codex")),
        ];
        let after = [
            moved.workspace().to_owned(),
            moved.git_view().to_owned(),
            moved.git_objects().to_owned(),
            moved.git_pointer(),
            moved.credentials(&AgentId::new("codex")),
        ];
        assert_eq!(
            before.iter().zip(&after).filter(|(a, b)| a == b).count(),
            0,
            "a path did not move with its root: {before:?} vs {after:?}"
        );
        let distinct: std::collections::BTreeSet<&String> = before.iter().collect();
        assert_eq!(distinct.len(), before.len(), "{before:?}");
    }

    #[test]
    fn a_path_component_is_cwd_dependent_exactly_when_it_is_not_absolute() {
        let table: &[(&str, &[&str])] = &[
            ("/usr/local/bin:/usr/bin:/bin", &[]),
            ("/bin", &[]),
            ("/usr/local/bin:.:/usr/bin", &["."]),
            (".", &["."]),
            ("..:/bin", &[".."]),
            ("/usr/bin:", &[""]),
            (":/usr/bin", &[""]),
            ("/a::/b", &[""]),
            ("", &[""]),
            ("bin:/usr/bin", &["bin"]),
            ("/usr/bin:tools/bin", &["tools/bin"]),
            (".:/usr/bin:", &[".", ""]),
            ("C:\\Windows", &["C", "\\Windows"]),
        ];
        let mut relative_rows = 0_usize;
        let mut absolute_rows = 0_usize;
        for (value, expected) in table {
            let found = cwd_dependent_path_components(value);
            assert_eq!(&found, expected, "`PATH={value}`");
            if expected.is_empty() {
                absolute_rows += 1;
            } else {
                relative_rows += 1;
            }
        }
        assert_eq!((absolute_rows, relative_rows), (2, 11), "the table shrank");

        let volumes = volumes();
        let layout = BoundaryLayout::new();
        let mut refused = 0_usize;
        let mut composed_ok = 0_usize;
        for role in ExecutionRole::all() {
            let agent = binding(&role);
            let scope = scope(&role, agent.as_ref(), &volumes, &layout);

            let hostile = ContainerEnvironment::from_image(vec![(
                "PATH".to_owned(),
                "/usr/local/bin:.:/usr/bin".to_owned(),
            )]);
            let refusal = hostile
                .compose(&scope, &[])
                .expect_err("a cwd-relative PATH is refused");
            let message = refusal.to_string();
            assert!(message.contains("PATH="), "{role}: {message}");
            assert!(message.contains("DESIGN.md:612"), "{role}: {message}");
            refused += 1;

            let silent = ContainerEnvironment::inherited();
            let refusal = silent
                .compose(&scope, &[])
                .expect_err("an environment naming no PATH is refused");
            assert!(
                refusal.to_string().contains("names no `PATH`"),
                "{role}: {refusal}"
            );
            refused += 1;

            let good = ContainerEnvironment::from_image(image_base());
            good.compose(&scope, &[])
                .expect("an absolute-only PATH composes");
            composed_ok += 1;
        }
        assert_eq!(
            (refused, composed_ok),
            (10, 5),
            "five roles, two refusals each"
        );
    }

    #[test]
    fn credential_scoping_follows_inv18s_split_not_the_predicate() {
        let expected: Vec<(ExecutionRole, bool)> = vec![
            (ExecutionRole::Probe(ProbeTarget::Shell), false),
            (
                ExecutionRole::Probe(ProbeTarget::Agent(AgentId::new("claude-code"))),
                true,
            ),
            (ExecutionRole::Implement, true),
            (ExecutionRole::Gate, false),
            (ExecutionRole::Review, true),
        ];
        assert_eq!(expected.len(), ExecutionRole::all().len());
        for (role, supplies) in &expected {
            assert_eq!(supplies_credential_location(role), *supplies, "{role}");
            assert_eq!(
                supplies_credential_location(role),
                role.is_slotted(),
                "{role}: INV-18 splits slots and credentials the same way"
            );
        }
        assert_eq!(expected.iter().filter(|(_, supplies)| *supplies).count(), 3);
    }
}
