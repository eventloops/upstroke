//! Turning captured bytes into raw and typed shapes.
//!
//! Every function here that reads a file's contents takes a [`FileSnapshot`]
//! and never a path. That is the whole of what makes a pre-lock validation
//! worth its ordering: the bytes that were validated and the bytes that are
//! parsed are the same object, so there is no second read for the file to
//! change between. A reader here that reached for the path instead would
//! reintroduce exactly the window the capture exists to close.
//!
//! `parse_pool` is the exception to the shape and not to the rule: it takes a
//! `&Path` and never reads through it. The path is what it names in the error
//! it returns and in one diagnostic string, nothing more. A later change that
//! wants a file's contents here reaches for the snapshot, never for that path.
//!
//! `[pools]` keeps the temperament the rest of the configuration surface has:
//! anything that would silently change what the estimator does is an error,
//! anything that only degrades what it can say is a warning that names the key.
//! Pool order is file order, and file order is preference — the span each entry
//! carries is what preserves it through a map.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree rather than by the file, so an out-of-line
// child of `src/config.rs` inherits that file's inner
// `#![allow(clippy::disallowed_methods)]` unless it says otherwise --
// `PR6-LANEF-004`, and the mistake two W1 pull requests then made
// independently (#100 and #102). Nothing here reaches a governed primitive, so
// all three governed lints are DENIED and this module takes no
// `effects/allowlist.toml` row: a row records an allowance, and this module
// takes none.
//
// The three are not equally load-bearing, and which is which is worth stating.
// `src/config.rs` allows `clippy::disallowed_methods` and that lint alone, so
// the first line below is the one that restores a level the parent removed
// outright: without it, a denied method here raises no diagnostic at all. The
// other two raise this module from clippy's default `warn` to `deny`, so a
// denied type or macro fails here on its own rather than only under CI's
// `-D warnings`. All three are written out because what decides the first one
// is a property of the parent's attribute rather than of this file, and a
// parent's attribute can widen without this file changing.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use super::{
    Allowance, FileSnapshot, Path, PathBuf, Pool, PoolKind, RawPool, RawPools, RawRepoConfig,
    Source, UpstrokeError, capacity,
};

pub(super) fn read_repo_config(
    snapshot: &FileSnapshot,
) -> Result<(RawRepoConfig, PathBuf), UpstrokeError> {
    let path = snapshot.path().to_path_buf();
    let Some(text) = snapshot.text()? else {
        if snapshot.required {
            return Err(UpstrokeError::Config {
                path,
                message: "file not found".to_owned(),
            });
        }
        return Ok((RawRepoConfig::default(), path));
    };
    let raw = toml::from_str(&text).map_err(|e| UpstrokeError::Config {
        path: path.clone(),
        message: e.to_string(),
    })?;
    Ok((raw, path))
}

/// Read `~/.upstroke/pools.toml` into typed pools (§17).
///
/// Temperament matches the rest of this file: anything that would silently
/// change what the estimator does is an error, and anything that only degrades
/// what it can say is a warning.
///
/// - unknown `kind` → **error**; it decides which estimator rule runs.
/// - unknown `sources` entry → **error**; dropping `signals` by typo would
///   discard §13's ground truth while the file still claims to have it.
/// - `safety_margin` / `reserve` outside `0.0..=1.0` → **error**; both are
///   fractions, and a "150% margin" has no reading that is merely degraded.
/// - `agent` with no adapter in this build → **warn**, pool kept and marked
///   unusable. §17's own example ships `[pools.local] agent = "aider"`, so
///   erroring would brick anyone who copied the documented file.
/// - unknown keys → **warn**, by name.
///
/// An **explicit** `--pools` path that does not exist is an error, the way an
/// explicit `--config` is in [`read_repo_config`]: a path someone typed and
/// that is not there is a typo, and answering it with "no pools connected —
/// run `upstroke connect`" sends them to regenerate a file that was never the
/// problem. A *discovered* one that is absent is the normal fresh case and
/// stays silent.
pub(super) fn read_pools(
    pools: Option<&FileSnapshot>,
    has_adapter: &dyn Fn(&str) -> bool,
    warnings: &mut Vec<String>,
) -> Result<Vec<Pool>, UpstrokeError> {
    let Some(snapshot) = pools else {
        return Ok(Vec::new());
    };
    let path = snapshot.path().to_path_buf();
    let Some(text) = snapshot.text()? else {
        if snapshot.required {
            return Err(UpstrokeError::Config {
                path,
                message: "pools file not found".to_owned(),
            });
        }
        return Ok(Vec::new());
    };
    let raw: RawPools = toml::from_str(&text).map_err(|e| UpstrokeError::Config {
        path: path.clone(),
        message: e.to_string(),
    })?;
    // Back into the order they were written in — see [`RawPools`].
    let mut entries: Vec<(String, toml::Spanned<toml::Value>)> =
        raw.pools.unwrap_or_default().into_iter().collect();
    entries.sort_by_key(|(_, spanned)| spanned.span().start);
    let mut pools = Vec::new();
    for (name, spanned) in entries {
        pools.push(parse_pool(
            &name,
            spanned.into_inner(),
            &path,
            has_adapter,
            warnings,
        )?);
    }
    Ok(pools)
}

fn parse_pool(
    name: &str,
    value: toml::Value,
    path: &Path,
    has_adapter: &dyn Fn(&str) -> bool,
    warnings: &mut Vec<String>,
) -> Result<Pool, UpstrokeError> {
    let config_error = |message: String| UpstrokeError::Config {
        path: path.to_path_buf(),
        message,
    };
    // A pool's name is its identity everywhere downstream — it is what an
    // attempt is attributed to and what the ledger prints. A blank one is
    // indistinguishable from "no pool" by the time it reaches the engine
    // (`pool_option` maps `""` to `None`), so the attribution would vanish
    // while the pool still matched for routing. Same reasoning as the
    // non-empty `[[gates]]` `name`.
    if name.trim().is_empty() {
        return Err(config_error(
            "a pool needs a non-empty name — `[pools.<name>]` is what attempts are attributed to"
                .to_owned(),
        ));
    }
    let raw: RawPool = value.try_into().map_err(|e| {
        config_error(format!(
            "[pools.{name}]: {e} (expected `kind` and `agent` strings, with optional `window`, \
             `weekly`, `sources`, `safety_margin`, `reserve`, `monthly_allowance`, `endpoint`, \
             and `profile`)"
        ))
    })?;

    let kind_text = raw.kind.ok_or_else(|| {
        config_error(format!(
            "[pools.{name}] has no `kind` — one of: {}",
            PoolKind::ACCEPTED.join(", ")
        ))
    })?;
    let kind = PoolKind::parse(&kind_text).ok_or_else(|| {
        config_error(format!(
            "[pools.{name}] `kind = \"{kind_text}\"` is not recognized (accepted: {})",
            PoolKind::ACCEPTED.join(", ")
        ))
    })?;
    let agent = raw
        .agent
        .ok_or_else(|| config_error(format!("[pools.{name}] has no `agent`")))?;

    let window = match raw.window {
        None => None,
        Some(text) => Some(capacity::parse_duration(&text).ok_or_else(|| {
            config_error(format!(
                "[pools.{name}] `window = \"{text}\"` is not a duration — write a number and one \
                 of s, m, h, d (for example \"5h\")"
            ))
        })?),
    };

    let mut sources = Vec::new();
    for entry in raw.sources.unwrap_or_default() {
        let source = Source::parse(&entry).ok_or_else(|| {
            config_error(format!(
                "[pools.{name}] `sources` entry `{entry}` is not recognized (accepted: {})",
                Source::ACCEPTED.join(", ")
            ))
        })?;
        if !sources.contains(&source) {
            sources.push(source);
        }
    }

    let fraction = |field: &str, value: Option<f64>, default: f64| -> Result<f64, UpstrokeError> {
        let Some(value) = value else {
            return Ok(default);
        };
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(config_error(format!(
                "[pools.{name}] `{field} = {value}` is out of range — it is a fraction of the \
                 pool between 0.0 and 1.0"
            )));
        }
        Ok(value)
    };
    let safety_margin = fraction(
        "safety_margin",
        raw.safety_margin,
        capacity::DEFAULT_SAFETY_MARGIN,
    )?;
    let reserve = fraction("reserve", raw.reserve, capacity::DEFAULT_RESERVE)?;

    let monthly_allowance = match raw.monthly_allowance {
        None => Allowance::Auto,
        Some(toml::Value::String(text)) if text.trim().eq_ignore_ascii_case("auto") => {
            Allowance::Auto
        }
        Some(toml::Value::Integer(units)) => Allowance::Units(units as f64),
        Some(toml::Value::Float(units)) => Allowance::Units(units),
        Some(other) => {
            return Err(config_error(format!(
                "[pools.{name}] `monthly_allowance` must be a number of units or the string \
                 \"auto\", found a {}",
                other.type_str()
            )));
        }
    };
    if let Allowance::Units(units) = monthly_allowance {
        if !units.is_finite() || units <= 0.0 {
            return Err(config_error(format!(
                "[pools.{name}] `monthly_allowance = {units}` is not an allowance — write \"auto\" if \
                 you do not know its size"
            )));
        }
    }

    for key in raw.unknown.keys() {
        warnings.push(format!(
            "unknown key `{key}` in [pools.{name}] in {} (ignored)",
            path.display()
        ));
    }

    let usable = has_adapter(&agent);
    if !usable {
        warnings.push(format!(
            "[pools.{name}] names agent `{agent}`, which has no adapter in this build — the pool \
             is listed but this engine can never draw from it"
        ));
    }

    Ok(Pool {
        name: name.to_owned(),
        kind,
        agent,
        window,
        weekly: raw.weekly.unwrap_or(false),
        sources,
        safety_margin,
        reserve,
        monthly_allowance,
        endpoint: raw.endpoint,
        profile: raw.profile,
        usable,
    })
}
