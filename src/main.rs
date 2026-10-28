#[cfg(not(target_arch = "wasm32"))]
const APP_ID: &str = "aseprite-metadata-editor";

#[cfg(not(target_arch = "wasm32"))]
fn native_options() -> eframe::NativeOptions {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png"))
        .expect("bundled application icon is valid PNG");
    eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_inner_size([1000.0, 700.0])
            .with_icon(icon),
        // egui's app_id sets Wayland identity; X11 needs its WM_CLASS as well.
        #[cfg(target_os = "linux")]
        window_builder: Some(Box::new(|attributes| {
            winit::platform::x11::WindowAttributesExtX11::with_name(attributes, APP_ID, APP_ID)
        })),
        ..Default::default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    eframe::run_native(
        "Aseprite Metadata Editor",
        native_options(),
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn linux_launcher_identity_and_supplied_icon_are_explicit() {
        let options = native_options();
        assert_eq!(options.viewport.app_id.as_deref(), Some(APP_ID));
        let icon = options.viewport.icon.unwrap();
        assert_eq!((icon.width, icon.height), (256, 256));
        let original =
            eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap();
        for y in 0..256usize {
            for x in 0..256usize {
                let offset = (y * 256 + x) * 4;
                let source = ((y / 16) * 16 + x / 16) * 4;
                assert_eq!(
                    &icon.rgba[offset..offset + 4],
                    &original.rgba[source..source + 4]
                );
            }
        }
    }
}
