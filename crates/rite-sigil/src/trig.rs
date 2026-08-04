//! Deterministic trigonometry.
//!
//! `f64::sin`, `cos` and `atan2` dispatch to the platform's libm, whose last
//! bit differs between Linux and macOS on some inputs — which made scene
//! goldens platform-dependent the first time macOS CI met them: one ornament
//! filament's anchor, one ULP apart. The `libm` crate is the same mathematics
//! in pure Rust, bit-identical on every platform and under wasm32, so every
//! place this crate turns an angle into a coordinate goes through here.
//!
//! `sqrt` needs no wrapper — IEEE 754 defines it exactly, and the router
//! already computes lengths as `sqrt(dx² + dy²)` rather than `hypot` for the
//! same reason. Additions, multiplications and `round` are exact too; it is
//! only the transcendental functions that a platform gets to interpret.

/// The transcendental functions this crate is allowed to use.
pub(crate) trait DeterministicTrig {
    fn dsin(self) -> f64;
    fn dcos(self) -> f64;
    fn datan2(self, other: f64) -> f64;
}

impl DeterministicTrig for f64 {
    #[inline]
    fn dsin(self) -> f64 {
        libm::sin(self)
    }

    #[inline]
    fn dcos(self) -> f64 {
        libm::cos(self)
    }

    #[inline]
    fn datan2(self, other: f64) -> f64 {
        libm::atan2(self, other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values agree with std to within an ULP — the point is not different
    /// answers, it is the *same* answer everywhere.
    #[test]
    fn deterministic_trig_tracks_std() {
        for i in 0..1000 {
            let x = (i as f64) * 0.0137 - 6.5;
            assert!((x.dsin() - x.sin()).abs() < 1e-12);
            assert!((x.dcos() - x.cos()).abs() < 1e-12);
            assert!((x.datan2(1.3) - x.atan2(1.3)).abs() < 1e-12);
        }
    }
}
