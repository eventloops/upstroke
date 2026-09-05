//! Extended notes: `docs/internals/config/read.md`

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
