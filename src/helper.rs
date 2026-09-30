//! 提权写入 helper：以管理员身份被父进程启动（`--write-hosts <中转文件> <sha256>`），
//! 校验中转文件内容哈希后，把字节原子写入系统 hosts 并退出。字节即真相，不做解析。
//! 退出码：0 成功 / 1 写入失败 / 2 读入或参数失败 / 3 哈希校验失败（内容被改动）。

use sha2::{Digest, Sha256};

pub fn write_hosts(tmp: &str, sha_hex: &str) -> i32 {
    let target = hostsmate::engine::system_hosts_path();
    let Ok(bytes) = std::fs::read(tmp) else {
        return 2;
    };
    // 中转文件在 UAC 确认窗口期内可能被同用户进程替换（TOCTOU）：
    // 与父进程经命令行传来的内容哈希比对，不一致即拒绝写入并删除中转文件
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !hex.eq_ignore_ascii_case(sha_hex) {
        let _ = std::fs::remove_file(tmp);
        return 3;
    }
    match hostsmate::engine::replace_file_bytes(&target, &bytes) {
        Ok(()) => {
            let _ = std::fs::remove_file(tmp);
            0
        }
        Err(_) => 1,
    }
}
