//! Convert a greyscale scan grid into per-cell laser power fractions (0.0 = off, 1.0 = full).

use crate::model::Dither;

/// `grid` is row-major greyscale (0 = black .. 255 = white) of size `cols*rows`.
/// Returns a power fraction per cell in `0.0..=1.0` (darker pixels burn more).
pub fn to_power(grid: &[u8], cols: usize, rows: usize, dither: Dither) -> Vec<f64> {
    match dither {
        Dither::Grayscale => grid
            .iter()
            .map(|&g| (255.0 - g as f64) / 255.0)
            .collect(),
        Dither::Threshold => grid
            .iter()
            .map(|&g| if g < 128 { 1.0 } else { 0.0 })
            .collect(),
        Dither::FloydSteinberg => diffuse(grid, cols, rows, &FLOYD, 16.0),
        Dither::Atkinson => diffuse(grid, cols, rows, &ATKINSON, 8.0),
        Dither::Jarvis => diffuse(grid, cols, rows, &JARVIS, 48.0),
        Dither::Ordered => ordered_bayer(grid, cols, rows),
    }
}

/// Error-diffusion kernel: (dx, dy, weight). dy >= 0, and for dy == 0, dx > 0.
type Kernel = [(isize, isize, f64)];

const FLOYD: [(isize, isize, f64); 4] = [
    (1, 0, 7.0),
    (-1, 1, 3.0),
    (0, 1, 5.0),
    (1, 1, 1.0),
];

const ATKINSON: [(isize, isize, f64); 6] = [
    (1, 0, 1.0),
    (2, 0, 1.0),
    (-1, 1, 1.0),
    (0, 1, 1.0),
    (1, 1, 1.0),
    (0, 2, 1.0),
];

const JARVIS: [(isize, isize, f64); 12] = [
    (1, 0, 7.0),
    (2, 0, 5.0),
    (-2, 1, 3.0),
    (-1, 1, 5.0),
    (0, 1, 7.0),
    (1, 1, 5.0),
    (2, 1, 3.0),
    (-2, 2, 1.0),
    (-1, 2, 3.0),
    (0, 2, 5.0),
    (1, 2, 3.0),
    (2, 2, 1.0),
];

/// Generic error diffusion to black/white using the given kernel and divisor.
fn diffuse(grid: &[u8], cols: usize, rows: usize, kernel: &Kernel, divisor: f64) -> Vec<f64> {
    let mut buf: Vec<f64> = grid.iter().map(|&g| (255.0 - g as f64) / 255.0).collect();
    let mut out = vec![0.0f64; cols * rows];
    let idx = |x: usize, y: usize| y * cols + x;
    for y in 0..rows {
        for x in 0..cols {
            let old = buf[idx(x, y)];
            let new = if old >= 0.5 { 1.0 } else { 0.0 };
            out[idx(x, y)] = new;
            let err = old - new;
            for &(dx, dy, w) in kernel {
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && (nx as usize) < cols && ny >= 0 && (ny as usize) < rows {
                    buf[idx(nx as usize, ny as usize)] += err * w / divisor;
                }
            }
        }
    }
    out
}

/// Ordered dithering with a normalized 4×4 Bayer threshold matrix.
fn ordered_bayer(grid: &[u8], cols: usize, rows: usize) -> Vec<f64> {
    const BAYER: [[f64; 4]; 4] = [
        [0.0, 8.0, 2.0, 10.0],
        [12.0, 4.0, 14.0, 6.0],
        [3.0, 11.0, 1.0, 9.0],
        [15.0, 7.0, 13.0, 5.0],
    ];
    let mut out = vec![0.0f64; cols * rows];
    for y in 0..rows {
        for x in 0..cols {
            let dark = (255.0 - grid[y * cols + x] as f64) / 255.0;
            let t = (BAYER[y % 4][x % 4] + 0.5) / 16.0;
            out[y * cols + x] = if dark > t { 1.0 } else { 0.0 };
        }
    }
    out
}

