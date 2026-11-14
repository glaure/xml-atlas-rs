//! XML Atlas desktop app.

mod app;
mod grid;
mod highlight;
mod text_view;
mod tree;

fn main() -> eframe::Result {
    env_logger::init();
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
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
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, path)))),
    )
}
