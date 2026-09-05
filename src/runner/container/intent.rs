//! Extended notes: `docs/internals/runner/container/intent.md`

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

use crate::error::UpstrokeError;
use crate::runner::InvocationId;

pub const CONTAINERS_DIR: &str = "containers";

pub const INTENT_SUFFIX: &str = ".intent";

pub const INTENT_STAGED_SUFFIX: &str = ".intent.tmp";

pub const NAME_PREFIX: &str = "upstroke";

pub const NAME_SEPARATOR: char = '-';

pub const INVOCATION_HASH_DOMAIN: &str = "upstroke.container-invocation.v1";

pub const INVOCATION_HASH_HEX_CHARS: usize = 16;

pub const MAX_COMPONENT_LEN: usize = 64;

pub const MAX_NAME_LEN: usize = 200;

pub const LABEL_PRIVATE_ROOT: &str = "upstroke.private_root";
pub const LABEL_RUN: &str = "upstroke.run";
pub const LABEL_RUN_DIR: &str = "upstroke.run_dir";
pub const LABEL_INCARNATION: &str = "upstroke.incarnation";
pub const LABEL_INVOCATION: &str = "upstroke.invocation";

pub const LABELS: &[&str] = &[
    LABEL_PRIVATE_ROOT,
    LABEL_RUN,
    LABEL_RUN_DIR,
    LABEL_INCARNATION,
    LABEL_INVOCATION,
];

const LABEL_UNRESERVED: &[u8] = b"/:.-_";

const HEX: &[u8; 16] = b"0123456789ABCDEF";

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

#[must_use]
pub fn private_root_label(private_root: &Path) -> String {
    path_label(private_root)
}

pub fn decode_path_label(value: &str) -> Result<PathBuf, UpstrokeError> {
    let refuse = |why: &str| UpstrokeError::Refused {
        message: format!(
            "the label value `{value}` is not a upstroke path label ({why}); a path label is its \
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerIntent {
    pub run_id: String,
    pub run_dir: String,
    pub incarnation: String,
    pub repo_key: String,
    pub invocation: String,
    pub runner_policy_sha256: String,
}

impl ContainerIntent {
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

    pub fn run_dir_path(&self) -> Result<PathBuf, UpstrokeError> {
        owner_run_dir(&self.run_dir, "intent record")
    }

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

pub fn owner_run_dir(value: &str, source: &str) -> Result<PathBuf, UpstrokeError> {
    if value.is_empty() {
        return Err(UpstrokeError::Refused {
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
    let path = decode_path_label(value).map_err(|error| UpstrokeError::Refused {
        message: format!("the {source}'s `{LABEL_RUN_DIR}` is unreadable: {error}"),
    })?;
    if !path.has_root() {
        return Err(UpstrokeError::Refused {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentWritten {
    name: ContainerName,
    path: PathBuf,
    record: ContainerIntent,
}

impl IntentWritten {
    pub fn certify(private_root: &Path, name: &ContainerName) -> Result<Self, UpstrokeError> {
        let path = name.intent_path(private_root);
        let bytes = fs::read(&path).map_err(|source| UpstrokeError::Io {
            path: path.clone(),
            source,
        })?;
        let record: ContainerIntent =
            serde_json::from_slice(&bytes).map_err(|error| UpstrokeError::Refused {
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

    #[must_use]
    pub const fn name(&self) -> &ContainerName {
        &self.name
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn record(&self) -> &ContainerIntent {
        &self.record
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainerName(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerNameParts {
    pub repo_key: String,
    pub run_id: String,
    pub incarnation: String,
    pub invocation_hash: String,
}

impl ContainerName {
    pub fn new(
        repo_key: &str,
        run_id: &str,
        incarnation: &str,
        invocation: &InvocationId,
    ) -> Result<Self, UpstrokeError> {
        Self::from_parts(repo_key, run_id, incarnation, &invocation_hash(invocation))
    }

    pub fn from_parts(
        repo_key: &str,
        run_id: &str,
        incarnation: &str,
        invocation_hash: &str,
    ) -> Result<Self, UpstrokeError> {
        validate_component("repo key", repo_key)?;
        validate_component("run id", run_id)?;
        validate_component("incarnation", incarnation)?;
        validate_component("invocation hash", invocation_hash)?;
        let rendered = format!("{NAME_PREFIX}-{repo_key}-{run_id}-{incarnation}-{invocation_hash}");
        if rendered.len() > MAX_NAME_LEN {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "the container name `{rendered}` is {} bytes; the limit is {MAX_NAME_LEN}",
                    rendered.len()
                ),
            });
        }
        Ok(Self(rendered))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn intent_file_name(&self) -> String {
        format!("{}{INTENT_SUFFIX}", self.0)
    }

    #[must_use]
    pub fn intent_path(&self, private_root: &Path) -> PathBuf {
        containers_dir(private_root).join(self.intent_file_name())
    }

    pub fn parse(value: &str) -> Result<ContainerNameParts, UpstrokeError> {
        let refuse = || UpstrokeError::Refused {
            message: format!(
                "`{value}` is not a upstroke container name: the name is \
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

    pub fn rebuild(value: &str) -> Result<Self, UpstrokeError> {
        let parts = Self::parse(value)?;
        Self::from_parts(
            &parts.repo_key,
            &parts.run_id,
            &parts.incarnation,
            &parts.invocation_hash,
        )
    }

    pub fn from_intent_file_name(file_name: &str) -> Result<Option<Self>, UpstrokeError> {
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

#[must_use]
pub fn containers_dir(private_root: &Path) -> PathBuf {
    private_root.join(CONTAINERS_DIR)
}

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

fn validate_component(what: &str, value: &str) -> Result<(), UpstrokeError> {
    if value.is_empty() {
        return Err(UpstrokeError::Refused {
            message: format!("a container name's {what} component is never empty"),
        });
    }
    if value.len() > MAX_COMPONENT_LEN {
        return Err(UpstrokeError::Refused {
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
        return Err(UpstrokeError::Refused {
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
