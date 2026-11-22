//! The egui application: layout, canvas, panels and glue to the device worker.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2 as EVec2};

use crate::device::{ConnTarget, DeviceHandle, Evt};
use crate::geometry::Vec2;
use crate::gcode::Toolpath;
use crate::model::{CutMode, Dither, Document, Object, ObjectKind};

#[derive(PartialEq, Eq)]
enum ConnKind {
    Usb,
    Wifi,
}

/// Active canvas tool.
#[derive(PartialEq, Eq, Clone, Copy)]
enum Tool {
    Select,
    Node,
    Rect,
    Ellipse,
    Line,
    Polygon,
    Polyline,
    Text,
    Measure,
}

/// Which interaction the current pointer drag is performing.
#[derive(Clone)]
enum Gesture {
    None,
    Pan,
    Move {
        obj: usize,
        start_pos: Vec2,
        ptr_start: Vec2,
        /// Other group members dragged along: (object index, start position).
        group: Vec<(usize, Vec2)>,
    },
    Scale {
        obj: usize,
        anchor_world: Vec2,
        local_anchor: Vec2,
        local_corner: Vec2,
        start_scale: Vec2,
    },
    Rotate {
        obj: usize,
        center: Vec2,
        start_pos: Vec2,
        start_rot: f64,
        start_angle: f64,
    },
}

/// A one-shot arrange action chosen from the properties panel.
#[derive(Clone, Copy)]
enum ArrangeAct {
    Group,
    Ungroup,
    MirrorH,
    MirrorV,
    Rotate90,
    /// (axis: 0 = X / 1 = Y, mode: -1 min, 0 centre, 1 max)
    Align(u8, i8),
    /// Distribute evenly along axis (0 = X, 1 = Y).
    Distribute(u8),
    OpenArray,
}

/// World (mm) <-> screen mapping for the canvas.
struct CanvasView {
    /// Pixels per millimetre.
    zoom: f32,
    /// Screen-space offset of world origin, relative to the canvas top-left.
    pan: EVec2,
    initialized: bool,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            zoom: 2.0,
            pan: EVec2::new(20.0, 20.0),
            initialized: false,
        }
    }
}

impl CanvasView {
    fn w2s(&self, origin: Pos2, w: Vec2) -> Pos2 {
        Pos2::new(
            origin.x + self.pan.x + w.x as f32 * self.zoom,
            origin.y + self.pan.y + w.y as f32 * self.zoom,
        )
    }
    fn s2w(&self, origin: Pos2, s: Pos2) -> Vec2 {
        Vec2::new(
            ((s.x - origin.x - self.pan.x) / self.zoom) as f64,
            ((s.y - origin.y - self.pan.y) / self.zoom) as f64,
        )
    }
}

pub struct RustburnApp {
    doc: Document,
    device: DeviceHandle,
    view: CanvasView,

    // connection form
    conn_kind: ConnKind,
    port: String,
    host: String,
    tcp_port: u16,
    ports_cache: Vec<String>,

    // jog
    jog_step: f64,
    jog_feed: f64,

    // interaction
    gesture: Gesture,
    snap: bool,
    snap_step: f64,
    lock_ratio: bool,

    // drawing tools
    tool: Tool,
    draft: Option<(Vec2, Vec2)>,
    cursor_world: Option<Vec2>,
    poly_points: Vec<Vec2>,
    polygon_sides: u32,
    text_content: String,
    text_height: f64,
    /// Selected font family for text (empty = automatic).
    text_font: String,
    /// Cached list of installed font families, for the picker.
    font_families: Vec<String>,
    // variable text / serial numbering
    num_series: bool,
    num_start: i64,
    num_count: u32,
    num_step: i64,
    num_spacing: f64,
    num_horizontal: bool,
    fontdb: usvg::fontdb::Database,

    // node editing: (object, polyline, point)
    node_drag: Option<(usize, usize, usize)>,
    // boolean operands (extra selection, added with Shift+click)
    multi: Vec<usize>,
    offset_dist: f64,

    // materials & test
    materials: Vec<crate::materials::Material>,
    new_material_name: String,
    machines: Vec<crate::machines::MachineProfile>,
    new_machine_name: String,
    /// Light (white) canvas background instead of the default dark one.
    canvas_light: bool,
    /// Last preferences written to disk, and the debounce timer for autosave.
    saved_prefs: crate::prefs::Prefs,
    prefs_dirty_at: Option<std::time::Instant>,
    mat_test: crate::gcode::MaterialTest,
    show_mat_test: bool,

    // camera / photo alignment
    cam_device: String, // v4l2 device for live capture, e.g. /dev/video0
    cam_orig: Option<(Vec<u8>, usize, usize)>, // pristine photo (rgba, w, h)
    cam_photo: Option<(Vec<u8>, usize, usize)>, // displayed/corrected (rgba, w, h)
    cam_raw_tex: Option<egui::TextureHandle>,
    cam_tex: Option<egui::TextureHandle>, // bed-aligned (warped)
    cam_show: bool,
    cam_opacity: f32,
    // lens distortion correction coefficients (radial k1/k2)
    cam_k1: f64,
    cam_k2: f64,
    calibrating: bool,
    cal_points: Vec<Vec2>,

    // bitmap tracing (vectorize an image)
    trace_threshold: u8,
    trace_invert: bool,
    trace_simplify: f64,

    // print & cut registration
    show_printcut: bool,
    pc_marks: Vec<Vec2>,            // designed mark positions (document mm)
    pc_measured: Vec<(f64, f64)>,  // measured machine X/Y per mark
    pc_transform: Option<crate::geometry::Affine>,
    pc_apply: bool,

    // GRBL settings ($$)
    grbl_settings: Vec<crate::grbl_settings::Setting>,
    show_grbl: bool,

    // help & configuration windows
    show_help: bool,
    show_config: bool,

    // job output options (like LightBurn's laser panel)
    cut_selected: bool,
    use_job_origin: bool,
    /// Which 3×3 point of the design bbox is the job origin: (col 0..2, row 0..2).
    job_origin: (u8, u8),
    // array / matrix duplicator
    show_array: bool,
    array_cols: u32,
    array_rows: u32,
    array_dx: f64,
    array_dy: f64,
    array_circular: bool,
    array_count: u32,
    array_radius: f64,
    // measure tool
    measure_a: Option<Vec2>,
    measure_b: Option<Vec2>,

    // undo / redo
    history: Vec<Document>,
    future: Vec<Document>,

    // output
    gcode: Vec<String>,
    gcode_msg: String,
    toolpath: Option<Toolpath>,
    show_toolpath: bool,
    // toolpath animation
    sim_playing: bool,
    sim_pos_mm: f64,
    sim_speed: f64, // mm/s of simulated head travel
    // snapping to other objects / bed guides
    snap_objects: bool,
    snap_guides: Vec<(Vec2, Vec2)>,

    // framing
    frame_power: f64,
    frame_speed: f64,

    // console
    console: Vec<String>,
    console_input: String,

    // textures for image objects
    textures: HashMap<u64, egui::TextureHandle>,

    project_path: Option<PathBuf>,
    status_msg: String,
}

impl RustburnApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let ports = crate::device::list_serial_ports();
        let default_port = ports
            .iter()
            .find(|p| p.contains("ttyUSB") || p.contains("ttyACM"))
            .cloned()
            .or_else(|| ports.first().cloned())
            .unwrap_or_else(|| "/dev/ttyUSB0".to_string());

        // Optional demo scene for screenshots/testing (RUSTBURN_DEMO=1).
        let doc = if std::env::var("RUSTBURN_DEMO").is_ok() {
            let mut d = crate::selftest::demo_document();
            d.selected = Some(0);
            d
        } else {
            Document::default()
        };

        let mut app = Self {
            doc,
            device: DeviceHandle::spawn(),
            view: CanvasView::default(),
            conn_kind: ConnKind::Usb,
            port: default_port,
            host: "192.168.4.1".to_string(),
            tcp_port: 23,
            ports_cache: ports,
            jog_step: 10.0,
            jog_feed: 3000.0,
            gesture: Gesture::None,
            snap: false,
            snap_step: 1.0,
            lock_ratio: true,
            tool: Tool::Select,
            draft: None,
            cursor_world: None,
            poly_points: Vec::new(),
            polygon_sides: 6,
            text_content: "TEXTO".to_string(),
            text_height: 20.0,
            text_font: String::new(),
            font_families: Vec::new(),
            num_series: false,
            num_start: 1,
            num_count: 10,
            num_step: 1,
            num_spacing: 30.0,
            num_horizontal: false,
            fontdb: crate::text::build_font_db(),
            node_drag: None,
            multi: Vec::new(),
            offset_dist: 1.0,
            materials: crate::materials::load(),
            new_material_name: String::new(),
            machines: crate::machines::load(),
            new_machine_name: String::new(),
            canvas_light: false,
            saved_prefs: crate::prefs::Prefs::default(),
            prefs_dirty_at: None,
            mat_test: crate::gcode::MaterialTest::default(),
            show_mat_test: false,
            cam_device: "/dev/video0".to_string(),
            cam_orig: None,
            cam_photo: None,
            cam_raw_tex: None,
            cam_tex: None,
            cam_show: true,
            cam_opacity: 0.6,
            cam_k1: 0.0,
            cam_k2: 0.0,
            calibrating: false,
            cal_points: Vec::new(),
            trace_threshold: 128,
            trace_invert: false,
            trace_simplify: 0.15,
            show_printcut: false,
            pc_marks: Vec::new(),
            pc_measured: Vec::new(),
            pc_transform: None,
            pc_apply: false,
            grbl_settings: Vec::new(),
            show_grbl: false,
            show_help: false,
            show_config: false,
            cut_selected: false,
            use_job_origin: false,
            job_origin: (0, 2), // front-left by default
            show_array: false,
            array_cols: 3,
            array_rows: 2,
            array_dx: 30.0,
            array_dy: 30.0,
            array_circular: false,
            array_count: 6,
            array_radius: 40.0,
            measure_a: None,
            measure_b: None,
            history: Vec::new(),
            future: Vec::new(),
            gcode: Vec::new(),
            gcode_msg: String::new(),
            toolpath: None,
            show_toolpath: true,
            sim_playing: false,
            sim_pos_mm: 0.0,
            sim_speed: 1500.0,
            snap_objects: true,
            snap_guides: Vec::new(),
            frame_power: 1.0,
            frame_speed: 3000.0,
            console: vec!["rustburn listo. Conéctate a la Phecda para empezar.".into()],
            console_input: String::new(),
            textures: HashMap::new(),
            project_path: None,
            status_msg: String::new(),
        };

        // Cache the installed font families for the text font picker.
        app.font_families = crate::text::available_families(&app.fontdb);

        // Restore last-used preferences (machine, background, font).
        let prefs = crate::prefs::load();
        app.canvas_light = prefs.canvas_light;
        if let Some(m) = &prefs.machine {
            app.doc.machine = m.clone();
        }
        if !prefs.text_font.is_empty()
            && app.font_families.iter().any(|f| *f == prefs.text_font)
        {
            app.text_font = prefs.text_font.clone();
        }
        app.saved_prefs = app.current_prefs();

        // Showcase Phase 3 primitives in the demo scene.
        if std::env::var("RUSTBURN_DEMO").is_ok() {
            let (hex, hpos) =
                crate::shapes::polygon(Vec2::new(320.0, 120.0), Vec2::new(355.0, 120.0), 6);
            app.add_vector(hex, hpos, "hexágono");
            if let Some(txt) = crate::text::text_to_polylines(&app.fontdb, "RUSTBURN", 26.0) {
                app.add_vector(txt, Vec2::new(60.0, 300.0), "texto");
            }
            // Bake (polylines, pos) into document coordinates.
            let bake = |(pls, pos): (Vec<crate::geometry::Polyline>, Vec2)| {
                pls.into_iter()
                    .map(|mut pl| {
                        for p in &mut pl.points {
                            p.x += pos.x;
                            p.y += pos.y;
                        }
                        pl
                    })
                    .collect::<Vec<_>>()
            };
            // Boolean union of two overlapping circles (Phase 3b).
            let c1 = bake(crate::shapes::ellipse(Vec2::new(165.0, 325.0), Vec2::new(225.0, 385.0)));
            let c2 = bake(crate::shapes::ellipse(Vec2::new(205.0, 325.0), Vec2::new(265.0, 385.0)));
            let uni = crate::geom_ops::boolean(&c1, &c2, crate::geom_ops::BoolOp::Union);
            app.push_vector_object(uni, 1, "unión");
            // Offset ring around the hexagon.
            let hexd = bake(crate::shapes::polygon(
                Vec2::new(320.0, 120.0),
                Vec2::new(355.0, 120.0),
                6,
            ));
            let ring = crate::geom_ops::offset(&hexd, 7.0);
            app.push_vector_object(ring, 1, "offset");

            // Synthetic "board photo" + perspective calibration to showcase alignment.
            let (pw, ph) = (240usize, 180usize);
            let mut buf = vec![0u8; pw * ph * 4];
            for y in 0..ph {
                for x in 0..pw {
                    let i = (y * pw + x) * 4;
                    let c = if ((x / 30) + (y / 30)) % 2 == 0 { 70u8 } else { 120u8 };
                    buf[i] = c;
                    buf[i + 1] = (c as f32 * 0.9) as u8;
                    buf[i + 2] = 60;
                    buf[i + 3] = 255;
                }
            }
            let ci = egui::ColorImage::from_rgba_unmultiplied([pw, ph], &buf);
            app.cam_raw_tex = Some(ctx.load_texture("cam_raw", ci, egui::TextureOptions::LINEAR));
            app.cam_orig = Some((buf.clone(), pw, ph));
            app.cam_photo = Some((buf, pw, ph));
            app.cal_points = vec![
                Vec2::new(22.0, 16.0),
                Vec2::new(224.0, 7.0),
                Vec2::new(233.0, 171.0),
                Vec2::new(9.0, 159.0),
            ];
            app.compute_calibration(&ctx);
            app.cam_show = true;
            app.cam_opacity = 0.45;

            // Populate GRBL settings with typical Phecda values for the showcase.
            for (n, v) in [
                (0u32, "3"),
                (1, "25"),
                (2, "0"),
                (3, "0"),
                (10, "1"),
                (11, "0.010"),
                (12, "0.002"),
                (20, "0"),
                (21, "0"),
                (22, "1"),
                (23, "3"),
                (24, "100"),
                (25, "2000"),
                (26, "250"),
                (27, "2"),
                (30, "1000"),
                (31, "0"),
                (32, "1"),
                (100, "80"),
                (101, "80"),
                (110, "12000"),
                (111, "12000"),
                (120, "1000"),
                (121, "1000"),
                (130, "400"),
                (131, "400"),
            ] {
                app.upsert_setting(n, v.to_string());
            }
            app.show_grbl = false;

            // Showcase Phase 7: image adjustments + a diffusion dither on the engrave image.
            if let Some(obj) = app.doc.objects.get_mut(1) {
                if let ObjectKind::Image(img) = &mut obj.kind {
                    img.adjust.brightness = 10.0;
                    img.adjust.contrast = 45.0;
                    img.adjust.gamma = 1.6;
                }
            }
            if let Some(l) = app.doc.layers.get_mut(1) {
                l.settings.dither = crate::model::Dither::Jarvis;
            }

            app.history.clear();
            app.multi.clear();
            app.tool = Tool::Select;
            app.doc.selected = Some(1); // engrave image → show adjustment panel
            app.show_mat_test = false;
        }

        // Pre-generate G-code so the toolpath preview is visible in the demo scene.
        if !app.doc.objects.is_empty() {
            let job = crate::gcode::generate(&app.doc);
            app.gcode = job.lines;
            app.recompute_toolpath();
        }
        app
    }

    /// Snapshot of the values persisted between sessions.
    fn current_prefs(&self) -> crate::prefs::Prefs {
        crate::prefs::Prefs {
            canvas_light: self.canvas_light,
            machine: Some(self.doc.machine.clone()),
            text_font: self.text_font.clone(),
        }
    }

    /// Persist preferences when they change, debounced so a slider drag does not
    /// write on every frame. Call once per frame.
    fn autosave_prefs(&mut self) {
        let cur = self.current_prefs();
        if cur == self.saved_prefs {
            self.prefs_dirty_at = None;
            return;
        }
        let since = *self.prefs_dirty_at.get_or_insert_with(std::time::Instant::now);
        if since.elapsed() >= std::time::Duration::from_millis(500) {
            let _ = crate::prefs::save(&cur);
            self.saved_prefs = cur;
            self.prefs_dirty_at = None;
        }
    }

    /// Reset to an empty project, keeping the current machine configuration.
    /// Undoable with Ctrl+Z (the previous document is pushed to history).
    fn new_project(&mut self) {
        self.snapshot();
        let machine = self.doc.machine.clone();
        self.doc = crate::model::Document::default();
        self.doc.machine = machine;
        self.textures.clear();
        self.project_path = None;
        self.gcode.clear();
        self.gcode_msg.clear();
        self.toolpath = None;
        self.multi.clear();
        self.poly_points.clear();
        self.measure_a = None;
        self.measure_b = None;
        self.sim_playing = false;
        self.sim_pos_mm = 0.0;
        self.status_msg = "Nuevo proyecto".into();
    }

    fn log(&mut self, s: impl Into<String>) {
        self.console.push(s.into());
        if self.console.len() > 600 {
            let drop = self.console.len() - 600;
            self.console.drain(0..drop);
        }
    }

    fn selected_layer(&self) -> usize {
        self.doc
            .selected
            .and_then(|i| self.doc.objects.get(i))
            .map(|o| o.layer)
            .unwrap_or(0)
    }

    /// Import a PDF as a raster image (`as_image`) or as vector paths.
    fn import_pdf(&mut self, as_image: bool) {
        let Some(path) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() else {
            return;
        };
        self.status_msg = "Convirtiendo PDF…".into();
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "pdf".into());
        let res = if as_image {
            crate::import::load_pdf(&path, 200)
        } else {
            crate::import::load_pdf_vector(&path)
        };
        match res {
            Ok(kind) => {
                self.add_imported(kind, name);
                self.status_msg = if as_image {
                    "PDF importado (imagen)".into()
                } else {
                    "PDF importado (vectores)".into()
                };
            }
            Err(e) => self.status_msg = format!("Error PDF: {e}"),
        }
    }

    fn add_imported(&mut self, kind: ObjectKind, name: String) {
        self.snapshot();
        let layer = self.selected_layer().min(self.doc.layers.len().saturating_sub(1));
        let id = self.doc.alloc_id();
        let mut obj = Object::new(id, name, layer, kind, Vec2::new(10.0, 10.0));
        // Fit to the bed and centre it, so even a large import arrives visible and
        // its resize handles are reachable on screen.
        if let Some(bb) = obj.local_bbox() {
            let (w, h) = (bb.width().max(1e-6), bb.height().max(1e-6));
            let (bw, bh) = (self.doc.machine.bed_w_mm, self.doc.machine.bed_h_mm);
            let fit = ((bw * 0.9 / w).min(bh * 0.9 / h)).min(1.0);
            obj.scale = Vec2::new(fit, fit);
            let (sw, sh) = (w * fit, h * fit);
            obj.pos = Vec2::new(
                (bw - sw) / 2.0 - bb.min.x * fit,
                (bh - sh) / 2.0 - bb.min.y * fit,
            );
        }
        let idx = self.doc.add_object(obj);
        self.doc.selected = Some(idx);
    }

    // --- undo / redo ---------------------------------------------------------
    /// Push the current document onto the undo stack (call before a mutation).
    fn snapshot(&mut self) {
        self.history.push(self.doc.clone());
        if self.history.len() > 80 {
            self.history.remove(0);
        }
        self.future.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.history.pop() {
            self.future.push(std::mem::replace(&mut self.doc, prev));
            self.textures.clear();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.future.pop() {
            self.history.push(std::mem::replace(&mut self.doc, next));
            self.textures.clear();
        }
    }

    fn duplicate_selected(&mut self) {
        let idx = self.selected_indices();
        if idx.is_empty() {
            return;
        }
        self.snapshot();
        // Clones of a group get a fresh shared group id.
        let regroup = idx.len() > 1 || self.any_grouped_selected();
        let new_gid = if regroup {
            Some(
                self.doc
                    .objects
                    .iter()
                    .filter_map(|o| o.group)
                    .max()
                    .unwrap_or(0)
                    + 1,
            )
        } else {
            None
        };
        let mut new_indices = Vec::new();
        for &i in &idx {
            if let Some(obj) = self.doc.objects.get(i).cloned() {
                let mut clone = obj;
                clone.id = self.doc.alloc_id();
                clone.pos.x += 5.0;
                clone.pos.y += 5.0;
                clone.group = new_gid;
                new_indices.push(self.doc.add_object(clone));
            }
        }
        if let Some(&first) = new_indices.first() {
            self.doc.selected = Some(first);
            self.multi = new_indices.into_iter().skip(1).collect();
        }
    }

    fn delete_selected(&mut self) {
        // Remove the whole current selection (object + group + extra picks).
        let mut idx = self.selected_indices();
        if idx.is_empty() {
            return;
        }
        self.snapshot();
        idx.sort_unstable();
        idx.dedup();
        for &i in idx.iter().rev() {
            if i < self.doc.objects.len() {
                self.doc.objects.remove(i);
            }
        }
        self.doc.selected = None;
        self.multi.clear();
    }

    // --- Grouping & selection helpers ------------------------------------

    /// All object indices currently acted on: the primary selection, any
    /// shift-picked extras, and every member of their groups. Deduplicated.
    fn selected_indices(&self) -> Vec<usize> {
        let mut seeds: Vec<usize> = Vec::new();
        if let Some(s) = self.doc.selected {
            seeds.push(s);
        }
        seeds.extend(self.multi.iter().copied());
        let mut out: Vec<usize> = Vec::new();
        for s in seeds {
            for m in self.group_members(s) {
                if !out.contains(&m) {
                    out.push(m);
                }
            }
        }
        out
    }

    /// Indices of every object sharing `idx`'s group (including `idx`). If the
    /// object has no group, just `[idx]`.
    fn group_members(&self, idx: usize) -> Vec<usize> {
        match self.doc.objects.get(idx).and_then(|o| o.group) {
            Some(g) => (0..self.doc.objects.len())
                .filter(|&i| self.doc.objects[i].group == Some(g))
                .collect(),
            None => vec![idx],
        }
    }

    /// Group members *other than* `idx`, paired with their start positions
    /// (used to drag the whole group together).
    fn group_drag_members(&self, idx: usize) -> Vec<(usize, Vec2)> {
        self.group_members(idx)
            .into_iter()
            .filter(|&i| i != idx)
            .filter_map(|i| self.doc.objects.get(i).map(|o| (i, o.pos)))
            .collect()
    }

    /// Select object `idx`; if it belongs to a group, light up the whole group.
    fn select_with_group(&mut self, idx: usize) {
        self.doc.selected = Some(idx);
        self.multi.clear();
        if self.doc.objects.get(idx).and_then(|o| o.group).is_some() {
            self.multi = self
                .group_members(idx)
                .into_iter()
                .filter(|&i| i != idx)
                .collect();
        }
    }

    /// Group the current selection (primary + extras) under a fresh id.
    fn group_selected(&mut self) {
        let idx = self.selected_indices();
        if idx.len() < 2 {
            self.status_msg = "Selecciona 2+ objetos (Shift+clic) para agrupar".into();
            return;
        }
        self.snapshot();
        let gid = self
            .doc
            .objects
            .iter()
            .filter_map(|o| o.group)
            .max()
            .unwrap_or(0)
            + 1;
        for &i in &idx {
            if let Some(o) = self.doc.objects.get_mut(i) {
                o.group = Some(gid);
            }
        }
        self.status_msg = format!("Agrupados {} objetos", idx.len());
    }

    /// Dissolve the group(s) touched by the current selection.
    fn ungroup_selected(&mut self) {
        let idx = self.selected_indices();
        if idx.is_empty() {
            return;
        }
        self.snapshot();
        for &i in &idx {
            if let Some(o) = self.doc.objects.get_mut(i) {
                o.group = None;
            }
        }
        self.multi.clear();
        self.status_msg = "Desagrupado".into();
    }

    /// Assign every selected object to layer `li` (palette click).
    fn assign_layer(&mut self, li: usize) {
        let idx = self.selected_indices();
        if idx.is_empty() || li >= self.doc.layers.len() {
            return;
        }
        self.snapshot();
        for &i in &idx {
            if let Some(o) = self.doc.objects.get_mut(i) {
                o.layer = li;
            }
        }
        self.status_msg = format!("Movido a capa {}", li + 1);
    }

    fn any_grouped_selected(&self) -> bool {
        self.selected_indices()
            .iter()
            .any(|&i| self.doc.objects.get(i).and_then(|o| o.group).is_some())
    }

    // --- Arrange: mirror / rotate / align --------------------------------

    /// Mirror the whole selection about its common centre (`horiz` = flip X).
    fn mirror_selected(&mut self, horiz: bool) {
        let idx = self.selected_indices();
        if idx.is_empty() {
            return;
        }
        let Some(bb) = self.selection_bbox(&idx) else { return };
        let (cx, cy) = ((bb.min.x + bb.max.x) / 2.0, (bb.min.y + bb.max.y) / 2.0);
        self.snapshot();
        for &i in &idx {
            if let Some(o) = self.doc.objects.get_mut(i) {
                if horiz {
                    o.scale.x = -o.scale.x;
                    o.pos.x = 2.0 * cx - o.pos.x;
                } else {
                    o.scale.y = -o.scale.y;
                    o.pos.y = 2.0 * cy - o.pos.y;
                }
            }
        }
        self.status_msg = if horiz { "Espejo horizontal" } else { "Espejo vertical" }.into();
    }

    /// Rotate the whole selection +90° about its common centre.
    fn rotate90_selected(&mut self) {
        let idx = self.selected_indices();
        if idx.is_empty() {
            return;
        }
        let Some(bb) = self.selection_bbox(&idx) else { return };
        let (cx, cy) = ((bb.min.x + bb.max.x) / 2.0, (bb.min.y + bb.max.y) / 2.0);
        self.snapshot();
        for &i in &idx {
            if let Some(o) = self.doc.objects.get_mut(i) {
                // Rotate the object's origin about the selection centre (+90° CW).
                let (dx, dy) = (o.pos.x - cx, o.pos.y - cy);
                o.pos = Vec2::new(cx - dy, cy + dx);
                o.rotation = (o.rotation + 90.0).rem_euclid(360.0);
            }
        }
        self.status_msg = "Rotado 90°".into();
    }

    /// Common bounding box of a set of object indices (document space).
    fn selection_bbox(&self, idx: &[usize]) -> Option<crate::geometry::BBox> {
        let mut acc = None;
        for &i in idx {
            acc = crate::geometry::BBox::union(acc, self.doc.objects.get(i).and_then(|o| o.bbox()));
        }
        acc
    }

    /// Snap the moved object's position `np` so one of its edges/centre lines up
    /// with another object's edge/centre or a bed guide. Returns the adjusted
    /// position and any guide lines to draw (document space).
    fn snap_to_objects(&self, obj: usize, np: Vec2) -> (Vec2, Vec<(Vec2, Vec2)>) {
        let mut guides = Vec::new();
        let Some(o) = self.doc.objects.get(obj) else { return (np, guides) };
        let Some(bb) = o.bbox() else { return (np, guides) };
        // Anchor offsets relative to pos (invariant under translation).
        let ox = [bb.min.x - o.pos.x, (bb.min.x + bb.max.x) / 2.0 - o.pos.x, bb.max.x - o.pos.x];
        let oy = [bb.min.y - o.pos.y, (bb.min.y + bb.max.y) / 2.0 - o.pos.y, bb.max.y - o.pos.y];

        let bw = self.doc.machine.bed_w_mm;
        let bh = self.doc.machine.bed_h_mm;
        let skip = self.group_members(obj);
        // Candidate target lines from other objects + the bed.
        let mut tx: Vec<f64> = vec![0.0, bw / 2.0, bw];
        let mut ty: Vec<f64> = vec![0.0, bh / 2.0, bh];
        for (i, other) in self.doc.objects.iter().enumerate() {
            if skip.contains(&i) {
                continue;
            }
            if let Some(ob) = other.bbox() {
                tx.extend_from_slice(&[ob.min.x, (ob.min.x + ob.max.x) / 2.0, ob.max.x]);
                ty.extend_from_slice(&[ob.min.y, (ob.min.y + ob.max.y) / 2.0, ob.max.y]);
            }
        }
        let thresh = (6.0 / self.view.zoom as f64).max(0.1);

        let mut best_x: Option<(f64, f64)> = None; // (delta_to_apply, target_line)
        for &a in &ox {
            let anchor = np.x + a;
            for &t in &tx {
                let d = t - anchor;
                if d.abs() <= thresh && best_x.map_or(true, |(bd, _)| d.abs() < bd.abs()) {
                    best_x = Some((d, t));
                }
            }
        }
        let mut best_y: Option<(f64, f64)> = None;
        for &a in &oy {
            let anchor = np.y + a;
            for &t in &ty {
                let d = t - anchor;
                if d.abs() <= thresh && best_y.map_or(true, |(bd, _)| d.abs() < bd.abs()) {
                    best_y = Some((d, t));
                }
            }
        }
        let mut res = np;
        if let Some((d, t)) = best_x {
            res.x += d;
            guides.push((Vec2::new(t, 0.0), Vec2::new(t, bh)));
        }
        if let Some((d, t)) = best_y {
            res.y += d;
            guides.push((Vec2::new(0.0, t), Vec2::new(bw, t)));
        }
        (res, guides)
    }

    /// Align the selection. `axis` 0 = horizontal (X), 1 = vertical (Y);
    /// `mode` -1 = min edge, 0 = centre, 1 = max edge.
    fn align_selected(&mut self, axis: u8, mode: i8) {
        let idx = self.selected_indices();
        if idx.len() < 2 {
            self.status_msg = "Selecciona 2+ objetos para alinear".into();
            return;
        }
        let Some(target) = self.selection_bbox(&idx) else { return };
        self.snapshot();
        for &i in &idx {
            let Some(bb) = self.doc.objects.get(i).and_then(|o| o.bbox()) else { continue };
            let Some(o) = self.doc.objects.get_mut(i) else { continue };
            if axis == 0 {
                let want = match mode {
                    -1 => target.min.x - bb.min.x,
                    1 => target.max.x - bb.max.x,
                    _ => (target.min.x + target.max.x) / 2.0 - (bb.min.x + bb.max.x) / 2.0,
                };
                o.pos.x += want;
            } else {
                let want = match mode {
                    -1 => target.min.y - bb.min.y,
                    1 => target.max.y - bb.max.y,
                    _ => (target.min.y + target.max.y) / 2.0 - (bb.min.y + bb.max.y) / 2.0,
                };
                o.pos.y += want;
            }
        }
        self.status_msg = "Alineado".into();
    }

    /// Distribute 3+ selected objects so their centres are evenly spaced along
    /// `axis` (0 = X, 1 = Y) between the first and last.
    fn distribute_selected(&mut self, axis: u8) {
        let idx = self.selected_indices();
        if idx.len() < 3 {
            self.status_msg = "Selecciona 3+ objetos para distribuir".into();
            return;
        }
        // Centre of each object along the axis.
        let mut items: Vec<(usize, f64)> = idx
            .iter()
            .filter_map(|&i| {
                self.doc.objects.get(i).and_then(|o| o.bbox()).map(|b| {
                    let c = if axis == 0 { (b.min.x + b.max.x) / 2.0 } else { (b.min.y + b.max.y) / 2.0 };
                    (i, c)
                })
            })
            .collect();
        if items.len() < 3 {
            return;
        }
        items.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let first = items.first().unwrap().1;
        let last = items.last().unwrap().1;
        let step = (last - first) / (items.len() as f64 - 1.0);
        self.snapshot();
        for (k, (i, c)) in items.iter().enumerate() {
            let target = first + step * k as f64;
            let d = target - c;
            if let Some(o) = self.doc.objects.get_mut(*i) {
                if axis == 0 {
                    o.pos.x += d;
                } else {
                    o.pos.y += d;
                }
            }
        }
        self.status_msg = "Distribuido".into();
    }

    /// Duplicate the selection into a grid or circular array.
    fn apply_array(&mut self) {
        let idx = self.selected_indices();
        if idx.is_empty() {
            self.status_msg = "Selecciona un objeto para la matriz".into();
            return;
        }
        let src: Vec<Object> = idx
            .iter()
            .filter_map(|&i| self.doc.objects.get(i).cloned())
            .collect();
        if src.is_empty() {
            return;
        }
        self.snapshot();
        let mut made = 0usize;
        if self.array_circular {
            let n = self.array_count.max(2);
            let Some(bb) = self.selection_bbox(&idx) else { return };
            let cx = (bb.min.x + bb.max.x) / 2.0;
            let cy = bb.min.y - self.array_radius; // centre is `radius` above the selection
            for k in 1..n {
                let ang = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
                let (sin, cos) = ang.sin_cos();
                for o in &src {
                    let mut c = o.clone();
                    c.id = self.doc.alloc_id();
                    c.group = None;
                    let (dx, dy) = (o.pos.x - cx, o.pos.y - cy);
                    c.pos = Vec2::new(cx + dx * cos - dy * sin, cy + dx * sin + dy * cos);
                    c.rotation = (o.rotation + ang.to_degrees()).rem_euclid(360.0);
                    self.doc.add_object(c);
                    made += 1;
                }
            }
        } else {
            let cols = self.array_cols.max(1);
            let rows = self.array_rows.max(1);
            for r in 0..rows {
                for col in 0..cols {
                    if r == 0 && col == 0 {
                        continue;
                    }
                    for o in &src {
                        let mut c = o.clone();
                        c.id = self.doc.alloc_id();
                        c.group = None;
                        c.pos = Vec2::new(
                            o.pos.x + col as f64 * self.array_dx,
                            o.pos.y + r as f64 * self.array_dy,
                        );
                        self.doc.add_object(c);
                        made += 1;
                    }
                }
            }
        }
        self.status_msg = format!("Matriz: {made} copia(s)");
    }

    /// Build the generation options from the laser-panel toggles.
    fn current_gen_options(&self) -> crate::gcode::GenOptions {
        let only_ids = if self.cut_selected {
            Some(
                self.selected_indices()
                    .iter()
                    .filter_map(|&i| self.doc.objects.get(i).map(|o| o.id))
                    .collect::<Vec<u64>>(),
            )
        } else {
            None
        };
        let relative_anchor = if self.use_job_origin {
            self.job_origin_point()
        } else {
            None
        };
        let align_transform = if self.pc_apply { self.pc_transform } else { None };
        crate::gcode::GenOptions {
            only_ids,
            relative_anchor,
            align_transform,
        }
    }

    /// The document-space point selected by the 3×3 job-origin picker.
    fn job_origin_point(&self) -> Option<Vec2> {
        let bb = if self.cut_selected {
            self.selection_bbox(&self.selected_indices())
        } else {
            self.doc.bbox()
        }?;
        let (col, row) = self.job_origin;
        let x = match col {
            0 => bb.min.x,
            2 => bb.max.x,
            _ => (bb.min.x + bb.max.x) / 2.0,
        };
        let y = match row {
            0 => bb.min.y,
            2 => bb.max.y,
            _ => (bb.min.y + bb.max.y) / 2.0,
        };
        Some(Vec2::new(x, y))
    }

    /// Generate G-code honouring the current options, updating messages.
    fn generate_gcode(&mut self) {
        let opts = self.current_gen_options();
        let job = crate::gcode::generate_with(&self.doc, &opts);
        self.gcode = job.lines;
        self.recompute_toolpath();
        let eta = self
            .toolpath
            .as_ref()
            .map(|t| format!(" · ⏱ {}", fmt_duration(t.seconds)))
            .unwrap_or_default();
        let extra = match (self.cut_selected, self.use_job_origin) {
            (true, true) => " · solo selección · origen de pieza",
            (true, false) => " · solo selección",
            (false, true) => " · origen de pieza",
            _ => "",
        };
        self.gcode_msg = format!(
            "{} líneas · {} capa(s) · {} objeto(s){}{}",
            self.gcode.len(),
            job.stats.layers_out,
            job.stats.objects_out,
            eta,
            extra
        );
        self.status_msg = "G-code generado".into();
    }

    fn recompute_toolpath(&mut self) {
        if self.gcode.is_empty() {
            self.toolpath = None;
        } else {
            self.toolpath = Some(crate::gcode::analyze(&self.gcode, &self.doc.machine));
        }
        self.sim_pos_mm = 0.0;
        self.sim_playing = false;
    }

    /// Total path length (cut + travel) of the current toolpath, in mm.
    fn sim_total_mm(&self) -> f64 {
        self.toolpath
            .as_ref()
            .map(|t| t.cut_mm + t.travel_mm)
            .unwrap_or(0.0)
    }

    /// Advance the simulation head by `dt` seconds while playing.
    fn tick_simulation(&mut self, dt: f64) {
        if !self.sim_playing {
            return;
        }
        let total = self.sim_total_mm();
        if total <= 0.0 {
            self.sim_playing = false;
            return;
        }
        self.sim_pos_mm += self.sim_speed * dt;
        if self.sim_pos_mm >= total {
            self.sim_pos_mm = total;
            self.sim_playing = false;
        }
    }

    /// Head position at the current simulated distance, plus whether it is
    /// cutting (non-rapid) there.
    fn sim_head(&self) -> Option<(Vec2, bool)> {
        let tp = self.toolpath.as_ref()?;
        let mut acc = 0.0;
        for seg in &tp.segs {
            let len = (seg.to.x - seg.from.x).hypot(seg.to.y - seg.from.y);
            if acc + len >= self.sim_pos_mm || len < 1e-9 {
                let t = if len < 1e-9 { 0.0 } else { (self.sim_pos_mm - acc) / len };
                let p = Vec2::new(
                    seg.from.x + (seg.to.x - seg.from.x) * t,
                    seg.from.y + (seg.to.y - seg.from.y) * t,
                );
                return Some((p, !seg.rapid));
            }
            acc += len;
        }
        tp.segs.last().map(|s| (s.to, !s.rapid))
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Don't steal keys while typing into a text field.
        if ctx.wants_keyboard_input() {
            return;
        }
        let (undo, redo, dup, del, group, ungroup) = ctx.input(|i| {
            let cmd = i.modifiers.command || i.modifiers.ctrl;
            (
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                cmd && (i.key_pressed(egui::Key::Y)
                    || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                cmd && i.key_pressed(egui::Key::D),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::G),
                cmd && i.modifiers.shift && i.key_pressed(egui::Key::G),
            )
        });
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if dup {
            self.duplicate_selected();
        }
        if del {
            self.delete_selected();
        }
        if group {
            self.group_selected();
        }
        if ungroup {
            self.ungroup_selected();
        }
    }

    fn upsert_setting(&mut self, num: u32, value: String) {
        match self.grbl_settings.iter_mut().find(|s| s.num == num) {
            Some(s) => s.value = value,
            None => {
                self.grbl_settings
                    .push(crate::grbl_settings::Setting { num, value });
                self.grbl_settings.sort_by_key(|s| s.num);
            }
        }
    }

    fn grbl_window(&mut self, ctx: &egui::Context) {
        if !self.show_grbl {
            return;
        }
        let connected = self.device.connected;
        let mut open = self.show_grbl;
        let mut write_all = false;
        egui::Window::new("⚙ Ajustes GRBL ($$)")
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(connected, egui::Button::new("🔄 Leer ($$)"))
                        .clicked()
                    {
                        self.device.query_settings();
                    }
                    if ui
                        .add_enabled(
                            connected && !self.grbl_settings.is_empty(),
                            egui::Button::new("💾 Escribir todos"),
                        )
                        .clicked()
                    {
                        write_all = true;
                    }
                    if !connected {
                        ui.weak("(conéctate a la Phecda para leer/escribir)");
                    }
                });
                ui.separator();
                if self.grbl_settings.is_empty() {
                    ui.weak("Sin datos. Pulsa «Leer ($$)» con la máquina conectada.");
                    return;
                }
                let mut write_one: Option<(u32, String)> = None;
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("grbl_grid")
                        .num_columns(4)
                        .striped(true)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            for s in &mut self.grbl_settings {
                                ui.monospace(format!("${}", s.num));
                                ui.add(
                                    egui::TextEdit::singleline(&mut s.value)
                                        .desired_width(90.0),
                                );
                                if ui
                                    .add_enabled(connected, egui::Button::new("✎"))
                                    .on_hover_text("Escribir este parámetro")
                                    .clicked()
                                {
                                    write_one = Some((s.num, s.value.clone()));
                                }
                                ui.label(crate::grbl_settings::describe(s.num));
                                ui.end_row();
                            }
                        });
                });
                if let Some((num, val)) = write_one {
                    self.device.write_setting(num, val.trim());
                    self.log(format!("> ${}={}", num, val.trim()));
                }
            });
        if write_all {
            for s in self.grbl_settings.clone() {
                self.device.write_setting(s.num, s.value.trim());
            }
            self.log(format!("> escritos {} parámetros", self.grbl_settings.len()));
        }
        self.show_grbl = open;
    }

    fn config_window(&mut self, ctx: &egui::Context) {
        if !self.show_config {
            return;
        }
        // (name, width_mm, height_mm, s_max) presets for common diode machines.
        const PRESETS: &[(&str, f64, f64, f64)] = &[
            ("Elegoo Phecda 20W", 400.0, 400.0, 1000.0),
            ("Atomstack X20 Pro", 400.0, 400.0, 1000.0),
            ("Ortur Laser Master 3", 400.0, 400.0, 1000.0),
            ("Sculpfun S9", 410.0, 420.0, 1000.0),
            ("Sculpfun S30 Pro Max", 935.0, 905.0, 1000.0),
            ("xTool D1 Pro", 430.0, 390.0, 1000.0),
            ("Genérica 300×300", 300.0, 300.0, 1000.0),
            ("Genérica 200×200", 200.0, 200.0, 1000.0),
        ];
        let mut open = self.show_config;
        let mut refit = false;
        let mut apply_profile: Option<usize> = None;
        let mut delete_profile: Option<usize> = None;
        let mut save_new = false;
        egui::Window::new("🛠 Configuración de la máquina")
            .open(&mut open)
            .default_width(360.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Perfil de máquina:");
                ui.horizontal_wrapped(|ui| {
                    for &(name, w, h, smax) in PRESETS {
                        if ui.button(name).clicked() {
                            self.doc.machine.bed_w_mm = w;
                            self.doc.machine.bed_h_mm = h;
                            self.doc.machine.s_max = smax;
                            refit = true;
                        }
                    }
                });

                // Perfiles de láser guardados por el usuario (ajustes completos).
                if !self.machines.is_empty() {
                    ui.add_space(4.0);
                    ui.label("Mis láseres:");
                    ui.horizontal_wrapped(|ui| {
                        for (i, prof) in self.machines.iter().enumerate() {
                            if ui.button(&prof.name).on_hover_text(
                                "Aplicar este láser (tamaño, origen, modo, baudios…)",
                            ).clicked() {
                                apply_profile = Some(i);
                            }
                            if ui.small_button("✕").on_hover_text("Borrar este láser").clicked() {
                                delete_profile = Some(i);
                            }
                        }
                    });
                }

                // Guardar los ajustes actuales como un láser nuevo.
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_machine_name)
                            .hint_text("Nombre del láser")
                            .desired_width(170.0),
                    );
                    let can_save = !self.new_machine_name.trim().is_empty();
                    if ui
                        .add_enabled(can_save, egui::Button::new("💾 Guardar como láser nuevo"))
                        .on_hover_text("Guarda la configuración actual con este nombre")
                        .clicked()
                    {
                        save_new = true;
                    }
                });

                ui.separator();
                let m = &mut self.doc.machine;
                egui::Grid::new("cfg_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Ancho de cama X (mm)");
                    ui.add(egui::DragValue::new(&mut m.bed_w_mm).range(10.0..=2000.0).speed(1.0));
                    ui.end_row();
                    ui.label("Alto de cama Y (mm)");
                    ui.add(egui::DragValue::new(&mut m.bed_h_mm).range(10.0..=2000.0).speed(1.0));
                    ui.end_row();
                    ui.label("S máx ($30)");
                    ui.add(egui::DragValue::new(&mut m.s_max).range(1.0..=100000.0));
                    ui.end_row();
                    ui.label("Baudios (USB)");
                    ui.add(egui::DragValue::new(&mut m.baud).range(9600..=1000000));
                    ui.end_row();
                    ui.label("Velocidad de viaje (mm/min)");
                    ui.add(egui::DragValue::new(&mut m.travel_speed).range(100.0..=60000.0));
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    ui.label("Modo láser:");
                    ui.selectable_value(&mut m.laser_mode, crate::model::LaserMode::Dynamic, "M4 dinámico");
                    ui.selectable_value(&mut m.laser_mode, crate::model::LaserMode::Constant, "M3");
                });
                ui.horizontal(|ui| {
                    ui.label("Origen:");
                    ui.selectable_value(&mut m.origin, crate::model::Origin::FrontLeft, "Frontal-izq");
                    ui.selectable_value(&mut m.origin, crate::model::Origin::TopLeft, "Superior-izq");
                });
                ui.checkbox(&mut m.optimize_travel, "Optimizar recorrido");
                ui.checkbox(&mut self.canvas_light, "Fondo blanco (lienzo claro)");
                ui.separator();
                if ui.button("🔍 Ajustar vista a la cama").clicked() {
                    refit = true;
                }
                ui.weak("Los presets ajustan tamaño de cama y S máx. «Guardar como láser nuevo» guarda toda la configuración actual (origen, modo, baudios…) y queda en «Mis láseres». Verifica siempre con tu máquina.");
            });
        // Apply deferred profile actions outside the window closure.
        if let Some(i) = apply_profile {
            if let Some(prof) = self.machines.get(i) {
                self.doc.machine = prof.settings.clone();
                refit = true;
                self.status_msg = format!("Láser «{}» aplicado", prof.name);
            }
        }
        if let Some(i) = delete_profile {
            if i < self.machines.len() {
                let name = self.machines.remove(i).name;
                match crate::machines::save(&self.machines) {
                    Ok(()) => self.status_msg = format!("Láser «{name}» borrado"),
                    Err(e) => self.status_msg = format!("Error al guardar láseres: {e}"),
                }
            }
        }
        if save_new {
            let name = self.new_machine_name.trim().to_string();
            if !name.is_empty() {
                // Reemplaza si ya existe uno con el mismo nombre.
                let prof = crate::machines::MachineProfile {
                    name: name.clone(),
                    settings: self.doc.machine.clone(),
                };
                if let Some(slot) = self.machines.iter_mut().find(|p| p.name == name) {
                    *slot = prof;
                } else {
                    self.machines.push(prof);
                }
                match crate::machines::save(&self.machines) {
                    Ok(()) => self.status_msg = format!("Láser «{name}» guardado"),
                    Err(e) => self.status_msg = format!("Error al guardar láser: {e}"),
                }
                self.new_machine_name.clear();
            }
        }
        if refit {
            self.view.initialized = false;
        }
        self.show_config = open;
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        let mut open = self.show_help;
        egui::Window::new("❓ Ayuda")
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                    ui.heading("rustburn — control láser GRBL");
                    ui.label("Aplicación nativa para láseres de diodo (Elegoo Phecda y compatibles GRBL 1.1).");
                    ui.add_space(6.0);

                    ui.strong("Lienzo");
                    ui.label("• Rueda del ratón: zoom alrededor del cursor.");
                    ui.label("• Arrastrar con botón central: desplazar la vista.");
                    ui.label("• Reglas y cuadrícula en mm (líneas cada 10 mm, mayor cada 50).");
                    ui.add_space(6.0);

                    ui.strong("Herramientas");
                    ui.label("• Seleccionar: mover, escalar (tiradores) y rotar objetos.");
                    ui.label("• Nodos: editar puntos (doble clic añade/elimina).");
                    ui.label("• Rectángulo, Elipse, Línea, Polígono, Polilínea, Texto.");
                    ui.label("• Operaciones: Unir, Restar, Intersecar, Soldar, Offset (Shift+clic añade operandos).");
                    ui.add_space(6.0);

                    ui.strong("Atajos de teclado");
                    ui.label("• Ctrl+Z / Ctrl+Y: deshacer / rehacer.");
                    ui.label("• Ctrl+D: duplicar selección.");
                    ui.label("• Ctrl+G / Ctrl+Shift+G: agrupar / desagrupar.");
                    ui.label("• Supr: eliminar selección.");
                    ui.add_space(6.0);

                    ui.strong("Organizar (panel Objeto)");
                    ui.label("• Agrupar/Desagrupar; Espejo H/V; Rotar 90°.");
                    ui.label("• Alinear (6 direcciones), Distribuir H/V, Matriz (rejilla/círculo).");
                    ui.label("• Shift+clic: añadir objetos a la selección.");
                    ui.add_space(6.0);

                    ui.strong("Edición avanzada");
                    ui.label("• Snap objetos: al mover, ajusta bordes/centros a otros objetos y a la cama.");
                    ui.label("• Operaciones: Cerrar trazado, Recortar a la cama, Suavizar nodos (bézier).");
                    ui.label("• Nodos (✎): arrastra para mover, doble clic borra un nodo o inserta en un tramo.");
                    ui.label("• Numeración/texto variable: plantilla con # (p.ej. 'Pieza ###').");
                    ui.label("• Simulación del recorrido: ▶/⏸/⏮, barra de progreso y velocidad.");
                    ui.add_space(6.0);

                    ui.strong("Cámara y Print&Cut");
                    ui.label("• Cámara USB en vivo: captura un fotograma (ffmpeg) y calíbralo (4 esquinas).");
                    ui.label("• Corrección de distorsión de lente (k1/k2) sobre la foto.");
                    ui.label("• Print&Cut (✛): añade marcas de registro, mide su posición y alinea el corte.");
                    ui.add_space(6.0);

                    ui.strong("Capas y materiales");
                    ui.label("• Cada capa tiene modo (Línea/Imagen), potencia %, velocidad y pasadas.");
                    ui.label("• Materiales: presets aplicables a la capa del objeto seleccionado.");
                    ui.add_space(6.0);

                    ui.strong("Grabado de imagen");
                    ui.label("• Ajustes: brillo, contraste, gamma, invertir.");
                    ui.label("• Tramado: Grises, Umbral, Floyd-Steinberg, Atkinson, Jarvis, Bayer.");
                    ui.label("• Vectorizar (⌁): traza la imagen a un umbral y crea contornos de corte.");
                    ui.add_space(6.0);

                    ui.strong("Máquina (GRBL)");
                    ui.label("• Conecta por USB (/dev/ttyUSB0) o WiFi (host:puerto).");
                    ui.label("• Jog, Home, fijar Origen, Encuadre (framing) a baja potencia.");
                    ui.label("• Ajustes GRBL: leer/escribir parámetros $$.");
                    ui.label("• Configuración 🛠: tamaño de cama X/Y y perfiles de otras máquinas.");
                    ui.add_space(6.0);

                    ui.strong("Flujo recomendado");
                    ui.label("1. Diseña o importa (SVG/imagen). 2. Asigna capa y parámetros.");
                    ui.label("3. Genera G-code y revisa el recorrido/tiempo. 4. Conecta, encuadra y envía.");
                    ui.add_space(6.0);
                    ui.weak("⚠ Seguridad: usa gafas adecuadas, ventilación y nunca dejes el láser sin vigilancia.");
                });
            });
        self.show_help = open;
    }

    /// Bottom status bar: layer colour palette + cursor read-out + selection info.
    fn status_bar_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            // Current layer of the selected object (for the outline highlight).
            let cur_layer = self.doc.selected.and_then(|i| self.doc.objects.get(i)).map(|o| o.layer);
            let mut assign: Option<usize> = None;
            for li in 0..self.doc.layers.len() {
                let layer = &self.doc.layers[li];
                let c = layer.color;
                let color = Color32::from_rgb(c[0], c[1], c[2]);
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                ui.painter().rect_filled(rect, 2.0, color);
                let outline = if cur_layer == Some(li) {
                    Stroke::new(2.0_f32, Color32::WHITE)
                } else {
                    Stroke::new(1.0_f32, Color32::from_gray(60))
                };
                ui.painter().rect_stroke(rect, 2.0, outline);
                let resp = resp.on_hover_text(format!(
                    "Capa {}: {} — clic para asignar la selección",
                    li + 1,
                    layer.name
                ));
                if resp.clicked() {
                    assign = Some(li);
                }
            }
            if let Some(li) = assign {
                self.assign_layer(li);
            }

            ui.separator();
            // Cursor read-out.
            match self.cursor_world {
                Some(cw) => ui.monospace(format!("X {:>7.2}  Y {:>7.2} mm", cw.x, cw.y)),
                None => ui.monospace("X   —      Y   —   mm"),
            };
            // Selection summary.
            let n = self.selected_indices().len();
            if n > 0 {
                ui.separator();
                if let Some(bb) = self.selection_bbox(&self.selected_indices()) {
                    ui.label(format!(
                        "sel: {} obj · {:.1}×{:.1} mm",
                        n,
                        bb.width(),
                        bb.height()
                    ));
                } else {
                    ui.label(format!("sel: {n} obj"));
                }
            }
            if self.tool == Tool::Measure {
                ui.separator();
                ui.label("📏 Medir: clic en 2 puntos");
            }
        });
    }

    fn array_window(&mut self, ctx: &egui::Context) {
        if !self.show_array {
            return;
        }
        let mut open = self.show_array;
        let mut apply = false;
        egui::Window::new("▦ Matriz / Array")
            .open(&mut open)
            .default_width(280.0)
            .show(ctx, |ui| {
                ui.label("Duplica la selección en rejilla o en círculo.");
                ui.separator();
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.array_circular, false, "▦ Rejilla");
                    ui.selectable_value(&mut self.array_circular, true, "◯ Circular");
                });
                ui.add_space(4.0);
                if self.array_circular {
                    egui::Grid::new("arr_circ").num_columns(2).show(ui, |ui| {
                        ui.label("Copias");
                        ui.add(egui::DragValue::new(&mut self.array_count).range(2..=200));
                        ui.end_row();
                        ui.label("Radio (mm)");
                        ui.add(egui::DragValue::new(&mut self.array_radius).range(1.0..=1000.0).speed(0.5));
                        ui.end_row();
                    });
                    ui.weak("Centro: el radio por encima de la selección.");
                } else {
                    egui::Grid::new("arr_grid").num_columns(2).show(ui, |ui| {
                        ui.label("Columnas");
                        ui.add(egui::DragValue::new(&mut self.array_cols).range(1..=100));
                        ui.end_row();
                        ui.label("Filas");
                        ui.add(egui::DragValue::new(&mut self.array_rows).range(1..=100));
                        ui.end_row();
                        ui.label("Separación X (mm)");
                        ui.add(egui::DragValue::new(&mut self.array_dx).speed(0.5));
                        ui.end_row();
                        ui.label("Separación Y (mm)");
                        ui.add(egui::DragValue::new(&mut self.array_dy).speed(0.5));
                        ui.end_row();
                    });
                }
                ui.separator();
                if ui.button("✅ Aplicar").clicked() {
                    apply = true;
                }
            });
        if apply {
            self.apply_array();
        }
        self.show_array = open;
    }

    // --- Print & Cut (registration marks) ------------------------------------

    /// Add cross-shaped registration marks at three corners of the design bbox and
    /// remember their nominal document positions for later alignment.
    fn add_registration_marks(&mut self) {
        let Some(bb) = self.doc.bbox() else {
            self.status_msg = "No hay diseño para marcar".into();
            return;
        };
        // Push the marks slightly outside the design so they don't overlap it.
        let m = 8.0;
        let marks = vec![
            Vec2::new(bb.min.x - m, bb.min.y - m), // esquina 1 (origen)
            Vec2::new(bb.max.x + m, bb.min.y - m), // esquina 2 (eje X)
            Vec2::new(bb.min.x - m, bb.max.y + m), // esquina 3 (eje Y)
        ];
        let arm = 4.0; // cross half-length, mm
        let mut polylines = Vec::new();
        for mk in &marks {
            polylines.push(crate::geometry::Polyline {
                points: vec![
                    Vec2::new(mk.x - arm, mk.y),
                    Vec2::new(mk.x + arm, mk.y),
                ],
                closed: false,
            });
            polylines.push(crate::geometry::Polyline {
                points: vec![
                    Vec2::new(mk.x, mk.y - arm),
                    Vec2::new(mk.x, mk.y + arm),
                ],
                closed: false,
            });
        }
        self.pc_marks = marks;
        self.pc_measured = self
            .pc_marks
            .iter()
            .map(|p| {
                let mach = crate::gcode::machine_to_doc(*p, &self.doc.machine);
                (mach.x, mach.y)
            })
            .collect();
        self.pc_transform = None;
        self.pc_apply = false;
        self.snapshot();
        let layer = self.selected_layer().min(self.doc.layers.len().saturating_sub(1));
        let id = self.doc.alloc_id();
        let obj = Object::new(
            id,
            "marcas de registro".into(),
            layer,
            ObjectKind::Vector(crate::model::VectorData { polylines }),
            Vec2::new(0.0, 0.0),
        );
        let idx = self.doc.add_object(obj);
        self.doc.selected = Some(idx);
        self.status_msg = "Marcas de registro añadidas (3 cruces)".into();
    }

    /// Fit the registration alignment from measured machine positions of the marks.
    fn compute_printcut_alignment(&mut self) {
        if self.pc_marks.len() < 2 || self.pc_measured.len() != self.pc_marks.len() {
            self.status_msg = "Añade marcas y mide su posición primero".into();
            return;
        }
        let src = self.pc_marks.clone();
        let dst: Vec<Vec2> = self
            .pc_measured
            .iter()
            .map(|&(mx, my)| crate::gcode::machine_to_doc(Vec2::new(mx, my), &self.doc.machine))
            .collect();
        match crate::geometry::fit_affine(&src, &dst) {
            Some(t) => {
                self.pc_transform = Some(t);
                self.pc_apply = true;
                self.status_msg = "Alineación print&cut calculada ✓".into();
            }
            None => {
                self.pc_transform = None;
                self.status_msg = "Alineación fallida (marcas colineales/duplicadas)".into();
            }
        }
    }

    fn printcut_window(&mut self, ctx: &egui::Context) {
        if !self.show_printcut {
            return;
        }
        let mut open = self.show_printcut;
        egui::Window::new("✛ Print & Cut")
            .open(&mut open)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.label("Corta sobre una impresión ya existente alineando el láser a marcas de registro.");
                ui.weak("1) Añade marcas y expórtalas/imprímelas con el diseño.");
                ui.weak("2) Coloca la hoja impresa en la cama.");
                ui.weak("3) Haz jog del cabezal a cada marca y anota su X/Y de máquina.");
                ui.weak("4) Calcula la alineación y genera el corte.");
                ui.separator();
                if ui.button("✛ Añadir marcas de registro").clicked() {
                    self.add_registration_marks();
                }
                if self.pc_marks.is_empty() {
                    ui.weak("Sin marcas todavía.");
                } else {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Marcas: diseño → medida (máquina)").strong());
                    egui::Grid::new("pc_marks_grid")
                        .num_columns(3)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Diseño (mm)");
                            ui.label("X máquina");
                            ui.label("Y máquina");
                            ui.end_row();
                            for (i, mk) in self.pc_marks.iter().enumerate() {
                                ui.label(format!("{:.1}, {:.1}", mk.x, mk.y));
                                if let Some(meas) = self.pc_measured.get_mut(i) {
                                    ui.add(egui::DragValue::new(&mut meas.0).speed(0.1).suffix(" mm"));
                                    ui.add(egui::DragValue::new(&mut meas.1).speed(0.1).suffix(" mm"));
                                }
                                ui.end_row();
                            }
                        });
                    ui.add_space(4.0);
                    if ui.button("📐 Calcular alineación").clicked() {
                        self.compute_printcut_alignment();
                    }
                    let has = self.pc_transform.is_some();
                    ui.add_enabled_ui(has, |ui| {
                        ui.checkbox(&mut self.pc_apply, "Aplicar alineación al corte");
                    });
                    if has {
                        ui.weak("La alineación se aplica al generar G-code.");
                    } else {
                        ui.weak("Aún sin alineación calculada.");
                    }
                }
            });
        self.show_printcut = open;
    }

    // --- event pump ----------------------------------------------------------
    fn pump_device(&mut self) {
        let events = self.device.poll();
        for e in events {
            match e {
                Evt::Connected(label) => self.log(format!("✓ Conectado: {label}")),
                Evt::Disconnected => self.log("✗ Desconectado"),
                Evt::ConnectError(e) => self.log(format!("✗ Error de conexión: {e}")),
                Evt::Rx(l) => {
                    if let Some((num, val)) = crate::grbl_settings::parse_line(&l) {
                        self.upsert_setting(num, val);
                    }
                    self.log(format!("< {l}"));
                }
                Evt::Tx(l) => self.log(format!("> {l}")),
                Evt::Alarm(a) => self.log(format!("⚠ ALARMA: {a}")),
                Evt::Error(e) => self.log(format!("⚠ {e}")),
                Evt::JobDone => self.log("✓ Trabajo completado"),
                Evt::Status(_) | Evt::Progress { .. } => {}
            }
        }
    }
}

impl eframe::App for RustburnApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Flush any pending preference changes on a clean close.
        let cur = self.current_prefs();
        if cur != self.saved_prefs {
            let _ = crate::prefs::save(&cur);
            self.saved_prefs = cur;
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Whole-app theme follows the light/dark canvas preference.
        let want_light = self.canvas_light;
        if ctx.style().visuals.dark_mode == want_light {
            ctx.set_visuals(if want_light {
                egui::Visuals::light()
            } else {
                egui::Visuals::dark()
            });
        }
        self.autosave_prefs();
        self.pump_device();
        self.handle_shortcuts(ctx);
        // Keep polling the worker for live status.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        // Advance the toolpath simulation.
        if self.sim_playing {
            let dt = ctx.input(|i| i.stable_dt).min(0.1) as f64;
            self.tick_simulation(dt);
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| self.toolbar_ui(ui));
        egui::SidePanel::right("right")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| self.right_panel_ui(ui));
        egui::TopBottomPanel::bottom("statusbar")
            .exact_height(26.0)
            .show(ctx, |ui| self.status_bar_ui(ui));
        egui::TopBottomPanel::bottom("console")
            .resizable(true)
            .default_height(160.0)
            .show(ctx, |ui| self.console_ui(ui));
        egui::SidePanel::left("tools")
            .resizable(false)
            .default_width(140.0)
            .show(ctx, |ui| self.tools_ui(ui));
        egui::CentralPanel::default().show(ctx, |ui| self.canvas_ui(ui));
        self.mat_test_window(ctx);
        self.grbl_window(ctx);
        self.config_window(ctx);
        self.help_window(ctx);
        self.array_window(ctx);
        self.printcut_window(ctx);
    }
}

// --- Toolbar -----------------------------------------------------------------
impl RustburnApp {
    fn toolbar_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("🗋 Nuevo proyecto")
                .on_hover_text("Vaciar y empezar de cero (Ctrl+Z para deshacer)")
                .clicked()
            {
                self.new_project();
            }
            if ui.button("📂 Abrir proyecto").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Proyecto rustburn", &["rbp", "json"])
                    .pick_file()
                {
                    match crate::project::load(&path) {
                        Ok(doc) => {
                            self.doc = doc;
                            self.textures.clear();
                            self.project_path = Some(path);
                            self.status_msg = "Proyecto cargado".into();
                        }
                        Err(e) => self.status_msg = format!("Error: {e}"),
                    }
                }
            }
            if ui.button("💾 Guardar proyecto").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Proyecto rustburn", &["rbp"])
                    .set_file_name("proyecto.rbp")
                    .save_file()
                {
                    match crate::project::save(&self.doc, &path) {
                        Ok(_) => self.status_msg = "Guardado".into(),
                        Err(e) => self.status_msg = format!("Error: {e}"),
                    }
                }
            }
            ui.separator();
            if ui.add_enabled(!self.history.is_empty(), egui::Button::new("↶"))
                .on_hover_text("Deshacer (Ctrl+Z)")
                .clicked()
            {
                self.undo();
            }
            if ui.add_enabled(!self.future.is_empty(), egui::Button::new("↷"))
                .on_hover_text("Rehacer (Ctrl+Y)")
                .clicked()
            {
                self.redo();
            }
            ui.separator();
            if ui.button("➕ Importar SVG").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("SVG", &["svg"])
                    .pick_file()
                {
                    match crate::import::load_svg(&path) {
                        Ok(kind) => {
                            let name = path
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| "svg".into());
                            self.add_imported(kind, name);
                            self.status_msg = "SVG importado".into();
                        }
                        Err(e) => self.status_msg = format!("Error SVG: {e}"),
                    }
                }
            }
            if ui.button("🖼 Importar imagen").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Imagen", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
                    .pick_file()
                {
                    match crate::import::load_image(&path) {
                        Ok(kind) => {
                            let name = path
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| "img".into());
                            self.add_imported(kind, name);
                            self.status_msg = "Imagen importada".into();
                        }
                        Err(e) => self.status_msg = format!("Error imagen: {e}"),
                    }
                }
            }
            ui.menu_button("📄 Importar PDF", |ui| {
                if ui
                    .button("✒ Como vectores (corte)")
                    .on_hover_text("Convierte los trazos del PDF a vectores (pdftocairo/inkscape).")
                    .clicked()
                {
                    ui.close_menu();
                    self.import_pdf(false);
                }
                if ui
                    .button("🖼 Como imagen (grabado)")
                    .on_hover_text("Rasteriza la 1ª página; luego puedes vectorizarla (poppler/ghostscript).")
                    .clicked()
                {
                    ui.close_menu();
                    self.import_pdf(true);
                }
            });
            ui.separator();
            if ui.button("⚙ Generar G-code").clicked() {
                self.generate_gcode();
            }
            ui.toggle_value(&mut self.show_toolpath, "👁 Recorrido");
            // Toolpath simulation controls.
            {
                let has_tp = self.toolpath.is_some();
                let total = self.sim_total_mm();
                let play_lbl = if self.sim_playing { "⏸" } else { "▶" };
                if ui.add_enabled(has_tp, egui::Button::new(play_lbl))
                    .on_hover_text("Simular recorrido")
                    .clicked()
                {
                    if self.sim_pos_mm >= total {
                        self.sim_pos_mm = 0.0;
                    }
                    self.sim_playing = !self.sim_playing;
                    self.show_toolpath = true;
                }
                if ui.add_enabled(has_tp, egui::Button::new("⏮"))
                    .on_hover_text("Reiniciar simulación")
                    .clicked()
                {
                    self.sim_pos_mm = 0.0;
                    self.sim_playing = false;
                }
                if has_tp && total > 0.0 {
                    ui.add(egui::Slider::new(&mut self.sim_pos_mm, 0.0..=total).show_value(false))
                        .on_hover_text("Progreso");
                    ui.add(egui::DragValue::new(&mut self.sim_speed).range(50.0..=20000.0).suffix(" mm/s").speed(50.0))
                        .on_hover_text("Velocidad de simulación");
                    ui.weak(format!("{:.0}%", 100.0 * self.sim_pos_mm / total));
                }
            }
            ui.toggle_value(&mut self.show_mat_test, "🔥 Test material");
            ui.toggle_value(&mut self.show_printcut, "✛ Print&Cut")
                .on_hover_text("Marcas de registro y alineación de corte sobre impresión");
            ui.toggle_value(&mut self.show_grbl, "⚙ Ajustes GRBL");
            ui.toggle_value(&mut self.show_config, "🛠 Configuración");
            ui.toggle_value(&mut self.show_help, "❓ Ayuda");
            if ui.add_enabled(!self.gcode.is_empty(), egui::Button::new("⬇ Guardar .nc"))
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("G-code", &["nc", "gcode"])
                    .set_file_name("trabajo.nc")
                    .save_file()
                {
                    let content = self.gcode.join("\n");
                    match std::fs::write(&path, content) {
                        Ok(_) => self.status_msg = "G-code guardado".into(),
                        Err(e) => self.status_msg = format!("Error: {e}"),
                    }
                }
            }
            ui.separator();
            ui.toggle_value(&mut self.snap, "🧲 Rejilla").on_hover_text("Ajustar a la cuadrícula");
            if self.snap {
                ui.add(
                    egui::DragValue::new(&mut self.snap_step)
                        .range(0.1..=50.0)
                        .speed(0.1)
                        .suffix(" mm"),
                );
            }
            ui.toggle_value(&mut self.snap_objects, "🎯 Snap objetos")
                .on_hover_text("Ajustar bordes/centros a otros objetos y a la cama");
            ui.separator();
            ui.label(egui::RichText::new(&self.status_msg).italics().weak());
        });
    }
}

// --- Right panel: layers, properties, laser control --------------------------
impl RustburnApp {
    fn right_panel_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.machine_ui(ui);
            ui.separator();
            self.layers_ui(ui);
            ui.separator();
            self.properties_ui(ui);
            ui.separator();
            self.materials_ui(ui);
            ui.separator();
            self.cam_ui(ui);
            ui.separator();
            self.laser_ui(ui);
        });
    }

    fn cam_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Cámara / foto");
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.label("Dispositivo:");
            ui.add(egui::TextEdit::singleline(&mut self.cam_device).desired_width(110.0));
        });
        ui.horizontal(|ui| {
            if ui.button("📸 Capturar cámara USB")
                .on_hover_text("Captura un fotograma del dispositivo V4L2 con ffmpeg")
                .clicked()
            {
                self.capture_cam_frame(&ctx);
            }
            if ui.button("📷 Cargar foto").clicked() {
                self.load_cam_photo(&ctx);
            }
        });
        if self.cam_photo.is_some() {
            let label = if self.calibrating {
                "… clic en 4 esquinas"
            } else {
                "🎯 Calibrar (4 esquinas)"
            };
            if ui.button(label).clicked() {
                self.calibrating = true;
                self.cal_points.clear();
            }
            ui.checkbox(&mut self.cam_show, "Mostrar superposición");
            ui.add(egui::Slider::new(&mut self.cam_opacity, 0.0..=1.0).text("Opacidad"));
            // Lens distortion correction (radial k1/k2).
            ui.collapsing("Distorsión de lente", |ui| {
                ui.weak("Corrige barril (−) o cojín (+)");
                let mut dirty = false;
                let r = ui.add(egui::Slider::new(&mut self.cam_k1, -0.5..=0.5).text("k1"));
                dirty |= r.changed();
                let r = ui.add(egui::Slider::new(&mut self.cam_k2, -0.3..=0.3).text("k2"));
                dirty |= r.changed();
                if ui.button("↺ Sin distorsión").clicked() {
                    self.cam_k1 = 0.0;
                    self.cam_k2 = 0.0;
                    dirty = true;
                }
                if dirty {
                    self.rebuild_cam_correction(&ctx);
                }
            });
            if ui.button("🗑 Quitar foto").clicked() {
                self.cam_orig = None;
                self.cam_photo = None;
                self.cam_tex = None;
                self.cam_raw_tex = None;
                self.cal_points.clear();
                self.calibrating = false;
            }
            if self.calibrating {
                ui.weak(format!("Marca la esquina {} de 4", self.cal_points.len() + 1));
                ui.weak("Orden: (0,0) → (X,0) → (X,Y) → (0,Y)");
            }
        } else {
            ui.weak("Carga una foto del tablero para alinear el diseño por perspectiva.");
        }
    }

    fn materials_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Materiales");
        ui.weak("▶ aplica a la capa del objeto seleccionado");
        let mut apply = None;
        let mut del = None;
        egui::ScrollArea::vertical()
            .max_height(140.0)
            .id_salt("materials_scroll")
            .show(ui, |ui| {
                for (i, m) in self.materials.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("▶").clicked() {
                            apply = Some(i);
                        }
                        ui.label(&m.name);
                        if ui.small_button("🗑").clicked() {
                            del = Some(i);
                        }
                    });
                }
            });
        if let Some(i) = apply {
            self.apply_material(i);
        }
        if let Some(i) = del {
            self.delete_material(i);
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_material_name)
                    .hint_text("Nombre nuevo")
                    .desired_width(140.0),
            );
            if ui.button("＋ Guardar capa").clicked() {
                self.save_current_material();
            }
        });
    }

    fn mat_test_window(&mut self, ctx: &egui::Context) {
        if !self.show_mat_test {
            return;
        }
        let mut open = self.show_mat_test;
        let mut gen = false;
        egui::Window::new("🔥 Test de material")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                let t = &mut self.mat_test;
                egui::Grid::new("mattest").num_columns(2).show(ui, |ui| {
                    ui.label("Columnas (potencia)");
                    ui.add(egui::DragValue::new(&mut t.cols).range(1..=12));
                    ui.end_row();
                    ui.label("Filas (velocidad)");
                    ui.add(egui::DragValue::new(&mut t.rows).range(1..=12));
                    ui.end_row();
                    ui.label("Potencia mín/máx %");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut t.min_power).range(0.0..=100.0));
                        ui.add(egui::DragValue::new(&mut t.max_power).range(0.0..=100.0));
                    });
                    ui.end_row();
                    ui.label("Velocidad mín/máx");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut t.min_speed).range(1.0..=20000.0));
                        ui.add(egui::DragValue::new(&mut t.max_speed).range(1.0..=20000.0));
                    });
                    ui.end_row();
                    ui.label("Celda / separación (mm)");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut t.cell_mm).range(2.0..=50.0));
                        ui.add(egui::DragValue::new(&mut t.gap_mm).range(0.0..=30.0));
                    });
                    ui.end_row();
                    ui.label("Intervalo relleno (mm)");
                    ui.add(egui::DragValue::new(&mut t.interval_mm).range(0.05..=1.0).speed(0.01));
                    ui.end_row();
                    ui.label("Origen X / Y (mm)");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut t.origin_x));
                        ui.add(egui::DragValue::new(&mut t.origin_y));
                    });
                    ui.end_row();
                });
                ui.separator();
                if ui.button("⚙ Generar test → G-code").clicked() {
                    gen = true;
                }
                ui.weak("Luego: Guardar .nc o Iniciar (conectado).");
            });
        self.show_mat_test = open;
        if gen {
            self.generate_material_test();
        }
    }

    fn machine_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Máquina");
        let m = &mut self.doc.machine;
        egui::Grid::new("machine").num_columns(2).show(ui, |ui| {
            ui.label("Cama X (mm)");
            ui.add(egui::DragValue::new(&mut m.bed_w_mm).range(10.0..=2000.0));
            ui.end_row();
            ui.label("Cama Y (mm)");
            ui.add(egui::DragValue::new(&mut m.bed_h_mm).range(10.0..=2000.0));
            ui.end_row();
            ui.label("S máx ($30)");
            ui.add(egui::DragValue::new(&mut m.s_max).range(1.0..=100000.0));
            ui.end_row();
            ui.label("Velocidad viaje");
            ui.add(egui::DragValue::new(&mut m.travel_speed).range(100.0..=60000.0));
            ui.end_row();
        });
        ui.horizontal(|ui| {
            ui.label("Modo láser:");
            ui.selectable_value(&mut m.laser_mode, crate::model::LaserMode::Dynamic, "M4 dinámico");
            ui.selectable_value(&mut m.laser_mode, crate::model::LaserMode::Constant, "M3");
        });
        ui.horizontal(|ui| {
            ui.label("Origen:");
            ui.selectable_value(&mut m.origin, crate::model::Origin::FrontLeft, "Frontal-izq");
            ui.selectable_value(&mut m.origin, crate::model::Origin::TopLeft, "Superior-izq");
        });
        ui.checkbox(&mut m.optimize_travel, "Optimizar recorrido (menos viajes)");
    }

    fn layers_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Capas");
        let mut set_mode: Option<(usize, CutMode)> = None;
        for (i, layer) in self.doc.layers.iter_mut().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgb(&mut layer.color);
                    ui.text_edit_singleline(&mut layer.name);
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut layer.visible, "Visible");
                    ui.checkbox(&mut layer.output, "Salida");
                });
                ui.horizontal(|ui| {
                    ui.label("Modo:");
                    if ui
                        .selectable_label(layer.settings.mode == CutMode::Line, "Línea")
                        .clicked()
                    {
                        set_mode = Some((i, CutMode::Line));
                    }
                    if ui
                        .selectable_label(layer.settings.mode == CutMode::Image, "Imagen")
                        .clicked()
                    {
                        set_mode = Some((i, CutMode::Image));
                    }
                });
                let s = &mut layer.settings;
                egui::Grid::new(("layer_grid", i)).num_columns(2).show(ui, |ui| {
                    ui.label("Potencia %");
                    ui.add(egui::DragValue::new(&mut s.power_pct).range(0.0..=100.0).suffix(" %"));
                    ui.end_row();
                    ui.label("Velocidad");
                    ui.add(egui::DragValue::new(&mut s.speed).range(1.0..=60000.0).suffix(" mm/min"));
                    ui.end_row();
                    if s.mode == CutMode::Line {
                        ui.label("Pasadas");
                        ui.add(egui::DragValue::new(&mut s.passes).range(1..=50));
                        ui.end_row();
                    } else {
                        ui.label("Intervalo");
                        ui.add(egui::DragValue::new(&mut s.line_interval_mm).range(0.02..=1.0).speed(0.01).suffix(" mm"));
                        ui.end_row();
                        ui.label("Overscan");
                        ui.add(egui::DragValue::new(&mut s.overscan_mm).range(0.0..=20.0).suffix(" mm"));
                        ui.end_row();
                    }
                });
                if s.mode == CutMode::Image {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Tramado:");
                        ui.selectable_value(&mut s.dither, Dither::Grayscale, "Grises");
                        ui.selectable_value(&mut s.dither, Dither::Threshold, "Umbral");
                        ui.selectable_value(&mut s.dither, Dither::FloydSteinberg, "F-S");
                        ui.selectable_value(&mut s.dither, Dither::Atkinson, "Atkinson");
                        ui.selectable_value(&mut s.dither, Dither::Jarvis, "Jarvis");
                        ui.selectable_value(&mut s.dither, Dither::Ordered, "Bayer");
                    });
                }
            });
        }
        if let Some((i, mode)) = set_mode {
            self.doc.layers[i].settings.mode = mode;
        }
        if ui.button("➕ Añadir capa").clicked() {
            let n = self.doc.layers.len();
            let colors = [[200, 30, 30], [30, 90, 220], [20, 160, 60], [210, 150, 20]];
            self.doc
                .layers
                .push(crate::model::Layer::new(format!("Capa {}", n + 1), colors[n % 4]));
        }
    }

    fn properties_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Objeto");
        let Some(idx) = self.doc.selected else {
            ui.weak("Ningún objeto seleccionado.");
            return;
        };
        if idx >= self.doc.objects.len() {
            self.doc.selected = None;
            return;
        }
        let nlayers = self.doc.layers.len();
        let lock = self.lock_ratio;
        let mut began = false;
        let mut do_delete = false;
        let mut do_dup = false;
        let mut img_dirty = false;
        let mut do_trace = false;
        let obj_id = self.doc.objects[idx].id;

        {
            let obj = &mut self.doc.objects[idx];
            ui.label(format!("Nombre: {}", obj.name));
            let (bw, bh) = obj
                .local_bbox()
                .map(|b| (b.width().max(1e-6), b.height().max(1e-6)))
                .unwrap_or((1.0, 1.0));

            egui::Grid::new("obj_props").num_columns(2).show(ui, |ui| {
                ui.label("X (mm)");
                let r = ui.add(egui::DragValue::new(&mut obj.pos.x).speed(0.5));
                began |= r.drag_started() || r.gained_focus();
                ui.end_row();
                ui.label("Y (mm)");
                let r = ui.add(egui::DragValue::new(&mut obj.pos.y).speed(0.5));
                began |= r.drag_started() || r.gained_focus();
                ui.end_row();

                let mut w = bw * obj.scale.x;
                let mut h = bh * obj.scale.y;
                ui.label("Ancho (mm)");
                let r = ui.add(egui::DragValue::new(&mut w).speed(0.3).range(0.1..=2000.0));
                began |= r.drag_started() || r.gained_focus();
                if r.changed() {
                    let new_sx = (w / bw).max(1e-4);
                    if lock {
                        let f = new_sx / obj.scale.x.max(1e-9);
                        obj.scale.y *= f;
                    }
                    obj.scale.x = new_sx;
                }
                ui.end_row();
                ui.label("Alto (mm)");
                let r = ui.add(egui::DragValue::new(&mut h).speed(0.3).range(0.1..=2000.0));
                began |= r.drag_started() || r.gained_focus();
                if r.changed() {
                    let new_sy = (h / bh).max(1e-4);
                    if lock {
                        let f = new_sy / obj.scale.y.max(1e-9);
                        obj.scale.x *= f;
                    }
                    obj.scale.y = new_sy;
                }
                ui.end_row();
                ui.label("Rotación");
                let r = ui.add(
                    egui::DragValue::new(&mut obj.rotation)
                        .speed(0.5)
                        .suffix("°"),
                );
                began |= r.drag_started() || r.gained_focus();
                ui.end_row();
            });

            ui.horizontal(|ui| {
                ui.label("Capa:");
                egui::ComboBox::from_id_salt("obj_layer")
                    .selected_text(format!("Capa {}", obj.layer + 1))
                    .show_ui(ui, |ui| {
                        for l in 0..nlayers {
                            let r = ui.selectable_value(&mut obj.layer, l, format!("Capa {}", l + 1));
                            began |= r.clicked();
                        }
                    });
            });
            if let ObjectKind::Image(img) = &mut obj.kind {
                ui.weak(format!(
                    "{}×{} px · origen {:.1}×{:.1} mm",
                    img.width_px, img.height_px, img.width_mm, img.height_mm
                ));
                ui.separator();
                ui.label("Ajustes de imagen");
                let a = &mut img.adjust;
                egui::Grid::new("img_adj").num_columns(2).show(ui, |ui| {
                    ui.label("Brillo");
                    let r = ui.add(egui::Slider::new(&mut a.brightness, -100.0..=100.0));
                    began |= r.drag_started();
                    img_dirty |= r.changed();
                    ui.end_row();
                    ui.label("Contraste");
                    let r = ui.add(egui::Slider::new(&mut a.contrast, -100.0..=100.0));
                    began |= r.drag_started();
                    img_dirty |= r.changed();
                    ui.end_row();
                    ui.label("Gamma");
                    let r = ui.add(egui::Slider::new(&mut a.gamma, 0.1..=5.0).logarithmic(true));
                    began |= r.drag_started();
                    img_dirty |= r.changed();
                    ui.end_row();
                });
                let r = ui.checkbox(&mut a.invert, "Invertir");
                began |= r.clicked();
                img_dirty |= r.changed();
                if ui.button("↺ Restablecer ajustes").clicked() {
                    *a = Default::default();
                    began = true;
                    img_dirty = true;
                }
                ui.separator();
                ui.label(egui::RichText::new("Vectorizar (trazar)").strong());
                ui.weak("Convierte la imagen en contornos de corte");
                egui::Grid::new("trace_opts").num_columns(2).show(ui, |ui| {
                    ui.label("Umbral");
                    ui.add(egui::Slider::new(&mut self.trace_threshold, 1..=254));
                    ui.end_row();
                    ui.label("Suavizado");
                    ui.add(
                        egui::Slider::new(&mut self.trace_simplify, 0.0..=2.0).suffix(" mm"),
                    );
                    ui.end_row();
                });
                ui.checkbox(&mut self.trace_invert, "Invertir (trazar lo claro)");
                if ui
                    .button("⌁ Vectorizar imagen")
                    .on_hover_text("Traza los bordes a este umbral y crea un objeto vectorial")
                    .clicked()
                {
                    do_trace = true;
                }
            }
        }
        if do_trace {
            self.trace_selected_image(idx);
        }
        if img_dirty {
            self.textures.remove(&obj_id);
        }

        if began {
            // Capture pre-edit state once per interaction (first frame has ~0 delta).
            self.history.push(self.doc.clone());
            if self.history.len() > 80 {
                self.history.remove(0);
            }
            self.future.clear();
        }

        ui.checkbox(&mut self.lock_ratio, "Mantener proporción");

        // --- Organizar: agrupar, espejo, rotar, alinear ---
        let mut act: Option<ArrangeAct> = None;
        ui.separator();
        ui.label(egui::RichText::new("Organizar").strong());
        let grouped = self.any_grouped_selected();
        let multi_sel = self.selected_indices().len() >= 2;
        ui.horizontal(|ui| {
            if ui.add_enabled(multi_sel, egui::Button::new("🔗 Agrupar"))
                .on_hover_text("Ctrl+G · agrupa la selección (Shift+clic añade objetos)")
                .clicked()
            {
                act = Some(ArrangeAct::Group);
            }
            if ui.add_enabled(grouped, egui::Button::new("✂ Desagrupar"))
                .on_hover_text("Ctrl+Shift+G")
                .clicked()
            {
                act = Some(ArrangeAct::Ungroup);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("⬌ Espejo H").clicked() {
                act = Some(ArrangeAct::MirrorH);
            }
            if ui.button("⬍ Espejo V").clicked() {
                act = Some(ArrangeAct::MirrorV);
            }
            if ui.button("⟳ 90°").clicked() {
                act = Some(ArrangeAct::Rotate90);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Alinear:");
            if ui.add_enabled(multi_sel, egui::Button::new("⬅")).on_hover_text("Izquierda").clicked() {
                act = Some(ArrangeAct::Align(0, -1));
            }
            if ui.add_enabled(multi_sel, egui::Button::new("⬌")).on_hover_text("Centro X").clicked() {
                act = Some(ArrangeAct::Align(0, 0));
            }
            if ui.add_enabled(multi_sel, egui::Button::new("➡")).on_hover_text("Derecha").clicked() {
                act = Some(ArrangeAct::Align(0, 1));
            }
            if ui.add_enabled(multi_sel, egui::Button::new("⬆")).on_hover_text("Arriba").clicked() {
                act = Some(ArrangeAct::Align(1, -1));
            }
            if ui.add_enabled(multi_sel, egui::Button::new("⬍")).on_hover_text("Centro Y").clicked() {
                act = Some(ArrangeAct::Align(1, 0));
            }
            if ui.add_enabled(multi_sel, egui::Button::new("⬇")).on_hover_text("Abajo").clicked() {
                act = Some(ArrangeAct::Align(1, 1));
            }
        });
        let multi3 = self.selected_indices().len() >= 3;
        ui.horizontal(|ui| {
            ui.label("Distribuir:");
            if ui.add_enabled(multi3, egui::Button::new("↔ H")).on_hover_text("Espaciado horizontal uniforme").clicked() {
                act = Some(ArrangeAct::Distribute(0));
            }
            if ui.add_enabled(multi3, egui::Button::new("↕ V")).on_hover_text("Espaciado vertical uniforme").clicked() {
                act = Some(ArrangeAct::Distribute(1));
            }
            if ui.button("▦ Matriz…").on_hover_text("Duplicar en rejilla o círculo").clicked() {
                act = Some(ArrangeAct::OpenArray);
            }
        });

        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("⧉ Duplicar").clicked() {
                do_dup = true;
            }
            if ui.button("🗑 Eliminar").clicked() {
                do_delete = true;
            }
        });
        match act {
            Some(ArrangeAct::Group) => self.group_selected(),
            Some(ArrangeAct::Ungroup) => self.ungroup_selected(),
            Some(ArrangeAct::MirrorH) => self.mirror_selected(true),
            Some(ArrangeAct::MirrorV) => self.mirror_selected(false),
            Some(ArrangeAct::Rotate90) => self.rotate90_selected(),
            Some(ArrangeAct::Align(ax, md)) => self.align_selected(ax, md),
            Some(ArrangeAct::Distribute(ax)) => self.distribute_selected(ax),
            Some(ArrangeAct::OpenArray) => self.show_array = true,
            None => {}
        }
        if do_dup {
            self.duplicate_selected();
        }
        if do_delete {
            self.delete_selected();
        }
    }

    fn laser_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Láser (GRBL)");

        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.conn_kind, ConnKind::Usb, "USB");
            ui.selectable_value(&mut self.conn_kind, ConnKind::Wifi, "WiFi");
            if ui.small_button("⟳").on_hover_text("Refrescar puertos").clicked() {
                self.ports_cache = crate::device::list_serial_ports();
            }
        });
        match self.conn_kind {
            ConnKind::Usb => {
                ui.horizontal(|ui| {
                    ui.label("Puerto:");
                    ui.text_edit_singleline(&mut self.port);
                });
                if !self.ports_cache.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        for p in self.ports_cache.clone() {
                            if ui.small_button(&p).clicked() {
                                self.port = p;
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Baudios:");
                    ui.add(egui::DragValue::new(&mut self.doc.machine.baud).range(9600..=1000000));
                });
            }
            ConnKind::Wifi => {
                ui.horizontal(|ui| {
                    ui.label("Host:");
                    ui.text_edit_singleline(&mut self.host);
                });
                ui.horizontal(|ui| {
                    ui.label("Puerto TCP:");
                    ui.add(egui::DragValue::new(&mut self.tcp_port).range(1..=65535));
                });
            }
        }

        ui.horizontal(|ui| {
            if self.device.connected {
                if ui.button("Desconectar").clicked() {
                    self.device.disconnect();
                }
            } else if ui.button("🔌 Conectar").clicked() {
                let target = match self.conn_kind {
                    ConnKind::Usb => ConnTarget::Serial {
                        path: self.port.clone(),
                        baud: self.doc.machine.baud,
                    },
                    ConnKind::Wifi => ConnTarget::Tcp {
                        host: self.host.clone(),
                        port: self.tcp_port,
                    },
                };
                self.device.connect(target);
            }
            let st = &self.device.status;
            let color = match st.state.as_str() {
                "Idle" => Color32::from_rgb(60, 180, 75),
                "Run" | "Jog" => Color32::from_rgb(60, 120, 230),
                "Alarm" => Color32::from_rgb(220, 60, 60),
                "Hold" => Color32::from_rgb(220, 160, 40),
                _ => Color32::GRAY,
            };
            ui.colored_label(color, if st.state.is_empty() { "—" } else { &st.state });
        });
        if let Some((x, y, z)) = self.device.status.mpos {
            ui.weak(format!("MPos  X{:.2}  Y{:.2}  Z{:.2}", x, y, z));
        }

        ui.add_space(4.0);
        ui.label("Mover (jog)");
        ui.horizontal(|ui| {
            ui.label("Paso:");
            for s in [0.1, 1.0, 10.0, 50.0] {
                ui.selectable_value(&mut self.jog_step, s, format!("{s}"));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Avance:");
            ui.add(egui::DragValue::new(&mut self.jog_feed).range(10.0..=20000.0).suffix(" mm/min"));
        });
        let enabled = self.device.connected;
        egui::Grid::new("jogpad").num_columns(3).show(ui, |ui| {
            ui.label("");
            if ui.add_enabled(enabled, egui::Button::new("↑ Y+")).clicked() {
                self.device.jog(0.0, self.jog_step, self.jog_feed);
            }
            ui.label("");
            ui.end_row();
            if ui.add_enabled(enabled, egui::Button::new("← X-")).clicked() {
                self.device.jog(-self.jog_step, 0.0, self.jog_feed);
            }
            if ui.add_enabled(enabled, egui::Button::new("⌂ Home")).clicked() {
                self.device.home();
            }
            if ui.add_enabled(enabled, egui::Button::new("→ X+")).clicked() {
                self.device.jog(self.jog_step, 0.0, self.jog_feed);
            }
            ui.end_row();
            ui.label("");
            if ui.add_enabled(enabled, egui::Button::new("↓ Y-")).clicked() {
                self.device.jog(0.0, -self.jog_step, self.jog_feed);
            }
            if ui.add_enabled(enabled, egui::Button::new("⦿ Origen")).clicked() {
                self.device.set_zero();
            }
            ui.end_row();
        });

        ui.add_space(6.0);
        ui.separator();
        ui.label(egui::RichText::new("Salida del trabajo").strong());
        ui.checkbox(&mut self.cut_selected, "Cortar solo gráficos seleccionados");
        ui.checkbox(&mut self.use_job_origin, "Usar origen de la pieza (coords. relativas)");
        ui.horizontal(|ui| {
            ui.label("Origen del trabajo:");
            ui.add_enabled_ui(self.use_job_origin, |ui| {
                egui::Grid::new("job_origin_grid").spacing([2.0, 2.0]).show(ui, |ui| {
                    for row in 0u8..3 {
                        for col in 0u8..3 {
                            let sel = self.job_origin == (col, row);
                            let lbl = if sel { "◉" } else { "○" };
                            if ui.selectable_label(sel, lbl).clicked() {
                                self.job_origin = (col, row);
                            }
                        }
                        ui.end_row();
                    }
                });
            });
            if self.use_job_origin {
                if let Some(p) = self.job_origin_point() {
                    ui.weak(format!("({:.1}, {:.1})", p.x, p.y));
                }
            }
        });

        ui.add_space(6.0);
        ui.label("Encuadre (framing)");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.frame_power).range(0.0..=100.0).prefix("P ").suffix(" %"));
            ui.add(egui::DragValue::new(&mut self.frame_speed).range(100.0..=20000.0).suffix(" mm/min"));
            if ui.add_enabled(enabled, egui::Button::new("▭ Trazar")).clicked() {
                if let Some(lines) = crate::gcode::frame(&self.doc, self.frame_power, self.frame_speed) {
                    self.device.send_job(lines);
                } else {
                    self.status_msg = "No hay diseño para encuadrar".into();
                }
            }
        });

        ui.add_space(6.0);
        ui.separator();
        ui.horizontal(|ui| {
            let can_send = enabled && !self.gcode.is_empty();
            if ui.add_enabled(can_send, egui::Button::new("▶ Iniciar")).clicked() {
                self.device.send_job(self.gcode.clone());
                self.log(format!("Enviando trabajo: {} líneas", self.gcode.len()));
            }
            if ui.add_enabled(enabled, egui::Button::new("⏸ Pausa")).clicked() {
                self.device.feed_hold();
            }
            if ui.add_enabled(enabled, egui::Button::new("⏵ Reanudar")).clicked() {
                self.device.resume();
            }
            if ui.add_enabled(enabled, egui::Button::new("⏹ Parar")).clicked() {
                self.device.abort();
            }
        });
        if !self.gcode_msg.is_empty() {
            ui.weak(&self.gcode_msg);
        }
        if self.device.job_total > 0 {
            let frac = self.device.job_done as f32 / self.device.job_total as f32;
            ui.add(egui::ProgressBar::new(frac).text(format!(
                "{} / {}",
                self.device.job_done, self.device.job_total
            )));
        }
    }
}

// --- Console -----------------------------------------------------------------
impl RustburnApp {
    fn console_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Consola");
            if ui.small_button("Limpiar").clicked() {
                self.console.clear();
            }
        });
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .max_height(ui.available_height() - 30.0)
            .show(ui, |ui| {
                for line in &self.console {
                    ui.monospace(line);
                }
            });
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.console_input)
                    .desired_width(ui.available_width() - 60.0)
                    .hint_text("Comando GRBL (p.ej. $$, G0 X10)…"),
            );
            let send = ui.button("Enviar").clicked()
                || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if send && !self.console_input.trim().is_empty() {
                let cmd = self.console_input.trim().to_string();
                self.device.send_line(cmd.clone());
                self.console_input.clear();
            }
        });
    }
}

// --- Tools -------------------------------------------------------------------
impl RustburnApp {
    fn tools_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Herramientas");
        let tools = [
            (Tool::Select, "🖱 Seleccionar"),
            (Tool::Node, "✎ Nodos"),
            (Tool::Rect, "▭ Rectángulo"),
            (Tool::Ellipse, "⬭ Elipse"),
            (Tool::Line, "／ Línea"),
            (Tool::Polygon, "⬡ Polígono"),
            (Tool::Polyline, "📈 Polilínea"),
            (Tool::Text, "🅣 Texto"),
            (Tool::Measure, "📏 Medir"),
        ];
        for (t, label) in tools {
            if ui
                .selectable_label(self.tool == t, label)
                .clicked()
            {
                self.tool = t;
                self.poly_points.clear();
                self.draft = None;
            }
        }
        ui.separator();
        match self.tool {
            Tool::Polygon => {
                ui.horizontal(|ui| {
                    ui.label("Lados:");
                    ui.add(egui::DragValue::new(&mut self.polygon_sides).range(3..=20));
                });
            }
            Tool::Polyline => {
                ui.weak("Clic para añadir puntos.\nEnter = terminar\nEsc = cancelar\nC = cerrar");
                if !self.poly_points.is_empty()
                    && ui.button("✓ Terminar").clicked()
                {
                    self.finish_polyline(false);
                }
            }
            Tool::Text => {
                ui.label("Texto:");
                ui.add(
                    egui::TextEdit::multiline(&mut self.text_content)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    ui.label("Fuente:");
                    let shown = if self.text_font.is_empty() {
                        "Automática".to_string()
                    } else {
                        self.text_font.clone()
                    };
                    egui::ComboBox::from_id_salt("text_font_pick")
                        .selected_text(shown)
                        .width(190.0)
                        .show_ui(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                                ui.selectable_value(&mut self.text_font, String::new(), "Automática");
                                for i in 0..self.font_families.len() {
                                    let fam = self.font_families[i].clone();
                                    ui.selectable_value(&mut self.text_font, fam.clone(), fam);
                                }
                            });
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Altura:");
                    ui.add(
                        egui::DragValue::new(&mut self.text_height)
                            .range(1.0..=500.0)
                            .suffix(" mm"),
                    );
                });
                ui.separator();
                ui.checkbox(&mut self.num_series, "Numeración / texto variable (serie)");
                if self.num_series {
                    ui.weak("Usa # como hueco del número (p.ej. 'Pieza ###').");
                    egui::Grid::new("num_series").num_columns(2).show(ui, |ui| {
                        ui.label("Desde");
                        ui.add(egui::DragValue::new(&mut self.num_start));
                        ui.end_row();
                        ui.label("Cantidad");
                        ui.add(egui::DragValue::new(&mut self.num_count).range(1..=999));
                        ui.end_row();
                        ui.label("Incremento");
                        ui.add(egui::DragValue::new(&mut self.num_step).range(-999..=999));
                        ui.end_row();
                        ui.label("Separación (mm)");
                        ui.add(egui::DragValue::new(&mut self.num_spacing).range(1.0..=500.0).speed(0.5));
                        ui.end_row();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Disposición:");
                        ui.selectable_value(&mut self.num_horizontal, false, "Vertical");
                        ui.selectable_value(&mut self.num_horizontal, true, "Horizontal");
                    });
                    let last = self.num_start + self.num_step * (self.num_count.max(1) as i64 - 1);
                    ui.weak(format!(
                        "Ej.: «{}» … «{}»",
                        Self::expand_template(&self.text_content, self.num_start),
                        Self::expand_template(&self.text_content, last)
                    ));
                    ui.weak("Clic en el lienzo coloca toda la serie.");
                } else {
                    ui.weak("Clic en el lienzo para colocarlo.");
                }
            }
            Tool::Node => {
                ui.weak(
                    "Arrastra un nodo para moverlo.\nDoble clic en un nodo = borrar\nDoble clic en un segmento = insertar",
                );
            }
            _ => {
                if self.tool != Tool::Select {
                    ui.weak("Arrastra en el lienzo para dibujar.");
                }
            }
        }

        ui.separator();
        ui.heading("Operaciones");
        ui.weak("Shift+clic: añadir operandos");
        if !self.multi.is_empty() {
            ui.weak(format!("{} operando(s) extra", self.multi.len()));
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("∪ Unir").clicked() {
                self.boolean_selected(crate::geom_ops::BoolOp::Union);
            }
            if ui.button("− Restar").clicked() {
                self.boolean_selected(crate::geom_ops::BoolOp::Difference);
            }
            if ui.button("∩ Intersecar").clicked() {
                self.boolean_selected(crate::geom_ops::BoolOp::Intersect);
            }
            if ui.button("⊕ Soldar")
                .on_hover_text("Funde contornos solapados en uno (ideal para soldar letras de texto cursivo)")
                .clicked()
            {
                self.weld_selected();
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.offset_dist)
                    .speed(0.1)
                    .range(-50.0..=50.0)
                    .suffix(" mm"),
            );
            if ui.button("⟳ Offset").clicked() {
                self.offset_selected(self.offset_dist);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("⊏ Cerrar trazado")
                .on_hover_text("Cierra los trazados abiertos del objeto seleccionado")
                .clicked()
            {
                self.close_paths_selected();
            }
            if ui.button("✂ Recortar a la cama")
                .on_hover_text("Elimina la geometría fuera del área de trabajo")
                .clicked()
            {
                self.clip_to_bed();
            }
            if ui.button("〰 Suavizar nodos")
                .on_hover_text("Redondea las esquinas con curvas tipo bézier (spline Catmull-Rom) a través de los nodos")
                .clicked()
            {
                self.smooth_selected();
            }
        });
        ui.weak("Nodos: arrastra para mover · doble clic en un nodo lo borra · doble clic en un tramo inserta uno");
    }

    /// Replace each polyline of the selection with a Catmull-Rom spline through its
    /// nodes, giving bézier-like rounded curves. Closed paths wrap; open paths pin
    /// their endpoints.
    fn smooth_selected(&mut self) {
        let idx = self.selected_indices();
        if idx.is_empty() {
            self.status_msg = "Selecciona un objeto vectorial".into();
            return;
        }
        let mut changed = 0;
        self.snapshot();
        for &i in &idx {
            if let Some(o) = self.doc.objects.get_mut(i) {
                if let ObjectKind::Vector(v) = &mut o.kind {
                    for pl in &mut v.polylines {
                        if pl.points.len() >= 3 {
                            pl.points = catmull_rom(&pl.points, pl.closed, 8);
                            changed += 1;
                        }
                    }
                }
            }
        }
        self.status_msg = if changed > 0 {
            format!("Suavizados {changed} trazado(s)")
        } else {
            "Nada que suavizar (mínimo 3 nodos)".into()
        };
    }

    /// Create a vector object from local polylines placed at `pos`, select it.
    fn add_vector(&mut self, polylines: Vec<crate::geometry::Polyline>, pos: Vec2, name: &str) {
        if polylines.is_empty() {
            return;
        }
        self.snapshot();
        let layer = self.selected_layer().min(self.doc.layers.len().saturating_sub(1));
        let id = self.doc.alloc_id();
        let obj = Object::new(
            id,
            name.to_string(),
            layer,
            ObjectKind::Vector(crate::model::VectorData { polylines }),
            pos,
        );
        let idx = self.doc.add_object(obj);
        self.doc.selected = Some(idx);
    }

    /// A vector object's polylines transformed into document (mm) coordinates.
    /// Close every open polyline of the selected vector object(s).
    fn close_paths_selected(&mut self) {
        let idx = self.selected_indices();
        if idx.is_empty() {
            return;
        }
        let mut changed = 0;
        self.snapshot();
        for &i in &idx {
            if let Some(o) = self.doc.objects.get_mut(i) {
                if let ObjectKind::Vector(v) = &mut o.kind {
                    for pl in &mut v.polylines {
                        if !pl.closed && pl.points.len() >= 3 {
                            pl.closed = true;
                            changed += 1;
                        }
                    }
                }
            }
        }
        self.status_msg = if changed > 0 {
            format!("Cerrados {changed} trazado(s)")
        } else {
            "No hay trazados abiertos".into()
        };
    }

    /// Clip the selected vector object(s) to the bed rectangle (drops anything
    /// outside the work area).
    fn clip_to_bed(&mut self) {
        let idx = self.selected_indices();
        if idx.is_empty() {
            self.status_msg = "Selecciona un objeto".into();
            return;
        }
        let (bw, bh) = (self.doc.machine.bed_w_mm, self.doc.machine.bed_h_mm);
        let bed = vec![crate::geometry::Polyline {
            points: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(bw, 0.0),
                Vec2::new(bw, bh),
                Vec2::new(0.0, bh),
            ],
            closed: true,
        }];
        self.snapshot();
        let mut clipped = 0;
        for &i in &idx {
            let pls = self.obj_doc_polylines(i);
            if pls.is_empty() {
                continue;
            }
            let result = crate::geom_ops::boolean(&pls, &bed, crate::geom_ops::BoolOp::Intersect);
            if let Some(o) = self.doc.objects.get_mut(i) {
                o.kind = ObjectKind::Vector(crate::model::VectorData { polylines: result });
                o.pos = Vec2::new(0.0, 0.0);
                o.rotation = 0.0;
                o.scale = Vec2::new(1.0, 1.0);
                clipped += 1;
            }
        }
        self.status_msg = format!("Recortados {clipped} objeto(s) a la cama");
    }

    /// Trace the selected image object into a vector object, overlaid with the
    /// same transform so it registers exactly on top of the engraving.
    fn trace_selected_image(&mut self, idx: usize) {
        let Some(obj) = self.doc.objects.get(idx) else {
            return;
        };
        let ObjectKind::Image(img) = &obj.kind else {
            self.status_msg = "Selecciona una imagen para vectorizar".into();
            return;
        };
        let proc = img.processed_gray();
        let (w, h) = (img.width_px as usize, img.height_px as usize);
        let pls = crate::trace::trace_bitmap(
            &proc,
            w,
            h,
            self.trace_threshold,
            self.trace_invert,
            img.width_mm,
            img.height_mm,
            self.trace_simplify,
        );
        if pls.is_empty() {
            self.status_msg = "El trazado no encontró contornos (ajusta el umbral)".into();
            return;
        }
        let n = pls.len();
        let (pos, rot, scale, layer) = (obj.pos, obj.rotation, obj.scale, obj.layer);
        self.snapshot();
        let id = self.doc.alloc_id();
        let mut vobj = Object::new(
            id,
            "vectorizado".into(),
            layer,
            ObjectKind::Vector(crate::model::VectorData { polylines: pls }),
            pos,
        );
        vobj.rotation = rot;
        vobj.scale = scale;
        let new_idx = self.doc.add_object(vobj);
        self.doc.selected = Some(new_idx);
        self.status_msg = format!("Imagen vectorizada: {n} contorno(s)");
    }

    fn obj_doc_polylines(&self, idx: usize) -> Vec<crate::geometry::Polyline> {
        let Some(o) = self.doc.objects.get(idx) else {
            return Vec::new();
        };
        let ObjectKind::Vector(v) = &o.kind else {
            return Vec::new();
        };
        let m = o.matrix();
        v.polylines
            .iter()
            .map(|pl| crate::geometry::Polyline {
                points: pl.points.iter().map(|p| m.apply(*p)).collect(),
                closed: pl.closed,
            })
            .collect()
    }

    fn operands(&self) -> Vec<usize> {
        let mut ops: Vec<usize> = Vec::new();
        if let Some(s) = self.doc.selected {
            ops.push(s);
        }
        for &i in &self.multi {
            if !ops.contains(&i) {
                ops.push(i);
            }
        }
        ops.retain(|&i| matches!(self.doc.objects.get(i).map(|o| &o.kind), Some(ObjectKind::Vector(_))));
        ops
    }

    fn push_vector_object(&mut self, polylines: Vec<crate::geometry::Polyline>, layer: usize, name: &str) {
        let id = self.doc.alloc_id();
        let obj = Object::new(
            id,
            name.to_string(),
            layer.min(self.doc.layers.len().saturating_sub(1)),
            ObjectKind::Vector(crate::model::VectorData { polylines }),
            Vec2::new(0.0, 0.0),
        );
        let idx = self.doc.add_object(obj);
        self.doc.selected = Some(idx);
    }

    fn boolean_selected(&mut self, op: crate::geom_ops::BoolOp) {
        let ops = self.operands();
        if ops.len() < 2 {
            self.status_msg = "Selecciona 2+ objetos vectoriales (Shift+clic)".into();
            return;
        }
        let subj = self.obj_doc_polylines(ops[0]);
        let mut clip = Vec::new();
        for &i in &ops[1..] {
            clip.extend(self.obj_doc_polylines(i));
        }
        let result = crate::geom_ops::boolean(&subj, &clip, op);
        if result.is_empty() {
            self.status_msg = "Resultado vacío".into();
            return;
        }
        let layer = self.doc.objects[ops[0]].layer;
        self.snapshot();
        let mut sorted = ops.clone();
        sorted.sort_unstable();
        for &i in sorted.iter().rev() {
            if i < self.doc.objects.len() {
                self.doc.objects.remove(i);
            }
        }
        self.multi.clear();
        self.doc.selected = None;
        self.push_vector_object(result, layer, "booleana");
        self.status_msg = "Operación booleana aplicada".into();
    }

    fn weld_selected(&mut self) {
        let Some(sel) = self.doc.selected else {
            return;
        };
        let pls = self.obj_doc_polylines(sel);
        if pls.is_empty() {
            return;
        }
        let result = crate::geom_ops::weld(&pls);
        if result.is_empty() {
            self.status_msg = "Resultado vacío".into();
            return;
        }
        self.snapshot();
        if let Some(o) = self.doc.objects.get_mut(sel) {
            o.kind = ObjectKind::Vector(crate::model::VectorData { polylines: result });
            o.pos = Vec2::new(0.0, 0.0);
            o.rotation = 0.0;
            o.scale = Vec2::new(1.0, 1.0);
        }
        self.status_msg = "Soldado".into();
    }

    fn offset_selected(&mut self, dist: f64) {
        let Some(sel) = self.doc.selected else {
            self.status_msg = "Selecciona un objeto".into();
            return;
        };
        let pls = self.obj_doc_polylines(sel);
        if pls.is_empty() {
            self.status_msg = "El objeto no es vectorial".into();
            return;
        }
        let result = crate::geom_ops::offset(&pls, dist);
        if result.is_empty() {
            self.status_msg = "Offset vacío (¿inset demasiado grande?)".into();
            return;
        }
        let layer = self.doc.objects[sel].layer;
        self.snapshot();
        self.push_vector_object(result, layer, "offset");
        self.status_msg = "Offset aplicado".into();
    }

    fn apply_material(&mut self, idx: usize) {
        let Some(mat) = self.materials.get(idx).cloned() else {
            return;
        };
        let layer = self.selected_layer();
        self.snapshot();
        if let Some(l) = self.doc.layers.get_mut(layer) {
            l.settings = mat.settings.clone();
        }
        self.status_msg = format!("'{}' aplicado a capa {}", mat.name, layer + 1);
    }

    fn save_current_material(&mut self) {
        let name = self.new_material_name.trim().to_string();
        if name.is_empty() {
            self.status_msg = "Pon un nombre al material".into();
            return;
        }
        let layer = self.selected_layer();
        let settings = match self.doc.layers.get(layer) {
            Some(l) => l.settings.clone(),
            None => return,
        };
        self.materials.push(crate::materials::Material {
            name,
            thickness_mm: 0.0,
            notes: String::new(),
            settings,
        });
        match crate::materials::save(&self.materials) {
            Ok(_) => self.status_msg = "Material guardado".into(),
            Err(e) => self.status_msg = format!("Error guardando: {e}"),
        }
        self.new_material_name.clear();
    }

    fn delete_material(&mut self, idx: usize) {
        if idx < self.materials.len() {
            self.materials.remove(idx);
            let _ = crate::materials::save(&self.materials);
        }
    }

    fn load_cam_photo(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Imagen", &["png", "jpg", "jpeg", "webp", "bmp"])
            .pick_file()
        {
            match self.set_cam_photo(&path, ctx) {
                Ok(_) => self.status_msg = "Foto cargada. Pulsa Calibrar.".into(),
                Err(e) => self.status_msg = format!("Error foto: {e}"),
            }
        }
    }

    /// Grab a single frame from the V4L2 device via ffmpeg, then load it as the
    /// board photo for perspective calibration.
    fn capture_cam_frame(&mut self, ctx: &egui::Context) {
        let dev = self.cam_device.trim().to_string();
        if dev.is_empty() {
            self.status_msg = "Indica el dispositivo (p.ej. /dev/video0)".into();
            return;
        }
        let out = std::env::temp_dir().join(format!(
            "rustburn_cam_{}.png",
            std::process::id()
        ));
        // -f v4l2 reads the webcam; -frames:v 1 grabs one frame; -y overwrites.
        let result = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-f", "v4l2", "-i"])
            .arg(&dev)
            .args(["-frames:v", "1", "-y"])
            .arg(&out)
            .output();
        match result {
            Ok(o) if o.status.success() && out.exists() => match self.set_cam_photo(&out, ctx) {
                Ok((w, h)) => {
                    self.status_msg = format!("Fotograma {w}×{h} capturado. Pulsa Calibrar.");
                    let _ = std::fs::remove_file(&out);
                }
                Err(e) => self.status_msg = format!("Error al leer el fotograma: {e}"),
            },
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                let first = err.lines().next().unwrap_or("fallo de ffmpeg");
                self.status_msg = format!("Cámara: {first}");
            }
            Err(e) => {
                self.status_msg = format!("No se pudo ejecutar ffmpeg ({e}). Instálalo para captura en vivo.");
            }
        }
    }

    /// Load an image file as the board photo; returns (width, height) in pixels.
    fn set_cam_photo(
        &mut self,
        path: &std::path::Path,
        ctx: &egui::Context,
    ) -> Result<(usize, usize), String> {
        let img = image::open(path).map_err(|e| e.to_string())?;
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width() as usize, rgba.height() as usize);
        let buf = rgba.into_raw();
        self.cam_orig = Some((buf, w, h));
        self.cam_tex = None;
        self.cal_points.clear();
        self.rebuild_cam_correction(ctx);
        Ok((w, h))
    }

    /// Rebuild the displayed photo from the pristine capture, applying the current
    /// lens-distortion correction, and refresh the raw texture. Re-runs the bed
    /// warp if a calibration is already in place.
    fn rebuild_cam_correction(&mut self, ctx: &egui::Context) {
        let Some((orig, w, h)) = self.cam_orig.as_ref() else {
            return;
        };
        let (w, h) = (*w, *h);
        let corrected = if self.cam_k1 == 0.0 && self.cam_k2 == 0.0 {
            orig.clone()
        } else {
            crate::homography::undistort(orig, w, h, self.cam_k1, self.cam_k2)
        };
        let ci = egui::ColorImage::from_rgba_unmultiplied([w, h], &corrected);
        self.cam_raw_tex = Some(ctx.load_texture("cam_raw", ci, egui::TextureOptions::LINEAR));
        self.cam_photo = Some((corrected, w, h));
        if self.cal_points.len() >= 4 {
            self.compute_calibration(ctx);
        }
    }

    fn compute_calibration(&mut self, ctx: &egui::Context) {
        let Some((buf, w, h)) = self.cam_photo.as_ref() else {
            return;
        };
        if self.cal_points.len() < 4 {
            return;
        }
        let bw = self.doc.machine.bed_w_mm;
        let bh = self.doc.machine.bed_h_mm;
        let src = [
            Vec2::new(0.0, 0.0),
            Vec2::new(bw, 0.0),
            Vec2::new(bw, bh),
            Vec2::new(0.0, bh),
        ];
        let dst = [
            self.cal_points[0],
            self.cal_points[1],
            self.cal_points[2],
            self.cal_points[3],
        ];
        let Some(hmg) = crate::homography::Homography::from_points(src, dst) else {
            self.status_msg = "Calibración fallida (puntos colineales)".into();
            return;
        };
        let out_w = (bw.round() as usize).clamp(64, 1200);
        let out_h = (bh.round() as usize).clamp(64, 1200);
        let warped = crate::homography::warp_to_bed(buf, *w, *h, &hmg, bw, bh, out_w, out_h);
        let ci = egui::ColorImage::from_rgba_unmultiplied([out_w, out_h], &warped);
        self.cam_tex = Some(ctx.load_texture("cam_bed", ci, egui::TextureOptions::LINEAR));
        self.status_msg = "Cámara calibrada ✓".into();
    }

    fn generate_material_test(&mut self) {
        self.gcode = crate::gcode::material_test(&self.doc.machine, &self.mat_test);
        self.recompute_toolpath();
        let eta = self
            .toolpath
            .as_ref()
            .map(|t| format!(" · ⏱ {}", fmt_duration(t.seconds)))
            .unwrap_or_default();
        self.gcode_msg = format!("Test de material: {} líneas{}", self.gcode.len(), eta);
        self.status_msg = "Test de material generado".into();
    }

    fn finish_polyline(&mut self, closed: bool) {
        if let Some((pls, pos)) = crate::shapes::polyline(&self.poly_points, closed) {
            self.add_vector(pls, pos, "polilínea");
        }
        self.poly_points.clear();
    }

    fn place_text(&mut self, at: Vec2) {
        if self.num_series {
            self.create_number_series(at);
            return;
        }
        let content = self.text_content.clone();
        if content.trim().is_empty() {
            self.status_msg = "Texto vacío".into();
            return;
        }
        let font = (!self.text_font.is_empty()).then(|| self.text_font.clone());
        match crate::text::text_to_polylines_family(&self.fontdb, &content, self.text_height, font.as_deref()) {
            Some(pls) => self.add_vector(pls, at, "texto"),
            None => self.status_msg = "No se encontró una fuente utilizable".into(),
        }
    }

    /// Expand the template, replacing the first run of `#` with `n` zero-padded
    /// to the run length (min 1). With no `#`, the number is appended.
    pub(crate) fn expand_template(template: &str, n: i64) -> String {
        if let Some(start) = template.find('#') {
            let run = template[start..].chars().take_while(|&c| c == '#').count();
            let num = if n < 0 {
                format!("-{:0>width$}", -n, width = run)
            } else {
                format!("{:0>width$}", n, width = run)
            };
            let mut s = String::with_capacity(template.len() + num.len());
            s.push_str(&template[..start]);
            s.push_str(&num);
            s.push_str(&template[start + run..]);
            s
        } else if template.trim().is_empty() {
            n.to_string()
        } else {
            format!("{template}{n}")
        }
    }

    /// Create a sequence of numbered text objects from the template in
    /// `text_content` (use `#`/`###` as the number placeholder).
    fn create_number_series(&mut self, at: Vec2) {
        let count = self.num_count.max(1);
        let template = self.text_content.clone();
        let font = (!self.text_font.is_empty()).then(|| self.text_font.clone());
        self.snapshot();
        let layer = self.selected_layer().min(self.doc.layers.len().saturating_sub(1));
        let mut made = 0usize;
        let mut first_idx = None;
        for k in 0..count {
            let n = self.num_start + self.num_step * k as i64;
            let label = Self::expand_template(&template, n);
            if label.trim().is_empty() {
                continue;
            }
            let Some(pls) = crate::text::text_to_polylines_family(
                &self.fontdb,
                &label,
                self.text_height,
                font.as_deref(),
            )
            else {
                self.status_msg = "No se encontró una fuente utilizable".into();
                return;
            };
            let pos = if self.num_horizontal {
                Vec2::new(at.x + k as f64 * self.num_spacing, at.y)
            } else {
                Vec2::new(at.x, at.y + k as f64 * self.num_spacing)
            };
            let id = self.doc.alloc_id();
            let obj = Object::new(
                id,
                format!("nº {n}"),
                layer,
                ObjectKind::Vector(crate::model::VectorData { polylines: pls }),
                pos,
            );
            let idx = self.doc.add_object(obj);
            first_idx.get_or_insert(idx);
            made += 1;
        }
        self.doc.selected = first_idx;
        self.multi.clear();
        self.status_msg = format!("Serie creada: {made} etiqueta(s)");
    }

    fn maybe_snap(&self, p: Vec2) -> Vec2 {
        if self.snap && self.snap_step > 0.0 {
            Vec2::new(
                (p.x / self.snap_step).round() * self.snap_step,
                (p.y / self.snap_step).round() * self.snap_step,
            )
        } else {
            p
        }
    }
}

// --- Canvas ------------------------------------------------------------------
impl RustburnApp {
    fn canvas_ui(&mut self, ui: &mut egui::Ui) {
        let size = ui.available_size();
        let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let origin = response.rect.min;

        // First-time fit: center the bed.
        if !self.view.initialized {
            let bw = self.doc.machine.bed_w_mm as f32;
            let bh = self.doc.machine.bed_h_mm as f32;
            let zx = (size.x - 40.0) / bw.max(1.0);
            let zy = (size.y - 40.0) / bh.max(1.0);
            self.view.zoom = zx.min(zy).max(0.2);
            self.view.pan = EVec2::new(20.0, 20.0);
            self.view.initialized = true;
        }

        // Zoom with scroll, around the pointer.
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            if let Some(ptr) = response.hover_pos() {
                let before = self.view.s2w(origin, ptr);
                let factor = (scroll * 0.0015).exp();
                self.view.zoom = (self.view.zoom * factor).clamp(0.2, 60.0);
                let after = self.view.s2w(origin, ptr);
                self.view.pan.x += (after.x - before.x) as f32 * self.view.zoom;
                self.view.pan.y += (after.y - before.y) as f32 * self.view.zoom;
            }
        }

        // Track the cursor position in world space (for drawing previews).
        self.cursor_world = response
            .hover_pos()
            .map(|p| self.maybe_snap(self.view.s2w(origin, p)));

        // Camera calibration takes over the canvas while active.
        if self.calibrating && self.cam_photo.is_some() {
            self.handle_calibration(&response, &painter, origin);
            return;
        }

        if self.tool == Tool::Measure {
            // Two clicks drop the two endpoints; a third starts over.
            if response.clicked() {
                if let Some(p) = response.interact_pointer_pos() {
                    let w = self.maybe_snap(self.view.s2w(origin, p));
                    if self.measure_a.is_none() || self.measure_b.is_some() {
                        self.measure_a = Some(w);
                        self.measure_b = None;
                    } else {
                        self.measure_b = Some(w);
                    }
                }
            }
            self.draw_scene(ui, &painter, origin);
            return;
        }

        if self.tool == Tool::Select {
            // Begin a gesture (move / scale / rotate / pan) on drag start.
            if response.drag_started() {
                let p = response.interact_pointer_pos();
                self.gesture = self.begin_gesture(origin, p);
            }
            if response.dragged() {
                let delta = response.drag_delta();
                let pw_opt = response
                    .interact_pointer_pos()
                    .map(|p| self.view.s2w(origin, p));
                self.apply_gesture(delta, pw_opt);
            }
            if response.drag_stopped() {
                self.gesture = Gesture::None;
                self.snap_guides.clear();
            }
            if response.clicked() {
                if let Some(p) = response.interact_pointer_pos() {
                    let hit = self.hit_test(origin, p);
                    let shift = response.ctx.input(|i| i.modifiers.shift);
                    if shift {
                        if let Some(i) = hit {
                            if let Some(pos) = self.multi.iter().position(|&x| x == i) {
                                self.multi.remove(pos);
                            } else {
                                self.multi.push(i);
                            }
                        }
                    } else if let Some(h) = hit {
                        self.select_with_group(h);
                    } else {
                        self.multi.clear();
                        self.doc.selected = None;
                    }
                }
            }
        } else {
            self.handle_draw_tool(&response, origin);
        }

        self.draw_scene(ui, &painter, origin);
    }

    fn handle_calibration(&mut self, response: &egui::Response, painter: &egui::Painter, origin: Pos2) {
        let bw = self.doc.machine.bed_w_mm;
        let bh = self.doc.machine.bed_h_mm;
        let bmin = self.view.w2s(origin, Vec2::new(0.0, 0.0));
        let bmax = self.view.w2s(origin, Vec2::new(bw, bh));
        let rect = Rect::from_two_pos(bmin, bmax);
        painter.rect_filled(rect, 0.0, Color32::from_gray(15));
        if let Some(tex) = &self.cam_raw_tex {
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        painter.rect_stroke(rect, 0.0, Stroke::new(1.5_f32, Color32::from_rgb(255, 210, 40)));
        let (w, h) = match &self.cam_photo {
            Some((_, w, h)) => (*w, *h),
            None => return,
        };
        for (i, p) in self.cal_points.iter().enumerate() {
            let u = (p.x / w as f64) as f32;
            let v = (p.y / h as f64) as f32;
            let s = Pos2::new(rect.min.x + u * rect.width(), rect.min.y + v * rect.height());
            painter.circle_filled(s, 5.0, Color32::from_rgb(255, 80, 80));
            painter.text(
                s + egui::vec2(7.0, -7.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{}", i + 1),
                egui::FontId::proportional(14.0),
                Color32::WHITE,
            );
        }
        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let u = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0) as f64;
                let v = ((p.y - rect.min.y) / rect.height()).clamp(0.0, 1.0) as f64;
                self.cal_points.push(Vec2::new(u * w as f64, v * h as f64));
                if self.cal_points.len() >= 4 {
                    self.compute_calibration(&response.ctx);
                    self.calibrating = false;
                }
            }
        }
    }

    /// World positions of the object's local bbox corners, paired with the local corner.
    fn obj_world_corners(&self, idx: usize) -> Option<[(Vec2, Vec2); 4]> {
        let obj = self.doc.objects.get(idx)?;
        let bb = obj.local_bbox()?;
        let m = obj.matrix();
        let locals = [
            Vec2::new(bb.min.x, bb.min.y),
            Vec2::new(bb.max.x, bb.min.y),
            Vec2::new(bb.max.x, bb.max.y),
            Vec2::new(bb.min.x, bb.max.y),
        ];
        Some(locals.map(|l| (l, m.apply(l))))
    }

    fn begin_gesture(&mut self, origin: Pos2, p_screen: Option<Pos2>) -> Gesture {
        let Some(ps) = p_screen else {
            return Gesture::Pan;
        };
        let pw = self.view.s2w(origin, ps);
        let zoom = self.view.zoom as f64;

        let mut decided: Option<Gesture> = None;
        if let Some(sel) = self.doc.selected {
            if let Some(obj) = self.doc.objects.get(sel) {
                let rotation = obj.rotation;
                let pos = obj.pos;
                let scale = obj.scale;
                if let Some(bb) = obj.bbox() {
                    let cx = (bb.min.x + bb.max.x) / 2.0;
                    let center = Vec2::new(cx, (bb.min.y + bb.max.y) / 2.0);
                    let rot_world = Vec2::new(cx, bb.min.y - 20.0 / zoom);
                    let rot_screen = self.view.w2s(origin, rot_world);
                    if ps.distance(rot_screen) < 11.0 {
                        let start_angle = (pw.y - center.y).atan2(pw.x - center.x);
                        decided = Some(Gesture::Rotate {
                            obj: sel,
                            center,
                            start_pos: pos,
                            start_rot: rotation,
                            start_angle,
                        });
                    }
                    let a = rotation.rem_euclid(360.0);
                    let near_unrot = a < 0.6 || a > 359.4;
                    if decided.is_none() && near_unrot {
                        if let Some(corners) = self.obj_world_corners(sel) {
                            for i in 0..4 {
                                let cs = self.view.w2s(origin, corners[i].1);
                                if ps.distance(cs) < 9.0 {
                                    let opp = (i + 2) % 4;
                                    decided = Some(Gesture::Scale {
                                        obj: sel,
                                        anchor_world: corners[opp].1,
                                        local_anchor: corners[opp].0,
                                        local_corner: corners[i].0,
                                        start_scale: scale,
                                    });
                                    break;
                                }
                            }
                        }
                    }
                    if decided.is_none()
                        && pw.x >= bb.min.x
                        && pw.x <= bb.max.x
                        && pw.y >= bb.min.y
                        && pw.y <= bb.max.y
                    {
                        decided = Some(Gesture::Move {
                            obj: sel,
                            start_pos: pos,
                            ptr_start: pw,
                            group: self.group_drag_members(sel),
                        });
                    }
                }
            }
        }
        if let Some(g) = decided {
            self.snapshot();
            return g;
        }

        if let Some(i) = self.hit_test(origin, ps) {
            let start_pos = self.doc.objects[i].pos;
            self.select_with_group(i);
            self.snapshot();
            return Gesture::Move {
                obj: i,
                start_pos,
                ptr_start: pw,
                group: self.group_drag_members(i),
            };
        }
        Gesture::Pan
    }

    fn apply_gesture(&mut self, delta: EVec2, pw_opt: Option<Vec2>) {
        match self.gesture {
            Gesture::Pan => self.view.pan += delta,
            Gesture::None => {}
            Gesture::Move {
                obj,
                start_pos,
                ptr_start,
                ref group,
            } => {
                let members = group.clone();
                let (snap, step) = (self.snap, self.snap_step);
                self.snap_guides.clear();
                if let Some(pw) = pw_opt {
                    let mut np = Vec2::new(
                        start_pos.x + (pw.x - ptr_start.x),
                        start_pos.y + (pw.y - ptr_start.y),
                    );
                    if snap && step > 0.0 {
                        np.x = (np.x / step).round() * step;
                        np.y = (np.y / step).round() * step;
                    }
                    // Snap edges/centres to other objects and bed guides.
                    if self.snap_objects {
                        let (snapped, guides) = self.snap_to_objects(obj, np);
                        np = snapped;
                        self.snap_guides = guides;
                    }
                    if let Some(o) = self.doc.objects.get_mut(obj) {
                        o.pos = np;
                    }
                    // Apply the same net translation to every other group member.
                    let d = Vec2::new(np.x - start_pos.x, np.y - start_pos.y);
                    for (gi, gstart) in members {
                        if let Some(go) = self.doc.objects.get_mut(gi) {
                            go.pos = Vec2::new(gstart.x + d.x, gstart.y + d.y);
                        }
                    }
                }
            }
            Gesture::Scale {
                obj,
                anchor_world,
                local_anchor,
                local_corner,
                start_scale,
            } => {
                let lock = self.lock_ratio;
                if let (Some(pw), Some(o)) = (pw_opt, self.doc.objects.get_mut(obj)) {
                    let dx = local_corner.x - local_anchor.x;
                    let dy = local_corner.y - local_anchor.y;
                    if dx.abs() > 1e-6 && dy.abs() > 1e-6 {
                        let mut sx = (pw.x - anchor_world.x) / dx;
                        let mut sy = (pw.y - anchor_world.y) / dy;
                        if sx.abs() < 0.01 {
                            sx = 0.01 * if sx < 0.0 { -1.0 } else { 1.0 };
                        }
                        if sy.abs() < 0.01 {
                            sy = 0.01 * if sy < 0.0 { -1.0 } else { 1.0 };
                        }
                        if lock {
                            let fx = sx / start_scale.x;
                            let fy = sy / start_scale.y;
                            let f = if (fx - 1.0).abs() >= (fy - 1.0).abs() { fx } else { fy };
                            sx = start_scale.x * f;
                            sy = start_scale.y * f;
                        }
                        o.scale = Vec2::new(sx, sy);
                        o.pos = Vec2::new(
                            anchor_world.x - sx * local_anchor.x,
                            anchor_world.y - sy * local_anchor.y,
                        );
                    }
                }
            }
            Gesture::Rotate {
                obj,
                center,
                start_pos,
                start_rot,
                start_angle,
            } => {
                if let (Some(pw), Some(o)) = (pw_opt, self.doc.objects.get_mut(obj)) {
                    let cur = (pw.y - center.y).atan2(pw.x - center.x);
                    let d = cur - start_angle;
                    o.rotation = start_rot + d.to_degrees();
                    // Orbit the local origin around the selection centre.
                    let (sin, cos) = d.sin_cos();
                    let rx = start_pos.x - center.x;
                    let ry = start_pos.y - center.y;
                    o.pos = Vec2::new(
                        center.x + cos * rx - sin * ry,
                        center.y + sin * rx + cos * ry,
                    );
                }
            }
        }
    }

    fn handle_draw_tool(&mut self, response: &egui::Response, origin: Pos2) {
        let pw = response
            .interact_pointer_pos()
            .map(|p| self.maybe_snap(self.view.s2w(origin, p)));
        match self.tool {
            Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Polygon => {
                if response.drag_started() {
                    if let Some(p) = pw {
                        self.draft = Some((p, p));
                    }
                }
                if response.dragged() {
                    if let (Some(p), Some((s, _))) = (pw, self.draft) {
                        self.draft = Some((s, p));
                    }
                }
                if response.drag_stopped() {
                    if let Some((s, e)) = self.draft.take() {
                        if s.distance(e) > 0.2 {
                            self.create_from_draft(s, e);
                        }
                    }
                }
            }
            Tool::Polyline => {
                if response.clicked() {
                    if let Some(p) = pw {
                        self.poly_points.push(p);
                    }
                }
                if response.double_clicked() {
                    self.finish_polyline(false);
                }
                let (enter, esc, close_key) = response.ctx.input(|i| {
                    (
                        i.key_pressed(egui::Key::Enter),
                        i.key_pressed(egui::Key::Escape),
                        i.key_pressed(egui::Key::C),
                    )
                });
                if enter {
                    self.finish_polyline(false);
                } else if close_key {
                    self.finish_polyline(true);
                } else if esc {
                    self.poly_points.clear();
                }
            }
            Tool::Text => {
                if response.clicked() {
                    if let Some(p) = pw {
                        self.place_text(p);
                    }
                }
            }
            Tool::Node => {
                let scr = response.interact_pointer_pos();
                if response.drag_started() {
                    // select an object under the cursor if none is targeted yet.
                    if self.hit_node(origin, scr).is_none() {
                        if let Some(p) = scr {
                            if let Some(i) = self.hit_test(origin, p) {
                                self.doc.selected = Some(i);
                            }
                        }
                    }
                    self.node_drag = self.hit_node(origin, scr);
                    if self.node_drag.is_some() {
                        self.snapshot();
                    }
                }
                if response.dragged() {
                    if let (Some((oi, pli, pti)), Some(w)) = (self.node_drag, pw) {
                        let inv = self.doc.objects.get(oi).and_then(|o| o.matrix().inverse());
                        if let (Some(inv), Some(o)) = (inv, self.doc.objects.get_mut(oi)) {
                            let local = inv.apply(w);
                            if let ObjectKind::Vector(v) = &mut o.kind {
                                if let Some(p) =
                                    v.polylines.get_mut(pli).and_then(|pl| pl.points.get_mut(pti))
                                {
                                    *p = local;
                                }
                            }
                        }
                    }
                }
                if response.drag_stopped() {
                    self.node_drag = None;
                }
                if response.double_clicked() {
                    if let Some((oi, pli, pti)) = self.hit_node(origin, scr) {
                        self.snapshot();
                        if let Some(ObjectKind::Vector(v)) =
                            self.doc.objects.get_mut(oi).map(|o| &mut o.kind)
                        {
                            if let Some(pl) = v.polylines.get_mut(pli) {
                                if pl.points.len() > 2 {
                                    pl.points.remove(pti);
                                }
                            }
                        }
                    } else if let Some((oi, pli, ins, mid)) = self.hit_segment(origin, scr) {
                        self.snapshot();
                        if let Some(ObjectKind::Vector(v)) =
                            self.doc.objects.get_mut(oi).map(|o| &mut o.kind)
                        {
                            if let Some(pl) = v.polylines.get_mut(pli) {
                                pl.points.insert(ins.min(pl.points.len()), mid);
                            }
                        }
                    }
                }
            }
            Tool::Select | Tool::Measure => {}
        }
    }

    /// Find a node of the selected vector object under the screen point.
    fn hit_node(&self, origin: Pos2, screen: Option<Pos2>) -> Option<(usize, usize, usize)> {
        let s = screen?;
        let sel = self.doc.selected?;
        let obj = self.doc.objects.get(sel)?;
        let ObjectKind::Vector(v) = &obj.kind else {
            return None;
        };
        let m = obj.matrix();
        for (pli, pl) in v.polylines.iter().enumerate() {
            for (pti, p) in pl.points.iter().enumerate() {
                if s.distance(self.view.w2s(origin, m.apply(*p))) < 8.0 {
                    return Some((sel, pli, pti));
                }
            }
        }
        None
    }

    /// Find a segment of the selected object near the point; returns (obj, pl, insert_index, local_midpoint).
    fn hit_segment(
        &self,
        origin: Pos2,
        screen: Option<Pos2>,
    ) -> Option<(usize, usize, usize, Vec2)> {
        let s = screen?;
        let sel = self.doc.selected?;
        let obj = self.doc.objects.get(sel)?;
        let ObjectKind::Vector(v) = &obj.kind else {
            return None;
        };
        let m = obj.matrix();
        let mut best: Option<(f32, usize, usize, Vec2)> = None;
        for (pli, pl) in v.polylines.iter().enumerate() {
            let n = pl.points.len();
            if n < 2 {
                continue;
            }
            let count = if pl.closed { n } else { n - 1 };
            for i in 0..count {
                let la = pl.points[i];
                let lb = pl.points[(i + 1) % n];
                let a = self.view.w2s(origin, m.apply(la));
                let b = self.view.w2s(origin, m.apply(lb));
                let d = dist_point_segment(s, a, b);
                if d < 8.0 && best.map_or(true, |(bd, _, _, _)| d < bd) {
                    let mid = Vec2::new((la.x + lb.x) / 2.0, (la.y + lb.y) / 2.0);
                    best = Some((d, pli, i + 1, mid));
                }
            }
        }
        best.map(|(_, pli, ins, mid)| (sel, pli, ins, mid))
    }

    fn create_from_draft(&mut self, s: Vec2, e: Vec2) {
        let (built, name) = match self.tool {
            Tool::Rect => (Some(crate::shapes::rect(s, e)), "rectángulo"),
            Tool::Ellipse => (Some(crate::shapes::ellipse(s, e)), "elipse"),
            Tool::Line => (Some(crate::shapes::line(s, e)), "línea"),
            Tool::Polygon => (
                Some(crate::shapes::polygon(s, e, self.polygon_sides)),
                "polígono",
            ),
            _ => (None, ""),
        };
        if let Some((pls, pos)) = built {
            self.add_vector(pls, pos, name);
        }
    }

    fn hit_test(&self, origin: Pos2, screen: Pos2) -> Option<usize> {
        let w = self.view.s2w(origin, screen);
        // topmost first
        for (i, obj) in self.doc.objects.iter().enumerate().rev() {
            if let Some(bb) = obj.bbox() {
                let m = 2.0 / self.view.zoom as f64; // small margin in mm
                if w.x >= bb.min.x - m
                    && w.x <= bb.max.x + m
                    && w.y >= bb.min.y - m
                    && w.y <= bb.max.y + m
                {
                    return Some(i);
                }
            }
        }
        None
    }

    fn draw_scene(&mut self, ui: &egui::Ui, painter: &egui::Painter, origin: Pos2) {
        let m = &self.doc.machine;
        let bed_min = self.view.w2s(origin, Vec2::new(0.0, 0.0));
        let bed_max = self.view.w2s(origin, Vec2::new(m.bed_w_mm, m.bed_h_mm));
        let bed = Rect::from_two_pos(bed_min, bed_max);
        let light = self.canvas_light;
        let bed_fill = if light {
            Color32::from_gray(250)
        } else {
            Color32::from_rgb(28, 30, 34)
        };
        let bed_stroke = if light {
            Color32::from_gray(120)
        } else {
            Color32::from_gray(90)
        };
        painter.rect_filled(bed, 0.0, bed_fill);
        painter.rect_stroke(bed, 0.0, Stroke::new(1.5_f32, bed_stroke));

        // Grid every 10 mm, major every 50 mm (dark lines on light, light on dark).
        let (grid_col, major_col) = if light {
            (
                Color32::from_rgba_unmultiplied(0, 0, 0, 24),
                Color32::from_rgba_unmultiplied(0, 0, 0, 55),
            )
        } else {
            (
                Color32::from_rgba_unmultiplied(255, 255, 255, 16),
                Color32::from_rgba_unmultiplied(255, 255, 255, 40),
            )
        };
        let mut x = 0.0;
        while x <= m.bed_w_mm + 0.01 {
            let a = self.view.w2s(origin, Vec2::new(x, 0.0));
            let b = self.view.w2s(origin, Vec2::new(x, m.bed_h_mm));
            let col = if (x % 50.0).abs() < 0.01 { major_col } else { grid_col };
            painter.line_segment([a, b], Stroke::new(1.0_f32, col));
            x += 10.0;
        }
        let mut y = 0.0;
        while y <= m.bed_h_mm + 0.01 {
            let a = self.view.w2s(origin, Vec2::new(0.0, y));
            let b = self.view.w2s(origin, Vec2::new(m.bed_w_mm, y));
            let col = if (y % 50.0).abs() < 0.01 { major_col } else { grid_col };
            painter.line_segment([a, b], Stroke::new(1.0_f32, col));
            y += 10.0;
        }

        // Camera / photo overlay (drawn under the design).
        if self.cam_show {
            if let Some(tex) = &self.cam_tex {
                let bmin = self.view.w2s(origin, Vec2::new(0.0, 0.0));
                let bmax = self.view.w2s(origin, Vec2::new(m.bed_w_mm, m.bed_h_mm));
                let rect = Rect::from_two_pos(bmin, bmax);
                let a = (self.cam_opacity * 255.0) as u8;
                painter.image(
                    tex.id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::from_white_alpha(a),
                );
            }
        }

        // Objects.
        let selected = self.doc.selected;
        // Pre-ensure textures for images (needs &mut self), collect draw data first.
        let ctx = ui.ctx().clone();
        for i in 0..self.doc.objects.len() {
            let (kind_is_image, id) = {
                let o = &self.doc.objects[i];
                (matches!(o.kind, ObjectKind::Image(_)), o.id)
            };
            if kind_is_image && !self.textures.contains_key(&id) {
                if let ObjectKind::Image(img) = &self.doc.objects[i].kind {
                    let proc = img.processed_gray();
                    let mut rgba = Vec::with_capacity((img.width_px * img.height_px * 4) as usize);
                    for &g in &proc {
                        rgba.extend_from_slice(&[g, g, g, 255]);
                    }
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [img.width_px as usize, img.height_px as usize],
                        &rgba,
                    );
                    let tex = ctx.load_texture(format!("img{id}"), color, egui::TextureOptions::LINEAR);
                    self.textures.insert(id, tex);
                }
            }
        }

        for (i, obj) in self.doc.objects.iter().enumerate() {
            let layer = &self.doc.layers[obj.layer.min(self.doc.layers.len() - 1)];
            if !layer.visible {
                continue;
            }
            let col = Color32::from_rgb(layer.color[0], layer.color[1], layer.color[2]);
            let m = obj.matrix();
            match &obj.kind {
                ObjectKind::Vector(v) => {
                    for pl in &v.polylines {
                        if pl.points.len() < 2 {
                            continue;
                        }
                        let mut pts: Vec<Pos2> = pl
                            .points
                            .iter()
                            .map(|p| self.view.w2s(origin, m.apply(*p)))
                            .collect();
                        if pl.closed {
                            pts.push(pts[0]);
                        }
                        painter.add(egui::Shape::line(pts, Stroke::new(1.5_f32, col)));
                    }
                }
                ObjectKind::Image(img) => {
                    let p0 = self.view.w2s(origin, m.apply(Vec2::new(0.0, 0.0)));
                    let p1 = self
                        .view
                        .w2s(origin, m.apply(Vec2::new(img.width_mm, img.height_mm)));
                    let r = Rect::from_two_pos(p0, p1);
                    if let Some(tex) = self.textures.get(&obj.id) {
                        painter.image(
                            tex.id(),
                            r,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        painter.rect_filled(r, 0.0, Color32::from_gray(80));
                    }
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, col));
                }
            }

            if self.multi.contains(&i) && selected != Some(i) {
                if let Some(bb) = obj.bbox() {
                    let a = self.view.w2s(origin, bb.min);
                    let b = self.view.w2s(origin, bb.max);
                    painter.rect_stroke(
                        Rect::from_two_pos(a, b).expand(3.0),
                        0.0,
                        Stroke::new(1.5_f32, Color32::from_rgb(255, 120, 40)),
                    );
                }
            }
            if selected == Some(i) {
                let yellow = Color32::from_rgb(255, 210, 40);
                if let Some(bb) = obj.bbox() {
                    let a = self.view.w2s(origin, bb.min);
                    let b = self.view.w2s(origin, bb.max);
                    painter.rect_stroke(
                        Rect::from_two_pos(a, b).expand(3.0),
                        0.0,
                        Stroke::new(1.5_f32, yellow),
                    );
                    // Rotation handle above the top edge.
                    let cx = (bb.min.x + bb.max.x) / 2.0;
                    let top = self.view.w2s(origin, Vec2::new(cx, bb.min.y));
                    let knob = self
                        .view
                        .w2s(origin, Vec2::new(cx, bb.min.y - 20.0 / self.view.zoom as f64));
                    painter.line_segment([top, knob], Stroke::new(1.0_f32, yellow));
                    painter.circle_filled(knob, 4.5, yellow);
                    // Scale handles at the corners (only when effectively unrotated).
                    let a_ang = obj.rotation.rem_euclid(360.0);
                    if a_ang < 0.6 || a_ang > 359.4 {
                        if let Some(corners) = self.obj_world_corners(i) {
                            for (_l, wpt) in corners {
                                let s = self.view.w2s(origin, wpt);
                                painter.rect_filled(
                                    Rect::from_center_size(s, egui::Vec2::splat(8.0)),
                                    0.0,
                                    Color32::WHITE,
                                );
                                painter.rect_stroke(
                                    Rect::from_center_size(s, egui::Vec2::splat(8.0)),
                                    0.0,
                                    Stroke::new(1.0_f32, Color32::from_gray(40)),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Toolpath preview overlay.
        if self.show_toolpath {
            if let Some(tp) = &self.toolpath {
                let cut_col = Color32::from_rgb(255, 140, 0);
                let travel_col = Color32::from_rgba_unmultiplied(120, 200, 255, 90);
                // Dim colours for the not-yet-reached part during simulation.
                let cut_dim = Color32::from_rgba_unmultiplied(255, 140, 0, 45);
                let travel_dim = Color32::from_rgba_unmultiplied(120, 200, 255, 30);
                let simulating = self.sim_playing || self.sim_pos_mm > 1e-6;
                let mut acc = 0.0f64;
                for seg in tp.segs.iter().take(60_000) {
                    let a = self.view.w2s(origin, seg.from);
                    let b = self.view.w2s(origin, seg.to);
                    let len = (seg.to.x - seg.from.x).hypot(seg.to.y - seg.from.y);
                    // Fraction of this segment already traversed by the head.
                    let done = if !simulating {
                        1.0
                    } else if acc >= self.sim_pos_mm {
                        0.0
                    } else if acc + len <= self.sim_pos_mm || len < 1e-9 {
                        1.0
                    } else {
                        (self.sim_pos_mm - acc) / len
                    };
                    let (done_col, pend_col, w) = if seg.rapid {
                        (travel_col, travel_dim, 0.7_f32)
                    } else {
                        (cut_col, cut_dim, 1.0_f32)
                    };
                    if done >= 1.0 {
                        painter.line_segment([a, b], Stroke::new(w, done_col));
                    } else if done <= 0.0 {
                        painter.line_segment([a, b], Stroke::new(w, pend_col));
                    } else {
                        let mid = Pos2::new(a.x + (b.x - a.x) * done as f32, a.y + (b.y - a.y) * done as f32);
                        painter.line_segment([a, mid], Stroke::new(w, done_col));
                        painter.line_segment([mid, b], Stroke::new(w, pend_col));
                    }
                    acc += len;
                }
                // Head marker.
                if simulating {
                    if let Some((p, cutting)) = self.sim_head() {
                        let hp = self.view.w2s(origin, p);
                        let hc = if cutting { Color32::from_rgb(255, 60, 60) } else { Color32::from_rgb(120, 220, 255) };
                        painter.circle_filled(hp, 5.0, hc);
                        painter.circle_stroke(hp, 7.0, Stroke::new(1.5_f32, Color32::WHITE));
                    }
                }
            }
        }

        // Drawing-tool previews.
        let preview = Color32::from_rgb(120, 220, 120);
        if let Some((s, e)) = self.draft {
            let built = match self.tool {
                Tool::Rect => Some(crate::shapes::rect(s, e)),
                Tool::Ellipse => Some(crate::shapes::ellipse(s, e)),
                Tool::Line => Some(crate::shapes::line(s, e)),
                Tool::Polygon => Some(crate::shapes::polygon(s, e, self.polygon_sides)),
                _ => None,
            };
            if let Some((pls, pos)) = built {
                for pl in &pls {
                    let mut pts: Vec<Pos2> = pl
                        .points
                        .iter()
                        .map(|p| self.view.w2s(origin, Vec2::new(pos.x + p.x, pos.y + p.y)))
                        .collect();
                    if pl.closed && !pts.is_empty() {
                        pts.push(pts[0]);
                    }
                    painter.add(egui::Shape::line(pts, Stroke::new(1.5_f32, preview)));
                }
            }
        }
        if self.tool == Tool::Polyline && !self.poly_points.is_empty() {
            let mut pts: Vec<Pos2> = self
                .poly_points
                .iter()
                .map(|p| self.view.w2s(origin, *p))
                .collect();
            if let Some(c) = self.cursor_world {
                pts.push(self.view.w2s(origin, c));
            }
            painter.add(egui::Shape::line(pts, Stroke::new(1.5_f32, preview)));
            for p in &self.poly_points {
                painter.circle_filled(self.view.w2s(origin, *p), 3.0, preview);
            }
        }

        // Node-edit handles on the selected vector object.
        if self.tool == Tool::Node {
            if let Some(sel) = self.doc.selected {
                if let Some(obj) = self.doc.objects.get(sel) {
                    if let ObjectKind::Vector(v) = &obj.kind {
                        let m = obj.matrix();
                        let cyan = Color32::from_rgb(40, 220, 220);
                        for pl in &v.polylines {
                            for p in &pl.points {
                                let s = self.view.w2s(origin, m.apply(*p));
                                painter.rect_filled(
                                    Rect::from_center_size(s, egui::Vec2::splat(7.0)),
                                    0.0,
                                    cyan,
                                );
                            }
                        }
                    }
                }
            }
        }
        // Snap guide lines (while dragging with object snapping).
        if !self.snap_guides.is_empty() {
            let gc = Color32::from_rgb(80, 220, 140);
            for (a, b) in &self.snap_guides {
                let sa = self.view.w2s(origin, *a);
                let sb = self.view.w2s(origin, *b);
                painter.line_segment([sa, sb], Stroke::new(1.0_f32, gc));
            }
        }

        // Measurement overlay (measure tool).
        if let Some(a) = self.measure_a {
            let cc = Color32::from_rgb(255, 210, 40);
            let sa = self.view.w2s(origin, a);
            let end = self.measure_b.or(self.cursor_world);
            if let Some(b) = end {
                let sb = self.view.w2s(origin, b);
                painter.line_segment([sa, sb], Stroke::new(1.5_f32, cc));
                painter.circle_filled(sa, 3.0, cc);
                painter.circle_filled(sb, 3.0, cc);
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let dist = (dx * dx + dy * dy).sqrt();
                let ang = (-dy).atan2(dx).to_degrees();
                let mid = Pos2::new((sa.x + sb.x) / 2.0, (sa.y + sb.y) / 2.0);
                let txt = format!("{:.2} mm  Δ{:.1},{:.1}  {:.1}°", dist, dx, dy, ang);
                painter.text(
                    mid + egui::vec2(6.0, -6.0),
                    egui::Align2::LEFT_BOTTOM,
                    txt,
                    egui::FontId::proportional(12.0),
                    cc,
                );
            } else {
                painter.circle_filled(sa, 3.0, cc);
            }
        }

        self.draw_rulers(painter, origin);
    }

    /// Draw mm rulers along all four edges of the canvas, tied to the grid.
    fn draw_rulers(&self, painter: &egui::Painter, origin: Pos2) {
        let area = painter.clip_rect();
        let m = &self.doc.machine;
        let zoom = self.view.zoom;
        const R: f32 = 22.0;
        let (bg, fg, tick) = if self.canvas_light {
            (
                Color32::from_gray(232),
                Color32::from_gray(70),
                Color32::from_gray(130),
            )
        } else {
            (
                Color32::from_gray(26),
                Color32::from_gray(160),
                Color32::from_gray(95),
            )
        };
        let font = egui::FontId::proportional(10.0);

        // Inner edges of the four ruler strips.
        let (x0, x1) = (area.min.x + R, area.max.x - R); // horizontal content span
        let (y0, y1) = (area.min.y + R, area.max.y - R); // vertical content span

        // Backgrounds for the four ruler strips.
        painter.rect_filled(Rect::from_min_max(area.min, Pos2::new(area.max.x, y0)), 0.0, bg); // top
        painter.rect_filled(Rect::from_min_max(Pos2::new(area.min.x, y1), area.max), 0.0, bg); // bottom
        painter.rect_filled(Rect::from_min_max(area.min, Pos2::new(x0, area.max.y)), 0.0, bg); // left
        painter.rect_filled(Rect::from_min_max(Pos2::new(x1, area.min.y), area.max), 0.0, bg); // right

        // Pick a label step so labels are ~>=48px apart; minor ticks at 1/5 of it.
        let steps = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0];
        let mut label_step = *steps.last().unwrap();
        for &s in &steps {
            if (s as f32) * zoom >= 48.0 {
                label_step = s;
                break;
            }
        }
        let minor = if label_step >= 5.0 { label_step / 5.0 } else { label_step };

        // Horizontal ruler ticks + labels (top and bottom).
        let mut x = 0.0;
        while x <= m.bed_w_mm + 1e-6 {
            let sx = self.view.w2s(origin, Vec2::new(x, 0.0)).x;
            if sx >= x0 && sx <= x1 {
                let major = (x % label_step).abs() < 1e-6;
                let h = if major { R * 0.5 } else { R * 0.28 };
                painter.line_segment(
                    [Pos2::new(sx, y0 - h), Pos2::new(sx, y0)],
                    Stroke::new(1.0_f32, tick),
                );
                painter.line_segment(
                    [Pos2::new(sx, y1), Pos2::new(sx, y1 + h)],
                    Stroke::new(1.0_f32, tick),
                );
                if major {
                    let label = format!("{}", x as i32);
                    painter.text(Pos2::new(sx + 2.0, area.min.y + 1.0), egui::Align2::LEFT_TOP, &label, font.clone(), fg);
                    painter.text(Pos2::new(sx + 2.0, area.max.y - 1.0), egui::Align2::LEFT_BOTTOM, &label, font.clone(), fg);
                }
            }
            x += minor;
        }

        // Vertical ruler ticks + labels (left and right).
        let mut y = 0.0;
        while y <= m.bed_h_mm + 1e-6 {
            let sy = self.view.w2s(origin, Vec2::new(0.0, y)).y;
            if sy >= y0 && sy <= y1 {
                let major = (y % label_step).abs() < 1e-6;
                let w = if major { R * 0.5 } else { R * 0.28 };
                painter.line_segment(
                    [Pos2::new(x0 - w, sy), Pos2::new(x0, sy)],
                    Stroke::new(1.0_f32, tick),
                );
                painter.line_segment(
                    [Pos2::new(x1, sy), Pos2::new(x1 + w, sy)],
                    Stroke::new(1.0_f32, tick),
                );
                if major {
                    let label = format!("{}", y as i32);
                    painter.text(Pos2::new(area.min.x + 1.0, sy + 1.0), egui::Align2::LEFT_TOP, &label, font.clone(), fg);
                    painter.text(Pos2::new(area.max.x - 1.0, sy + 1.0), egui::Align2::RIGHT_TOP, &label, font.clone(), fg);
                }
            }
            y += minor;
        }

        // Corner labels (all four corners).
        let unit = egui::FontId::proportional(9.0);
        for (cmin, cmax, align, anchor) in [
            (area.min, Pos2::new(x0, y0), egui::Align2::LEFT_TOP, area.min + egui::vec2(3.0, 3.0)),
            (Pos2::new(x1, area.min.y), Pos2::new(area.max.x, y0), egui::Align2::RIGHT_TOP, Pos2::new(area.max.x - 3.0, area.min.y + 3.0)),
            (Pos2::new(area.min.x, y1), Pos2::new(x0, area.max.y), egui::Align2::LEFT_BOTTOM, Pos2::new(area.min.x + 3.0, area.max.y - 3.0)),
            (Pos2::new(x1, y1), area.max, egui::Align2::RIGHT_BOTTOM, Pos2::new(area.max.x - 3.0, area.max.y - 3.0)),
        ] {
            painter.rect_filled(Rect::from_min_max(cmin, cmax), 0.0, bg);
            painter.text(anchor, align, "mm", unit.clone(), fg);
        }

        // Cursor position indicators on all four rulers.
        if let Some(cw) = self.cursor_world {
            let cc = Color32::from_rgb(255, 180, 40);
            let sx = self.view.w2s(origin, Vec2::new(cw.x, 0.0)).x;
            let sy = self.view.w2s(origin, Vec2::new(0.0, cw.y)).y;
            if sx >= x0 && sx <= x1 {
                painter.line_segment([Pos2::new(sx, area.min.y), Pos2::new(sx, y0)], Stroke::new(1.5_f32, cc));
                painter.line_segment([Pos2::new(sx, y1), Pos2::new(sx, area.max.y)], Stroke::new(1.5_f32, cc));
            }
            if sy >= y0 && sy <= y1 {
                painter.line_segment([Pos2::new(area.min.x, sy), Pos2::new(x0, sy)], Stroke::new(1.5_f32, cc));
                painter.line_segment([Pos2::new(x1, sy), Pos2::new(area.max.x, sy)], Stroke::new(1.5_f32, cc));
            }
        }
    }
}

/// Distance from a screen point to a segment (in pixels).
fn dist_point_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.x * ab.x + ab.y * ab.y;
    if len2 < 1e-6 {
        return p.distance(a);
    }
    let t = (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len2).clamp(0.0, 1.0);
    let proj = Pos2::new(a.x + t * ab.x, a.y + t * ab.y);
    p.distance(proj)
}

/// Catmull-Rom spline through `pts`, subdividing each segment into `sub` steps.
/// For `closed` the control sequence wraps; otherwise the endpoints are pinned
/// (duplicated) so the curve starts and ends exactly on them.
pub(crate) fn catmull_rom(pts: &[Vec2], closed: bool, sub: usize) -> Vec<Vec2> {
    let n = pts.len();
    if n < 3 || sub < 1 {
        return pts.to_vec();
    }
    let at = |i: isize| -> Vec2 {
        if closed {
            pts[i.rem_euclid(n as isize) as usize]
        } else {
            pts[i.clamp(0, n as isize - 1) as usize]
        }
    };
    let seg_count = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(seg_count * sub + 1);
    for i in 0..seg_count {
        let p0 = at(i as isize - 1);
        let p1 = at(i as isize);
        let p2 = at(i as isize + 1);
        let p3 = at(i as isize + 2);
        for s in 0..sub {
            let t = s as f64 / sub as f64;
            let t2 = t * t;
            let t3 = t2 * t;
            // Standard Catmull-Rom basis (tension 0.5).
            let x = 0.5
                * ((2.0 * p1.x)
                    + (-p0.x + p2.x) * t
                    + (2.0 * p0.x - 5.0 * p1.x + 4.0 * p2.x - p3.x) * t2
                    + (-p0.x + 3.0 * p1.x - 3.0 * p2.x + p3.x) * t3);
            let y = 0.5
                * ((2.0 * p1.y)
                    + (-p0.y + p2.y) * t
                    + (2.0 * p0.y - 5.0 * p1.y + 4.0 * p2.y - p3.y) * t2
                    + (-p0.y + 3.0 * p1.y - 3.0 * p2.y + p3.y) * t3);
            out.push(Vec2::new(x, y));
        }
    }
    if !closed {
        out.push(pts[n - 1]);
    }
    out
}

/// Format a duration in seconds as `Hh Mm Ss` (compact).
fn fmt_duration(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}h {m}m {sec}s")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}
