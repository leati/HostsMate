//! M1 引擎单元测试（对应实现计划 T7 的 14 组用例）

use super::backup::timestamp_name;
use super::conflict::normalize_domain;
use super::parser::is_valid_domain;
use super::*;
use std::net::IpAddr;
use std::time::{Duration, SystemTime};

/// 旧版 hosts 工具 v3 格式夹具（头注释 + 分组 + #Off + 签名尾注）
const OLD_FORMAT: &str = "#头部注释：公司环境映射\n\
#第二行注释\n\
\n\
#Group: 开发\n\
10.0.0.5\t\tapi.test.com\t#测试环境后端\n\
#Off 10.0.0.6\t\tapi2.test.com\t#备用环境（停用）\n\
192.168.1.10\t\tnas.home\t#家里 NAS\n\
\n\
#Group: 屏蔽\n\
0.0.0.0\t\tads.example.com\t#广告域\n\
0.0.0.0\t\ttracker.net\n\
\n\
#This host file is build by HostsToolV3 (Legacy)\n\
#Last update: 2012/03/15\n";

fn rec_of(entry: &Entry) -> &Record {
    match entry {
        Entry::Record(r) => r,
        Entry::Verbatim(v) => panic!("期望记录行，得到 Verbatim: {v:?}"),
    }
}

fn verb_of(entry: &Entry) -> &str {
    match entry {
        Entry::Verbatim(v) => v,
        Entry::Record(r) => panic!("期望 Verbatim，得到记录: {r:?}"),
    }
}

/// 1. 纯 hosts（无任何标记）→ 全部进「默认」方案
#[test]
fn plain_hosts_goes_to_default_scheme() {
    let doc = parse_str("127.0.0.1 localhost\n10.0.0.1 foo.example.com # x\n");
    assert!(doc.prelude.is_empty());
    assert_eq!(doc.schemes.len(), 1);
    assert_eq!(doc.schemes[0].name, DEFAULT_SCHEME);
    assert_eq!(doc.schemes[0].entries.len(), 2);
    assert!(doc.schemes[0].entries.iter().all(|e| rec_of(e).enabled));
}

/// 1b. 无标记普通文件往返：不得引入 `#Group:` 组头（去除分类后保持纯净）
#[test]
fn plain_round_trip_keeps_headerless() {
    let src = "127.0.0.1 localhost
10.0.0.1 foo.example.com # x
";
    let doc = parse_str(src);
    assert!(doc.schemes.iter().all(|s| !s.explicit));
    let g = generate_str(&doc);
    assert!(!g.contains("#Group:"), "普通文件不应被写入组头: {g}");
    assert_eq!(parse_str(&g), doc, "往返应收敛");
}

/// 2. 旧版格式夹具：结构断言（prelude / 分组 / #Off / 尾注）
#[test]
fn old_format_structure() {
    let doc = parse_str(OLD_FORMAT);
    assert_eq!(
        doc.prelude,
        vec!["#头部注释：公司环境映射", "#第二行注释", ""]
    );
    assert_eq!(doc.schemes.len(), 2);
    assert_eq!(doc.schemes[0].name, "开发");
    assert_eq!(doc.schemes[1].name, "屏蔽");

    let dev = &doc.schemes[0].entries;
    assert_eq!(dev.len(), 4); // 3 记录 + 1 空行
    let r0 = rec_of(&dev[0]);
    assert_eq!(r0.domain, "api.test.com");
    assert_eq!(r0.ip, IpAddr::from([10, 0, 0, 5]));
    assert!(r0.enabled);
    assert_eq!(r0.comment.as_deref(), Some("测试环境后端"));
    let r1 = rec_of(&dev[1]);
    assert!(!r1.enabled);
    assert_eq!(r1.domain, "api2.test.com");
    assert_eq!(verb_of(&dev[3]), "");

    let block = &doc.schemes[1].entries;
    assert_eq!(block.len(), 5); // 2 记录 + 空行 + 2 行签名
    assert_eq!(verb_of(&block[2]), "");
    assert!(verb_of(&block[3]).starts_with("#This host file is build by HostsToolV3"));
}

/// 3. 往返幂等：结构等价 + 文本收敛
#[test]
fn round_trip_idempotent() {
    let doc1 = parse_str(OLD_FORMAT);
    let g1 = generate_str(&doc1);
    let doc2 = parse_str(&g1);
    assert_eq!(doc1, doc2, "生成文本重新解析应与原文档结构一致");
    let g2 = generate_str(&doc2);
    assert_eq!(g1, g2, "generate∘parse 应收敛");
}

/// 4. 块内注释/空行原位保留（顺序不变）
#[test]
fn verbatim_kept_in_place() {
    let src = "#Group: A\n1.1.1.1 a.com\n#中间注释\n2.2.2.2 b.com\n";
    let doc = parse_str(src);
    let e = &doc.schemes[0].entries;
    assert_eq!(e.len(), 3);
    assert_eq!(rec_of(&e[0]).domain, "a.com");
    assert_eq!(verb_of(&e[1]), "#中间注释");
    assert_eq!(rec_of(&e[2]).domain, "b.com");
    let g = generate_str(&doc);
    assert!(g.contains("#中间注释\n") || g.contains("#中间注释\r\n"));
}

/// 5. 非法行原样保留（坏 IP / 孤立 IP / 纯单词 / #Office 注释）
#[test]
fn unknown_lines_preserved() {
    let src = concat!(
        "999.1.1.1 bad.com\n",   // 非法 IP
        "justdomain\n",          // 无 IP
        "1.2.3.4\n",             // 无域名
        "hello world\n",         // 首词非 IP
        "#Office note\n",        // #Off 前缀但非记录 → 注释
    );
    let doc = parse_str(src);
    // 无任何记录 → 不创建方案块，全部原样保留在 prelude
    assert!(doc.schemes.is_empty());
    assert_eq!(doc.prelude.len(), 5);
    for (i, line) in src.lines().enumerate() {
        assert_eq!(doc.prelude[i], line);
    }
    // 生成的文本逐行含原内容
    let g = generate_str(&doc);
    for line in src.lines() {
        assert!(g.contains(line), "丢失原样行: {line}");
    }
}

/// 6. 一行多域名拆分为多条记录（同 IP 同备注），保存为多行
#[test]
fn multi_domain_line_split() {
    let doc = parse_str("1.2.3.4 a.com b.com # c\n");
    let entries = &doc.schemes[0].entries;
    assert_eq!(entries.len(), 2);
    assert_eq!(rec_of(&entries[0]).domain, "a.com");
    assert_eq!(rec_of(&entries[1]).domain, "b.com");
    let r0 = rec_of(&entries[0]);
    assert_eq!(r0.comment.as_deref(), Some(" c")); // 备注保留 # 后原文（含前导空格）
    let g = generate_str(&doc);
    assert!(g.contains("1.2.3.4\ta.com # c"));
    assert!(g.contains("1.2.3.4\tb.com # c"));
}

/// 7. UTF-8 BOM 读取
#[test]
fn utf8_bom_decoded() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice("# 中文注释\n1.2.3.4 a.com\n".as_bytes());
    let doc = parse_bytes(&bytes);
    assert_eq!(doc.prelude, vec!["# 中文注释"]);
    assert_eq!(rec_of(&doc.schemes[0].entries[0]).domain, "a.com");
}

/// 8. GBK 中文注释读取 → 输出 UTF-8
#[test]
fn gbk_decoded_and_utf8_output() {
    let text = "# 中文备注\n1.2.3.4 a.com # 备注测试\n";
    let (gbk, _enc, _ok) = encoding_rs::GBK.encode(text);
    assert!(std::str::from_utf8(&gbk).is_err() || gbk.len() != text.len()); // 确实不是 UTF-8 直拷
    let doc = parse_bytes(&gbk);
    assert_eq!(doc.prelude, vec!["# 中文备注"]);
    assert_eq!(rec_of(&doc.schemes[0].entries[0]).comment.as_deref(), Some(" 备注测试"));
    let out = generate_bytes(&doc);
    assert_eq!(std::str::from_utf8(&out).unwrap(), generate_str(&doc));
    assert!(String::from_utf8_lossy(&out).contains("备注测试"));
}

/// 9. #Off 从严判定：#Off/#OFF + 空白 = 禁用记录；其余 = 注释
#[test]
fn off_prefix_strict() {
    let src = "#Off 1.2.3.4 x.com\n#OFF\t1.2.3.4 y.com\n#OffIce note\n";
    let doc = parse_str(src);
    let e = &doc.schemes[0].entries;
    assert_eq!(e.len(), 3);
    let r0 = rec_of(&e[0]);
    assert!(!r0.enabled && r0.domain == "x.com");
    let r1 = rec_of(&e[1]);
    assert!(!r1.enabled && r1.domain == "y.com");
    assert_eq!(verb_of(&e[2]), "#OffIce note");
}

/// 10. 真冲突：同域名不同 IP → Real，首条生效
#[test]
fn real_conflict_first_effective() {
    let src = "#Group: A\n1.1.1.1 a.com\n2.2.2.2 a.com\n3.3.3.3 b.com\n";
    let groups = detect_conflicts(&parse_str(src));
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert_eq!(g.domain, "a.com");
    assert_eq!(g.kind, ConflictKind::Real);
    assert_eq!(g.members.len(), 2);
    assert!(g.members[0].effective);
    assert!(!g.members[1].effective);
    assert_eq!(g.members[0].scheme, 0);
    assert_eq!((g.members[0].entry, g.members[1].entry), (0, 1));
}

/// 11. 冗余重复：同域名同 IP → Redundant
#[test]
fn redundant_conflict() {
    let src = "1.1.1.1 a.com\n1.1.1.1 a.com\n";
    let groups = detect_conflicts(&parse_str(src));
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].kind, ConflictKind::Redundant);
    assert_eq!(groups[0].members.len(), 2);
}

/// 12. 禁用记录不参与冲突
#[test]
fn disabled_records_not_conflicting() {
    let src = "1.1.1.1 a.com\n#Off 2.2.2.2 a.com\n";
    assert!(detect_conflicts(&parse_str(src)).is_empty());
}

/// 13. 归一化：大小写 + 尾部点
#[test]
fn domain_normalization() {
    assert_eq!(normalize_domain("A.COM."), "a.com");
    let src = "1.1.1.1 A.COM\n1.1.1.1 a.com.\n";
    let groups = detect_conflicts(&parse_str(src));
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].domain, "a.com");
    assert_eq!(groups[0].kind, ConflictKind::Redundant);
}

/// 14a. 备份快照 + 超量修剪
#[test]
fn backup_snapshot_and_prune() {
    let dir = std::env::temp_dir().join(format!("hm-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let hosts = dir.join("hosts");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&hosts, "1.2.3.4 a.com\n").unwrap();

    for name in [
        "hosts-20200101-000001.host",
        "hosts-20200102-000001.host",
        "hosts-20200103-000001.host",
    ] {
        std::fs::write(dir.join(name), "old").unwrap();
    }

    let snap = snapshot_backup(&hosts, &dir, 2).unwrap();
    assert!(snap.exists());
    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("hosts-") && n.ends_with(".host"))
        .collect();
    left.sort();
    assert_eq!(left.len(), 2, "应只保留最近 2 份: {left:?}");
    assert!(left[0].starts_with("hosts-20200103")); // 次新
    assert!(left[1].starts_with(&format!("hosts-{}", &timestamp_name(SystemTime::now()))[..8]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 14b. 原子写盘替换既有文件，无临时残留
#[test]
fn atomic_write_replaces() {
    let dir = std::env::temp_dir().join(format!("hm-test-w-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("hosts");
    std::fs::write(&target, "OLD").unwrap();
    write_atomic(&target, b"NEW").unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "NEW");
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".hm-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "临时文件应被清理");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 时间戳转换锚点：2026-09-26 00:00:00 UTC = 1,790,380,800
#[test]
fn timestamp_conversion() {
    let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_380_800);
    assert_eq!(timestamp_name(t), "20260926-000000");
    assert_eq!(timestamp_name(SystemTime::UNIX_EPOCH), "19700101-000000");
}

/// 15. 域名白名单：合法域名通过；命令注入与“写盘重载变形”形态全拒。
/// （`#` 截断形态由入库口拦截——parse 侧 `#` 属注释语法，无法也无需区分）
#[test]
fn domain_whitelist() {
    assert!(is_valid_domain("a.com"));
    assert!(is_valid_domain("A.COM-2.example"));
    assert!(is_valid_domain("xn--fiqs8s.example"));
    assert!(is_valid_domain("_dmarc.example.com"));
    assert!(is_valid_domain("localhost"));
    assert!(!is_valid_domain(""));
    assert!(!is_valid_domain("evil&calc"));
    assert!(!is_valid_domain("evil|whoami"));
    assert!(!is_valid_domain("a<b>c"));
    assert!(!is_valid_domain("a%PATH%"));
    assert!(!is_valid_domain("a b"));
    assert!(!is_valid_domain("域名.cn"));
    assert!(!is_valid_domain(&"x".repeat(254)));
}

/// 16. 含非法域名 token 的记录行整体按原样保留：不结构化（防注入面）、
/// 不部分拆用（防丢行）、往返收敛
#[test]
fn malicious_domain_lines_preserved_verbatim() {
    let src = "1.2.3.4 good.com\n1.2.3.4 evil&calc\n2.2.2.2 ok.cn bad;token\n";
    let doc = parse_str(src);
    let e = &doc.schemes[0].entries;
    assert_eq!(e.len(), 3);
    assert_eq!(rec_of(&e[0]).domain, "good.com");
    assert_eq!(verb_of(&e[1]), "1.2.3.4 evil&calc", "注入形态应整行原样保留");
    assert_eq!(verb_of(&e[2]), "2.2.2.2 ok.cn bad;token", "好 token 混坏 token 也整行保留");
    let g = generate_str(&doc);
    assert_eq!(parse_str(&g), doc, "含保留行的往返应收敛");
    assert!(g.contains("1.2.3.4 evil&calc"), "原行内容不得丢失");
}

/// 17. 提权写盘用的原子替换：内容替换成功、无临时残留、
/// 目标不存在时失败且不留半成品
#[test]
fn replace_file_bytes_atomic() {
    let dir = std::env::temp_dir().join(format!("hm-test-r-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("hosts");
    std::fs::write(&target, "OLD").unwrap();
    replace_file_bytes(&target, b"NEW").unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "NEW");
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".hm-new"))
        .collect();
    assert!(leftovers.is_empty(), "替换临时文件应被清理");
    // 目标不存在（GetNamedSecurityInfo 失败）：报错且不产生残留
    let missing = dir.join("none");
    assert!(replace_file_bytes(&missing, b"x").is_err());
    assert!(!missing.exists());
    assert!(!dir.join("none.hm-new").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
