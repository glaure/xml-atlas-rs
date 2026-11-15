//! XML Atlas desktop app.

mod app;
mod cell_pane;
mod disk_watch;
mod find_bar;
mod grid;
mod highlight;
mod icons;
mod nav;
mod problems_panel;
mod scroll;
mod settings;
mod system_theme;
mod tabs;
mod text_view;
mod tree;

/// Set to `wgpu` to draw with wgpu instead of OpenGL (D21). The app sets it itself when
/// OpenGL fails, and starts again.
const RENDERER_VAR: &str = "XML_ATLAS_RENDERER";

/// The application id: the name of the desktop entry and icon the Linux packages install
/// (PKG-33), so that the desktop shows the window with them.
const APP_ID: &str = "io.github.glaure.XmlAtlas";

fn main() -> eframe::Result {
    env_logger::init();
    // Each file named opens in its own tab (OPEN-1, OPEN-5).
    let paths: Vec<std::path::PathBuf> = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .collect();
    let wgpu = std::env::var(RENDERER_VAR).is_ok_and(|v| v.eq_ignore_ascii_case("wgpu"));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("XML Atlas")
            .with_app_id(APP_ID)
            .with_inner_size([1280.0, 820.0])
            .with_drag_and_drop(true),
        // OpenGL starts in about half the time and memory of wgpu (D21).
        renderer: if wgpu {
            eframe::Renderer::Wgpu
        } else {
            eframe::Renderer::Glow
        },
        ..Default::default()
    };
    let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let app_started = started.clone();
    let result = eframe::run_native(
        "XML Atlas",
        options,
        Box::new(move |cc| {
            app_started.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(Box::new(app::App::new(cc, paths)))
        }),
    );
    // OpenGL failed before there was a window: start again with wgpu. A second event loop is
    // not allowed in one process, hence a new one.
    if let Err(e) = &result
        && !wgpu
        && !started.load(std::sync::atomic::Ordering::Relaxed)
    {
        log::warn!("OpenGL failed ({e}); starting again with wgpu");
        let status = std::env::current_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .args(std::env::args_os().skip(1))
                .env(RENDERER_VAR, "wgpu")
                .status()
        });
        match status {
            Ok(s) => std::process::exit(s.code().unwrap_or(1)),
            Err(e) => log::error!("cannot start again with wgpu: {e}"),
        }
    }
    result
}
