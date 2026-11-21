//! Document model: machine settings, layers (with cut settings), objects and the
//! top-level [`Document`] that ties them together.

use serde::{Deserialize, Serialize};

use crate::geometry::{Affine, BBox, Polyline, Vec2};

/// How GRBL drives the laser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaserMode {
    /// `M4` — dynamic power, scales with speed. Best for diode engraving/cutting.
    Dynamic,
    /// `M3` — constant power.
    Constant,
}

impl LaserMode {
    pub fn code(self) -> &'static str {
        match self {
            LaserMode::Dynamic => "M4",
            LaserMode::Constant => "M3",
        }
    }
}

/// Where the job origin sits relative to the machine bed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    /// Machine 0,0 is the front-left; Y grows away from the operator (GRBL default).
    FrontLeft,
    /// Document top-left maps directly to machine 0,0 (Y grows "down" on the machine).
    TopLeft,
}

/// Physical machine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineSettings {
    pub bed_w_mm: f64,
    pub bed_h_mm: f64,
    /// Maximum S value (`$30`), the power at 100 %.
    pub s_max: f64,
    /// Travel (rapid) speed for `G0`, mm/min.
    pub travel_speed: f64,
    pub laser_mode: LaserMode,
    pub origin: Origin,
    /// Baud rate for the USB serial connection.
    pub baud: u32,
    /// Reorder vector cuts to minimise travel moves.
    #[serde(default = "default_true")]
    pub optimize_travel: bool,
}

fn default_true() -> bool {
    true
}

impl Default for MachineSettings {
    fn default() -> Self {
        // Elegoo Phecda: ~400 x 400 mm work area, GRBL 1.1, S max 1000.
        Self {
            bed_w_mm: 400.0,
            bed_h_mm: 400.0,
            s_max: 1000.0,
            travel_speed: 6000.0,
            laser_mode: LaserMode::Dynamic,
            origin: Origin::FrontLeft,
            baud: 115200,
            optimize_travel: true,
        }
    }
}

/// What a layer does with its geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CutMode {
    /// Follow vector outlines (cut / score / line engrave).
    Line,
    /// Raster-engrave an image scanline by scanline.
    Image,
}

/// Dithering strategy for image engraving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dither {
    /// Modulate laser power by pixel grey level (no dithering).
    Grayscale,
    /// Hard threshold to black/white.
    Threshold,
    /// Floyd–Steinberg error diffusion to black/white.
    FloydSteinberg,
    /// Atkinson error diffusion (softer, higher contrast).
    Atkinson,
    /// Jarvis–Judice–Ninke error diffusion (wider kernel, smooth gradients).
    Jarvis,
    /// Ordered Bayer 4×4 dithering (regular pattern, fast).
    Ordered,
}

/// Per-image brightness / contrast / gamma / invert adjustments applied before engraving.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ImageAdjust {
    /// -100..100 (added to each pixel).
    pub brightness: f64,
    /// -100..100 (scaled around mid-grey).
    pub contrast: f64,
    /// 0.1..5.0, 1.0 = neutral.
    pub gamma: f64,
    pub invert: bool,
}

impl Default for ImageAdjust {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 0.0,
            gamma: 1.0,
            invert: false,
        }
    }
}

impl ImageAdjust {
    pub fn is_identity(&self) -> bool {
        self.brightness == 0.0
            && self.contrast == 0.0
            && (self.gamma - 1.0).abs() < 1e-9
            && !self.invert
    }

    /// Map a single 0..=255 grey value through the adjustments.
    pub fn map(&self, px: u8) -> u8 {
        let b = self.brightness / 100.0 * 255.0;
        let cc = (self.contrast / 100.0 * 255.0).clamp(-255.0, 255.0);
        let factor = (259.0 * (cc + 255.0)) / (255.0 * (259.0 - cc));
        let g = self.gamma.max(0.01);
        let mut v = px as f64 + b;
        v = factor * (v - 128.0) + 128.0;
        v = v.clamp(0.0, 255.0);
        let mut f = (v / 255.0).powf(1.0 / g);
        if self.invert {
            f = 1.0 - f;
        }
        (f * 255.0).round().clamp(0.0, 255.0) as u8
    }
}

/// Per-layer cut parameters, mirroring LightBurn's cut settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CutSettings {
    pub mode: CutMode,
    /// Laser power at full-on, as a percentage (0–100).
    pub power_pct: f64,
    /// Minimum power for `M4` dynamic mode (percentage).
    pub min_power_pct: f64,
    /// Cutting/engraving feed rate, mm/min.
    pub speed: f64,
    /// Number of passes (vector mode).
    pub passes: u32,
    // --- image/raster only ---
    /// Distance between scan lines, mm (1 / lines-per-mm). 0.1 mm ≈ 254 DPI.
    pub line_interval_mm: f64,
    pub dither: Dither,
    /// Extra travel beyond each scan line edge for acceleration, mm.
    pub overscan_mm: f64,
}

impl Default for CutSettings {
    fn default() -> Self {
        Self {
            mode: CutMode::Line,
            power_pct: 80.0,
            min_power_pct: 0.0,
            speed: 1000.0,
            passes: 1,
            line_interval_mm: 0.1,
            dither: Dither::Grayscale,
            overscan_mm: 2.0,
        }
    }
}

/// A named output layer with a colour and cut settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub color: [u8; 3],
    pub visible: bool,
    pub output: bool,
    pub settings: CutSettings,
}

impl Layer {
    pub fn new(name: impl Into<String>, color: [u8; 3]) -> Self {
        Self {
            name: name.into(),
            color,
            visible: true,
            output: true,
            settings: CutSettings::default(),
        }
    }
}

/// Raster image data (greyscale), with its physical footprint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub width_px: u32,
    pub height_px: u32,
    /// Row-major greyscale, 0 = black, 255 = white.
    pub gray: Vec<u8>,
    /// Physical width in mm (before the object transform).
    pub width_mm: f64,
    pub height_mm: f64,
    #[serde(default)]
    pub adjust: ImageAdjust,
}

impl ImageData {
    pub fn pixel(&self, x: u32, y: u32) -> u8 {
        self.gray[(y * self.width_px + x) as usize]
    }

    /// Greyscale buffer with brightness/contrast/gamma/invert applied.
    pub fn processed_gray(&self) -> Vec<u8> {
        if self.adjust.is_identity() {
            return self.gray.clone();
        }
        self.gray.iter().map(|&g| self.adjust.map(g)).collect()
    }
}

/// Vector geometry (one or more polylines) in local mm coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorData {
    pub polylines: Vec<Polyline>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObjectKind {
    Vector(VectorData),
    Image(ImageData),
}

/// A placed design object, assigned to a layer, with a decomposed transform
/// (position / rotation / scale) so it can be edited with handles and numeric fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Object {
    pub id: u64,
    pub name: String,
    pub layer: usize,
    pub kind: ObjectKind,
    /// Translation of the object's local origin, in mm (document space).
    pub pos: Vec2,
    /// Rotation in degrees (clockwise, screen convention).
    pub rotation: f64,
    /// Non-uniform scale factors.
    pub scale: Vec2,
    /// Group id: objects sharing the same id move/transform together.
    #[serde(default)]
    pub group: Option<u64>,
}

impl Object {
    pub fn new(id: u64, name: String, layer: usize, kind: ObjectKind, pos: Vec2) -> Self {
        Self {
            id,
            name,
            layer,
            kind,
            pos,
            rotation: 0.0,
            scale: Vec2::new(1.0, 1.0),
            group: None,
        }
    }

    /// The full local→document affine transform.
    pub fn matrix(&self) -> Affine {
        Affine::trs(self.pos.x, self.pos.y, self.rotation, self.scale.x, self.scale.y)
    }

    /// Local-space corner points (before the transform).
    fn local_points(&self) -> Vec<Vec2> {
        match &self.kind {
            ObjectKind::Vector(v) => v
                .polylines
                .iter()
                .flat_map(|pl| pl.points.iter().copied())
                .collect(),
            ObjectKind::Image(img) => vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(img.width_mm, 0.0),
                Vec2::new(img.width_mm, img.height_mm),
                Vec2::new(0.0, img.height_mm),
            ],
        }
    }

    /// Bounding box in local coordinates (before scale/rotation).
    pub fn local_bbox(&self) -> Option<BBox> {
        BBox::from_points(self.local_points().iter())
    }

    /// Unrotated size in mm (local size × scale).
    pub fn size(&self) -> Vec2 {
        match self.local_bbox() {
            Some(bb) => Vec2::new(bb.width() * self.scale.x, bb.height() * self.scale.y),
            None => Vec2::new(0.0, 0.0),
        }
    }

    /// Bounding box in document coordinates (transform applied).
    pub fn bbox(&self) -> Option<BBox> {
        let m = self.matrix();
        let pts: Vec<Vec2> = self.local_points().iter().map(|p| m.apply(*p)).collect();
        BBox::from_points(pts.iter())
    }
}

/// The complete design document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub machine: MachineSettings,
    pub layers: Vec<Layer>,
    pub objects: Vec<Object>,
    #[serde(skip)]
    pub selected: Option<usize>,
    #[serde(skip)]
    next_id: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            machine: MachineSettings::default(),
            layers: vec![
                Layer::new("Capa 1", [200, 30, 30]),
                Layer::new("Capa 2", [30, 90, 220]),
            ],
            objects: Vec::new(),
            selected: None,
            next_id: 1,
        }
    }
}

impl Document {
    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn add_object(&mut self, mut obj: Object) -> usize {
        if obj.id == 0 {
            obj.id = self.alloc_id();
        }
        self.next_id = self.next_id.max(obj.id + 1);
        self.objects.push(obj);
        self.objects.len() - 1
    }

    pub fn bbox(&self) -> Option<BBox> {
        self.objects
            .iter()
            .fold(None, |acc, o| BBox::union(acc, o.bbox()))
    }
}
