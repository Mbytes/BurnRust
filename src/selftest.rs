//! Headless self-test: exercises the model + G-code generator without a display.
//! Run with `cargo run -- --selftest`.

use crate::geometry::{Polyline, Vec2};
use crate::model::{CutMode, Document, ImageData, Layer, Object, ObjectKind, VectorData};

/// Build a small demo document (a rectangle to cut + a gradient image to engrave).
pub fn demo_document() -> Document {
    let mut doc = Document::default();

    // Layer 0: Line (vector cut). Layer 1: Image (raster engrave).
    doc.layers = vec![
        Layer::new("Corte", [200, 30, 30]),
        Layer::new("Grabado", [30, 90, 220]),
    ];
    doc.layers[0].settings.mode = CutMode::Line;
    doc.layers[0].settings.power_pct = 80.0;
    doc.layers[0].settings.speed = 1200.0;
    doc.layers[1].settings.mode = CutMode::Image;
    doc.layers[1].settings.power_pct = 60.0;
    doc.layers[1].settings.speed = 3000.0;
    doc.layers[1].settings.line_interval_mm = 0.2;

    // A 40x30 mm rectangle on the cut layer.
    let rect = Polyline {
        points: vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(40.0, 0.0),
            Vec2::new(40.0, 30.0),
            Vec2::new(0.0, 30.0),
        ],
        closed: true,
    };
    doc.add_object(Object::new(
        0,
        "rect".into(),
        0,
        ObjectKind::Vector(VectorData {
            polylines: vec![rect],
        }),
        Vec2::new(20.0, 20.0),
    ));

    // A tiny gradient image (8x6 px) on the engrave layer.
    let (w, h) = (8u32, 6u32);
    let mut gray = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            gray.push(((x * 255) / (w - 1)) as u8);
            let _ = y;
        }
    }
    doc.add_object(Object::new(
        0,
        "img".into(),
        1,
        ObjectKind::Image(ImageData {
            width_px: w,
            height_px: h,
            gray,
            width_mm: 16.0,
            height_mm: 12.0,
            adjust: Default::default(),
        }),
        Vec2::new(100.0, 60.0),
    ));
    doc
}

pub fn run() {
    let doc = demo_document();
    let job = crate::gcode::generate(&doc);
    println!("=== rustburn self-test ===");
    println!(
        "Documento: {} capas, {} objetos, cama {}x{} mm",
        doc.layers.len(),
        doc.objects.len(),
        doc.machine.bed_w_mm,
        doc.machine.bed_h_mm
    );
    println!(
        "G-code: {} líneas · {} capas con salida · {} objetos",
        job.stats.lines, job.stats.layers_out, job.stats.objects_out
    );

    // Basic assertions on content.
    let text = job.lines.join("\n");
    assert!(text.contains("M4 S800"), "falta la potencia de corte M4 S800");
    assert!(text.contains("G1 X"), "faltan movimientos de corte G1");
    let scan_moves = job.lines.iter().filter(|l| l.contains(" S") && l.starts_with("G1 X")).count();
    assert!(scan_moves > 0, "no se generaron líneas de grabado raster");

    println!("\n--- primeras 16 líneas ---");
    for l in job.lines.iter().take(16) {
        println!("{l}");
    }

    // Framing check.
    if let Some(frame) = crate::gcode::frame(&doc, 1.0, 3000.0) {
        println!("\nEncuadre: {} líneas", frame.len());
    }

    // Toolpath analysis + time estimate.
    let tp = crate::gcode::analyze(&job.lines, &doc.machine);
    println!(
        "Recorrido: {} segmentos · corte {:.1} mm · viaje {:.1} mm · ~{:.1} s",
        tp.segs.len(),
        tp.cut_mm,
        tp.travel_mm,
        tp.seconds
    );
    assert!(tp.seconds > 0.0 && !tp.segs.is_empty(), "análisis de recorrido vacío");

    // Round-trip the project through JSON.
    let json = serde_json::to_string(&doc).expect("serialize");
    let back: Document = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.objects.len(), doc.objects.len());
    println!("\nProyecto JSON: round-trip OK ({} bytes)", json.len());

    // Shapes.
    let (rect_pl, _) = crate::shapes::rect(Vec2::new(0.0, 0.0), Vec2::new(30.0, 20.0));
    let (ell_pl, _) = crate::shapes::ellipse(Vec2::new(0.0, 0.0), Vec2::new(40.0, 20.0));
    let (poly_pl, _) = crate::shapes::polygon(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), 6);
    assert_eq!(rect_pl[0].points.len(), 4);
    assert!(ell_pl[0].points.len() > 16 && poly_pl[0].points.len() == 6);
    println!(
        "Formas: rect {} pts · elipse {} pts · hexágono {} pts",
        rect_pl[0].points.len(),
        ell_pl[0].points.len(),
        poly_pl[0].points.len()
    );

    // Text to outlines (uses system fonts).
    let db = crate::text::build_font_db();
    match crate::text::text_to_polylines(&db, "Rust", 20.0) {
        Some(pls) => {
            let pts: usize = pls.iter().map(|p| p.points.len()).sum();
            println!("Texto 'Rust': {} contornos, {} puntos", pls.len(), pts);
            assert!(!pls.is_empty());
        }
        None => println!("Texto: no se encontró una fuente del sistema (ok en CI sin fuentes)"),
    }

    // Boolean operations + offset.
    let sq = |ox: f64, oy: f64, s: f64| crate::geometry::Polyline {
        points: vec![
            Vec2::new(ox, oy),
            Vec2::new(ox + s, oy),
            Vec2::new(ox + s, oy + s),
            Vec2::new(ox, oy + s),
        ],
        closed: true,
    };
    let a = vec![sq(0.0, 0.0, 10.0)];
    let b = vec![sq(5.0, 5.0, 10.0)];
    let uni = crate::geom_ops::boolean(&a, &b, crate::geom_ops::BoolOp::Union);
    let inter = crate::geom_ops::boolean(&a, &b, crate::geom_ops::BoolOp::Intersect);
    let diff = crate::geom_ops::boolean(&a, &b, crate::geom_ops::BoolOp::Difference);
    let off = crate::geom_ops::offset(&a, 2.0);
    println!(
        "Booleanas: unión {} · intersección {} · diferencia {} contornos · offset {}",
        uni.len(),
        inter.len(),
        diff.len(),
        off.len()
    );
    assert!(!uni.is_empty() && !inter.is_empty() && !diff.is_empty() && !off.is_empty());
    // L-shaped union is a single contour with at least 6 vertices.
    assert!(uni[0].points.len() >= 6, "unión con muy pocos vértices");
    assert!(inter[0].points.len() >= 4, "intersección inválida");

    // Path optimization: scatter several rectangles, compare travel optimized vs not.
    let mut d2 = Document::default();
    d2.layers = vec![Layer::new("Corte", [200, 30, 30])];
    for (i, &(x, y)) in [(10.0, 10.0), (300.0, 300.0), (20.0, 290.0), (280.0, 15.0), (150.0, 150.0)]
        .iter()
        .enumerate()
    {
        let r = crate::geometry::Polyline {
            points: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 10.0),
                Vec2::new(0.0, 10.0),
            ],
            closed: true,
        };
        d2.add_object(Object::new(
            i as u64 + 1,
            format!("r{i}"),
            0,
            ObjectKind::Vector(VectorData { polylines: vec![r] }),
            Vec2::new(x, y),
        ));
    }
    d2.machine.optimize_travel = false;
    let t_off = crate::gcode::analyze(&crate::gcode::generate(&d2).lines, &d2.machine).travel_mm;
    d2.machine.optimize_travel = true;
    let t_on = crate::gcode::analyze(&crate::gcode::generate(&d2).lines, &d2.machine).travel_mm;
    println!("Optimización: viaje {:.0} mm → {:.0} mm", t_off, t_on);
    assert!(t_on <= t_off + 1e-6, "la optimización no redujo el viaje");

    // Material test generator.
    let mt = crate::gcode::MaterialTest::default();
    let test_lines = crate::gcode::material_test(&d2.machine, &mt);
    let tp2 = crate::gcode::analyze(&test_lines, &d2.machine);
    println!(
        "Test material: {} líneas, {} celdas, ~{:.0} s",
        test_lines.len(),
        mt.cols * mt.rows,
        tp2.seconds
    );
    assert!(test_lines.len() > 50 && tp2.seconds > 0.0);

    // Materials library defaults.
    let mats = crate::materials::defaults();
    assert!(!mats.is_empty());
    println!("Biblioteca de materiales: {} presets", mats.len());

    // Image adjustments (brightness / contrast / gamma / invert).
    let mut adj = crate::model::ImageAdjust::default();
    assert!(adj.is_identity() && adj.map(100) == 100);
    adj.invert = true;
    assert_eq!(adj.map(0), 255);
    assert_eq!(adj.map(255), 0);
    let mut adjb = crate::model::ImageAdjust::default();
    adjb.brightness = 50.0;
    assert!(adjb.map(100) > 100, "el brillo no aclara");
    let mut adjg = crate::model::ImageAdjust::default();
    adjg.gamma = 2.0;
    assert!(adjg.map(128) > 128, "gamma>1 no aclara los medios");
    println!("Ajustes de imagen: identidad/invert/brillo/gamma OK");

    // New dithering algorithms produce pure black/white of the right size.
    let ramp: Vec<u8> = (0..64).map(|i| (i * 4) as u8).collect();
    for d in [
        crate::model::Dither::Atkinson,
        crate::model::Dither::Jarvis,
        crate::model::Dither::Ordered,
    ] {
        let p = crate::gcode_dither::to_power(&ramp, 8, 8, d);
        assert_eq!(p.len(), 64);
        assert!(p.iter().all(|&v| v == 0.0 || v == 1.0), "{:?} no es B/N", d);
    }
    println!("Tramado: Atkinson/Jarvis/Bayer OK");

    // Homography (perspective alignment).
    let src = [
        Vec2::new(0.0, 0.0),
        Vec2::new(100.0, 0.0),
        Vec2::new(100.0, 80.0),
        Vec2::new(0.0, 80.0),
    ];
    let dst = [
        Vec2::new(12.0, 6.0),
        Vec2::new(188.0, 3.0),
        Vec2::new(176.0, 150.0),
        Vec2::new(5.0, 140.0),
    ];
    let hmg = crate::homography::Homography::from_points(src, dst).expect("homografía");
    let mut max_err = 0.0f64;
    for i in 0..4 {
        let r = hmg.apply(src[i]);
        max_err = max_err.max((r.x - dst[i].x).abs().max((r.y - dst[i].y).abs()));
    }
    println!("Homografía: error máx en esquinas = {:.2e}", max_err);
    assert!(max_err < 1e-6, "la homografía no mapea las esquinas");

    // Warp a small source image into bed space (identity mapping fills the bed).
    let (sw, sh) = (8usize, 6usize);
    let srcimg = vec![200u8; sw * sh * 4];
    let idh = crate::homography::Homography::from_points(
        [
            Vec2::new(0.0, 0.0),
            Vec2::new(40.0, 0.0),
            Vec2::new(40.0, 30.0),
            Vec2::new(0.0, 30.0),
        ],
        [
            Vec2::new(0.0, 0.0),
            Vec2::new((sw - 1) as f64, 0.0),
            Vec2::new((sw - 1) as f64, (sh - 1) as f64),
            Vec2::new(0.0, (sh - 1) as f64),
        ],
    )
    .unwrap();
    let warped = crate::homography::warp_to_bed(&srcimg, sw, sh, &idh, 40.0, 30.0, 40, 30);
    let opaque = warped.chunks(4).filter(|p| p[3] == 255).count();
    println!("Warp: {}/{} píxeles opacos", opaque, 40 * 30);
    assert!(opaque > 40 * 30 / 2, "el warp no rellenó la cama");

    println!("\n✓ Self-test completado sin errores.");
}
