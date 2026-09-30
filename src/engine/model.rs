//! 数据模型：单文件就地标注（规格 §2.1）

use std::net::IpAddr;

/// 孤儿记录（第一个 `#Group:` 之前）归属的方案名
pub const DEFAULT_SCHEME: &str = "默认";

/// 一条 hosts 记录（一行一个域名；多域名行在解析期拆分）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub ip: IpAddr,
    /// 域名，保留原始大小写
    pub domain: String,
    /// `#` 之后到行尾的备注原文（不含 `#`，保留前导空格以实现往返稳定）
    pub comment: Option<String>,
    pub enabled: bool,
}

/// 方案块内的条目：记录行或原样保留行（注释/空行/未知行）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Record(Record),
    Verbatim(String),
}

/// 方案 = 文件内的一个 `#Group:` 块
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    pub name: String,
    /// 文件中是否存在 `#Group:` 标记（隐式默认组写出时不加组头，保持普通文件纯净）
    pub explicit: bool,
    pub entries: Vec<Entry>,
}

/// 整个 hosts 文件的结构化视图
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    /// 第一个 `#Group:` 之前的注释/空行/未知行（原样，不含行尾换行）
    pub prelude: Vec<String>,
    /// 方案块，顺序 = 文件顺序；「默认」若存在必在首位
    pub schemes: Vec<Scheme>,
}
