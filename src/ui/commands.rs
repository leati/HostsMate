//! 命令层：保存编排（备份/提权/flushdns）、源码模式、导入导出、设置窗、右键菜单、外部修改检测

use std::path::PathBuf;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Controls::{Dialogs::{
    GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_OVERWRITEPROMPT,
    OFN_PATHMUSTEXIST, OPEN_FILENAME_FLAGS, OPENFILENAMEW,
}, EM_SETSEL};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, EndPaint, FillRect, HBRUSH, HDC, InvalidateRect, PAINTSTRUCT,
    SetBkMode, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON,
    BS_PUSHBUTTON, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    EN_CHANGE, ES_AUTOHSCROLL, GetClientRect, GetWindowRect,
    GetWindowLongPtrW, GWLP_USERDATA, HMENU, IDYES, LB_ADDSTRING, LB_GETCURSEL,
    LB_GETSELITEMS, LB_GETSELCOUNT, LB_GETTEXT, LB_GETTEXTLEN, LB_RESETCONTENT, LB_SETCURSEL,
    LBN_SELCHANGE, LBS_EXTENDEDSEL, LBS_HASSTRINGS, LBS_NOTIFY, MB_ICONERROR,
    MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_YESNO, MENU_ITEM_FLAGS, MF_CHECKED,
    MF_GRAYED,
    MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MessageBoxW, PostMessageW, RegisterClassW,
    SendMessageW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, SWP_NOMOVE,
    SWP_NOZORDER, SW_SHOW,
    SW_SHOWNORMAL, TrackPopupMenu, TRACK_POPUP_MENU_FLAGS, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    LB_DELETESTRING, LB_INSERTSTRING,
    WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_NCDESTROY, WM_PAINT, WM_SETFONT, WINDOW_EX_STYLE, WINDOW_STYLE,
    WNDCLASSW,
    WS_CAPTION,
    WS_CHILD, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use super::app::{
    App, CatFilter, CategoryRules, COL_DOMAIN, ConflictRole, FilterState, SortMode, CM_COPY,
    CM_DELETE, CM_EDIT, CM_OPENHTTP, CM_PING, CM_PING6, CM_PINGIP, CM_PINGT, CM_PIN, CM_CLEANUP,
    CM_TOGGLE, CHIP_ALL, CHIP_DISABLED, CHIP_ENABLED, UserCategory, CAT_ALL, CAT_BASE,
    CAT_FALLBACK, IDC_ADMIN, IDC_BKDEL, IDC_BKLIST, IDC_CAT_ADD, IDC_CAT_DEL, IDC_CAT_DOWN,
    IDC_CAT_UP, IDC_CAT_RESET, IDC_CATLIST, IDC_CATKW, IDC_CATNAME, IDC_DNS, IDC_KEEP,
    IDC_OPENBK, IDC_RESTORE, IDCANCEL, IDOK, IDM_ABOUT, IDM_DELETE, IDM_EXPORT,
    IDM_FOCUS_SEARCH, IDM_IMPORT, IDM_NEW,
    IDM_SAVE, IDM_SETTINGS, IDM_SORT_DOMAIN, IDM_SORT_FILE, IDM_SORT_IP,
};
use super::create;
use super::elevate;
use super::table;

// ---------- 工具栏命令 ----------

pub unsafe fn on_command(app: &mut App, id: i32) {
    match id {
        IDM_NEW => table::new_record(app),
        IDM_DELETE => table::delete_selected(app),
        IDM_SAVE => save_file(app),
        IDM_IMPORT => import_file(app),
        IDM_EXPORT => export_file(app),
        IDM_SETTINGS => open_settings(app),
        IDM_FOCUS_SEARCH => {
            set_focus_search(app);
        }
        IDM_ABOUT => {
            MessageBoxW(
                app.hwnd_main,
                w!("HostsMate v0.1.0\n\n绿色单文件 Windows hosts 管理器\nRust + Win32 原生实现\n\n数据模型：单文件就地标注\n方案叠加开关 · 两级冲突检测 · 自动备份 · 保存后刷新 DNS"),
                w!("关于 HostsMate"),
                MB_OK | MB_ICONINFORMATION,
            );
        }
        _ => {}
    }
}

pub unsafe fn set_focus_search(app: &mut App) {
    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(app.hwnd_search);
    SendMessageW(app.hwnd_search, EM_SETSEL, WPARAM(0), LPARAM(-1));
}

/// 子控件 WM_COMMAND（搜索/清理/冲突列表）路由
pub unsafe fn on_search_changed(app: &mut App, code: u32) {
    if code == EN_CHANGE {
        app.filter = table::read_edit_text(app.hwnd_search).trim().to_lowercase();
        app.busy = true;
        table::populate_records(app);
        app.busy = false;
    }
}

/// 状态筛选 chips 点击
pub unsafe fn on_chip(app: &mut App, id: i32) {
    app.filter_state = match id {
        CHIP_ALL => FilterState::All,
        CHIP_ENABLED => FilterState::Enabled,
        CHIP_DISABLED => FilterState::Disabled,
        _ => FilterState::Conflicted,
    };
    app.busy = true;
    table::populate_records(app);
    app.busy = false;
    // 自绘按钮是独立子窗口，父窗口失效不会带动它们重画，需单独失效
    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(app.hwnd_status, None, false);
    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(app.hwnd_main, None, false);
}

pub unsafe fn on_category(app: &mut App, id: i32) {
    app.category = if id == CAT_ALL {
        CatFilter::All
    } else if id == CAT_FALLBACK {
        // 兜底「自定义」= 分类数下标
        CatFilter::Cat(app.settings.rules.cats.len())
    } else {
        CatFilter::Cat((id - CAT_BASE).max(0) as usize)
    };
    table::populate_records(app);
    // 同上：必须逐个失效分类按钮，否则旧选中项的蓝色残留在屏幕上
    for (h, _) in &app.hwnd_chips {
        let _ = InvalidateRect(*h, None, false);
    }
    let _ = InvalidateRect(app.hwnd_main, None, false);
}

pub unsafe fn status_menu(app: &mut App) {
    let mut rc = RECT::default();
    let _ = GetWindowRect(app.hwnd_status, &mut rc);
    let Ok(menu) = CreatePopupMenu() else { return };
    for (id, label, state) in [
        (CHIP_ALL, "全部状态", FilterState::All),
        (CHIP_ENABLED, "已启用", FilterState::Enabled),
        (CHIP_DISABLED, "已禁用", FilterState::Disabled),
        (super::app::CHIP_CONFLICT, "有冲突", FilterState::Conflicted),
    ] {
        let flag = if app.filter_state == state { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_STRING.0 | flag.0), id as usize, &HSTRING::from(label));
    }
    let cmd = TrackPopupMenu(menu, TRACK_POPUP_MENU_FLAGS(TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0),
        rc.left, rc.bottom, 0, app.hwnd_main, None).0 as i32;
    let _ = DestroyMenu(menu);
    if cmd != 0 { on_chip(app, cmd); }
}

/// 排序下拉菜单
pub unsafe fn sort_menu(app: &mut App) {
    let mut rc = RECT::default();
    let _ = GetWindowRect(app.hwnd_sort, &mut rc);
    let Ok(menu) = CreatePopupMenu() else { return };
    let (cf, cd, ci) = match app.sort_mode {
        SortMode::FileOrder => (MF_CHECKED, MF_UNCHECKED, MF_UNCHECKED),
        SortMode::Domain => (MF_UNCHECKED, MF_CHECKED, MF_UNCHECKED),
        SortMode::Ip => (MF_UNCHECKED, MF_UNCHECKED, MF_CHECKED),
    };
    let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_STRING.0 | cf.0), IDM_SORT_FILE as usize, w!("文件顺序"));
    let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_STRING.0 | cd.0), IDM_SORT_DOMAIN as usize, w!("按域名"));
    let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_STRING.0 | ci.0), IDM_SORT_IP as usize, w!("按 IP 地址"));
    let cmd = TrackPopupMenu(
        menu,
        TRACK_POPUP_MENU_FLAGS(TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0),
        rc.left,
        rc.bottom,
        0,
        app.hwnd_main,
        None,
    )
    .0 as i32;
    let _ = DestroyMenu(menu);
    if cmd != 0 {
        on_sort(app, cmd);
    }
}

pub unsafe fn on_sort(app: &mut App, id: i32) {
    app.sort_mode = match id {
        IDM_SORT_DOMAIN => SortMode::Domain,
        IDM_SORT_IP => SortMode::Ip,
        _ => SortMode::FileOrder,
    };
    app.busy = true;
    table::populate_records(app);
    app.busy = false;
    let _ = InvalidateRect(app.hwnd_main, None, false);
}

// 行内编辑回调（table.rs edit_proc 调用）
pub unsafe fn commit_edit(app: &mut App) {
    table::apply_commit(app);
}

pub unsafe fn cancel_edit(app: &mut App) {
    table::cancel_edit_impl(app);
}

// ---------- 保存编排（规格 §2.4 / §4） ----------

pub unsafe fn save_file(app: &mut App) {
    if app.busy {
        return;
    }
    // 保存前外部修改检查
    match external_state(app) {
        ExternalState::Same => {}
        ExternalState::ChangedClean => {
            let _ = app.backup_now();
            if app.load_from_disk().is_err() {
                return;
            }
            app.status_msg = "已重新加载外部修改".into();
        }
        ExternalState::ChangedDirty => {
            let r = MessageBoxW(
                app.hwnd_main,
                w!("文件已被外部程序修改，且本地有未保存修改。\n\n是 = 用本地修改覆盖外部更改\n否 = 放弃本次保存（先重载外部版本）"),
                w!("检测到外部修改"),
                MB_YESNO | MB_ICONWARNING,
            );
            if r != IDYES {
                let _ = app.backup_now();
                let _ = app.load_from_disk();
                app.busy = true;
                table::populate_all(app);
                app.busy = false;
                table::refresh_chrome(app);
                return;
            }
        }
    }

    // 1. 备份（失败则中止保存——单文件模型下备份是数据安全底线）
    if let Err(e) = app.backup_now() {
        MessageBoxW(app.hwnd_main, &HSTRING::from(e), w!("保存中止"), MB_OK | MB_ICONERROR);
        return;
    }

    // 2. 写盘：系统 hosts 且非管理员 → 私有目录中转文件 + UAC 提权 helper
    let bytes = hostsmate::engine::generate_bytes(&app.doc);
    let target_is_system = app.path == hostsmate::engine::system_hosts_path();
    let mut dns_note = String::new();
    let mut leftover_tmp: Option<PathBuf> = None;
    let result = if !target_is_system || app.is_admin {
        app.write_direct()
    } else {
        // 中转文件不得放共享 %TEMP%：同用户进程可在 UAC 确认窗口期替换内容，
        // 提权 helper 会把替换后的字节写进系统 hosts。改放私有数据目录，
        // 并以 SHA-256 锁定内容，helper 校验不过即拒写（替换攻击至多导致本次保存失败）。
        // 注：ShellExecuteExW(runas) 无法向提权子进程继承句柄，匿名管道在此链路不可行。
        let _ = std::fs::create_dir_all(&app.data_dir);
        let tmp = app.data_dir.join(format!("save-{}.tmp", std::process::id()));
        leftover_tmp = Some(tmp.clone());
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if let Err(e) = std::fs::write(&tmp, &bytes) {
            Err(format!("临时文件写入失败：{e}"))
        } else {
            let exe = std::env::current_exe().unwrap_or_default();
            let args = format!("--write-hosts \"{}\" {hex}", tmp.display());
            match elevate::elevate_and_wait(&exe, &args, app.hwnd_main) {
                Ok(0) => Ok(()),
                Ok(3) => Err("中转文件校验失败（提权前内容被改动），已拒绝写入".into()),
                Ok(code) => Err(format!("提权写入失败（退出码 {code}）")),
                Err(e) => Err(e),
            }
        }
    };
    // helper 成功时会自删中转文件；UAC 取消 / helper 失败 / 写 tmp 失败都可能留下——统一幂等清理
    if let Some(p) = &leftover_tmp {
        let _ = std::fs::remove_file(p);
    }

    match result {
        Ok(()) => {
            app.dirty = false;
            app.ext_sig = app.ext_sig_of(&app.path);
            app.last_save = super::app::file_time_label(&app.path);
            if app.settings.flushdns {
                match elevate::flushdns() {
                    Ok(()) => dns_note.push_str(" · DNS 缓存已刷新"),
                    Err(e) => dns_note.push_str(&format!(" · {e}")),
                }
            }
            let ts = hostsmate::engine::backup::timestamp_name(std::time::SystemTime::now());
            app.status_msg = format!("已保存 {}{}", &ts[9..], dns_note);
            app.busy = true;
            table::populate_all(app);
            app.busy = false;
            table::refresh_chrome(app);
        }
        Err(e) => {
            if e.contains("已取消") {
                app.status_msg = e;
                table::refresh_chrome(app);
            } else {
                MessageBoxW(app.hwnd_main, &HSTRING::from(e), w!("保存失败"), MB_OK | MB_ICONERROR);
            }
        }
    }
}

// ---------- 外部修改检测（WM_ACTIVATE 触发） ----------

enum ExternalState {
    Same,
    ChangedClean,
    ChangedDirty,
}

fn external_state(app: &App) -> ExternalState {
    let Some(sig) = app.ext_sig_of(&app.path) else {
        return ExternalState::Same;
    };
    if app.ext_sig == Some(sig) {
        return ExternalState::Same;
    }
    if app.dirty {
        ExternalState::ChangedDirty
    } else {
        ExternalState::ChangedClean
    }
}

pub unsafe fn check_external(app: &mut App) {
    if app.busy || app.edit.is_some() {
        return;
    }
    match external_state(app) {
        ExternalState::Same => {}
        ExternalState::ChangedClean => {
            let _ = app.backup_now();
            if app.load_from_disk().is_ok() {
                app.status_msg = "检测到外部修改，已重新加载".into();
                app.busy = true;
                table::populate_all(app);
                app.busy = false;
                table::refresh_chrome(app);
            }
        }
        ExternalState::ChangedDirty => {
            let r = MessageBoxW(
                app.hwnd_main,
                w!("文件已被外部程序修改，且本地有未保存修改。\n\n是 = 放弃本地修改，重新加载文件\n否 = 保留本地修改（保存时将覆盖外部更改）"),
                w!("检测到外部修改"),
                MB_YESNO | MB_ICONWARNING,
            );
            if r == IDYES {
                let _ = app.backup_now();
                let _ = app.load_from_disk();
            } else {
                app.ext_sig = app.ext_sig_of(&app.path); // 不再重复提示
                return;
            }
            app.busy = true;
            table::populate_all(app);
            app.busy = false;
            table::refresh_chrome(app);
        }
    }
}

// ---------- 导入 / 导出（规格 §2.2） ----------

unsafe fn file_dialog(hwnd: HWND, save: bool) -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let filter = HSTRING::from("Hosts 文件\0*.host;*.hosts;*.txt\0所有文件\0*.*\0");
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: std::mem::transmute(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        Flags: OPEN_FILENAME_FLAGS(if save {
            OFN_OVERWRITEPROMPT.0 | OFN_PATHMUSTEXIST.0 | OFN_HIDEREADONLY.0
        } else {
            OFN_FILEMUSTEXIST.0 | OFN_PATHMUSTEXIST.0 | OFN_HIDEREADONLY.0
        }),
        ..Default::default()
    };
    let ok = if save {
        GetSaveFileNameW(&mut ofn)
    } else {
        GetOpenFileNameW(&mut ofn)
    };
    if ok.as_bool() {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(0);
        let s = String::from_utf16_lossy(&buf[..end]);
        if s.is_empty() { None } else { Some(PathBuf::from(s)) }
    } else {
        None
    }
}

unsafe fn import_file(app: &mut App) {
    let Some(path) = file_dialog(app.hwnd_main, false) else { return };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            MessageBoxW(app.hwnd_main, &HSTRING::from(format!("读取失败：{e}")), w!("导入失败"), MB_OK | MB_ICONERROR);
            return;
        }
    };
    let imported = hostsmate::engine::parse_bytes(&bytes);

    // 平铺追加：来源文件的全部记录追加到当前文件末尾（不再保留来源分组结构）
    let mut count = 0usize;
    if app.doc.schemes.is_empty() {
        app.doc.schemes.push(hostsmate::engine::Scheme {
            name: hostsmate::engine::DEFAULT_SCHEME.to_string(),
            explicit: false,
            entries: Vec::new(),
        });
    }
    let last = app.doc.schemes.len() - 1;
    for s2 in imported.schemes {
        count += s2.entries.iter().filter(|e| matches!(e, hostsmate::engine::Entry::Record(_))).count();
        app.doc.schemes[last].entries.extend(s2.entries);
    }
    if count == 0 {
        MessageBoxW(app.hwnd_main, w!("未发现可导入的记录"), w!("导入"), MB_OK | MB_ICONINFORMATION);
        return;
    }
    app.mark_dirty();
    app.busy = true;
    table::populate_all(app);
    app.busy = false;
    app.status_msg = format!("导入 {} 条记录", count);
    table::refresh_chrome(app);
}

unsafe fn export_file(app: &mut App) {
    let Some(path) = file_dialog(app.hwnd_main, true) else { return };
    let text = hostsmate::engine::generate_str(&app.doc);
    match std::fs::write(&path, text.as_bytes()) {
        Ok(()) => {
            app.status_msg = format!("已导出 {}", path.display());
            table::refresh_chrome(app);
        }
        Err(e) => {
            MessageBoxW(app.hwnd_main, &HSTRING::from(format!("导出失败：{e}")), w!("导出失败"), MB_OK | MB_ICONERROR);
        }
    }
}

// ---------- 冲突清理 ----------

pub unsafe fn cleanup_selected_conflict(app: &mut App) {
    if app.conflicts.is_empty() {
        return;
    }
    // 优先：选中行所属的冗余组；否则第一个冗余组
    let gi = table::first_selected_row(app)
        .and_then(|row| app.row_map.get(row).copied())
        .and_then(|cell| {
            app.conflicts
                .iter()
                .position(|g| g.kind == hostsmate::engine::conflict::ConflictKind::Redundant && g.members.iter().any(|m| (m.scheme, m.entry) == cell))
        })
        .or_else(|| {
            app.conflicts
                .iter()
                .position(|g| g.kind == hostsmate::engine::conflict::ConflictKind::Redundant)
        });
    let Some(gi) = gi else {
        MessageBoxW(
            app.hwnd_main,
            w!("「清理冗余」只作用于冗余重复（同域名同 IP）。真冲突请用右键「置顶生效」处理。"),
            w!("清理冗余"),
            MB_OK | MB_ICONINFORMATION,
        );
        return;
    };
    let n = app.cleanup_redundant(gi);
    app.busy = true;
    table::populate_all(app);
    app.busy = false;
    app.status_msg = format!("已清理 {n} 条冗余记录");
    table::refresh_chrome(app);
}

// ---------- 右键菜单（规格 §3） ----------

pub unsafe fn show_record_menu(app: &mut App, x: i32, y: i32) {
    let Some(row) = table::first_selected_row(app) else { return };
    let Some(&cell) = app.row_map.get(row) else { return };
    let (domain, ip_str, enabled) = match app.record(cell.0, cell.1) {
        Some(r) => (r.domain.clone(), r.ip.to_string(), r.enabled),
        None => return,
    };
    let role = app.roles.get(&cell).copied();

    let Ok(menu) = CreatePopupMenu() else { return };
    // 菜单项标志：可用 = MF_STRING；禁用 = MF_STRING | MF_GRAYED
    let item_flags = |enabled: bool| MENU_ITEM_FLAGS(if enabled { MF_STRING.0 } else { MF_STRING.0 | MF_GRAYED.0 });
    let mut text;
    unsafe {
        text = HSTRING::from(if enabled { "禁用此记录" } else { "启用此记录" });
        let _ = AppendMenuW(menu, item_flags(true), CM_TOGGLE as usize, &text);
        text = HSTRING::from("编辑…");
        let _ = AppendMenuW(menu, item_flags(true), CM_EDIT as usize, &text);
        text = HSTRING::from("删除…");
        let _ = AppendMenuW(menu, item_flags(true), CM_DELETE as usize, &text);
        text = HSTRING::from("置顶生效");
        let pin_on = role == Some(ConflictRole::RealOverridden);
        let _ = AppendMenuW(menu, item_flags(pin_on), CM_PIN as usize, &text);
        text = HSTRING::from("复制");
        let _ = AppendMenuW(menu, item_flags(true), CM_COPY as usize, &text);
        text = HSTRING::from("清理冗余重复");
        let has_redundant = app
            .conflicts
            .iter()
            .any(|g| g.kind == hostsmate::engine::conflict::ConflictKind::Redundant);
        let _ = AppendMenuW(menu, item_flags(has_redundant), CM_CLEANUP as usize, &text);
        let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_SEPARATOR.0), 0, PCWSTR::null());
        text = HSTRING::from(format!("ping {}", domain));
        let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_STRING.0), CM_PING as usize, &text);
        text = HSTRING::from(format!("ping -t {}", domain));
        let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_STRING.0), CM_PINGT as usize, &text);
        text = HSTRING::from(format!("ping -6 {}", domain));
        let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_STRING.0), CM_PING6 as usize, &text);
        text = HSTRING::from(format!("ping {}", ip_str));
        let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_STRING.0), CM_PINGIP as usize, &text);
        let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_SEPARATOR.0), 0, PCWSTR::null());
        text = HSTRING::from(format!("打开 http://{}/", domain));
        let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_STRING.0), CM_OPENHTTP as usize, &text);

        let cmd = TrackPopupMenu(
            menu,
            TRACK_POPUP_MENU_FLAGS(TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0),
            x,
            y,
            0,
            app.hwnd_main,
            None,
        )
        .0 as i32;
        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyMenu(menu);
        context_action(app, cmd, row, cell, domain, ip_str);
    }
}

unsafe fn context_action(
    app: &mut App,
    cmd: i32,
    row: usize,
    cell: (usize, usize),
    domain: String,
    ip_str: String,
) {
    match cmd {
        CM_TOGGLE => table::toggle_selected(app),
        CM_EDIT => table::start_edit(app, row, COL_DOMAIN),
        CM_DELETE => table::delete_selected(app),
        CM_PIN => table::pin_selected(app),
        CM_CLEANUP => cleanup_selected_conflict(app),
        CM_COPY => {
            if let Some(r) = app.record(cell.0, cell.1) {
                copy_to_clipboard(app.hwnd_main, &format!("{}\t{}", r.ip, r.domain));
            }
        }
        CM_PING => ping_cmd(&domain),
        CM_PINGT => ping_t_cmd(&domain),
        CM_PING6 => ping6_cmd(&domain),
        CM_PINGIP => ping_cmd(&ip_str),
        CM_OPENHTTP => open_http(&domain),
        _ => {}
    }
}

/// ping 类命令的域名/IP 必须过白名单（ip_str 来自 IpAddr 格式化天然安全；
/// 域名可能来自外部导入文件，拼接 cmd /c 前再拦一次，纵深防御）
fn safe_host_arg(s: &str) -> Option<&str> {
    hostsmate::engine::parser::is_valid_domain(s).then_some(s)
}

fn ping_cmd(host: &str) {
    if let Some(h) = safe_host_arg(host) {
        unsafe { shell_cmd(&format!("/c ping {h} & pause")) };
    }
}

fn ping_t_cmd(host: &str) {
    if let Some(h) = safe_host_arg(host) {
        unsafe { shell_cmd(&format!("/c ping -t {h}")) };
    }
}

fn ping6_cmd(host: &str) {
    if let Some(h) = safe_host_arg(host) {
        unsafe { shell_cmd(&format!("/c ping -6 {h} & pause")) };
    }
}

fn open_http(domain: &str) {
    // URL 与命令行同理：域名白名单外的内容不拼进协议处理器
    if let Some(d) = safe_host_arg(domain) {
        unsafe { shell_url(&format!("http://{d}/")) };
    }
}

unsafe fn shell_cmd(args: &str) {
    let f = HSTRING::from("cmd.exe");
    let p = HSTRING::from(args);
    let _ = ShellExecuteW(HWND::default(), w!("open"), &f, &p, PCWSTR::null(), SW_SHOWNORMAL);
}

unsafe fn shell_url(url: &str) {
    let f = HSTRING::from(url);
    let _ = ShellExecuteW(HWND::default(), w!("open"), &f, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
}

unsafe fn copy_to_clipboard(hwnd: HWND, text: &str) {
    if OpenClipboard(hwnd).is_err() {
        return;
    }
    let _ = EmptyClipboard();
    let mut v: Vec<u16> = text.encode_utf16().collect();
    v.push(0);
    let byte_len = v.len() * 2;
    if let Ok(hg) = GlobalAlloc(GMEM_MOVEABLE, byte_len) {
        let dst = GlobalLock(hg);
        if !dst.is_null() {
            std::ptr::copy_nonoverlapping(v.as_ptr() as *const u8, dst as *mut u8, byte_len);
            let _ = GlobalUnlock(hg);
            if SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(hg.0)).is_err() {
                // 罕见：设置失败时句柄仍留在剪贴板分配中，由系统回收
            }
        }
    }
    let _ = CloseClipboard();
}

// ---------- 设置窗（规格 §2.4 备份恢复 + §4 管理员重启 + 自定义分类管理） ----------

struct SettingsUi {
    main: HWND,
    backup_dir: PathBuf,
    hwnd_dns: HWND,
    hwnd_keep: HWND,
    hwnd_list: HWND,
    /// 分类列表 + 名称/关键词编辑框（选中行直接编辑，EN_CHANGE 即时写回工作副本）
    hwnd_catlist: HWND,
    hwnd_catname: HWND,
    hwnd_catkw: HWND,
    /// 分类工作副本（确定时才写回 app.settings）
    cats: Vec<UserCategory>,
    cat_sel: isize,
    /// SetWindowText 回显会触发 EN_CHANGE，此标记用于区分用户输入
    loading: bool,
    /// 页面底色刷（与主窗一致的浅灰蓝）
    bg_brush: HBRUSH,
}

pub unsafe fn open_settings(app: &mut App) {
    let hmod = GetModuleHandleW(None).unwrap_or_default();
    let hinst = windows::Win32::Foundation::HINSTANCE(hmod.0);
    let wc = WNDCLASSW {
        lpfnWndProc: Some(settings_proc),
        hInstance: hinst,
        hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
            windows::Win32::Foundation::HINSTANCE::default(),
            windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
        )
        .unwrap_or_default(),
        hbrBackground: HBRUSH(6usize as *mut core::ffi::c_void),
        lpszClassName: w!("HostsMateSettingsWnd"),
        ..Default::default()
    };
    RegisterClassW(&wc); // 重复注册失败无碍

    let ctx = Box::new(SettingsUi {
        main: app.hwnd_main,
        backup_dir: app.backup_dir.clone(),
        hwnd_dns: HWND::default(),
        hwnd_keep: HWND::default(),
        hwnd_list: HWND::default(),
        hwnd_catlist: HWND::default(),
        hwnd_catname: HWND::default(),
        hwnd_catkw: HWND::default(),
        cats: app.settings.rules.cats.clone(),
        cat_sel: 0,
        loading: false,
        bg_brush: HBRUSH::default(),
    });
    let ptr = Box::into_raw(ctx);

    let hwnd = match CreateWindowExW(
        Default::default(),
        w!("HostsMateSettingsWnd"),
        w!("HostsMate 设置"),
        WS_POPUP | WS_CAPTION | WS_SYSMENU,
        220,
        160,
        430,
        620,
        app.hwnd_main,
        HMENU::default(),
        hinst,
        None,
    ) {
        Ok(h) => h,
        Err(_) => {
            drop(Box::from_raw(ptr));
            return;
        }
    };
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
    // 子控件布局按 96 DPI 裸像素设计；高 DPI 下窗口与坐标须等比放大，否则字体变大后截字/越界
    let dpi = GetDpiForWindow(hwnd).max(96) as i32;
    let _ = SetWindowPos(hwnd, HWND::default(), 0, 0, 430 * dpi / 96, 620 * dpi / 96,
        SWP_NOMOVE | SWP_NOZORDER);
    build_settings_children(hwnd, ptr, dpi);
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = SetForegroundWindow(hwnd);
}

unsafe fn build_settings_children(hwnd: HWND, ptr: *mut SettingsUi, dpi: i32) {
    let hinst = GetModuleHandleW(None).unwrap_or_default();
    let ctx = &mut *ptr;
    let app = &mut *(GetWindowLongPtrW(ctx.main, GWLP_USERDATA) as *mut App);
    let font = app.font;
    ctx.bg_brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00FC_F9F6));

    // 96 DPI 设计坐标按实际 DPI 换算；创建的控件全部收进 all，统一设字体
    let mut all: Vec<HWND> = Vec::new();
    let sc = |v: i32| v * dpi / 96;
    // 输入类控件用 CLIENTEDGE 凹陷边框：控件自绘，平铺在页面底色上否则完全看不见
    let edge = windows::Win32::UI::WindowsAndMessaging::WS_EX_CLIENTEDGE;
    let none = WINDOW_EX_STYLE(0);
    let mut mk = |ex: WINDOW_EX_STYLE,
              class: PCWSTR,
              text: PCWSTR,
              style: u32,
              x: i32,
              y: i32,
              w: i32,
              h: i32,
              id: i32|
     -> HWND {
        let h = CreateWindowExW(
            ex,
            class,
            text,
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
            sc(x),
            sc(y),
            sc(w),
            sc(h),
            hwnd,
            HMENU(id as isize as *mut core::ffi::c_void),
            hinst,
            None,
        )
        .unwrap_or_default();
        all.push(h);
        h
    };

    mk(none, w!("STATIC"), w!("保存成功后自动刷新 DNS 缓存"), 0, 20, 20, 300, 20, 0);
    ctx.hwnd_dns = mk(none, w!("BUTTON"), w!("启用"), BS_AUTOCHECKBOX as u32, 40, 44, 120, 20, IDC_DNS);
    mk(none, w!("STATIC"), w!("备份保留份数（1-200）"), 0, 20, 76, 300, 20, 0);
    ctx.hwnd_keep = mk(
        edge,
        w!("EDIT"),
        PCWSTR::null(),
        ES_AUTOHSCROLL as u32,
        40,
        98,
        80,
        22,
        IDC_KEEP,
    );
    mk(none, w!("BUTTON"), w!("以管理员身份重启"), BS_PUSHBUTTON as u32, 220, 96, 170, 30, IDC_ADMIN);

    // 分类管理：列表选中一行，下方名称/关键词框即时编辑；顺序即匹配优先级
    mk(none, w!("STATIC"), w!("分类规则（自上而下优先；xx* = 域名前缀）"), 0, 20, 134, 306, 20, 0);
    mk(none, w!("BUTTON"), w!("恢复默认"), BS_PUSHBUTTON as u32, 330, 130, 80, 26, IDC_CAT_RESET);
    ctx.hwnd_catlist = mk(
        edge,
        w!("LISTBOX"),
        PCWSTR::null(),
        LBS_NOTIFY as u32 | LBS_HASSTRINGS as u32 | WS_VSCROLL.0 | WS_TABSTOP.0,
        20,
        158,
        370,
        72,
        IDC_CATLIST,
    );
    mk(none, w!("BUTTON"), w!("添加"), BS_PUSHBUTTON as u32, 20, 236, 62, 26, IDC_CAT_ADD);
    mk(none, w!("BUTTON"), w!("上移"), BS_PUSHBUTTON as u32, 88, 236, 62, 26, IDC_CAT_UP);
    mk(none, w!("BUTTON"), w!("下移"), BS_PUSHBUTTON as u32, 156, 236, 62, 26, IDC_CAT_DOWN);
    mk(none, w!("BUTTON"), w!("删除"), BS_PUSHBUTTON as u32, 224, 236, 62, 26, IDC_CAT_DEL);
    ctx.hwnd_catname = mk(
        edge,
        w!("EDIT"),
        PCWSTR::null(),
        ES_AUTOHSCROLL as u32,
        20,
        270,
        110,
        22,
        IDC_CATNAME,
    );
    ctx.hwnd_catkw = mk(
        edge,
        w!("EDIT"),
        PCWSTR::null(),
        ES_AUTOHSCROLL as u32,
        140,
        270,
        250,
        22,
        IDC_CATKW,
    );
    set_cue(ctx.hwnd_catname, "分类名称");
    set_cue(ctx.hwnd_catkw, "关键词，逗号分隔");

    // 备份管理：多选列表，可批量删除；恢复只支持单选
    mk(none, w!("STATIC"), w!("备份（可多选后删除；恢复只认单个选中）"), 0, 20, 304, 340, 20, 0);
    ctx.hwnd_list = mk(
        edge,
        w!("LISTBOX"),
        PCWSTR::null(),
        LBS_NOTIFY as u32 | LBS_HASSTRINGS as u32 | LBS_EXTENDEDSEL as u32
            | WS_VSCROLL.0 | WS_TABSTOP.0,
        20,
        326,
        370,
        210,
        IDC_BKLIST,
    );
    mk(none, w!("BUTTON"), w!("恢复所选"), BS_PUSHBUTTON as u32, 20, 548, 96, 30, IDC_RESTORE);
    mk(none, w!("BUTTON"), w!("删除所选"), BS_PUSHBUTTON as u32, 122, 548, 96, 30, IDC_BKDEL);
    mk(none, w!("BUTTON"), w!("打开文件夹"), BS_PUSHBUTTON as u32, 224, 548, 84, 30, IDC_OPENBK);
    mk(none, w!("BUTTON"), w!("确定"), BS_DEFPUSHBUTTON as u32, 314, 548, 76, 30, IDOK);

    // 初始化值
    SendMessageW(
        ctx.hwnd_dns,
        BM_SETCHECK,
        WPARAM(if app.settings.flushdns { 1 } else { 0 }),
        LPARAM(0),
    );
    let _ = SetWindowTextW(ctx.hwnd_keep, &HSTRING::from(app.settings.backup_keep.to_string()));
    resync_catlist(ctx);
    fill_backup_list(ctx);

    // 全部控件（含 STATIC/按钮）统一用消息字体：默认字体不随 app 缩放，高 DPI 下会失配
    for h in all {
        SendMessageW(h, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(0));
    }
}

unsafe fn set_cue(h: HWND, text: &str) {
    let cue = HSTRING::from(text);
    // EM_SETCUEBANNER：占位提示（输入为空时灰字显示）
    SendMessageW(h, 0x1501, WPARAM(1), LPARAM(cue.as_ptr() as isize));
}

/// 从工作副本重建分类列表框显示（名称 ｜ 关键词预览），保持选中行
unsafe fn resync_catlist(ctx: &mut SettingsUi) {
    ctx.loading = true;
    SendMessageW(ctx.hwnd_catlist, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
    for cat in &ctx.cats {
        let kw = CategoryRules::join_list(&cat.keywords);
        let line = if kw.is_empty() {
            format!("{}　（无关键词）", cat.name)
        } else {
            format!("{}　｜ {}", cat.name, kw)
        };
        let h = HSTRING::from(line);
        SendMessageW(ctx.hwnd_catlist, LB_ADDSTRING, WPARAM(0), LPARAM(h.as_ptr() as isize));
    }
    let sel = if ctx.cat_sel >= 0 && (ctx.cat_sel as usize) < ctx.cats.len() { ctx.cat_sel } else { -1 };
    ctx.cat_sel = sel;
    if sel >= 0 {
        SendMessageW(ctx.hwnd_catlist, LB_SETCURSEL, WPARAM(sel as usize), LPARAM(0));
    }
    ctx.loading = false;
    load_cat_edits(ctx);
}

/// 选中行 → 名称/关键词编辑框（回显，抑制 EN_CHANGE 写回）
unsafe fn load_cat_edits(ctx: &mut SettingsUi) {
    ctx.loading = true;
    let valid = ctx.cat_sel >= 0 && (ctx.cat_sel as usize) < ctx.cats.len();
    if valid {
        let cat = &ctx.cats[ctx.cat_sel as usize];
        let _ = SetWindowTextW(ctx.hwnd_catname, &HSTRING::from(&cat.name));
        let _ = SetWindowTextW(ctx.hwnd_catkw, &HSTRING::from(CategoryRules::join_list(&cat.keywords)));
    } else {
        let _ = SetWindowTextW(ctx.hwnd_catname, &HSTRING::default());
        let _ = SetWindowTextW(ctx.hwnd_catkw, &HSTRING::default());
    }
    let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(ctx.hwnd_catname, valid);
    let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(ctx.hwnd_catkw, valid);
    ctx.loading = false;
}

/// 编辑框 → 选中行（EN_CHANGE 即时写回 + 确定前兜底提交）
unsafe fn commit_cat_edits(ctx: &mut SettingsUi) {
    if ctx.loading || ctx.cat_sel < 0 || (ctx.cat_sel as usize) >= ctx.cats.len() {
        return;
    }
    let i = ctx.cat_sel as usize;
    let name = table::read_edit_text(ctx.hwnd_catname);
    let name = name.trim().to_string();
    ctx.cats[i].name = if name.is_empty() { format!("分类{}", i + 1) } else { name };
    ctx.cats[i].keywords = CategoryRules::parse_list(&table::read_edit_text(ctx.hwnd_catkw));
}

/// 编辑框内容变化：即时写回选中行并刷新该行显示
unsafe fn cat_edit_changed(ctx: &mut SettingsUi) {
    if ctx.loading {
        return;
    }
    commit_cat_edits(ctx);
    if ctx.cat_sel >= 0 && (ctx.cat_sel as usize) < ctx.cats.len() {
        let i = ctx.cat_sel as usize;
        let kw = CategoryRules::join_list(&ctx.cats[i].keywords);
        let line = if kw.is_empty() {
            format!("{}　（无关键词）", ctx.cats[i].name)
        } else {
            format!("{}　｜ {}", ctx.cats[i].name, kw)
        };
        let h = HSTRING::from(line);
        SendMessageW(ctx.hwnd_catlist, LB_DELETESTRING, WPARAM(i), LPARAM(0));
        SendMessageW(ctx.hwnd_catlist, LB_INSERTSTRING, WPARAM(i), LPARAM(h.as_ptr() as isize));
        SendMessageW(ctx.hwnd_catlist, LB_SETCURSEL, WPARAM(i), LPARAM(0));
    }
}

unsafe extern "system" fn settings_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCDESTROY {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsUi;
        if !ptr.is_null() {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
        }
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsUi;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let ctx = &mut *ptr;
    if msg == WM_PAINT {
        // 页面底色（输入框可见性由 CLIENTEDGE 边框保证，不依赖父窗自绘）
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let _ = FillRect(hdc, &rc, ctx.bg_brush);
        let _ = EndPaint(hwnd, &ps);
        return LRESULT(0);
    }
    // STATIC/复选框文字底色改为页面底色，去掉控件默认的灰块
    if msg == WM_CTLCOLORSTATIC {
        let hdc = HDC(wp.0 as *mut core::ffi::c_void);
        let _ = SetBkMode(hdc, TRANSPARENT);
        return LRESULT(ctx.bg_brush.0 as isize);
    }
    if msg == WM_COMMAND {
        let id = (wp.0 & 0xFFFF) as i32;
        let code = (wp.0 >> 16) as u32;
        match id {
            IDOK => {
                collect_and_store(ctx);
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            IDCANCEL => {
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            IDC_ADMIN => match elevate::admin_restart(ctx.main) {
                Ok(()) => {
                    let _ = PostMessageW(ctx.main, WM_CLOSE, WPARAM(0), LPARAM(0));
                    let _ = DestroyWindow(hwnd);
                }
                Err(e) => {
                    MessageBoxW(hwnd, &HSTRING::from(e), w!("重启失败"), MB_OK | MB_ICONERROR);
                }
            },
            IDC_RESTORE => restore_selected_backup(ctx, hwnd),
            IDC_BKDEL => delete_selected_backups(ctx),
            IDC_CATLIST if code == LBN_SELCHANGE as u32 => {
                ctx.cat_sel = SendMessageW(ctx.hwnd_catlist, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                load_cat_edits(ctx);
                return LRESULT(0);
            }
            IDC_CATNAME | IDC_CATKW if code == EN_CHANGE => {
                cat_edit_changed(ctx);
                return LRESULT(0);
            }
            IDC_CAT_ADD => {
                commit_cat_edits(ctx);
                let mut k = ctx.cats.len() + 1;
                while ctx.cats.iter().any(|c| c.name == format!("分类{k}")) {
                    k += 1;
                }
                ctx.cats.push(UserCategory { name: format!("分类{k}"), keywords: Vec::new() });
                ctx.cat_sel = ctx.cats.len() as isize - 1;
                resync_catlist(ctx);
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(ctx.hwnd_catname);
                return LRESULT(0);
            }
            IDC_CAT_DEL => {
                if ctx.cat_sel >= 0 && (ctx.cat_sel as usize) < ctx.cats.len() {
                    ctx.cats.remove(ctx.cat_sel as usize);
                    let n = ctx.cats.len() as isize;
                    ctx.cat_sel = if n == 0 { -1 } else { ctx.cat_sel.min(n - 1) };
                    resync_catlist(ctx);
                }
                return LRESULT(0);
            }
            IDC_CAT_UP => {
                let i = ctx.cat_sel;
                if i > 0 && (i as usize) < ctx.cats.len() {
                    ctx.cats.swap(i as usize, i as usize - 1);
                    ctx.cat_sel = i - 1;
                    resync_catlist(ctx);
                }
                return LRESULT(0);
            }
            IDC_CAT_DOWN => {
                let i = ctx.cat_sel;
                if i >= 0 && (i as usize) + 1 < ctx.cats.len() {
                    ctx.cats.swap(i as usize, i as usize + 1);
                    ctx.cat_sel = i + 1;
                    resync_catlist(ctx);
                }
                return LRESULT(0);
            }
            IDC_CAT_RESET => {
                ctx.cats = CategoryRules::default().cats;
                ctx.cat_sel = 0;
                resync_catlist(ctx);
                return LRESULT(0);
            }
            IDC_OPENBK => {
                let dir = HSTRING::from(ctx.backup_dir.as_os_str());
                let _ = ShellExecuteW(hwnd, w!("explore"), &dir, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
                return LRESULT(0);
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

unsafe fn collect_and_store(ctx: &mut SettingsUi) {
    let flushdns = SendMessageW(ctx.hwnd_dns, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 != 0;
    let keep_text = table::read_edit_text(ctx.hwnd_keep);
    let keep = keep_text.trim().parse::<u32>().unwrap_or(20).clamp(1, 200);
    // 编辑框最后一次输入可能尚未触发 EN_CHANGE，确定前显式提交一次
    commit_cat_edits(ctx);
    let app = &mut *(GetWindowLongPtrW(ctx.main, GWLP_USERDATA) as *mut App);
    app.settings.flushdns = flushdns;
    app.settings.backup_keep = keep;
    app.settings.rules.cats = ctx.cats.clone();
    // 分类可能被删减：当前分类筛选越界（大于兜底下标）时回到「全部」
    if let CatFilter::Cat(i) = app.category {
        if i > app.settings.rules.cats.len() {
            app.category = CatFilter::All;
        }
    }
    app.settings.store(&app.data_dir);
    // 规则可能变了：重算分类计数，刷新表格筛选与 chips 显示
    app.refresh_conflicts();
    app.busy = true;
    table::populate_records(app);
    app.busy = false;
    // 分类数量可能变化，chips 必须整行重建（内部会重排布局）
    create::rebuild_chips(app);
    for (h, _) in &app.hwnd_chips {
        let _ = InvalidateRect(*h, None, false);
    }
    let _ = InvalidateRect(app.hwnd_main, None, false);
}

/// 备份列表回填（文件名倒序 = 新→旧）
unsafe fn fill_backup_list(ctx: &SettingsUi) {
    SendMessageW(ctx.hwnd_list, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
    if let Ok(entries) = std::fs::read_dir(&ctx.backup_dir) {
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("hosts-") && n.ends_with(".host"))
            .collect();
        names.sort();
        names.reverse();
        for n in names {
            let h = HSTRING::from(n);
            SendMessageW(ctx.hwnd_list, LB_ADDSTRING, WPARAM(0), LPARAM(h.as_ptr() as isize));
        }
    }
}

/// 多选列表取全部选中下标（空 / 出错返回空 Vec）
unsafe fn get_sel_items(list: HWND) -> Vec<u32> {
    let n = SendMessageW(list, LB_GETSELCOUNT, WPARAM(0), LPARAM(0)).0;
    if n <= 0 {
        return Vec::new();
    }
    let mut buf = vec![0u32; n as usize];
    SendMessageW(list, LB_GETSELITEMS, WPARAM(n as usize), LPARAM(buf.as_mut_ptr() as isize));
    buf
}

unsafe fn list_item_text(list: HWND, idx: u32) -> String {
    let len = SendMessageW(list, LB_GETTEXTLEN, WPARAM(idx as usize), LPARAM(0)).0 as usize;
    let mut buf = vec![0u16; len + 1];
    SendMessageW(list, LB_GETTEXT, WPARAM(idx as usize), LPARAM(buf.as_mut_ptr() as isize));
    let end = buf.iter().position(|&c| c == 0).unwrap_or(0);
    String::from_utf16_lossy(&buf[..end])
}

unsafe fn restore_selected_backup(ctx: &SettingsUi, hwnd: HWND) {
    let sel = get_sel_items(ctx.hwnd_list);
    if sel.is_empty() {
        MessageBoxW(ctx.hwnd_list, w!("请先在列表中选择一份备份"), w!("恢复"), MB_OK | MB_ICONINFORMATION);
        return;
    }
    if sel.len() > 1 {
        MessageBoxW(
            ctx.hwnd_list,
            w!("恢复只支持单选：请只保留一份备份的选中。\n（多选用于「删除所选」批量清理）"),
            w!("恢复"),
            MB_OK | MB_ICONINFORMATION,
        );
        return;
    }
    let name = list_item_text(ctx.hwnd_list, sel[0]);
    let path = ctx.backup_dir.join(name);
    if MessageBoxW(
        ctx.hwnd_list,
        w!("将用所选备份替换当前 hosts 内容，并按正常保存流程写入（可能弹出 UAC）。继续？"),
        w!("恢复备份"),
        MB_YESNO | MB_ICONWARNING,
    ) != IDYES
    {
        return;
    }
    let app = &mut *(GetWindowLongPtrW(ctx.main, GWLP_USERDATA) as *mut App);
    match std::fs::read(&path) {
        Ok(bytes) => {
            app.doc = hostsmate::engine::parse_bytes(&bytes);
            app.mark_dirty();
            let _ = DestroyWindow(hwnd);
            save_file(app);
        }
        Err(e) => {
            MessageBoxW(ctx.hwnd_list, &HSTRING::from(format!("备份读取失败：{e}")), w!("恢复失败"), MB_OK | MB_ICONERROR);
        }
    }
}

/// 批量删除选中的备份文件，成功后刷新列表
unsafe fn delete_selected_backups(ctx: &SettingsUi) {
    let sel = get_sel_items(ctx.hwnd_list);
    if sel.is_empty() {
        MessageBoxW(ctx.hwnd_list, w!("请先在列表中选择要删除的备份"), w!("删除备份"), MB_OK | MB_ICONINFORMATION);
        return;
    }
    let prompt = HSTRING::from(format!("确定删除所选 {} 份备份？\n删除后无法恢复。", sel.len()));
    if MessageBoxW(ctx.hwnd_list, &prompt, w!("删除备份"), MB_YESNO | MB_ICONWARNING) != IDYES {
        return;
    }
    let mut errs: Vec<String> = Vec::new();
    for idx in sel {
        let name = list_item_text(ctx.hwnd_list, idx);
        if let Err(e) = std::fs::remove_file(ctx.backup_dir.join(&name)) {
            errs.push(format!("{name}：{e}"));
        }
    }
    fill_backup_list(ctx);
    if !errs.is_empty() {
        let msg = HSTRING::from(format!("部分备份删除失败：\n{}", errs.join("\n")));
        MessageBoxW(ctx.hwnd_list, &msg, w!("删除备份"), MB_OK | MB_ICONERROR);
    }
}
