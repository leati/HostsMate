//! 主窗口：消息循环、设计稿视觉（头部/搜索胶囊/状态条由 WM_PAINT 绘制）、
//! 自绘按钮 DRAWITEM 分发、布局（DPI 感知）、外部修改检测、右键菜单

use std::path::PathBuf;

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreatePen, DrawTextW, EndPaint, FillRect, InvalidateRect,
    PAINTSTRUCT, PS_SOLID,
};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateAcceleratorTableW, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect,
    GetMessageW, GetWindowLongPtrW, GetWindowRect, LoadCursorW, MessageBoxW, MoveWindow,
    PostQuitMessage, RegisterClassW, SendMessageW, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, SystemParametersInfoW,
    TranslateAcceleratorW, TranslateMessage, ACCEL, CS_HREDRAW, CS_VREDRAW,
    FCONTROL, FVIRTKEY, GWLP_USERDATA, HMENU, IDC_ARROW, MB_ICONWARNING, MSG, SPI_GETWORKAREA,
    SW_SHOW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WA_INACTIVE, WINDOW_EX_STYLE,
    WM_ACTIVATE, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY, WM_DRAWITEM, WM_NCDESTROY,
    WM_NCCALCSIZE, WM_NCHITTEST, WM_LBUTTONUP, WM_NOTIFY, WM_PAINT, WM_SETFOCUS,
    WM_SIZE, WM_SYSCOMMAND, WNDCLASSW, WS_CLIPCHILDREN,
    WS_OVERLAPPEDWINDOW,
};

use super::app::{
    App, CatFilter, FilterState, SortMode, CAT_ALL, CAT_BASE, CAT_FALLBACK,
    CHIP_ALL, CHIP_CONFLICT, CHIP_DISABLED, CHIP_ENABLED, IDC_SEARCH, IDM_ABOUT, IDM_DELETE,
    IDM_EXPORT, IDM_FOCUS_SEARCH, IDM_IMPORT, IDM_NEW, IDM_SAVE, IDM_SETTINGS, IDM_SORT,
    IDM_SORT_DOMAIN, IDM_SORT_FILE, IDM_SORT_IP, IDM_STATUS, chip_base_w, is_category_chip,
};
use super::commands;
use super::create::{set_title, TB_DEFS};
use super::draw;
use super::table;

pub unsafe fn run_main(file: PathBuf) -> i32 {
    let hmod = match GetModuleHandleW(None) {
        Ok(h) => h,
        Err(_) => return 1,
    };
    let hinst = HINSTANCE(hmod.0);

    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: hinst,
        hCursor: LoadCursorW(HINSTANCE::default(), IDC_ARROW).unwrap_or_default(),
        hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(6usize as *mut core::ffi::c_void),
        lpszClassName: w!("HostsMateMainWnd"),
        ..Default::default()
    };
    let _ = RegisterClassW(&wc);

    let mut work = RECT::default();
    if SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut work as *mut _ as _),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_err() {
        work.right = 1366;
        work.bottom = 768;
    }
    let work_w = work.right - work.left;
    let work_h = work.bottom - work.top;
    let initial_w = 1580.min((work_w - 24).max(960));
    let initial_h = 916.min((work_h - 24).max(640));
    let initial_x = work.left + (work_w - initial_w) / 2;
    let initial_y = work.top + (work_h - initial_h) / 2;

    let hwnd = match CreateWindowExW(
        WINDOW_EX_STYLE(0),
        wc.lpszClassName,
        w!("Hosts 文件编辑器"),
        WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
        initial_x,
        initial_y,
        initial_w,
        initial_h,
        HWND::default(),
        HMENU::default(),
        hinst,
        None,
    ) {
        Ok(h) => h,
        Err(_) => return 1,
    };

    let app = Box::new(App::new(file));
    let ptr = Box::into_raw(app);
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);

    {
        let app = &mut *ptr;
        // 根修复回执：主窗口句柄必须先入状态
        app.hwnd_main = hwnd;
        if let Err(e) = super::create::create_children(hwnd, app) {
            MessageBoxW(
                HWND::default(),
                &HSTRING::from(format!("控件创建失败：{e}")),
                w!("HostsMate"),
                MB_ICONWARNING,
            );
            return 1;
        }
        app.is_admin = super::elevate::is_admin();
        if let Err(e) = app.load_from_disk() {
            app.status_msg = e.clone();
            MessageBoxW(hwnd, &HSTRING::from(e), w!("读取警告"), MB_ICONWARNING);
        } else if app.path == hostsmate::engine::system_hosts_path() && !app.is_admin {
            app.status_msg = "普通权限运行：保存时将弹出 UAC 授权写入系统 hosts".into();
        }
        set_title(app);
        table::populate_all(app);
    }

    let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE,
        &DWMWCP_ROUND as *const _ as *const core::ffi::c_void,
        std::mem::size_of_val(&DWMWCP_ROUND) as u32);
    let _ = SetWindowPos(hwnd, HWND::default(), 0, 0, 0, 0,
        SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER);
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = InvalidateRect(hwnd, None, false);

    let accels = [
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: 'N' as u16,
            cmd: IDM_NEW as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: 'S' as u16,
            cmd: IDM_SAVE as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: 'F' as u16,
            cmd: IDM_FOCUS_SEARCH as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY,
            key: 0x2E, // VK_DELETE
            cmd: IDM_DELETE as u16,
        },
    ];
    let haccel = CreateAcceleratorTableW(&accels).unwrap_or_default();

    let mut msg = MSG::default();
    loop {
        let r = GetMessageW(&mut msg, HWND::default(), 0, 0);
        if r.0 <= 0 {
            break if r.0 == 0 { msg.wParam.0 as i32 } else { 1 };
        }
        // 行内编辑激活时旁路加速键，避免 Del/Ctrl+S 劫持编辑框按键。
        // 通过窗口 USERDATA 取指针：WM_NCDESTROY 已释放并清零后，
        // 队列里残留消息再进循环时裸指针 ptr 是悬垂的
        let live = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if !live.is_null() && (*live).edit.is_none() {
            TranslateAcceleratorW(hwnd, haccel, &msg);
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCALCSIZE && wp.0 != 0 {
        return LRESULT(0); // 标题栏改由客户区绘制
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let app = &mut *ptr;
    match msg {
        WM_NCHITTEST => {
            let sx = (lp.0 & 0xFFFF) as i16 as i32;
            let sy = ((lp.0 >> 16) & 0xFFFF) as i16 as i32;
            let mut wr = RECT::default();
            let _ = GetWindowRect(hwnd, &mut wr);
            let x = sx - wr.left;
            let y = sy - wr.top;
            let w = wr.right - wr.left;
            let h = wr.bottom - wr.top;
            let dpi = GetDpiForWindow(hwnd).clamp(96, 120) as i32;
            let edge = 6 * dpi / 96;
            let left = x < edge;
            let right = x >= w - edge;
            let top = y < edge;
            let bottom = y >= h - edge;
            let hit = if top && left { 13 } // HTTOPLEFT
                else if top && right { 14 } // HTTOPRIGHT
                else if bottom && left { 16 } // HTBOTTOMLEFT
                else if bottom && right { 17 } // HTBOTTOMRIGHT
                else if left { 10 } // HTLEFT
                else if right { 11 } // HTRIGHT
                else if top { 12 } // HTTOP
                else if bottom { 15 } // HTBOTTOM
                else if y < 53 * dpi / 96 && x < w - 166 * dpi / 96 { 2 } // HTCAPTION
                else { 1 }; // HTCLIENT
            LRESULT(hit)
        }
        WM_LBUTTONUP => {
            let x = (lp.0 & 0xFFFF) as i16 as i32;
            let y = ((lp.0 >> 16) & 0xFFFF) as i16 as i32;
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let dpi = GetDpiForWindow(hwnd).clamp(96, 120) as i32;
            let sc = |v: i32| v * dpi / 96;
            if y < sc(53) && x >= rc.right - sc(166) {
                let command = if x < rc.right - sc(110) { 0xF020 } // SC_MINIMIZE
                    else if x < rc.right - sc(55) {
                        if windows::Win32::UI::WindowsAndMessaging::IsZoomed(hwnd).as_bool() {
                            0xF120 // SC_RESTORE
                        } else { 0xF030 } // SC_MAXIMIZE
                    } else { 0xF060 }; // SC_CLOSE
                SendMessageW(hwnd, WM_SYSCOMMAND, WPARAM(command), LPARAM(0));
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_PAINT => {
            paint_chrome(app, hwnd);
            LRESULT(0)
        }
        WM_SIZE => {
            layout(app);
            LRESULT(0)
        }
        WM_SETFOCUS => {
            let _ = SetFocus(app.hwnd_records);
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if wp.0 & 0xFFFF != WA_INACTIVE as usize {
                commands::check_external(app);
            }
            LRESULT(0)
        }
        WM_DRAWITEM => {
            draw_item(app, lp);
            LRESULT(1)
        }
        WM_COMMAND => {
            let id = (wp.0 & 0xFFFF) as i32;
            let code = (wp.0 >> 16) as u32;
            match id {
                IDM_NEW | IDM_DELETE | IDM_SAVE | IDM_IMPORT | IDM_EXPORT | IDM_SETTINGS
                | IDM_ABOUT => commands::on_command(app, id),
                IDM_SORT => commands::sort_menu(app),
                IDM_STATUS => commands::status_menu(app),
                IDM_SORT_FILE | IDM_SORT_DOMAIN | IDM_SORT_IP => commands::on_sort(app, id),
                id if is_category_chip(id) => commands::on_category(app, id),
                CHIP_ALL | CHIP_ENABLED | CHIP_DISABLED | CHIP_CONFLICT => {
                    commands::on_chip(app, id)
                }
                IDC_SEARCH => commands::on_search_changed(app, code),
                _ => {}
            }
            LRESULT(0)
        }
        WM_NOTIFY => table::on_notify(app, lp),
        WM_CONTEXTMENU => {
            if wp.0 == app.hwnd_records.0 as usize {
                let (mut x, mut y) =
                    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
                if x == -1 && y == -1 {
                    let mut rc = RECT::default();
                    let _ = GetWindowRect(app.hwnd_records, &mut rc);
                    x = rc.left + 60;
                    y = rc.top + 60;
                }
                commands::show_record_menu(app, x, y);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CLOSE => {
            // 有未保存修改时先询问（是=保存并退出 / 否=直接退出 / 取消=留下）
            if app.dirty {
                let r = MessageBoxW(
                    app.hwnd_main,
                    w!("有未保存的修改，是否保存后退出？\n\n是 = 保存并退出\n否 = 不保存直接退出\n取消 = 留在程序"),
                    w!("关闭 Hosts 文件编辑器"),
                    windows::Win32::UI::WindowsAndMessaging::MB_YESNOCANCEL
                        | windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING,
                );
                if r == windows::Win32::UI::WindowsAndMessaging::IDCANCEL {
                    return LRESULT(0);
                }
                if r == windows::Win32::UI::WindowsAndMessaging::IDYES {
                    commands::save_file(app);
                    if app.dirty {
                        // 保存未成功（UAC 取消/写盘失败）：留在程序
                        return LRESULT(0);
                    }
                }
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// ---------- 设计稿视觉：头部 / 搜索胶囊 / 底部状态条 ----------

// 96 DPI 基准布局尺寸，paint_chrome 的分层横线与 layout 的行坐标共用，
// 两处各写一份裸数字会在调整头部高度时漂移（分隔线穿过 chips 的教训）
const SC_HEADER_H: i32 = 48;
const SC_TOOLBAR_H: i32 = 62;
const SC_CHIPS_GAP: i32 = 6;
const SC_CHIPS_H: i32 = 38;
const SC_CHIPS_TABLE_GAP: i32 = 13;
const SC_STATUS_H: i32 = 45;

unsafe fn paint_chrome(app: &mut App, hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let dpi = GetDpiForWindow(hwnd).clamp(96, 120) as i32;
    let sc = |v: i32| v * dpi / 96;

    // 浅灰蓝背景与分层横线（横线位置 = layout 的行边界，错位会穿过按钮）
    fill_color(hdc, &rc, windows::Win32::Foundation::COLORREF(0x00FC_F9F6));
    fill_color(hdc, &RECT { left: 0, top: 0, right: rc.right, bottom: sc(SC_HEADER_H) },
        windows::Win32::Foundation::COLORREF(0x00FC_F7F1));
    let toolbar_bottom = sc(SC_HEADER_H + SC_TOOLBAR_H);
    fill_color(hdc, &RECT { left: 0, top: sc(SC_HEADER_H), right: rc.right, bottom: toolbar_bottom }, draw::C_WHITE);
    fill_color(hdc, &RECT { left: 0, top: sc(SC_HEADER_H) - 1, right: rc.right, bottom: sc(SC_HEADER_H) }, draw::C_BORDER_LIGHT);
    fill_color(hdc, &RECT { left: 0, top: toolbar_bottom - 1, right: rc.right, bottom: toolbar_bottom }, draw::C_BORDER_LIGHT);
    paint_window_buttons(hdc, rc.right, dpi);
    let table_top = sc(SC_HEADER_H + SC_TOOLBAR_H + SC_CHIPS_GAP + SC_CHIPS_H + SC_CHIPS_TABLE_GAP);
    let table_bottom = rc.bottom - sc(SC_STATUS_H + 10);
    draw::round_rect(hdc, &RECT { left: sc(18), top: table_top,
        right: rc.right - sc(14), bottom: table_bottom }, sc(9),
        draw::C_WHITE, Some(draw::C_BORDER_LIGHT));

    // —— 头部：蓝色圆角应用图标 + 标题 ——
    let isz = sc(38);
    let irc = RECT {
        left: sc(17),
        top: sc(5),
        right: sc(17) + isz,
        bottom: sc(5) + isz,
    };
    draw::round_rect(hdc, &irc, sc(10), draw::C_PRIMARY, None);
    let mut doc = irc;
    doc.left += isz / 4;
    doc.right -= isz / 4;
    doc.top += isz / 5;
    doc.bottom -= isz / 5;
    draw::round_rect(hdc, &doc, sc(4), draw::C_WHITE, None);
    let line_brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(draw::C_PRIMARY);
    let old = windows::Win32::Graphics::Gdi::SelectObject(hdc, line_brush);
    let _ = FillRect(
        hdc,
        &RECT {
            left: doc.left + sc(4),
            top: doc.top + sc(5),
            right: doc.right - sc(4),
            bottom: doc.top + sc(7),
        },
        line_brush,
    );
    let _ = FillRect(
        hdc,
        &RECT {
            left: doc.left + sc(4),
            top: doc.top + sc(10),
            right: doc.right - sc(8),
            bottom: doc.top + sc(12),
        },
        line_brush,
    );
    windows::Win32::Graphics::Gdi::SelectObject(hdc, old);
    let _ = windows::Win32::Graphics::Gdi::DeleteObject(line_brush);

    let old_font = windows::Win32::Graphics::Gdi::SelectObject(hdc, app.header_font);
    let title = RECT {
        left: sc(70),
        top: sc(5),
        right: rc.right,
        bottom: sc(43),
    };
    text_out(hdc, &title, "Hosts 文件编辑器", draw::C_TEXT);
    let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old_font);

    // —— 搜索胶囊：圆角边框 + 放大镜 + Ctrl+F ——
    let mut er = RECT::default();
    let _ = GetWindowRect(app.hwnd_search, &mut er);
    let mut top_left = windows::Win32::Foundation::POINT { x: er.left, y: er.top };
    let mut bottom_right = windows::Win32::Foundation::POINT { x: er.right, y: er.bottom };
    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut top_left);
    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut bottom_right);
    er.left = top_left.x;
    er.top = top_left.y;
    er.right = bottom_right.x;
    er.bottom = bottom_right.y;
    if er.right > er.left {
        // 胶囊高度固定（视觉尺寸不随 EDIT 行高收窄而变），锚定 EDIT 垂直中心
        let cy = (er.top + er.bottom) / 2;
        let pill = RECT {
            left: er.left - sc(34),
            top: cy - sc(20),
            right: er.right + sc(58),
            bottom: cy + sc(20),
        };
        draw::round_rect(
            hdc,
            &pill,
            (pill.bottom - pill.top) / 2,
            draw::C_WHITE,
            Some(draw::C_BORDER),
        );
        let cx = pill.left + sc(16);
        let cy = (pill.top + pill.bottom) / 2;
        let r = sc(5);
        let pen = CreatePen(PS_SOLID, sc(2) / 2 + 1, draw::C_SUBTEXT);
        let old_pen = windows::Win32::Graphics::Gdi::SelectObject(hdc, pen);
        let brush = windows::Win32::Graphics::Gdi::GetStockObject(
            windows::Win32::Graphics::Gdi::NULL_BRUSH,
        );
        let old_brush = windows::Win32::Graphics::Gdi::SelectObject(hdc, brush);
        let _ = windows::Win32::Graphics::Gdi::Ellipse(hdc, cx - r, cy - r, cx + r, cy + r);
        let _ = windows::Win32::Graphics::Gdi::MoveToEx(hdc, cx + r, cy + r, None);
        let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, cx + r + sc(5), cy + r + sc(5));
        windows::Win32::Graphics::Gdi::SelectObject(hdc, old_pen);
        windows::Win32::Graphics::Gdi::SelectObject(hdc, old_brush);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(pen);
        let old_font = windows::Win32::Graphics::Gdi::SelectObject(hdc, app.sub_font);
        // 左边界须在搜索 EDIT 右缘之外，否则 EDIT 子窗口的白色背景会盖掉 C 的左半
        let hint = RECT {
            left: er.right + sc(2),
            top: pill.top,
            right: pill.right - sc(10),
            bottom: pill.bottom,
        };
        text_out_right(hdc, &hint, "Ctrl+F", draw::C_SUBTEXT);
        let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old_font);
    }

    // —— 底部状态条 ——
    let sy = rc.bottom - sc(45);
    let band = RECT {
        left: rc.left,
        top: sy,
        right: rc.right,
        bottom: rc.bottom,
    };
    fill_color(hdc, &band, draw::C_BAND_BG);
    fill_color(hdc, &RECT { left: 0, top: sy, right: rc.right, bottom: sy + 1 }, draw::C_BORDER_LIGHT);
    let old_font = windows::Win32::Graphics::Gdi::SelectObject(hdc, app.sub_font);
    let path_r = RECT {
        left: sc(55),
        top: sy,
        right: sc(55) + sc(480),
        bottom: rc.bottom,
    };
    let icon_rc = RECT { left: sc(25), top: sy + sc(11), right: sc(41), bottom: sy + sc(30) };
    draw::round_rect(hdc, &icon_rc, sc(2), draw::C_SUBTEXT, None);
    text_out(hdc, &RECT { left: sc(29), top: sy + sc(10), right: sc(42), bottom: sy + sc(29) },
        "≡", draw::C_WHITE);
    text_out(hdc, &path_r, &app.path.display().to_string(), draw::C_TEXT);
    let total = app.count_enabled + app.count_disabled;
    let path_w = draw::text_width(hdc, &app.path.display().to_string());
    let count_r = RECT {
        left: sc(55) + path_w + sc(25),
        top: sy,
        right: rc.right,
        bottom: rc.bottom,
    };
    text_out(hdc, &count_r, &format!("{} 条记录", total), draw::C_SUBTEXT);

    let writable = app.path != hostsmate::engine::system_hosts_path() || app.is_admin;
    let (wt, wc) = if writable {
        ("可写", draw::C_GREEN_TEXT)
    } else {
        ("需管理员", draw::C_SUBTEXT)
    };
    let check = RECT { left: rc.right - sc(276), top: sy + sc(14),
        right: rc.right - sc(256), bottom: sy + sc(34) };
    draw::round_rect(hdc, &check, sc(10), draw::C_GREEN_TEXT, None);
    let pen = CreatePen(PS_SOLID, sc(2).max(1), draw::C_WHITE);
    let old_pen = windows::Win32::Graphics::Gdi::SelectObject(hdc, pen);
    let _ = windows::Win32::Graphics::Gdi::MoveToEx(hdc, check.left + sc(5), check.top + sc(10), None);
    let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, check.left + sc(9), check.top + sc(14));
    let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, check.left + sc(16), check.top + sc(6));
    windows::Win32::Graphics::Gdi::SelectObject(hdc, old_pen);
    let _ = windows::Win32::Graphics::Gdi::DeleteObject(pen);
    let writable_label = RECT { left: rc.right - sc(252), top: sy,
        right: rc.right - sc(211), bottom: rc.bottom };
    text_out(hdc, &writable_label, wt, wc);
    fill_color(hdc, &RECT { left: rc.right - sc(207), top: sy + sc(12),
        right: rc.right - sc(207) + 1, bottom: rc.bottom - sc(12) }, draw::C_BORDER_LIGHT);
    let last = RECT {
        left: rc.right - sc(195),
        top: sy,
        right: rc.right - sc(20),
        bottom: rc.bottom,
    };
    let last_text = match &app.last_save {
        Some(t) => format!("上次保存：{}", t),
        None => "上次保存：—".into(),
    };
    text_out_right(hdc, &last, &last_text, draw::C_SUBTEXT);
    let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old_font);

    let _ = EndPaint(hwnd, &ps);
}

unsafe fn fill_color(hdc: windows::Win32::Graphics::Gdi::HDC, rc: &RECT,
    color: windows::Win32::Foundation::COLORREF) {
    let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(color);
    let _ = FillRect(hdc, rc, brush);
    let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush);
}

unsafe fn paint_window_buttons(hdc: windows::Win32::Graphics::Gdi::HDC, right: i32, dpi: i32) {
    let sc = |v: i32| v * dpi / 96;
    let pen = CreatePen(PS_SOLID, sc(2).max(1), draw::C_TEXT);
    let old = windows::Win32::Graphics::Gdi::SelectObject(hdc, pen);
    let y = sc(24);
    let x1 = right - sc(138);
    let x2 = right - sc(82);
    let x3 = right - sc(27);
    let _ = windows::Win32::Graphics::Gdi::MoveToEx(hdc, x1 - sc(7), y, None);
    let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, x1 + sc(7), y);
    let _ = windows::Win32::Graphics::Gdi::Rectangle(hdc, x2 - sc(7), y - sc(7),
        x2 + sc(7), y + sc(7));
    let _ = windows::Win32::Graphics::Gdi::MoveToEx(hdc, x3 - sc(7), y - sc(7), None);
    let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, x3 + sc(7), y + sc(7));
    let _ = windows::Win32::Graphics::Gdi::MoveToEx(hdc, x3 + sc(7), y - sc(7), None);
    let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, x3 - sc(7), y + sc(7));
    let _ = windows::Win32::Graphics::Gdi::SelectObject(hdc, old);
    let _ = windows::Win32::Graphics::Gdi::DeleteObject(pen);
}

unsafe fn text_out(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rc: &RECT,
    text: &str,
    color: windows::Win32::Foundation::COLORREF,
) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    windows::Win32::Graphics::Gdi::SetBkMode(hdc, windows::Win32::Graphics::Gdi::TRANSPARENT);
    windows::Win32::Graphics::Gdi::SetTextColor(hdc, color);
    let _ = DrawTextW(
        hdc,
        &mut wide,
        rc as *const RECT as *mut RECT,
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
}

unsafe fn text_out_right(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rc: &RECT,
    text: &str,
    color: windows::Win32::Foundation::COLORREF,
) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    windows::Win32::Graphics::Gdi::SetBkMode(hdc, windows::Win32::Graphics::Gdi::TRANSPARENT);
    windows::Win32::Graphics::Gdi::SetTextColor(hdc, color);
    let _ = DrawTextW(
        hdc,
        &mut wide,
        rc as *const RECT as *mut RECT,
        windows::Win32::Graphics::Gdi::DT_RIGHT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
}

// ---------- 自绘按钮 DRAWITEM 分发 ----------

/// chip ID → (筛选, 文案, 计数, 图标序号)；分类被删后未重建的越界 chip 回退「全部」样式
fn chip_info(app: &App, id: i32) -> (CatFilter, String, usize, usize) {
    let total = app.count_enabled + app.count_disabled;
    let n = app.settings.rules.cats.len();
    if id == CAT_ALL {
        return (CatFilter::All, "全部".to_string(), total, 0);
    }
    if id == CAT_FALLBACK {
        let count = app.category_counts.get(n).copied().unwrap_or(0);
        return (CatFilter::Cat(n), "自定义".to_string(), count, 5);
    }
    let i = (id - CAT_BASE).max(0) as usize;
    match app.settings.rules.cats.get(i) {
        Some(cat) => {
            let count = app.category_counts.get(i).copied().unwrap_or(0);
            (CatFilter::Cat(i), cat.name.clone(), count, icon_index_for(i, &cat.name))
        }
        None => (CatFilter::All, "全部".to_string(), total, 0),
    }
}

/// 分类图标：默认四类沿用原图标序（广告=1 跟踪=2 分析=3 CDN=4），其余按位置轮换
fn icon_index_for(i: usize, name: &str) -> usize {
    match name {
        "广告" => 1,
        "跟踪" => 2,
        "分析" => 3,
        "CDN" => 4,
        _ => i % 4 + 1,
    }
}

unsafe fn draw_item(app: &mut App, lp: LPARAM) {
    let di = &*(lp.0 as *const DRAWITEMSTRUCT);
    let id = di.CtlID as i32;
    let (pressed, disabled) = draw::states(di.itemState.0);
    let hdc = di.hDC;
    let rc = di.rcItem;
    let dpi = GetDpiForWindow(app.hwnd_main).clamp(96, 120) as i32;

    // 工具栏按钮
    if let Some((_, text, icon, primary)) = TB_DEFS.iter().find(|(bid, _, _, _)| *bid == id) {
        fill_color(hdc, &rc, draw::C_WHITE);
        draw::draw_pill(
            hdc,
            &rc,
            draw::PillSpec {
                text,
                icon: Some(*icon),
                primary: *primary,
                selected: false,
                pressed,
                disabled,
                dot: None,
                radius: 10 * dpi / 96,
            },
            &app.btn_dcs,
            dpi,
        );
        return;
    }
    // 分类筛选（chips 随自定义分类动态生成，文案/计数/图标按当前设置取）
    if is_category_chip(id) {
        fill_color(hdc, &rc, windows::Win32::Foundation::COLORREF(0x00FC_F9F6));
        let (filter, label, count, index) = chip_info(app, id);
        let selected = app.category == filter;
        draw::draw_category_chip(hdc, &rc, &label, count, index, selected, dpi, &app.cat_dcs,
            chip_base_w(id));
        return;
    }
    if id == IDM_STATUS {
        fill_color(hdc, &rc, windows::Win32::Foundation::COLORREF(0x00FC_F9F6));
        let label = match app.filter_state {
            FilterState::All => "全部状态  ▾",
            FilterState::Enabled => "已启用  ▾",
            FilterState::Disabled => "已禁用  ▾",
            FilterState::Conflicted => "有冲突  ▾",
        };
        draw::draw_pill(hdc, &rc, draw::PillSpec { text: label, icon: None,
            primary: false, selected: false, pressed, disabled, dot: None,
            radius: 8 * dpi / 96 }, &app.btn_dcs, dpi);
        return;
    }
    // 排序下拉按钮
    if id == IDM_SORT {
        fill_color(hdc, &rc, windows::Win32::Foundation::COLORREF(0x00FC_F9F6));
        let label = match app.sort_mode {
            SortMode::FileOrder => "☷  ▾",
            SortMode::Domain => "域名 ▾",
            SortMode::Ip => "IP ▾",
        };
        draw::draw_pill(
            hdc,
            &rc,
            draw::PillSpec {
                text: label,
                icon: None,
                primary: false,
                selected: false,
                pressed,
                disabled,
                dot: None,
                radius: (rc.bottom - rc.top) / 2,
            },
            &app.btn_dcs,
            dpi,
        );
        return;
    }
}

// ---------- 布局（DPI 感知） ----------

pub unsafe fn layout(app: &mut App) {
    let dpi = GetDpiForWindow(app.hwnd_main).clamp(96, 120) as i32;
    let sc = |v: i32| v * dpi / 96;

    let mut rc = RECT::default();
    let _ = GetClientRect(app.hwnd_main, &mut rc);
    let width = rc.right;

    // 头部（纯绘制区）
    let header_h = sc(SC_HEADER_H);
    // 工具栏行
    let tb_y = header_h;
    let tb_h = sc(SC_TOOLBAR_H);
    // chips 行
    let chips_y = tb_y + tb_h + sc(SC_CHIPS_GAP);
    let chips_h = sc(SC_CHIPS_H);
    // 状态条
    let status_h = sc(SC_STATUS_H);

    // 工具栏按钮（紧凑），记录末端位置供搜索框弹性布局
    let margin = sc(18);
    let button_bases = [102, 94, 94, 98, 100, 93, 104];
    let compact = width < sc(1200);
    let button_gap = sc(if compact { 6 } else { 10 });
    let section_gap = sc(if compact { 8 } else { 35 });
    let search_outer_w = if compact { sc(240) } else {
        (width * 29 / 100).clamp(sc(280), sc(368))
    };
    let search_left = width - margin - search_outer_w;
    let gaps = button_gap * 5 + section_gap;
    let button_room = (search_left - sc(22) - margin - gaps)
        .max(sc(if compact { 410 } else { 490 }));
    let button_total = sc(button_bases.iter().sum::<i32>());
    let mut x = margin;
    for (i, ((h, _), base)) in app.hwnd_tbtns.iter().zip(button_bases).enumerate() {
        let w = sc(base).min(sc(base) * button_room / button_total);
        let _ = MoveWindow(*h, x, tb_y + sc(8), w, sc(38), true);
        x += w + if i == 4 { section_gap } else { button_gap };
    }
    // 搜索框只占胶囊内部，右侧预留快捷键提示
    let search_x = search_left;
    let search_w = search_outer_w - sc(34) - sc(58);
    // 单行 EDIT 的正文与占位提示（EM_SETCUEBANNER）都按顶对齐绘制，
    // 高度须收到一行字高才能在胶囊里垂直居中
    let mut tm = windows::Win32::Graphics::Gdi::TEXTMETRICW::default();
    let hdc_search = windows::Win32::Graphics::Gdi::GetDC(app.hwnd_search);
    let old_font = windows::Win32::Graphics::Gdi::SelectObject(hdc_search, app.font);
    let _ = windows::Win32::Graphics::Gdi::GetTextMetricsW(hdc_search, &mut tm);
    windows::Win32::Graphics::Gdi::SelectObject(hdc_search, old_font);
    windows::Win32::Graphics::Gdi::ReleaseDC(app.hwnd_search, hdc_search);
    let search_h = tm.tmHeight + sc(4);
    let _ = MoveWindow(
        app.hwnd_search,
        search_x + sc(34),
        tb_y + (tb_h - search_h) / 2,
        search_w,
        search_h,
        true,
    );

    // chips + 排序（chip 数量随自定义分类变化，基准宽度按 ID 取）
    let status_w = sc(121);
    let sort_w = sc(70);
    let chip_gap = sc(10);
    let chip_end = width - margin - status_w - sort_w - sc(12) - sc(20);
    let chip_bases: Vec<i32> = app.hwnd_chips.iter().map(|(_, id)| chip_base_w(*id)).collect();
    let gaps = chip_gap * app.hwnd_chips.len().saturating_sub(1) as i32;
    let chip_room = chip_end - margin - gaps;
    let chip_total = sc(chip_bases.iter().sum::<i32>());
    let mut cx = margin;
    for ((h, _), base) in app.hwnd_chips.iter().zip(chip_bases.iter()) {
        let w = sc(*base).min(sc(*base) * chip_room / chip_total).max(sc(76));
        let _ = MoveWindow(*h, cx, chips_y, w, chips_h, true);
        cx += w + chip_gap;
    }
    let _ = MoveWindow(app.hwnd_status, width - margin - status_w - sort_w - sc(12), chips_y,
        status_w, chips_h, true);
    let _ = MoveWindow(app.hwnd_sort, width - margin - sort_w, chips_y, sort_w, chips_h, true);

    // 记录表
    let table_y = chips_y + chips_h + sc(SC_CHIPS_TABLE_GAP);
    let table_h = rc.bottom - status_h - table_y - sc(10);
    let _ = MoveWindow(app.hwnd_records, margin + 1, table_y + 1,
        width - margin - sc(14) - 2, (table_h - 2).max(50), true);

    // 列宽自适应（0x101E = LVM_SETCOLUMNWIDTH）；基准用记录表自身客户区
    let mut crc = RECT::default();
    let _ = GetClientRect(app.hwnd_records, &mut crc);
    let avail = (crc.right - 2).max(240);
    let w_select = sc(if compact { 40 } else { 44 });
    let w_actions = sc(if compact { 78 } else { 114 });
    let w_enabled = sc(if compact { 74 } else { 98 });
    let w_conflict = sc(if compact { 92 } else { 143 });
    let rest = (avail - w_select - w_enabled - w_conflict - w_actions).max(160);
    let w_domain = rest * 39 / 100;
    let w_ip = rest * 21 / 100;
    let w_comment = rest - w_domain - w_ip;
    for (col, w) in [
        (0usize, w_select),
        (1, w_domain),
        (2, w_ip),
        (3, w_comment),
        (4, w_enabled),
        (5, w_conflict),
        (6, w_actions),
    ] {
        SendMessageW(app.hwnd_records, 0x101E, WPARAM(col), LPARAM(w.max(30) as isize));
    }
}
