//! Convert a text string into vector outlines (polylines) using a system font.

use usvg::fontdb;

use crate::geometry::{flatten_cubic, flatten_quad, Polyline, Vec2};

const FLATTEN_TOL_MM: f64 = 0.05;

/// DejaVu Sans, embedded so text always works even on machines with no usable
/// system fonts (older/minimal installs where `load_system_fonts` finds nothing).
/// DejaVu is freely redistributable (Bitstream Vera + public-domain additions).
const EMBEDDED_FONT: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

/// Build (and cache) a font database with the system fonts plus the embedded one.
pub fn build_font_db() -> fontdb::Database {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    // Always load the bundled fallback so text never fails for lack of a font.
    db.load_font_data(EMBEDDED_FONT.to_vec());
    db
}

/// Font DB with ONLY the embedded font loaded (no system fonts). Mirrors the
/// situation on a machine where `load_system_fonts` finds nothing, so tests can
/// exercise the real fallback path.
pub fn build_font_db_embedded_only() -> fontdb::Database {
    let mut db = fontdb::Database::new();
    db.load_font_data(EMBEDDED_FONT.to_vec());
    db
}

/// Sorted, de-duplicated list of font family names available in `db` (the fonts
/// installed on this machine, plus the bundled DejaVu Sans). For the UI picker.
pub fn available_families(db: &fontdb::Database) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for f in db.faces() {
        if let Some((name, _)) = f.families.first() {
            if !name.is_empty() {
                set.insert(name.clone());
            }
        }
    }
    set.into_iter().collect()
}

/// Convert `text` into polylines (mm) with the automatically-chosen font.
pub fn text_to_polylines(db: &fontdb::Database, text: &str, height_mm: f64) -> Option<Vec<Polyline>> {
    text_to_polylines_family(db, text, height_mm, None)
}

/// Convert `text` into polylines (mm) with cap/em height `height_mm`, optionally
/// using the font `family` (by name) when given.
///
/// Tries, in order: the requested family (if any), the preferred system sans,
/// any DejaVu face, then every other face — because a font the DB indexed can
/// still fail to load or lack outlines. As a guaranteed last resort it parses the
/// **embedded** DejaVu Sans bytes directly (bypassing fontdb entirely), so text
/// always works even on machines with no usable system fonts.
pub fn text_to_polylines_family(
    db: &fontdb::Database,
    text: &str,
    height_mm: f64,
    family: Option<&str>,
) -> Option<Vec<Polyline>> {
    // 1) Try database faces, best candidate first.
    for id in candidate_face_ids(db, family) {
        let got = db
            .with_face_data(id, |data, index| {
                ttf_parser::Face::parse(data, index)
                    .ok()
                    .and_then(|face| render_face(&face, text, height_mm))
            })
            .flatten();
        if let Some(polys) = got {
            return Some(polys);
        }
    }
    // 2) Guaranteed fallback: parse the embedded font bytes directly.
    let face = ttf_parser::Face::parse(EMBEDDED_FONT, 0).ok()?;
    render_face(&face, text, height_mm)
}

/// Ordered list of faces to try: the requested `family` first (if any), then the
/// preferred sans-serif, then DejaVu, then the rest.
fn candidate_face_ids(db: &fontdb::Database, family: Option<&str>) -> Vec<fontdb::ID> {
    let want = family
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let mut chosen: Vec<fontdb::ID> = Vec::new();
    let mut preferred = None;
    let mut dejavu = None;
    let mut rest: Vec<fontdb::ID> = Vec::new();
    for f in db.faces() {
        let fam = f
            .families
            .first()
            .map(|(s, _)| s.to_lowercase())
            .unwrap_or_default();
        if let Some(w) = &want {
            if fam == *w || fam.contains(w.as_str()) {
                chosen.push(f.id);
                continue;
            }
        }
        if preferred.is_none() && fam.contains("sans") && !fam.contains("mono") {
            preferred = Some(f.id);
        } else if dejavu.is_none() && fam.contains("dejavu") && !fam.contains("mono") {
            dejavu = Some(f.id);
        } else {
            rest.push(f.id);
        }
    }
    let mut out = Vec::new();
    out.extend(chosen);
    out.extend(preferred);
    out.extend(dejavu);
    out.extend(rest);
    out
}

/// Render `text` with a parsed face into mm-space polylines. `None` if the face
/// has no usable metrics or produced no geometry.
fn render_face(face: &ttf_parser::Face, text: &str, height_mm: f64) -> Option<Vec<Polyline>> {
    let upem = face.units_per_em() as f64;
    if upem <= 0.0 {
        return None;
    }
    let scale = height_mm / upem;
    let ascent = face.ascender() as f64;
    let mut polys: Vec<Polyline> = Vec::new();
    let mut pen_x = 0.0f64;

    for ch in text.chars() {
        if ch == '\n' {
            pen_x = 0.0;
            continue;
        }
        let gid = match face.glyph_index(ch) {
            Some(g) => g,
            None => {
                pen_x += upem * 0.5 * scale;
                continue;
            }
        };
        let mut ol = Outline {
            polylines: Vec::new(),
            cur: Polyline::new(),
            last: Vec2::new(0.0, 0.0),
            scale,
            pen_x,
            ascent,
        };
        face.outline_glyph(gid, &mut ol);
        ol.flush();
        polys.extend(ol.polylines);
        let adv = face.glyph_hor_advance(gid).unwrap_or(upem as u16) as f64;
        pen_x += adv * scale;
    }
    if polys.is_empty() {
        None
    } else {
        Some(polys)
    }
}

/// `ttf_parser` outline sink that accumulates mm-space polylines (Y pointing down).
struct Outline {
    polylines: Vec<Polyline>,
    cur: Polyline,
    last: Vec2,
    scale: f64,
    pen_x: f64,
    ascent: f64,
}

impl Outline {
    fn map(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(
            self.pen_x + x as f64 * self.scale,
            (self.ascent - y as f64) * self.scale,
        )
    }
    fn flush(&mut self) {
        if self.cur.points.len() >= 2 {
            self.polylines
                .push(std::mem::replace(&mut self.cur, Polyline::new()));
        } else {
            self.cur.points.clear();
        }
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.last = self.map(x, y);
        self.cur = Polyline::new();
        self.cur.points.push(self.last);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.map(x, y);
        self.cur.points.push(p);
        self.last = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let c = self.map(x1, y1);
        let e = self.map(x, y);
        flatten_quad(self.last, c, e, FLATTEN_TOL_MM, &mut self.cur.points);
        self.last = e;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let c1 = self.map(x1, y1);
        let c2 = self.map(x2, y2);
        let e = self.map(x, y);
        flatten_cubic(self.last, c1, c2, e, FLATTEN_TOL_MM, &mut self.cur.points);
        self.last = e;
    }
    fn close(&mut self) {
        self.cur.closed = true;
        self.flush();
    }
}
