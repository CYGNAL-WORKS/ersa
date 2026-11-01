// Tauri's build step: reads tauri.conf.json and the capabilities, and on
// Windows embeds the icon and manifest in the executable. The app manifest
// declares the Shell's own commands, so capabilities/default.json must grant
// each one by name to the windows that may call it.
fn main() {
    let commands = tauri_build::AppManifest::new().commands(&["core_attach", "core_request"]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(commands))
        .expect("the Tauri build step failed");
}
