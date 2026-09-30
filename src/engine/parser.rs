//! 解析：文本 → Doc（规格 §2.1/§2.2）
//!
//! 原则：识别不了的行一律原样保留（Verbatim），绝不丢弃；
//! `#Off` 从严判定（后接空白且剩余可解析为记录才算禁用记录），
//! 修复旧版「#Office note 之类注释被误删」的缺陷。

use std::net::IpAddr;

use super::model::{Doc, Entry, Record, Scheme, DEFAULT_SCHEME};

pub const GROUP_HEAD: &str = "#Group: ";
const OFF_PREFIX: &str = "#off";

/// 解码并解析整个 hosts 文件
pub fn parse_bytes(bytes: &[u8]) -> Doc {
    parse_str(&super::encoding::decode_bytes(bytes))
}

/// 解析 hosts 文本（行分隔符兼容 LF/CRLF）
pub fn parse_str(text: &str) -> Doc {
    let mut doc = Doc {
        prelude: Vec::new(),
        schemes: Vec::new(),
    };
    // current = Some(方案下标)；None = 尚未遇到任何 #Group（孤儿区）
    let mut current: Option<usize> = None;

    for line in text.lines() {
        let trimmed = line.trim();

        if let Some(name) = trimmed.strip_prefix(GROUP_HEAD) {
            doc.schemes.push(Scheme {
                name: name.trim().to_string(),
                explicit: true,
                entries: Vec::new(),
            });
            current = Some(doc.schemes.len() - 1);
            continue;
        }

        // 禁用记录：#off（忽略大小写）+ 空白 + 可解析的记录体
        if let Some(rest) = strip_off_prefix(trimmed) {
            if let Some(records) = parse_record_line(rest, false) {
                for rec in records {
                    push_record(&mut doc, &mut current, rec);
                }
                continue;
            }
        }

        if trimmed.is_empty() || trimmed.starts_with('#') {
            push_verbatim(&mut doc, &mut current, line.to_string());
            continue;
        }

        match parse_record_line(trimmed, true) {
            Some(records) => {
                for rec in records {
                    push_record(&mut doc, &mut current, rec);
                }
            }
            None => push_verbatim(&mut doc, &mut current, line.to_string()),
        }
    }
    doc
}

/// `#off` 前缀（忽略大小写），且必须后接空白；返回其后剩余部分
fn strip_off_prefix(trimmed: &str) -> Option<&str> {
    // 字符边界安全切片：首 4 字节落在多字节字符内时按非记录处理
    let head = trimmed.get(..OFF_PREFIX.len())?;
    if !head.eq_ignore_ascii_case(OFF_PREFIX) {
        return None;
    }
    let rest = &trimmed[OFF_PREFIX.len()..];
    rest.starts_with(char::is_whitespace).then_some(rest)
}

/// 解析记录体：`IP 域名... [#备注]`；一行多域名拆成多条记录
fn parse_record_line(line: &str, enabled: bool) -> Option<Vec<Record>> {
    let line = line.trim();
    let (body, comment) = match line.find('#') {
        Some(i) => (&line[..i], Some(line[i + 1..].to_string())),
        None => (line, None),
    };
    let mut tokens = body.split_whitespace();
    let ip: IpAddr = tokens.next()?.parse().ok()?;
    let mut records = Vec::new();
    for domain in tokens {
        records.push(Record {
            ip,
            domain: domain.to_string(),
            comment: comment.clone(),
            enabled,
        });
    }
    if records.is_empty() {
        None
    } else {
        Some(records)
    }
}

fn ensure_default(doc: &mut Doc) -> usize {
    if doc.schemes.first().is_none_or(|s| s.name != DEFAULT_SCHEME) {
        doc.schemes.insert(
            0,
            Scheme {
                name: DEFAULT_SCHEME.to_string(),
                explicit: false,
                entries: Vec::new(),
            },
        );
    }
    0
}

fn push_record(doc: &mut Doc, current: &mut Option<usize>, rec: Record) {
    let idx = match *current {
        Some(i) => i,
        None => ensure_default(doc),
    };
    doc.schemes[idx].entries.push(Entry::Record(rec));
}

fn push_verbatim(doc: &mut Doc, current: &mut Option<usize>, line: String) {
    let idx = match *current {
        Some(i) => i,
        None => {
            // 孤儿区：若已出现孤儿记录（默认方案已建），注释归入其中以保持原位；
            // 首条记录之前的注释/空行才是真正的文件头（prelude）
            if doc
                .schemes
                .first()
                .is_some_and(|s| s.name == DEFAULT_SCHEME)
            {
                0
            } else {
                doc.prelude.push(line);
                return;
            }
        }
    };
    doc.schemes[idx].entries.push(Entry::Verbatim(line));
}
