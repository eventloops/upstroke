//! Extended notes: `docs/internals/runner/host/environment.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::ffi::{OsStr, OsString};

use crate::error::UpstrokeError;
use crate::runner::{AgentId, ExecutionRole};

use super::{RESERVED_ALWAYS, credential_location, reserved_keys, supplies_credentials};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyCase {
    Sensitive,
    Insensitive,
}

impl KeyCase {
    pub const ALL: &'static [Self] = &[Self::Sensitive, Self::Insensitive];

    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Insensitive
        } else {
            Self::Sensitive
        }
    }

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

#[derive(Debug)]
pub struct HostEnvironment {
    base: Vec<(OsString, OsString)>,
    case: KeyCase,
}

impl HostEnvironment {
    #[must_use]
    pub fn from_process() -> Self {
        Self {
            base: std::env::vars_os().collect(),
            case: KeyCase::current(),
        }
    }

    #[must_use]
    pub fn with_base(base: Vec<(OsString, OsString)>, case: KeyCase) -> Self {
        Self { base, case }
    }

    #[must_use]
    pub fn base(&self) -> &[(OsString, OsString)] {
        &self.base
    }

    #[must_use]
    pub const fn case(&self) -> KeyCase {
        self.case
    }

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
                         (design/08_design_trait_surface.md:57-63)"
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
