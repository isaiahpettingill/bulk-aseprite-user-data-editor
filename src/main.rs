#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default();
    eframe::run_native(
        "Aseprite Metadata Editor",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([1000.0, 700.0])
                .with_icon(icon),
            ..Default::default()
        },
        Box::new(|_| Ok(Box::<aseprite_metadata_editor::Editor>::default())),
    )
}
#[cfg(target_arch = "wasm32")]
fn main() {
    wasm_bindgen_futures::spawn_local(async {
        aseprite_metadata_editor::start()
            .await
            .expect("Editor startup failed");
    });
}
