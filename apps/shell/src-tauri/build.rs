// Tauri's build step: reads tauri.conf.json and the capabilities, and on
// Windows embeds the icon and manifest in the executable.
fn main() {
    tauri_build::build()
}
