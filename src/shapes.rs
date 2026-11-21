//! Primitive shape builders. Each returns polylines in local mm coordinates plus the
//! document-space position for the object's local origin.

use crate::geometry::{Polyline, Vec2};

/// Rectangle spanning two opposite corners.
pub fn rect(a: Vec2, b: Vec2) -> (Vec<Polyline>, Vec2) {
    let min = Vec2::new(a.x.min(b.x), a.y.min(b.y));
    let w = (a.x - b.x).abs();
    let h = (a.y - b.y).abs();
    let pl = Polyline {
        points: vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(w, 0.0),
            Vec2::new(w, h),
            Vec2::new(0.0, h),
        ],
        closed: true,
    };
    (vec![pl], min)
}

/// Ellipse inscribed in the bounding box of two corners.
pub fn ellipse(a: Vec2, b: Vec2) -> (Vec<Polyline>, Vec2) {
    let min = Vec2::new(a.x.min(b.x), a.y.min(b.y));
    let w = (a.x - b.x).abs();
    let h = (a.y - b.y).abs();
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (cx, cy) = (rx, ry);
    let segs = 72;
    let mut pts = Vec::with_capacity(segs);
    for i in 0..segs {
        let t = i as f64 / segs as f64 * std::f64::consts::TAU;
        pts.push(Vec2::new(cx + rx * t.cos(), cy + ry * t.sin()));
    }
    (
        vec![Polyline {
            points: pts,
            closed: true,
        }],
        min,
    )
}

/// Straight line segment between two points.
pub fn line(a: Vec2, b: Vec2) -> (Vec<Polyline>, Vec2) {
    let pl = Polyline {
        points: vec![Vec2::new(0.0, 0.0), Vec2::new(b.x - a.x, b.y - a.y)],
        closed: false,
    };
    (vec![pl], a)
}

/// Regular polygon centred at `center`, with a vertex reaching `edge`.
pub fn polygon(center: Vec2, edge: Vec2, sides: u32) -> (Vec<Polyline>, Vec2) {
    let n = sides.max(3);
    let r = center.distance(edge).max(0.01);
    let start = (edge.y - center.y).atan2(edge.x - center.x);
    let mut pts = Vec::with_capacity(n as usize);
    for i in 0..n {
        let t = start + i as f64 / n as f64 * std::f64::consts::TAU;
        pts.push(Vec2::new(r * t.cos(), r * t.sin()));
    }
    (
        vec![Polyline {
            points: pts,
            closed: true,
        }],
        center,
    )
}

/// Open polyline through the given document-space points (origin at the first point).
pub fn polyline(points: &[Vec2], closed: bool) -> Option<(Vec<Polyline>, Vec2)> {
    if points.len() < 2 {
        return None;
    }
    let origin = points[0];
    let local: Vec<Vec2> = points
        .iter()
        .map(|p| Vec2::new(p.x - origin.x, p.y - origin.y))
        .collect();
    Some((
        vec![Polyline {
            points: local,
            closed,
        }],
        origin,
    ))
}
