//! xml-viewer: e-Gov などの電子文書（XML + XSL）を表示するビューア。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod api;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("xml-viewer は Windows 専用です");
}
