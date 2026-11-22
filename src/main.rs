//! rustburn — native Rust laser design & control app (LightBurn-like) for GRBL diode
//! lasers such as the Elegoo Phecda. Linux-native, no license required.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Scaffolding for upcoming phases keeps some helpers unused for now.
#![allow(dead_code)]

mod app;
mod device;
mod selftest;
mod gcode;
mod gcode_dither;
mod geometry;
mod geom_ops;
mod grbl_settings;
mod homography;
mod i18n;
mod import;
mod machines;
mod materials;
mod model;
mod prefs;
mod project;
mod shapes;
mod text;
mod trace;

use std::sync::atomic::{AtomicBool, Ordering};

/// Set by the log watcher when the GL backend reports it cannot create a usable
/// context (e.g. `GLXBadFBConfig` on a machine without working 3D acceleration).
static GL_CONTEXT_FAILED: AtomicBool = AtomicBool::new(false);

/// A `log::Log` that forwards to env_logger but flags GL context-creation
/// failures so we can fall back to software rendering.
struct WatchLogger(env_logger::Logger);

impl log::Log for WatchLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        self.0.enabled(m)
    }
    fn log(&self, record: &log::Record) {
        let msg = format!("{}", record.args());
        if msg.contains("Failed to create context")
            || msg.contains("BadFBConfig")
            || msg.contains("GLXBadFBConfig")
        {
            GL_CONTEXT_FAILED.store(true, Ordering::SeqCst);
        }
        self.0.log(record);
    }
    fn flush(&self) {
        self.0.flush()
    }
}

fn init_logging() {
    let logger = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,rustburn=info"),
    )
    .build();
    let level = logger.filter();
    if log::set_boxed_logger(Box::new(WatchLogger(logger))).is_ok() {
        log::set_max_level(level);
    }
}

fn main() -> eframe::Result<()> {
    init_logging();

    // Headless self-test: build a sample document and dump G-code stats, no GUI.
    if std::env::args().any(|a| a == "--selftest") {
        selftest::run();
        return Ok(());
    }

    // Internal: invisible GL probe child. Exits 0 if hardware GL is fine, non-zero
    // if the context failed (parent then re-runs with software rendering).
    if std::env::var_os("RUSTBURN_GL_PROBE").is_some() {
        return run_gl_probe();
    }

    // Auto-detect broken 3D acceleration and fall back to software, unless the
    // user has already forced a choice.
    if std::env::var_os("LIBGL_ALWAYS_SOFTWARE").is_none()
        && std::env::var_os("RUSTBURN_NO_GL_FALLBACK").is_none()
        && !hardware_gl_ok()
    {
        std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
        log::warn!(
            "Aceleración 3D no disponible; usando renderizado por software (llvmpipe). \
             Define RUSTBURN_NO_GL_FALLBACK=1 para desactivar esta detección."
        );
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("rustburn — control láser (GRBL)"),
        ..Default::default()
    };

    eframe::run_native(
        "rustburn",
        native_options,
        Box::new(|cc| Ok(Box::new(app::RustburnApp::new(cc)))),
    )
}

/// Run this same binary as an invisible probe child with a watchdog. Returns
/// true when hardware GL works, false when we should force software rendering.
fn hardware_gl_ok() -> bool {
    use std::process::Command;
    use std::time::{Duration, Instant};

    let Ok(exe) = std::env::current_exe() else {
        return true; // can't probe → assume OK, don't break good machines
    };
    let child = Command::new(exe)
        .env("RUSTBURN_GL_PROBE", "1")
        .env("RUSTBURN_NO_GL_FALLBACK", "1") // never recurse
        .spawn();
    let Ok(mut child) = child else {
        return true;
    };

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(6) {
                    // Stuck creating/using the context → treat as broken.
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return true,
        }
    }
}

/// The probe child: create an invisible window + GL context, close on the first
/// frame, then exit 0 (GL fine) or non-zero (context-creation failure detected).
fn run_gl_probe() -> eframe::Result<()> {
    struct ProbeApp;
    impl eframe::App for ProbeApp {
        fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_visible(false)
            .with_inner_size([64.0, 64.0]),
        ..Default::default()
    };
    let res = eframe::run_native(
        "rustburn-gl-probe",
        options,
        Box::new(|cc| {
            // Inspect the real GL context: egui needs OpenGL 3.x. An old driver
            // (e.g. Intel GMA, OpenGL 2.1) creates a context but can't render,
            // so treat version < 3.0 — or no context at all — as a failure.
            match &cc.gl {
                Some(gl) => {
                    use eframe::glow::HasContext as _;
                    let ver = unsafe { gl.get_parameter_string(eframe::glow::VERSION) };
                    if gl_major_version(&ver).map_or(true, |maj| maj < 3) {
                        GL_CONTEXT_FAILED.store(true, Ordering::SeqCst);
                    }
                }
                None => GL_CONTEXT_FAILED.store(true, Ordering::SeqCst),
            }
            Ok(Box::new(ProbeApp))
        }),
    );

    // Hard failure to create the context, GL too old, or the GLX error was seen.
    if res.is_err() || GL_CONTEXT_FAILED.load(Ordering::SeqCst) {
        std::process::exit(7);
    }
    std::process::exit(0);
}

/// Parse the major version from an OpenGL `VERSION` string: "2.1 Mesa 23.2.1" →
/// `Some(2)`, "4.6 (Core Profile) Mesa…" → `Some(4)`, "OpenGL ES 3.0 Mesa…" →
/// `Some(3)`. Returns the first token that parses as an integer.
pub(crate) fn gl_major_version(v: &str) -> Option<u32> {
    v.split(['.', ' ', '(', ')'])
        .find_map(|s| s.parse::<u32>().ok())
}
