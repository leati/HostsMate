//! 生成与写盘（规格 §2.1/§4）
//!
//! 生成不添加任何合成行，保证 generate∘parse 收敛（幂等）；
//! 写盘 = 同目录临时文件 + rename 原子替换（Windows 语义 MOVEFILE_REPLACE_EXISTING）。

use std::io::Write;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use super::model::{Doc, Entry, Record};

/// Doc → hosts 文本（CRLF 行尾，Windows 惯例，与旧版一致）
pub fn generate_str(doc: &Doc) -> String {
    let mut out = String::new();
    for line in &doc.prelude {
        out.push_str(line);
        out.push_str("\r\n");
    }
    for scheme in &doc.schemes {
        if scheme.explicit {
        out.push_str(super::parser::GROUP_HEAD);
        out.push_str(&scheme.name);
        out.push_str("\r\n");
        }
        for entry in &scheme.entries {
            match entry {
                Entry::Verbatim(v) => {
                    out.push_str(v);
                    out.push_str("\r\n");
                }
                Entry::Record(r) => push_record_line(&mut out, r),
            }
        }
    }
    out
}

/// 规范化记录行：`[#Off ]IP\t域名[ #备注]`
fn push_record_line(out: &mut String, r: &Record) {
    if !r.enabled {
        out.push_str("#Off ");
    }
    out.push_str(&r.ip.to_string());
    out.push('\t');
    out.push_str(&r.domain);
    if let Some(c) = &r.comment {
        out.push_str(" #");
        out.push_str(c);
    }
    out.push_str("\r\n");
}

/// Doc → 字节（UTF-8 无 BOM）
pub fn generate_bytes(doc: &Doc) -> Vec<u8> {
    super::encoding::encode_utf8(&generate_str(doc))
}

/// 原子写盘：写同目录临时文件后 rename 替换；共享冲突重试 2 次
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("hm-tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    let mut last_err = None;
    for attempt in 0..3 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt < 2 {
                    sleep(Duration::from_millis(150));
                }
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last_err.expect("至少一次 rename 尝试"))
}

/// 便捷封装：生成 + 原子写盘
pub fn save_doc(path: &Path, doc: &Doc) -> std::io::Result<()> {
    write_atomic(path, &generate_bytes(doc))
}
