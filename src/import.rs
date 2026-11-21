//! Import of SVG vectors and raster images into the document model.

use std::path::Path;

use anyhow::{Context, Result};
use usvg::tiny_skia_path::PathSegment;

use crate::geometry::{flatten_cubic, flatten_quad, Polyline, Vec2};
use crate::model::{ImageData, ObjectKind, VectorData};

/// SVG user units are px; convert to millimetres assuming this DPI.
const SVG_DPI: f64 = 96.0;
const PX_TO_MM: f64 = 25.4 / SVG_DPI;
/// Curve flattening tolerance, in mm.
const FLATTEN_TOL_MM: f64 = 0.05;

/// Load an SVG file and flatten every path into polylines (in mm, document space).
pub fn load_svg(path: &Path) -> Result<ObjectKind> {
    let data = std::fs::read(path).with_context(|| format!("leyendo {}", path.display()))?;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(&data, &opt).context("parseando SVG")?;

    let mut polylines = Vec::new();
    collect_group(tree.root(), &mut polylines);

    if polylines.is_empty() {
        anyhow::bail!("el SVG no contiene trazos vectoriales (¿solo texto o imágenes?)");
    }
    Ok(ObjectKind::Vector(VectorData { polylines }))
}

fn collect_group(group: &usvg::Group, out: &mut Vec<Polyline>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect_group(g, out),
            usvg::Node::Path(p) => collect_path(p, out),
            // Embedded raster images / text inside the SVG are skipped for now.
            usvg::Node::Image(_) | usvg::Node::Text(_) => {}
        }
    }
}

fn collect_path(path: &usvg::Path, out: &mut Vec<Polyline>) {
    let t = path.abs_transform();
    // Map a tiny_skia point (px, local) to document mm coordinates.
    let map = |px: f32, py: f32| -> Vec2 {
        let x = t.sx * px + t.kx * py + t.tx;
        let y = t.ky * px + t.sy * py + t.ty;
        Vec2::new(x as f64 * PX_TO_MM, y as f64 * PX_TO_MM)
    };

    let mut cur = Polyline::new();
    let mut last = Vec2::new(0.0, 0.0);

    for seg in path.data().segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                flush(&mut cur, out);
                last = map(p.x, p.y);
                cur = Polyline::new();
                cur.points.push(last);
            }
            PathSegment::LineTo(p) => {
                last = map(p.x, p.y);
                cur.points.push(last);
            }
            PathSegment::QuadTo(p1, p) => {
                let c1 = map(p1.x, p1.y);
                let end = map(p.x, p.y);
                flatten_quad(last, c1, end, FLATTEN_TOL_MM, &mut cur.points);
                last = end;
            }
            PathSegment::CubicTo(p1, p2, p) => {
                let c1 = map(p1.x, p1.y);
                let c2 = map(p2.x, p2.y);
                let end = map(p.x, p.y);
                flatten_cubic(last, c1, c2, end, FLATTEN_TOL_MM, &mut cur.points);
                last = end;
            }
            PathSegment::Close => {
                cur.closed = true;
                flush(&mut cur, out);
                cur = Polyline::new();
            }
        }
    }
    flush(&mut cur, out);
}

fn flush(cur: &mut Polyline, out: &mut Vec<Polyline>) {
    if cur.points.len() >= 2 {
        out.push(std::mem::replace(cur, Polyline::new()));
    } else {
        cur.points.clear();
    }
}

/// Load a raster image as a greyscale [`ImageData`], sized from its pixel dimensions.
pub fn load_image(path: &Path) -> Result<ObjectKind> {
    let img = image::open(path).with_context(|| format!("abriendo imagen {}", path.display()))?;
    let luma = img.to_luma8();
    let (w, h) = (luma.width(), luma.height());
    let gray = luma.into_raw();
    let width_mm = w as f64 * PX_TO_MM;
    let height_mm = h as f64 * PX_TO_MM;
    Ok(ObjectKind::Image(ImageData {
        width_px: w,
        height_px: h,
        gray,
        width_mm,
        height_mm,
        adjust: Default::default(),
    }))
}
