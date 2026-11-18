// Tauri's build step: reads tauri.conf.json and the capabilities, and on
// Windows embeds the icon and manifest in the executable. The app manifest
// declares the Shell's own commands, so capabilities/default.json must grant
// each one by name to the windows that may call it.
fn main() {
    let commands = tauri_build::AppManifest::new().commands(&[
        "core_attach",
        "core_request",
        "shell_show",
        "shell_hide",
        "shell_quit_heard",
        "shell_quit",
        "shell_open_link",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(commands))
        .expect("the Tauri build step failed");
    // The tests that run Tauri's mock runtime (tests/close_and_quit.rs) link
    // Windows' common controls, whose version 6 only a manifest asks for;
    // without it the test binary cannot start. The Tauri build step above
    // embeds one in the executable only.
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target == "windows" && env == "msvc" {
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
