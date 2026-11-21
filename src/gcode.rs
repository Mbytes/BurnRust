//! G-code generation for GRBL laser controllers (vector cutting + raster engraving).

use crate::geometry::Vec2;
use crate::model::{CutMode, Document, MachineSettings, ObjectKind, Origin};

/// Result of a generation pass.
pub struct GcodeJob {
    pub lines: Vec<String>,
    pub stats: GcodeStats,
}

#[derive(Default, Clone, Copy)]
pub struct GcodeStats {
    pub lines: usize,
    pub layers_out: usize,
    pub objects_out: usize,
}

/// Map a document-space point to machine coordinates, honouring the origin
/// setting, then subtract `off` (a machine-space offset for a relative job
/// origin; pass `Vec2::new(0.0, 0.0)` for absolute output).
fn to_machine(p: Vec2, m: &MachineSettings, off: Vec2) -> Vec2 {
    let mapped = match m.origin {
        Origin::FrontLeft => Vec2::new(p.x, m.bed_h_mm - p.y),
        Origin::TopLeft => p,
    };
    Vec2::new(mapped.x - off.x, mapped.y - off.y)
}

/// Options controlling a generation pass.
#[derive(Clone, Default)]
pub struct GenOptions {
    /// When `Some`, only objects whose id is listed are emitted.
    pub only_ids: Option<Vec<u64>>,
    /// When `Some(anchor)`, output is shifted so this document-space point maps
    /// to machine (0,0) — "start from job origin" / relative coordinates.
    pub relative_anchor: Option<Vec2>,
}

fn fmt(v: f64) -> String {
    // 3 decimals, trailing zeros trimmed.
    let mut s = format!("{:.3}", v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

/// Generate a full G-code program for the document (absolute, all objects).
pub fn generate(doc: &Document) -> GcodeJob {
    generate_with(doc, &GenOptions::default())
}

/// Generate a G-code program honouring `opts` (object filter, relative origin).
pub fn generate_with(doc: &Document, opts: &GenOptions) -> GcodeJob {
    let m = &doc.machine;
    let mut out: Vec<String> = Vec::new();
    let mut stats = GcodeStats::default();

    // Machine-space offset for a relative job origin.
    let off = match opts.relative_anchor {
        Some(a) => to_machine(a, m, Vec2::new(0.0, 0.0)),
        None => Vec2::new(0.0, 0.0),
    };
    let included = |o: &crate::model::Object| match &opts.only_ids {
        Some(ids) => ids.contains(&o.id),
        None => true,
    };

    out.push("; rustburn G-code (GRBL laser)".into());
    out.push(format!(
        "; cama {}x{} mm, Smax {}, modo {}",
        fmt(m.bed_w_mm),
        fmt(m.bed_h_mm),
        fmt(m.s_max),
        m.laser_mode.code()
    ));
    out.push("G21".into()); // mm
    out.push("G90".into()); // absolute
    out.push("M5".into()); // laser off
    out.push(format!("G0 F{}", fmt(m.travel_speed)));

    for (li, layer) in doc.layers.iter().enumerate() {
        if !layer.output || !layer.visible {
            continue;
        }
        let objs: Vec<&crate::model::Object> = doc
            .objects
            .iter()
            .filter(|o| o.layer == li && included(o))
            .collect();
        if objs.is_empty() {
            continue;
        }
        let mut emitted = false;
        out.push(format!("; --- capa {}: {} ---", li + 1, layer.name));

        // Gather all vector paths of the layer (machine coords) for joint ordering.
        let mut paths: Vec<(Vec<Vec2>, bool)> = Vec::new();
        for obj in &objs {
            match (&obj.kind, layer.settings.mode) {
                (ObjectKind::Vector(v), _) => {
                    let mat = obj.matrix();
                    for pl in &v.polylines {
                        if pl.points.len() < 2 {
                            continue;
                        }
                        let mp = pl
                            .points
                            .iter()
                            .map(|p| to_machine(mat.apply(*p), m, off))
                            .collect::<Vec<_>>();
                        paths.push((mp, pl.closed));
                    }
                    emitted = true;
                    stats.objects_out += 1;
                }
                (ObjectKind::Image(_), CutMode::Image) => {
                    emit_raster(&mut out, doc, obj, layer, off);
                    emitted = true;
                    stats.objects_out += 1;
                }
                (ObjectKind::Image(_), CutMode::Line) => {
                    out.push(format!(
                        "; (imagen '{}' omitida: la capa está en modo Línea)",
                        obj.name
                    ));
                }
            }
        }
        if !paths.is_empty() {
            let ordered = if m.optimize_travel {
                optimize_order(paths)
            } else {
                paths
            };
            emit_paths(&mut out, &ordered, &layer.settings, m);
        }
        if emitted {
            stats.layers_out += 1;
        }
    }

    out.push("M5".into());
    out.push("G0 X0 Y0".into());
    stats.lines = out.len();
    GcodeJob { lines: out, stats }
}

/// Greedy nearest-neighbour ordering of paths to minimise travel; open paths may be
/// reversed to put their nearer end first. Starts from the machine origin.
fn optimize_order(mut remaining: Vec<(Vec<Vec2>, bool)>) -> Vec<(Vec<Vec2>, bool)> {
    let mut result = Vec::with_capacity(remaining.len());
    let mut cur = Vec2::new(0.0, 0.0);
    while !remaining.is_empty() {
        let mut best_i = 0;
        let mut best_d = f64::MAX;
        let mut best_rev = false;
        for (i, (pts, closed)) in remaining.iter().enumerate() {
            if pts.is_empty() {
                continue;
            }
            let ds = cur.distance(pts[0]);
            if ds < best_d {
                best_d = ds;
                best_i = i;
                best_rev = false;
            }
            if !closed {
                let de = cur.distance(*pts.last().unwrap());
                if de < best_d {
                    best_d = de;
                    best_i = i;
                    best_rev = true;
                }
            }
        }
        let (mut pts, closed) = remaining.remove(best_i);
        if best_rev {
            pts.reverse();
        }
        cur = if closed {
            pts[0]
        } else {
            *pts.last().unwrap()
        };
        result.push((pts, closed));
    }
    result
}

/// Emit cutting moves for a set of machine-space paths, repeating for all passes.
fn emit_paths(
    out: &mut Vec<String>,
    paths: &[(Vec<Vec2>, bool)],
    s: &crate::model::CutSettings,
    m: &MachineSettings,
) {
    let power = (s.power_pct / 100.0 * m.s_max).round();
    for _pass in 0..s.passes.max(1) {
        for (mp, closed) in paths {
            if mp.len() < 2 {
                continue;
            }
            out.push("M5".into());
            out.push(format!("G0 X{} Y{}", fmt(mp[0].x), fmt(mp[0].y)));
            out.push(format!("{} S{}", m.laser_mode.code(), fmt(power)));
            out.push(format!(
                "G1 X{} Y{} F{}",
                fmt(mp[1].x),
                fmt(mp[1].y),
                fmt(s.speed)
            ));
            for p in &mp[2..] {
                out.push(format!("G1 X{} Y{}", fmt(p.x), fmt(p.y)));
            }
            if *closed {
                out.push(format!("G1 X{} Y{}", fmt(mp[0].x), fmt(mp[0].y)));
            }
            out.push("M5".into());
        }
    }
}

fn emit_raster(
    out: &mut Vec<String>,
    doc: &Document,
    obj: &crate::model::Object,
    layer: &crate::model::Layer,
    off: Vec2,
) {
    let m = &doc.machine;
    let s = &layer.settings;
    let ObjectKind::Image(img) = &obj.kind else {
        return;
    };
    let interval = s.line_interval_mm.max(0.02);
    let cols = (img.width_mm / interval).round().max(1.0) as usize;
    let rows = (img.height_mm / interval).round().max(1.0) as usize;

    // Sample the (adjusted) image onto the scan grid (greyscale 0..=255).
    let proc = img.processed_gray();
    let mut grid = vec![0u8; cols * rows];
    for ry in 0..rows {
        let py = ((ry as f64 + 0.5) / rows as f64 * img.height_px as f64) as u32;
        let py = py.min(img.height_px - 1);
        for rx in 0..cols {
            let px = ((rx as f64 + 0.5) / cols as f64 * img.width_px as f64) as u32;
            let px = px.min(img.width_px - 1);
            grid[ry * cols + rx] = proc[(py * img.width_px + px) as usize];
        }
    }

    // Convert grid greys into per-cell power fractions (0.0..=1.0).
    let power_grid = crate::gcode_dither::to_power(&grid, cols, rows, s.dither);
    let s_cut = s.power_pct / 100.0 * m.s_max;

    out.push(format!("{} S0", m.laser_mode.code()));
    out.push(format!("G1 F{}", fmt(s.speed)));

    // local-space helper: cell (rx, ry) -> local mm, then to machine.
    let mat = obj.matrix();
    let local = |x_mm: f64, y_mm: f64| to_machine(mat.apply(Vec2::new(x_mm, y_mm)), m, off);

    for ry in 0..rows {
        let y_mm = (ry as f64 + 0.5) * interval;
        let ltr = ry % 2 == 0; // bidirectional

        // Build run-length segments of constant power across the row.
        let mut runs: Vec<(usize, usize, f64)> = Vec::new(); // (start_col, end_col_excl, frac)
        let mut c = 0;
        while c < cols {
            let frac = power_grid[ry * cols + c];
            let mut e = c + 1;
            while e < cols && (power_grid[ry * cols + e] - frac).abs() < f64::EPSILON {
                e += 1;
            }
            runs.push((c, e, frac));
            c = e;
        }

        let order: Vec<(usize, usize, f64)> = if ltr {
            runs
        } else {
            runs.into_iter().rev().collect()
        };

        // Overscan start point (laser off), then travel to first cell edge.
        let x_first_edge = if ltr { 0.0 } else { cols as f64 * interval };
        let over = if ltr { -s.overscan_mm } else { s.overscan_mm };
        let start = local(x_first_edge + over, y_mm);
        out.push("S0".into());
        out.push(format!("G0 X{} Y{}", fmt(start.x), fmt(start.y)));
        // Return to the row edge at engraving speed with laser off.
        let edge = local(x_first_edge, y_mm);
        out.push(format!("G1 X{} Y{} S0", fmt(edge.x), fmt(edge.y)));

        for (cs, ce, frac) in order {
            // X end of this run in local mm.
            let x_end_mm = if ltr {
                ce as f64 * interval
            } else {
                cs as f64 * interval
            };
            let p = local(x_end_mm, y_mm);
            let sval = (frac * s_cut).round();
            out.push(format!("G1 X{} Y{} S{}", fmt(p.x), fmt(p.y), fmt(sval)));
        }

        // Overscan exit, laser off.
        let x_last_edge = if ltr { cols as f64 * interval } else { 0.0 };
        let over2 = if ltr { s.overscan_mm } else { -s.overscan_mm };
        let exit = local(x_last_edge + over2, y_mm);
        out.push(format!("G1 X{} Y{} S0", fmt(exit.x), fmt(exit.y)));
    }
    out.push("M5".into());
}

/// Parameters for a power × speed material test grid.
#[derive(Clone)]
pub struct MaterialTest {
    pub cols: u32,
    pub rows: u32,
    pub min_power: f64,
    pub max_power: f64,
    pub min_speed: f64,
    pub max_speed: f64,
    pub cell_mm: f64,
    pub gap_mm: f64,
    pub interval_mm: f64,
    pub origin_x: f64,
    pub origin_y: f64,
}

impl Default for MaterialTest {
    fn default() -> Self {
        Self {
            cols: 5,
            rows: 5,
            min_power: 10.0,
            max_power: 100.0,
            min_speed: 500.0,
            max_speed: 6000.0,
            cell_mm: 10.0,
            gap_mm: 4.0,
            interval_mm: 0.15,
            origin_x: 20.0,
            origin_y: 20.0,
        }
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Generate a material-test program: columns vary power, rows vary speed, each cell
/// a filled square engraved at that setting.
pub fn material_test(m: &MachineSettings, t: &MaterialTest) -> Vec<String> {
    let mut out = vec![
        "; rustburn - test de material (columnas=potencia, filas=velocidad)".to_string(),
        "G21".into(),
        "G90".into(),
        "M5".into(),
        format!("G0 F{}", fmt(m.travel_speed)),
    ];
    let cols = t.cols.max(1);
    let rows = t.rows.max(1);
    let interval = t.interval_mm.max(0.05);
    let pitch = t.cell_mm + t.gap_mm;

    for r in 0..rows {
        let sv = if rows == 1 {
            t.min_speed
        } else {
            lerp(t.min_speed, t.max_speed, r as f64 / (rows - 1) as f64)
        };
        for c in 0..cols {
            let pv = if cols == 1 {
                t.min_power
            } else {
                lerp(t.min_power, t.max_power, c as f64 / (cols - 1) as f64)
            };
            let s_val = (pv / 100.0 * m.s_max).round();
            let x0 = t.origin_x + c as f64 * pitch;
            let y0 = t.origin_y + r as f64 * pitch;
            out.push(format!("; celda c{} f{}: P{:.0}% V{:.0}", c, r, pv, sv));

            let n_lines = (t.cell_mm / interval).round().max(1.0) as u32;
            out.push(format!("{} S0", m.laser_mode.code()));
            for i in 0..n_lines {
                let y = y0 + i as f64 * interval;
                let (xa, xb) = if i % 2 == 0 {
                    (x0, x0 + t.cell_mm)
                } else {
                    (x0 + t.cell_mm, x0)
                };
                let pa = to_machine(Vec2::new(xa, y), m, Vec2::new(0.0, 0.0));
                let pb = to_machine(Vec2::new(xb, y), m, Vec2::new(0.0, 0.0));
                out.push(format!("G0 X{} Y{} S0", fmt(pa.x), fmt(pa.y)));
                out.push(format!(
                    "G1 X{} Y{} F{} S{}",
                    fmt(pb.x),
                    fmt(pb.y),
                    fmt(sv),
                    fmt(s_val)
                ));
            }
            out.push("M5".into());
        }
    }
    out.push("M5".into());
    out.push("G0 X0 Y0".into());
    out
}

/// One drawn toolpath segment, in document coordinates.
pub struct ToolSeg {
    pub from: Vec2,
    pub to: Vec2,
    pub rapid: bool,
}

/// Parsed toolpath plus time/length estimates.
pub struct Toolpath {
    pub segs: Vec<ToolSeg>,
    pub seconds: f64,
    pub cut_mm: f64,
    pub travel_mm: f64,
}

/// Parse a generated G-code program back into drawable segments and estimate run time.
pub fn analyze(lines: &[String], m: &MachineSettings) -> Toolpath {
    let to_doc = |mx: f64, my: f64| match m.origin {
        Origin::FrontLeft => Vec2::new(mx, m.bed_h_mm - my),
        Origin::TopLeft => Vec2::new(mx, my),
    };
    let (mut x, mut y) = (0.0f64, 0.0f64);
    let mut feed = m.travel_speed.max(1.0);
    let mut rapid = true;
    let mut segs = Vec::new();
    let (mut seconds, mut cut, mut travel) = (0.0f64, 0.0f64, 0.0f64);

    for raw in lines {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (mut nx, mut ny, mut moved) = (x, y, false);
        for tok in line.split_whitespace() {
            if tok.len() < 2 {
                continue;
            }
            let (c, rest) = tok.split_at(1);
            match c.to_ascii_uppercase().as_str() {
                "G" => match rest {
                    "0" | "00" => rapid = true,
                    "1" | "01" => rapid = false,
                    _ => {}
                },
                "X" => {
                    if let Ok(v) = rest.parse::<f64>() {
                        nx = v;
                        moved = true;
                    }
                }
                "Y" => {
                    if let Ok(v) = rest.parse::<f64>() {
                        ny = v;
                        moved = true;
                    }
                }
                "F" => {
                    if let Ok(v) = rest.parse::<f64>() {
                        if v > 0.0 {
                            feed = v;
                        }
                    }
                }
                _ => {}
            }
        }
        if moved {
            let d = ((nx - x).powi(2) + (ny - y).powi(2)).sqrt();
            let speed = if rapid { m.travel_speed.max(1.0) } else { feed.max(1.0) };
            seconds += d / speed * 60.0;
            if rapid {
                travel += d;
            } else {
                cut += d;
            }
            segs.push(ToolSeg {
                from: to_doc(x, y),
                to: to_doc(nx, ny),
                rapid,
            });
            x = nx;
            y = ny;
        }
    }
    Toolpath {
        segs,
        seconds,
        cut_mm: cut,
        travel_mm: travel,
    }
}

/// Build a framing program: trace the design's bounding box at low power.
pub fn frame(doc: &Document, power_pct: f64, speed: f64) -> Option<Vec<String>> {
    let bb = doc.bbox()?;
    let m = &doc.machine;
    let corners = [
        Vec2::new(bb.min.x, bb.min.y),
        Vec2::new(bb.max.x, bb.min.y),
        Vec2::new(bb.max.x, bb.max.y),
        Vec2::new(bb.min.x, bb.max.y),
        Vec2::new(bb.min.x, bb.min.y),
    ]
    .map(|p| to_machine(p, m, Vec2::new(0.0, 0.0)));

    let power = (power_pct / 100.0 * m.s_max).round();
    let mut out = vec![
        "G21".to_string(),
        "G90".to_string(),
        "M5".to_string(),
        format!("G0 X{} Y{}", fmt(corners[0].x), fmt(corners[0].y)),
        format!("{} S{}", m.laser_mode.code(), fmt(power)),
    ];
    for c in &corners[1..] {
        out.push(format!("G1 X{} Y{} F{}", fmt(c.x), fmt(c.y), fmt(speed)));
    }
    out.push("M5".into());
    Some(out)
}
