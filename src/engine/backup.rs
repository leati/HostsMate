//! 备份快照（规格 §2.4）：写盘前快照，按份数修剪
//!
//! 文件名 `hosts-YYYYMMDD-HHMMSS.host`（UTC，零填充 → 字典序即时间序）。
//! 单文件模型下该备份承担数据库级重要性。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 快照 hosts_path 到 backup_dir，保留最近 keep 份，返回快照路径
pub fn snapshot_backup(hosts_path: &Path, backup_dir: &Path, keep: usize) -> std::io::Result<PathBuf> {
    fs::create_dir_all(backup_dir)?;
    let name = format!("hosts-{}.host", timestamp_name(SystemTime::now()));
    let dest = backup_dir.join(name);
    fs::copy(hosts_path, &dest)?;
    prune_backups(backup_dir, keep)?;
    Ok(dest)
}

/// 删除最旧的快照，只保留 keep 份（同一秒内多个快照按文件名去重计数，可能保留略多，无害）
fn prune_backups(backup_dir: &Path, keep: usize) -> std::io::Result<()> {
    let mut names: Vec<String> = fs::read_dir(backup_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("hosts-") && n.ends_with(".host"))
        .collect();
    names.sort(); // 零填充时间戳 → 字典序 = 时间序
    if names.len() > keep {
        for name in &names[..names.len() - keep] {
            let _ = fs::remove_file(backup_dir.join(name));
        }
    }
    Ok(())
}

/// SystemTime → `YYYYMMDD-HHMMSS`（UTC）
pub fn timestamp_name(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        y,
        m,
        d,
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// 天数（自 1970-01-01）→ 公历年月日（Howard Hinnant 算法）
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}
