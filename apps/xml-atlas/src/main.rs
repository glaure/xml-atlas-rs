//! XML Atlas desktop app.

mod app;
mod cell_pane;
mod disk_watch;
mod find_bar;
mod grid;
mod help;
mod highlight;
mod icons;
#[cfg(target_os = "macos")]
mod macos_open;
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

/// The window's icon, for the taskbar on Windows and X11; drawn like
/// `packaging/linux/io.github.glaure.XmlAtlas.svg`. Wayland takes the desktop entry's.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon-64.png");

fn main() -> eframe::Result {
    env_logger::init();
    // Each file named opens in its own tab (OPEN-1, OPEN-5).
    let paths: Vec<std::path::PathBuf> = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .collect();
    // Files from Finder, the Dock and `open` (D26); elsewhere they come as arguments.
    #[cfg(target_os = "macos")]
    let opened = Some(macos_open::listen());
    #[cfg(not(target_os = "macos"))]
    let opened = None;
    let wgpu = std::env::var(RENDERER_VAR).is_ok_and(|v| v.eq_ignore_ascii_case("wgpu"));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("XML Atlas")
        .with_app_id(APP_ID);
    match eframe::icon_data::from_png_bytes(ICON_PNG) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(e) => log::warn!("window icon: {e}"),
    }
    let options = eframe::NativeOptions {
        viewport: viewport
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
    // The app takes the receiver; when it never starts, the files in it go to the restart.
    let opened = std::sync::Arc::new(std::sync::Mutex::new(opened));
    let app_opened = opened.clone();
    let result = eframe::run_native(
        "XML Atlas",
        options,
        Box::new(move |cc| {
            app_started.store(true, std::sync::atomic::Ordering::Relaxed);
            let opened = app_opened.lock().ok().and_then(|mut o| o.take());
            Ok(Box::new(app::App::new(cc, paths, opened)))
        }),
    );
    // OpenGL failed before there was a window: start again with wgpu. A second event loop is
    // not allowed in one process, hence a new one.
    if let Err(e) = &result
        && !wgpu
        && !started.load(std::sync::atomic::Ordering::Relaxed)
    {
        log::warn!("OpenGL failed ({e}); starting again with wgpu");
        // Files from Finder came as an event to this process: the next one gets them as
        // arguments.
        let from_finder: Vec<std::path::PathBuf> = opened
            .lock()
            .ok()
            .and_then(|mut o| o.take())
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        let status = std::env::current_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .args(std::env::args_os().skip(1))
                .args(from_finder)
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

#[cfg(test)]
mod tests {
    /// `unsafe` is allowed only in `macos_open.rs` (`docs/decisions.md`, D26).
    #[test]
    fn unsafe_code_is_allowed_only_for_files_from_finder() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut allowing = Vec::new();
        for entry in std::fs::read_dir(&src).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            if text.contains(&["allow(", "unsafe_code)"].concat()) {
                allowing.push(path.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
        assert_eq!(allowing, ["macos_open.rs"]);
    }
}
