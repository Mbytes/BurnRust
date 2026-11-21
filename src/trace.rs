//! Bitmap tracing: convert a greyscale raster into vector outlines.
//!
//! Uses marching squares with mid-edge crossings (no interpolation) so every
//! contour vertex lands on an exact half-pixel grid point; segments then stitch
//! into closed loops by exact endpoint matching. The blocky result is smoothed
//! with the Douglas–Peucker simplifier. Output is in local millimetres, sized to
//! the image's physical footprint.

use std::collections::HashMap;

use crate::geometry::{Polyline, Vec2};

type P = (i32, i32);

/// Trace `gray` (row-major, `w×h`, 0 = black) at `threshold`: pixels darker than
/// the threshold are "on" (the ink). `invert` swaps on/off. Returns closed
/// polylines in local mm, mapped onto `width_mm × height_mm`. `simplify_mm` is the
/// Douglas–Peucker tolerance (0 keeps the staircase).
pub fn trace_bitmap(
    gray: &[u8],
    w: usize,
    h: usize,
    threshold: u8,
    invert: bool,
    width_mm: f64,
    height_mm: f64,
    simplify_mm: f64,
) -> Vec<Polyline> {
    if w == 0 || h == 0 || gray.len() < w * h {
        return Vec::new();
    }
    let on = |x: i32, y: i32| -> bool {
        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
            return false;
        }
        let dark = gray[y as usize * w + x as usize] < threshold;
        dark ^ invert
    };

    // Undirected contour graph: each mid-edge point -> the points it connects to.
    let mut adj: HashMap<P, Vec<P>> = HashMap::new();
    let mut add_edge = |a: P, b: P| {
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    };

    for cy in 0..=(h as i32) {
        for cx in 0..=(w as i32) {
            let tl = on(cx - 1, cy - 1);
            let tr = on(cx, cy - 1);
            let br = on(cx, cy);
            let bl = on(cx - 1, cy);
            let v = (tl as u8) | (tr as u8) << 1 | (br as u8) << 2 | (bl as u8) << 3;
            // Mid-edge crossing points in doubled sample coordinates (integers).
            let t = (2 * cx - 1, 2 * cy - 2);
            let b = (2 * cx - 1, 2 * cy);
            let l = (2 * cx - 2, 2 * cy - 1);
            let r = (2 * cx, 2 * cy - 1);
            match v {
                1 | 14 => add_edge(l, t),
                2 | 13 => add_edge(t, r),
                3 | 12 => add_edge(l, r),
                4 | 11 => add_edge(r, b),
                6 | 9 => add_edge(t, b),
                7 | 8 => add_edge(b, l),
                5 => {
                    add_edge(l, t);
                    add_edge(r, b);
                }
                10 => {
                    add_edge(t, r);
                    add_edge(b, l);
                }
                _ => {}
            }
        }
    }

    // Walk the graph into closed loops, consuming edges as we go.
    let mut loops: Vec<Vec<P>> = Vec::new();
    loop {
        let start = match adj.iter().find(|(_, v)| !v.is_empty()) {
            Some((&k, _)) => k,
            None => break,
        };
        let mut path = vec![start];
        let mut cur = start;
        loop {
            let next = match adj.get_mut(&cur).and_then(|v| v.pop()) {
                Some(n) => n,
                None => break,
            };
            // Remove the reverse half-edge so we never walk it backwards.
            if let Some(nv) = adj.get_mut(&next) {
                if let Some(pos) = nv.iter().position(|p| *p == cur) {
                    nv.swap_remove(pos);
                }
            }
            cur = next;
            if cur == start {
                break;
            }
            path.push(cur);
        }
        if path.len() >= 3 {
            loops.push(path);
        }
    }

    // Map doubled sample coords to local mm (pixel centre convention), simplify.
    let to_mm = |p: P| -> Vec2 {
        let sx = p.0 as f64 / 2.0;
        let sy = p.1 as f64 / 2.0;
        Vec2::new(
            (sx + 0.5) / w as f64 * width_mm,
            (sy + 0.5) / h as f64 * height_mm,
        )
    };

    loops
        .into_iter()
        .filter_map(|lp| {
            let pts: Vec<Vec2> = lp.into_iter().map(to_mm).collect();
            let simp = if simplify_mm > 0.0 {
                simplify_closed(&pts, simplify_mm)
            } else {
                pts
            };
            if simp.len() >= 3 {
                Some(Polyline {
                    points: simp,
                    closed: true,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Douglas–Peucker simplification of a closed ring: anchor on the two farthest
/// points, simplify each half, recombine.
fn simplify_closed(points: &[Vec2], tol: f64) -> Vec<Vec2> {
    let n = points.len();
    if n < 4 {
        return points.to_vec();
    }
    // Farthest point from points[0] gives a stable split for a closed ring.
    let mut far = 1;
    let mut far_d = 0.0;
    for (i, p) in points.iter().enumerate() {
        let d = points[0].distance(*p);
        if d > far_d {
            far_d = d;
            far = i;
        }
    }
    let first: Vec<Vec2> = points[..=far].to_vec();
    let mut second: Vec<Vec2> = points[far..].to_vec();
    second.push(points[0]);

    let a = dp(&first, tol);
    let b = dp(&second, tol);
    // Recombine, dropping the shared join vertices.
    let mut out = a;
    out.pop();
    if b.len() > 1 {
        out.extend_from_slice(&b[1..b.len() - 1]);
    }
    out
}

/// Classic iterative Douglas–Peucker on an open polyline (keeps both ends).
fn dp(points: &[Vec2], tol: f64) -> Vec<Vec2> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let mut max_d = 0.0;
        let mut idx = a;
        for i in (a + 1)..b {
            let d = dist_point_line(points[i], points[a], points[b]);
            if d > max_d {
                max_d = d;
                idx = i;
            }
        }
        if max_d > tol {
            keep[idx] = true;
            stack.push((a, idx));
            stack.push((idx, b));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| if k { Some(*p) } else { None })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_a_filled_square() {
        // 10x10 image with a 6x6 dark block in the middle.
        let (w, h) = (10usize, 10usize);
        let mut g = vec![255u8; w * h];
        for y in 2..8 {
            for x in 2..8 {
                g[y * w + x] = 0;
            }
        }
        let pls = trace_bitmap(&g, w, h, 128, false, 10.0, 10.0, 0.0);
        assert!(!pls.is_empty(), "no se trazó ningún contorno");
        assert!(pls[0].closed);
    }
}
