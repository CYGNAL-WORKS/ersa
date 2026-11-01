//! The `ersa` executable: opens the window. Everything else is in the
//! `ersa_shell` library, so tests can use it without a window.

// Without this, a release build on Windows opens a console window next to the
// app. Debug builds keep the console for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ersa_shell::run();
}
