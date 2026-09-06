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

const EXACTLY_REPRESENTABLE_UNITS: i64 = 1 << 53;

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
    match toml::from_str(&text) {
        Ok(raw) => Ok((raw, path)),
        Err(e) => Err(UpstrokeError::Config {
            path,
            message: e.to_string(),
        }),
    }
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
    let raw: RawPools = match toml::from_str(&text) {
        Ok(raw) => raw,
        Err(e) => {
            return Err(UpstrokeError::Config {
                path,
                message: e.to_string(),
            });
        }
    };
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
        Some(toml::Value::Integer(units)) => {
            if units > EXACTLY_REPRESENTABLE_UNITS {
                return Err(config_error(format!(
                    "[pools.{name}] `monthly_allowance = {units}` is larger than an allowance is \
                     held to ({EXACTLY_REPRESENTABLE_UNITS}) — above that an allowance cannot be \
                     stored without changing the number you wrote"
                )));
            }
            Allowance::Units(units as f64)
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pool_value(body: &str) -> toml::Value {
        toml::from_str(body).expect("valid pool body")
    }

    fn any_adapter(_: &str) -> bool {
        true
    }

    /// A path this process has not created and no other run can be holding: the process id
    /// separates concurrent runs and the counter separates calls within one. Nothing is created
    /// here, so nothing needs removing — these tests want a name whose whole point is that it
    /// resolves to no file, and reaching for `fs` to guarantee that would put a governed
    /// primitive in a module that denies all three and takes no `effects/allowlist.toml` row.
    fn absent_path(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "upstroke-config-read-absent-{}-{unique}-{tag}.toml",
            std::process::id()
        ))
    }

    #[test]
    fn read_repo_config_of_a_required_absent_file_is_an_error() {
        let path = absent_path("required");
        let snapshot = crate::config::snapshot_file(&path, true);
        let err = read_repo_config(&snapshot).expect_err("a required file that is absent errors");
        assert!(matches!(err, UpstrokeError::Config { .. }));
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn read_repo_config_of_an_optional_absent_file_is_the_default() {
        let path = absent_path("optional");
        let snapshot = crate::config::snapshot_file(&path, false);
        let (raw, returned_path) = read_repo_config(&snapshot).expect("absent optional defaults");
        assert_eq!(returned_path, path);
        assert!(raw.routing.is_none());
        assert!(raw.pins.is_none());
    }

    #[test]
    fn a_blank_pool_name_is_refused() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let err = parse_pool(
            "   ",
            pool_value("kind = \"credits\"\nagent = \"claude-code\"\n"),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect_err("a blank name is refused");
        assert!(err.to_string().contains("non-empty name"));
    }

    #[test]
    fn an_unrecognized_kind_is_refused_naming_the_accepted_set() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let err = parse_pool(
            "p",
            pool_value("kind = \"subscription\"\nagent = \"claude-code\"\n"),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect_err("an unrecognized kind is refused");
        assert!(err.to_string().contains("subscription"));
    }

    #[test]
    fn a_missing_agent_is_refused() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let err = parse_pool(
            "p",
            pool_value("kind = \"credits\"\n"),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect_err("a pool with no agent is refused");
        assert!(err.to_string().contains("no `agent`"));
    }

    #[test]
    fn an_unparseable_window_is_refused() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let err = parse_pool(
            "p",
            pool_value(
                "kind = \"subscription-window\"\nagent = \"claude-code\"\nwindow = \"soon\"\n",
            ),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect_err("an unparseable window is refused");
        assert!(err.to_string().contains("soon"));
    }

    #[test]
    fn safety_margin_outside_zero_to_one_is_refused() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let err = parse_pool(
            "p",
            pool_value("kind = \"credits\"\nagent = \"claude-code\"\nsafety_margin = 1.5\n"),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect_err("an out-of-range safety_margin is refused");
        assert!(err.to_string().contains("safety_margin"));
    }

    #[test]
    fn reserve_outside_zero_to_one_is_refused() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let err = parse_pool(
            "p",
            pool_value("kind = \"credits\"\nagent = \"claude-code\"\nreserve = -0.1\n"),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect_err("a negative reserve is refused");
        assert!(err.to_string().contains("reserve"));
    }

    #[test]
    fn monthly_allowance_accepts_auto_case_and_whitespace_insensitively() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let pool = parse_pool(
            "p",
            pool_value(
                "kind = \"credits\"\nagent = \"claude-code\"\nmonthly_allowance = \" AUTO \"\n",
            ),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect("auto, any case and padding, is accepted");
        assert_eq!(pool.monthly_allowance, Allowance::Auto);
    }

    #[test]
    fn monthly_allowance_accepts_an_integer_as_units() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let pool = parse_pool(
            "p",
            pool_value("kind = \"credits\"\nagent = \"claude-code\"\nmonthly_allowance = 300\n"),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect("an integer allowance is accepted");
        assert_eq!(pool.monthly_allowance, Allowance::Units(300.0));
    }

    #[test]
    fn monthly_allowance_accepts_a_float_as_units() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let pool = parse_pool(
            "p",
            pool_value("kind = \"credits\"\nagent = \"claude-code\"\nmonthly_allowance = 12.5\n"),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect("a float allowance is accepted");
        assert_eq!(pool.monthly_allowance, Allowance::Units(12.5));
    }

    #[test]
    fn the_largest_exactly_representable_integer_allowance_is_accepted_unchanged() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let pool = parse_pool(
            "p",
            pool_value(&format!(
                "kind = \"credits\"\nagent = \"claude-code\"\nmonthly_allowance = {EXACTLY_REPRESENTABLE_UNITS}\n"
            )),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect("the boundary value is an allowance");
        // Written out rather than spelled `EXACTLY_REPRESENTABLE_UNITS as f64`, which is the very
        // cast under test: the expected value has to come from outside the code being proved.
        assert_eq!(EXACTLY_REPRESENTABLE_UNITS, 9_007_199_254_740_992);
        assert_eq!(
            pool.monthly_allowance,
            Allowance::Units(9_007_199_254_740_992.0),
            "the boundary value survives the cast as the number that was written"
        );
    }

    #[test]
    fn an_integer_allowance_the_cast_would_change_is_refused() {
        let path = Path::new("pools.toml");
        for units in [EXACTLY_REPRESENTABLE_UNITS + 1, i64::MAX] {
            // i64::MAX casts to 9223372036854775808.0, one more than it is: accepting it would
            // silently store an allowance the operator did not write.
            let mut warnings = Vec::new();
            let err = parse_pool(
                "p",
                pool_value(&format!(
                    "kind = \"credits\"\nagent = \"claude-code\"\nmonthly_allowance = {units}\n"
                )),
                path,
                &any_adapter,
                &mut warnings,
            )
            .expect_err("an allowance past the exactly-representable range is refused");
            assert!(err.to_string().contains("monthly_allowance"), "got: {err}");
        }
    }

    #[test]
    fn monthly_allowance_rejects_zero_and_negative_and_non_finite() {
        let path = Path::new("pools.toml");
        for body in [
            "kind = \"credits\"\nagent = \"claude-code\"\nmonthly_allowance = 0\n",
            "kind = \"credits\"\nagent = \"claude-code\"\nmonthly_allowance = -5\n",
            "kind = \"credits\"\nagent = \"claude-code\"\nmonthly_allowance = nan\n",
            "kind = \"credits\"\nagent = \"claude-code\"\nmonthly_allowance = inf\n",
        ] {
            let mut warnings = Vec::new();
            let err = parse_pool("p", pool_value(body), path, &any_adapter, &mut warnings)
                .expect_err(&format!("{body} is not a usable allowance"));
            assert!(err.to_string().contains("monthly_allowance"), "got: {err}");
        }
    }

    #[test]
    fn monthly_allowance_of_the_wrong_shape_names_the_type_it_saw() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let err = parse_pool(
            "p",
            pool_value("kind = \"credits\"\nagent = \"claude-code\"\nmonthly_allowance = true\n"),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect_err("a boolean allowance is refused");
        assert!(err.to_string().contains("monthly_allowance"));
    }

    #[test]
    fn duplicate_sources_entries_are_deduplicated_in_file_order() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let pool = parse_pool(
            "p",
            pool_value(
                "kind = \"credits\"\nagent = \"claude-code\"\nsources = [\"signals\", \"signals\"]\n",
            ),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect("duplicate sources parse");
        assert_eq!(pool.sources.len(), 1);
    }

    #[test]
    fn an_agent_with_no_adapter_warns_and_the_pool_stays_but_unusable() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let pool = parse_pool(
            "p",
            pool_value("kind = \"credits\"\nagent = \"nonexistent\"\n"),
            path,
            &|_: &str| false,
            &mut warnings,
        )
        .expect("a pool for an unknown adapter is still a pool");
        assert!(!pool.usable);
        assert!(warnings.iter().any(|w| w.contains("nonexistent")));
    }

    #[test]
    fn an_unknown_key_warns_by_name_and_does_not_fail_the_pool() {
        let path = Path::new("pools.toml");
        let mut warnings = Vec::new();
        let pool = parse_pool(
            "p",
            pool_value("kind = \"credits\"\nagent = \"claude-code\"\nfrobnicate = true\n"),
            path,
            &any_adapter,
            &mut warnings,
        )
        .expect("an unknown key degrades to a warning, not a refusal");
        assert!(pool.usable);
        assert!(warnings.iter().any(|w| w.contains("frobnicate")));
    }
}
