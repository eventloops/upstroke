//! Extended notes: `docs/internals/agent/proc/hooks.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use crate::error::UpstrokeError;
use crate::topology::effects::{Injection, InjectionMode, SubEffectPoint};

pub trait SpawnHooks {
    fn point(&mut self, point: SubEffectPoint) -> Injection;

    fn point_mode(&mut self, point: SubEffectPoint, mode: InjectionMode) -> Injection {
        let _ = mode;
        self.point(point)
    }

    fn child_created(&mut self, _pid: u32) {}
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl SpawnHooks for NoHooks {
    fn point(&mut self, _point: SubEffectPoint) -> Injection {
        Injection::Proceed
    }
}

pub(super) fn apply(injection: Injection, point: SubEffectPoint) -> Result<(), UpstrokeError> {
    match injection {
        Injection::Proceed => Ok(()),
        Injection::Kill => std::process::abort(),
        Injection::Error => Err(UpstrokeError::Refused {
            message: format!(
                "the process funnel was made to fail at its `{point}` containment step"
            ),
        }),
    }
}

#[cfg(windows)]
pub(super) fn apply_io(injection: Injection, point: SubEffectPoint) -> std::io::Result<()> {
    match injection {
        Injection::Proceed => Ok(()),
        Injection::Kill => std::process::abort(),
        Injection::Error => Err(std::io::Error::other(format!(
            "the process funnel was made to fail at its `{point}` containment step"
        ))),
    }
}
