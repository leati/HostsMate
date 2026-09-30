//! hosts 引擎：单文件就地标注模型（规格 §2）

pub mod backup;
pub mod conflict;
pub mod encoding;
pub mod model;
pub mod parser;
pub mod writer;

#[cfg(test)]
mod tests;

pub use backup::snapshot_backup;
pub use conflict::{detect_conflicts, ConflictGroup, ConflictKind, ConflictMember};
pub use encoding::{decode_bytes, encode_utf8};
pub use model::{Doc, Entry, Record, Scheme, DEFAULT_SCHEME};
pub use parser::{parse_bytes, parse_str};
pub use writer::{generate_bytes, generate_str, replace_file_bytes, save_doc, write_atomic};

/// 系统 hosts 文件路径（%SystemRoot%\System32\drivers\etc\hosts）
pub fn system_hosts_path() -> std::path::PathBuf {
    std::env::var_os("SystemRoot")
        .map(|r| {
            std::path::PathBuf::from(r)
                .join("System32")
                .join("drivers")
                .join("etc")
                .join("hosts")
        })
        .unwrap_or_else(|| std::path::PathBuf::from("C:\\Windows\\System32\\drivers\\etc\\hosts"))
}
