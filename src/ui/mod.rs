//! UI 层：Win32 原生双栏主从窗口

mod app;
mod commands;
mod draw;
mod create;
mod elevate;
mod table;
mod wnd;

use std::path::PathBuf;

pub fn run(file: PathBuf) -> i32 {
    unsafe { wnd::run_main(file) }
}
