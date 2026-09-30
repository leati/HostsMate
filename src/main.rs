//! HostsMate —— 绿色单文件 Windows hosts 管理器
//! 数据模型：单文件就地标注（规格 docs/superpowers/specs/2026-09-26-hostsmate-design.md）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod helper;
mod ui;

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // 提权 helper：<中转文件> <sha256> 校验后原子写入系统 hosts，随即退出
    if args.first().map_or(false, |a| a == "--write-hosts") {
        let code = match (args.get(1), args.get(2)) {
            (Some(p), Some(sha)) => helper::write_hosts(p, sha),
            _ => 2,
        };
        std::process::exit(code);
    }

    // 默认管理系统 hosts；--file 可指定任意 hosts 文件（M2 调试习惯保留）
    let file = parse_file_arg(&args).unwrap_or_else(hostsmate::engine::system_hosts_path);
    std::process::exit(ui::run(file));
}

fn parse_file_arg(args: &[String]) -> Option<PathBuf> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--file" {
            if let Some(p) = it.next() {
                return Some(PathBuf::from(p));
            }
        } else if !a.starts_with('-') {
            return Some(PathBuf::from(a));
        }
    }
    None
}
