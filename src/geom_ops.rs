//! Boolean operations and offset/contour on vector geometry, backed by `i_overlay`.
//! All geometry here is in document (mm) coordinates.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::outline::offset::OutlineOffset;
use i_overlay::mesh::style::OutlineStyle;

use crate::geometry::{Polyline, Vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Intersect,
    Difference,
}

type Contour = Vec<[f64; 2]>;
type Shape = Vec<Contour>;

fn to_shape(polylines: &[Polyline]) -> Shape {
    polylines
        .iter()
        .filter(|p| p.points.len() >= 3)
        .map(|p| p.points.iter().map(|v| [v.x, v.y]).collect())
        .collect()
}

fn from_shapes(shapes: Vec<Shape>) -> Vec<Polyline> {
    let mut out = Vec::new();
    for shape in shapes {
        for contour in shape {
            if contour.len() >= 3 {
                out.push(Polyline {
                    points: contour.into_iter().map(|p| Vec2::new(p[0], p[1])).collect(),
                    closed: true,
                });
            }
        }
    }
    out
}

/// Combine two polyline sets with a boolean operation.
pub fn boolean(a: &[Polyline], b: &[Polyline], op: BoolOp) -> Vec<Polyline> {
    let subj = to_shape(a);
    let clip = to_shape(b);
    if subj.is_empty() {
        return Vec::new();
    }
    let rule = match op {
        BoolOp::Union => OverlayRule::Union,
        BoolOp::Intersect => OverlayRule::Intersect,
        BoolOp::Difference => OverlayRule::Difference,
    };
    let shapes = subj.overlay(&clip, rule, FillRule::NonZero);
    from_shapes(shapes)
}

/// Union all contours within a single set (weld overlapping subpaths).
pub fn weld(a: &[Polyline]) -> Vec<Polyline> {
    let subj = to_shape(a);
    if subj.is_empty() {
        return Vec::new();
    }
    let shapes = subj.overlay(&Vec::<Contour>::new(), OverlayRule::Subject, FillRule::NonZero);
    from_shapes(shapes)
}

fn signed_area(c: &Contour) -> f64 {
    let n = c.len();
    let mut a = 0.0;
    for i in 0..n {
        let p = c[i];
        let q = c[(i + 1) % n];
        a += p[0] * q[1] - q[0] * p[1];
    }
    a * 0.5
}

/// Offset (outset if `dist > 0`, inset if `dist < 0`) a set of closed polylines.
/// The outline builder is winding-sensitive, so we normalise outer contours to
/// positive area and retry with the opposite winding if the first pass is empty.
pub fn offset(a: &[Polyline], dist: f64) -> Vec<Polyline> {
    let mut subj = to_shape(a);
    if subj.is_empty() || dist == 0.0 {
        return a.to_vec();
    }
    // Orient each contour to positive (CCW) signed area.
    for c in subj.iter_mut() {
        if signed_area(c) < 0.0 {
            c.reverse();
        }
    }
    let out = subj.outline(OutlineStyle::new(dist));
    if !out.is_empty() {
        return from_shapes(out);
    }
    // Retry with reversed winding.
    for c in subj.iter_mut() {
        c.reverse();
    }
    let out2 = subj.outline(OutlineStyle::new(dist));
    from_shapes(out2)
}
