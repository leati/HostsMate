//! 冲突自动检测（规格 §2.3）
//!
//! 只判定有效启用的记录（未带 #Off；方案开关已物化到记录行）。
//! 归一化：小写 + 去尾部点。同域名多条：文件顺序第一条生效。
//! 组内 IP 有异 → 真冲突；全同 → 冗余重复。

use std::collections::HashMap;
use std::net::IpAddr;

use super::model::{Doc, Entry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// 同域名、不同 IP —— 覆盖导致某条配置悄悄失效
    Real,
    /// 同域名、同 IP —— 无害但应清理
    Redundant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictMember {
    /// 所在方案下标（doc.schemes）
    pub scheme: usize,
    /// 所在条目下标（scheme.entries）
    pub entry: usize,
    pub ip: IpAddr,
    /// 是否为该域名的生效记录（文件顺序第一条）
    pub effective: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictGroup {
    /// 归一化后的域名
    pub domain: String,
    pub kind: ConflictKind,
    /// 按文件顺序排列的成员
    pub members: Vec<ConflictMember>,
}

/// 域名归一化：去首尾空白、小写、去尾部点
pub fn normalize_domain(domain: &str) -> String {
    domain
        .trim()
        .to_lowercase()
        .trim_end_matches('.')
        .to_string()
}

/// 全文冲突检测；组按域名首次出现的文件顺序返回
pub fn detect_conflicts(doc: &Doc) -> Vec<ConflictGroup> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<ConflictMember>> = HashMap::new();

    for (si, scheme) in doc.schemes.iter().enumerate() {
        for (ei, entry) in scheme.entries.iter().enumerate() {
            let Entry::Record(r) = entry else {
                continue;
            };
            if !r.enabled {
                continue;
            }
            let key = normalize_domain(&r.domain);
            if key.is_empty() {
                continue;
            }
            let list = groups.entry(key.clone()).or_default();
            let effective = list.is_empty();
            if effective {
                order.push(key);
            }
            list.push(ConflictMember {
                scheme: si,
                entry: ei,
                ip: r.ip,
                effective,
            });
        }
    }

    order
        .into_iter()
        .filter_map(|domain| {
            let members = groups.remove(&domain)?;
            if members.len() < 2 {
                return None;
            }
            let first_ip = members[0].ip;
            let kind = if members.iter().any(|m| m.ip != first_ip) {
                ConflictKind::Real
            } else {
                ConflictKind::Redundant
            };
            Some(ConflictGroup {
                domain,
                kind,
                members,
            })
        })
        .collect()
}
