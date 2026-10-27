//! The tolerance model: how close two results must be to count as the same, for the geometry at
//! hand rather than in absolute drawing units.
//!
//! A fixed distance such as `1e-9` is far too loose for a part drawn in micrometres and below
//! the floating-point resolution of survey coordinates (a point at 5e6 m has a spacing of about
//! 1e-9 m between neighbouring `f64` values). AutoCAD's geometry kernel works with a tolerance
//! context relative to the geometry; so does this one:
//!
//! ```text
//! dist = max(REL × size, ROUND × magnitude, TINY)
//! ```
//!
//! - `size` is the extent of the geometry being compared (a bounding-box diagonal, a radius plus
//!   an offset distance …): results within one part in 10⁹ of it are the same point;
//! - `magnitude` is how far that geometry lies from the origin: no test can be finer than the
//!   rounding of its coordinates (`ROUND` is a small multiple of the machine epsilon);
//! - `TINY` keeps the tolerance positive for a point-sized input.
//!
//! [`Tol`] is the one helper; the polyline offset (`poly_offset.rs`) compares through it (or
//! through [`REL`] for dimensionless ratios such as angles and curve parameters). Other code still
//! uses the absolute [`crate::EPS`] and can move over to it.

use crate::{Bounds2, Vec2};

/// Relative tolerance: two results within this fraction of the size of the geometry are the
/// same.
pub const REL: f64 = 1e-9;
/// Coordinate rounding: a tolerance never goes below this many units in the last place of the
/// largest coordinate involved.
pub const ROUND: f64 = 64.0 * f64::EPSILON;
/// The smallest tolerance (for point-sized geometry at the origin).
pub const TINY: f64 = 1e-300;

/// A distance tolerance for one piece of geometry. See the module documentation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tol {
    /// Distances at or below this are zero; points this close are the same point.
    pub dist: f64,
}

impl Tol {
    /// The tolerance for geometry of extent `size` lying up to `magnitude` from the origin.
    /// Non-finite input gives the old absolute tolerance ([`crate::EPS`]), never NaN.
    pub fn new(size: f64, magnitude: f64) -> Tol {
        let (size, magnitude) = (size.abs(), magnitude.abs());
        if !(size.is_finite() && magnitude.is_finite()) {
            return Tol { dist: crate::EPS };
        }
        Tol { dist: (REL * size).max(ROUND * magnitude).max(TINY) }
    }

    /// For the geometry inside `b` (its diagonal and its farthest corner from the origin).
    pub fn of_bounds(b: &Bounds2) -> Tol {
        if b.is_empty() {
            return Tol { dist: TINY };
        }
        let magnitude = b.min.x.abs().max(b.min.y.abs()).max(b.max.x.abs()).max(b.max.y.abs());
        Tol::new(b.width().hypot(b.height()), magnitude)
    }

    /// For the geometry through `pts`.
    pub fn of_points<I: IntoIterator<Item = Vec2>>(pts: I) -> Tol {
        Tol::of_bounds(&Bounds2::from_points(pts))
    }

    /// For a circle or arc of `radius` round `center`.
    pub fn of_circle(center: Vec2, radius: f64) -> Tol {
        Tol::new(2.0 * radius, center.x.abs().max(center.y.abs()) + radius.abs())
    }

    /// This tolerance times `k` (looser for `k > 1`).
    pub fn times(self, k: f64) -> Tol {
        let d = self.dist * k.abs();
        Tol { dist: if d.is_finite() && d > 0.0 { d } else { self.dist } }
    }

    /// Whether `a` and `b` are the same point.
    pub fn near(&self, a: Vec2, b: Vec2) -> bool {
        a.near(b, self.dist)
    }

    /// Whether a distance (or length) is zero.
    pub fn is_zero(&self, d: f64) -> bool {
        d.abs() <= self.dist
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerance_follows_the_size_and_position_of_the_geometry() {
        // A 10 mm part at the origin: one part in 1e9 of its size.
        let t = Tol::new(10.0, 10.0);
        assert!((t.dist - 1e-8).abs() < 1e-20);
        // The same part at survey coordinates: the coordinate rounding decides.
        let far = Tol::new(10.0, 5e6);
        assert!(far.dist > 5e6 * f64::EPSILON && far.dist < 1e-7, "{far:?}");
        // A micrometre part keeps a micrometre-scale tolerance, not 1e-9 units.
        let tiny = Tol::new(1e-6, 1e-6);
        assert!(tiny.dist < 1e-14);
        assert!(!tiny.near(Vec2::ZERO, Vec2::new(1e-12, 0.0)));
        // Hostile input never gives NaN or zero.
        for t in [Tol::new(f64::NAN, 1.0), Tol::new(1.0, f64::INFINITY), Tol::new(0.0, 0.0), Tol::of_bounds(&Bounds2::EMPTY)] {
            assert!(t.dist > 0.0 && t.dist.is_finite(), "{t:?}");
        }
        assert_eq!(Tol::new(1.0, 1.0).times(f64::NAN), Tol::new(1.0, 1.0));
        assert!(Tol::of_circle(Vec2::new(1e6, 0.0), 1.0).dist >= 1e6 * ROUND);
    }
}
