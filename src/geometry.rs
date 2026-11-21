//! Basic 2D geometry: points, bounding boxes, affine transforms and curve flattening.

use serde::{Deserialize, Serialize};

/// A 2D point / vector in millimetres (document space).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn distance(self, o: Vec2) -> f64 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}

/// A polyline: an ordered list of points, optionally closed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Polyline {
    pub points: Vec<Vec2>,
    pub closed: bool,
}

impl Polyline {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            closed: false,
        }
    }
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy)]
pub struct BBox {
    pub min: Vec2,
    pub max: Vec2,
}

impl BBox {
    pub fn empty() -> Option<BBox> {
        None
    }

    pub fn from_points<'a>(pts: impl IntoIterator<Item = &'a Vec2>) -> Option<BBox> {
        let mut it = pts.into_iter();
        let first = it.next()?;
        let mut bb = BBox {
            min: *first,
            max: *first,
        };
        for p in it {
            bb.expand(*p);
        }
        Some(bb)
    }

    pub fn expand(&mut self, p: Vec2) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }

    pub fn union(a: Option<BBox>, b: Option<BBox>) -> Option<BBox> {
        match (a, b) {
            (None, x) | (x, None) => x,
            (Some(mut a), Some(b)) => {
                a.expand(b.min);
                a.expand(b.max);
                Some(a)
            }
        }
    }

    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }
}

/// 2D affine transform stored as an SVG-style matrix:
/// `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Affine {
    fn default() -> Self {
        Self::identity()
    }
}

#[allow(dead_code)]
impl Affine {
    pub const fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    pub fn translate(tx: f64, ty: f64) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: tx,
            f: ty,
        }
    }

    /// Compose: `self * other` (other applied first).
    pub fn then(self, m: Affine) -> Affine {
        Affine {
            a: m.a * self.a + m.b * self.c,
            b: m.a * self.b + m.b * self.d,
            c: m.c * self.a + m.d * self.c,
            d: m.c * self.b + m.d * self.d,
            e: m.e * self.a + m.f * self.c + self.e,
            f: m.e * self.b + m.f * self.d + self.f,
        }
    }

    pub fn apply(&self, p: Vec2) -> Vec2 {
        Vec2 {
            x: self.a * p.x + self.c * p.y + self.e,
            y: self.b * p.x + self.d * p.y + self.f,
        }
    }

    /// Inverse transform, if invertible.
    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return None;
        }
        let ia = self.d / det;
        let ib = -self.b / det;
        let ic = -self.c / det;
        let id = self.a / det;
        Some(Affine {
            a: ia,
            b: ib,
            c: ic,
            d: id,
            e: -(ia * self.e + ic * self.f),
            f: -(ib * self.e + id * self.f),
        })
    }

    /// Build translate * rotate(deg) * scale, around the origin.
    pub fn trs(tx: f64, ty: f64, rot_deg: f64, sx: f64, sy: f64) -> Affine {
        let r = rot_deg.to_radians();
        let (sin, cos) = r.sin_cos();
        // scale then rotate then translate
        Affine {
            a: cos * sx,
            b: sin * sx,
            c: -sin * sy,
            d: cos * sy,
            e: tx,
            f: ty,
        }
    }
}

/// Flatten a quadratic Bézier into line points (excluding the start point).
pub fn flatten_quad(p0: Vec2, p1: Vec2, p2: Vec2, tol: f64, out: &mut Vec<Vec2>) {
    let steps = quad_steps(p0, p1, p2, tol);
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let mt = 1.0 - t;
        let x = mt * mt * p0.x + 2.0 * mt * t * p1.x + t * t * p2.x;
        let y = mt * mt * p0.y + 2.0 * mt * t * p1.y + t * t * p2.y;
        out.push(Vec2::new(x, y));
    }
}

/// Flatten a cubic Bézier into line points (excluding the start point).
pub fn flatten_cubic(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, tol: f64, out: &mut Vec<Vec2>) {
    let steps = cubic_steps(p0, p1, p2, p3, tol);
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let mt = 1.0 - t;
        let x = mt * mt * mt * p0.x
            + 3.0 * mt * mt * t * p1.x
            + 3.0 * mt * t * t * p2.x
            + t * t * t * p3.x;
        let y = mt * mt * mt * p0.y
            + 3.0 * mt * mt * t * p1.y
            + 3.0 * mt * t * t * p2.y
            + t * t * t * p3.y;
        out.push(Vec2::new(x, y));
    }
}

fn quad_steps(p0: Vec2, p1: Vec2, p2: Vec2, tol: f64) -> usize {
    let dev = dist_point_line(p1, p0, p2);
    steps_from_dev(dev, tol)
}

fn cubic_steps(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, tol: f64) -> usize {
    let dev = dist_point_line(p1, p0, p3).max(dist_point_line(p2, p0, p3));
    steps_from_dev(dev, tol)
}

fn steps_from_dev(dev: f64, tol: f64) -> usize {
    if dev <= tol || tol <= 0.0 {
        return 1;
    }
    let n = (dev / tol).sqrt().ceil() as usize * 4;
    n.clamp(2, 256)
}

fn dist_point_line(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 {
        return p.distance(a);
    }
    ((p.x - a.x) * dy - (p.y - a.y) * dx).abs() / len
}
