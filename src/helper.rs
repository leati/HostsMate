//! 提权写入 helper：以管理员身份被父进程启动（`--write-hosts <临时文件>`），
//! 一次性把最终字节写入系统 hosts 后退出。退出码：0 成功 / 1 写入失败 / 2 读入失败。
//! 写入方式为覆盖既有文件内容（保留原 ACL），不做解析——字节即真相。

pub fn write_hosts(tmp: &str) -> i32 {
    let target = hostsmate::engine::system_hosts_path();
    let Ok(bytes) = std::fs::read(tmp) else {
        return 2;
    };
    match std::fs::write(&target, &bytes) {
        Ok(()) => {
            let _ = std::fs::remove_file(tmp);
            0
        }
        Err(_) => 1,
    }
}
