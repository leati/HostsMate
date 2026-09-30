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

/// 原子替换目标文件内容：字节写入目标同目录临时文件 → 复制目标既有安全
/// 描述符（Owner/Group/DACL）→ MoveFileEx(REPLACE_EXISTING) 替换。
/// 任一步失败时原文件保持不动。供提权 helper 写系统 hosts：
/// 直接 fs::write 覆盖在中断时会留下半截文件，且替换后新文件的 ACL
/// 来自临时文件——不显式复制，hosts 上 Everyone 只读之类的授权会丢失。
#[cfg(windows)]
pub fn replace_file_bytes(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION,
        OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    };
    use windows::Win32::Security::Authorization::{
        GetNamedSecurityInfoW, SE_FILE_OBJECT, SetNamedSecurityInfoW,
    };
    use windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MoveFileExW};

    let tmp = target.with_extension("hm-new");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    let io_err = |msg: String| std::io::Error::new(std::io::ErrorKind::Other, msg);
    let replace = || -> std::io::Result<()> {
        unsafe {
            let wtarget = HSTRING::from(target.as_os_str());
            let mut owner = PSID(std::ptr::null_mut());
            let mut group = PSID(std::ptr::null_mut());
            let mut dacl: *mut ACL = std::ptr::null_mut();
            let mut sacl: *mut ACL = std::ptr::null_mut();
            let mut sd = PSECURITY_DESCRIPTOR(std::ptr::null_mut());
            let info: OBJECT_SECURITY_INFORMATION = OWNER_SECURITY_INFORMATION
                | GROUP_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION;
            let rc = GetNamedSecurityInfoW(
                &wtarget,
                SE_FILE_OBJECT,
                info,
                Some(&mut owner),
                Some(&mut group),
                Some(&mut dacl),
                Some(&mut sacl),
                &mut sd,
            );
            if rc.0 != 0 {
                return Err(io_err(format!("读取目标 ACL 失败（Win32 错误 {}）", rc.0)));
            }
            // owner/group/dacl 指针都指向 sd 内部缓冲：先设置到临时文件，再释放 sd
            let wtmp = HSTRING::from(tmp.as_os_str());
            let rc2 = SetNamedSecurityInfoW(
                &wtmp,
                SE_FILE_OBJECT,
                info,
                owner,
                group,
                Some(dacl as *const ACL),
                None,
            );
            let _ = LocalFree(windows::Win32::Foundation::HLOCAL(sd.0 as *mut core::ffi::c_void));
            if rc2.0 != 0 {
                return Err(io_err(format!("复制 ACL 到临时文件失败（Win32 错误 {}）", rc2.0)));
            }
            MoveFileExW(&wtmp, &wtarget, MOVEFILE_REPLACE_EXISTING)
                .map_err(|e| io_err(format!("原子替换失败：{e}")))?;
        }
        Ok(())
    };
    if let Err(e) = replace() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}
