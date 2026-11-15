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

fn main() -> eframe::Result {
    env_logger::init();
    // Each file named opens in its own tab (OPEN-1, OPEN-5).
    let paths = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("XML Atlas")
            .with_inner_size([1280.0, 820.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "XML Atlas",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, paths)))),
    )
}
