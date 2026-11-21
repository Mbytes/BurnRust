//! Perspective (homography) math for camera / photo alignment onto the laser bed.

use crate::geometry::Vec2;

/// A 3×3 homography matrix (row-major).
#[derive(Clone, Copy, Debug)]
pub struct Homography(pub [f64; 9]);

impl Homography {
    /// Project a point `p` through the homography.
    pub fn apply(&self, p: Vec2) -> Vec2 {
        let h = &self.0;
        let x = h[0] * p.x + h[1] * p.y + h[2];
        let y = h[3] * p.x + h[4] * p.y + h[5];
        let w = h[6] * p.x + h[7] * p.y + h[8];
        if w.abs() < 1e-12 {
            Vec2::new(x, y)
        } else {
            Vec2::new(x / w, y / w)
        }
    }

    /// Solve the homography mapping 4 `src` points to 4 `dst` points (DLT, h8 = 1).
    pub fn from_points(src: [Vec2; 4], dst: [Vec2; 4]) -> Option<Homography> {
        // Build the 8×8 linear system A·h = b for h0..h7.
        let mut a = [[0.0f64; 8]; 8];
        let mut b = [0.0f64; 8];
        for i in 0..4 {
            let (sx, sy) = (src[i].x, src[i].y);
            let (dx, dy) = (dst[i].x, dst[i].y);
            let r0 = 2 * i;
            let r1 = 2 * i + 1;
            a[r0] = [sx, sy, 1.0, 0.0, 0.0, 0.0, -sx * dx, -sy * dx];
            b[r0] = dx;
            a[r1] = [0.0, 0.0, 0.0, sx, sy, 1.0, -sx * dy, -sy * dy];
            b[r1] = dy;
        }
        let h = solve8(&mut a, &mut b)?;
        Some(Homography([
            h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.0,
        ]))
    }
}

/// Gaussian elimination with partial pivoting for an 8×8 system.
fn solve8(a: &mut [[f64; 8]; 8], b: &mut [f64; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        // pivot
        let mut piv = col;
        let mut best = a[col][col].abs();
        for r in (col + 1)..8 {
            if a[r][col].abs() > best {
                best = a[r][col].abs();
                piv = r;
            }
        }
        if best < 1e-12 {
            return None;
        }
        a.swap(col, piv);
        b.swap(col, piv);
        // eliminate
        for r in 0..8 {
            if r == col {
                continue;
            }
            let f = a[r][col] / a[col][col];
            if f == 0.0 {
                continue;
            }
            for c in col..8 {
                a[r][c] -= f * a[col][c];
            }
            b[r] -= f * b[col];
        }
    }
    let mut x = [0.0f64; 8];
    for i in 0..8 {
        x[i] = b[i] / a[i][i];
    }
    Some(x)
}

/// Warp a source RGBA image into a bed-aligned RGBA image.
///
/// `h_bed_to_src` maps bed-mm coordinates to source-image pixels. The output image
/// of `out_w × out_h` covers the bed `0..bed_w × 0..bed_h`. Pixels outside the source
/// are left transparent. Bilinear sampling.
pub fn warp_to_bed(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    h_bed_to_src: &Homography,
    bed_w: f64,
    bed_h: f64,
    out_w: usize,
    out_h: usize,
) -> Vec<u8> {
    let mut out = vec![0u8; out_w * out_h * 4];
    if src_w == 0 || src_h == 0 {
        return out;
    }
    for oy in 0..out_h {
        let by = (oy as f64 + 0.5) / out_h as f64 * bed_h;
        for ox in 0..out_w {
            let bx = (ox as f64 + 0.5) / out_w as f64 * bed_w;
            let sp = h_bed_to_src.apply(Vec2::new(bx, by));
            let fx = sp.x - 0.5;
            let fy = sp.y - 0.5;
            if fx < 0.0 || fy < 0.0 || fx >= (src_w - 1) as f64 || fy >= (src_h - 1) as f64 {
                continue;
            }
            let x0 = fx.floor() as usize;
            let y0 = fy.floor() as usize;
            let tx = fx - x0 as f64;
            let ty = fy - y0 as f64;
            let o = (oy * out_w + ox) * 4;
            for ch in 0..4 {
                let p00 = src[(y0 * src_w + x0) * 4 + ch] as f64;
                let p10 = src[(y0 * src_w + x0 + 1) * 4 + ch] as f64;
                let p01 = src[((y0 + 1) * src_w + x0) * 4 + ch] as f64;
                let p11 = src[((y0 + 1) * src_w + x0 + 1) * 4 + ch] as f64;
                let top = p00 + (p10 - p00) * tx;
                let bot = p01 + (p11 - p01) * tx;
                out[o + ch] = (top + (bot - top) * ty).round().clamp(0.0, 255.0) as u8;
            }
            // Force opaque where we sampled inside the source.
            out[o + 3] = 255;
        }
    }
    out
}
