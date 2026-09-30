//! 权限与提权：管理员检测、UAC 提权执行自身、DNS 缓存刷新

use std::path::Path;

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HANDLE, HWND, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, WaitForSingleObject, CREATE_NO_WINDOW,
    PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::Shell::{
    IsUserAnAdmin, ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS,
    SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin().as_bool() }
}

/// 以 runas 提权执行自身并等待退出，返回子进程退出码（用户取消 UAC → Err）
pub fn elevate_and_wait(exe: &Path, args: &str, hwnd: HWND) -> Result<u32, String> {
    let file = HSTRING::from(exe.as_os_str());
    let params = HSTRING::from(args);
    let mut code_out = 0u32;
    unsafe {
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI,
            hwnd,
            lpVerb: w!("runas"),
            lpFile: std::mem::transmute(file.as_ptr()),
            lpParameters: std::mem::transmute(params.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        ShellExecuteExW(&mut sei).map_err(|e| {
            if e.code().0 & 0xFFFF == 1223 {
                "已取消管理员授权，未保存".to_string()
            } else {
                format!("提权失败：{e}")
            }
        })?;
        if sei.hProcess.is_invalid() {
            return Err("提权失败（未获得进程句柄）".into());
        }
        if WaitForSingleObject(sei.hProcess, 30_000) != WAIT_OBJECT_0 {
            return Err("提权写入进程等待超时".into());
        }
        GetExitCodeProcess(sei.hProcess, &mut code_out).map_err(|e| format!("获取结果失败：{e}"))?;
    }
    Ok(code_out)
}

/// `ipconfig /flushdns`（隐藏窗口，不阻塞失败）
pub fn flushdns() -> Result<(), String> {
    let sysroot = std::env::var_os("SystemRoot")
        .map(|r| HSTRING::from(format!("{}\\System32\\ipconfig.exe", r.to_string_lossy())))
        .unwrap_or_else(|| HSTRING::from("C:\\Windows\\System32\\ipconfig.exe"));
    let cmdline = HSTRING::from("ipconfig /flushdns");
    let mut cmdline_buf: Vec<u16> = cmdline.as_wide().to_vec();
    cmdline_buf.push(0);

    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            &sysroot,
            windows::core::PWSTR(cmdline_buf.as_mut_ptr()),
            None,
            None,
            windows::Win32::Foundation::BOOL(0),
            CREATE_NO_WINDOW,
            None,
            windows::core::PCWSTR::null(),
            &si,
            &mut pi,
        )
        .map_err(|e| format!("flushdns 启动失败：{e}"))?;
        let _ = WaitForSingleObject(pi.hProcess, 10_000);
        let mut code = 0u32;
        let _ = GetExitCodeProcess(pi.hProcess, &mut code);
        if code != 0 {
            return Err(format!("flushdns 退出码 {code}"));
        }
    }
    Ok(())
}

/// 以管理员身份重启自身（成功时调用方应退出当前实例）
pub fn admin_restart(hwnd: HWND) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("{e}"))?;
    elevate_and_wait_ex(&exe, "", hwnd).map(|_| ())
}

/// runas 但不等待（用于重启自身）；独立于 elevate_and_wait 以复用校验逻辑
fn elevate_and_wait_ex(exe: &Path, args: &str, hwnd: HWND) -> Result<u32, String> {
    elevate_and_wait(exe, args, hwnd)
}

// HANDLE 在 0.58 中仅用于类型占位，保留导入以备扩展
#[allow(dead_code)]
fn _handle_type_hint(_: HANDLE) {}
