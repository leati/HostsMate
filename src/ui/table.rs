//! 表格交互：全局平铺记录表（过滤/冲突列）、行内编辑、自绘复选框与冲突底色、底部冲突条

use std::net::IpAddr;

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, InvalidateRect, ReleaseDC};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    CDDS_ITEMPOSTPAINT, CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDDS_SUBITEM, CDRF_DODEFAULT,
    CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYPOSTPAINT, CDRF_NOTIFYSUBITEMDRAW,
    CDRF_SKIPDEFAULT, EM_SETSEL,
    LIST_VIEW_ITEM_STATE_FLAGS, LVIF_PARAM, LVIF_STATE, LVIF_TEXT, LVIR_BOUNDS, LVIS_FOCUSED,
    LVIS_SELECTED, LVITEMW, LVNI_SELECTED, LVM_DELETEALLITEMS,
    LVM_ENSUREVISIBLE, LVM_INSERTITEMW, LVM_REDRAWITEMS, LVM_SETITEMTEXTW, NM_CLICK,
    NM_CUSTOMDRAW, NM_DBLCLK, NMHDR, NMITEMACTIVATE, NMLVCUSTOMDRAW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VIRTUAL_KEY, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
    GetParent, GetWindowLongPtrW, MessageBoxW,
    SendMessageW, SetWindowLongPtrW, SetWindowTextW,
    ES_AUTOHSCROLL, GWLP_USERDATA, GWLP_WNDPROC, MB_ICONERROR, MB_ICONQUESTION, MB_OK, MB_YESNO, IDYES,
    WM_CHAR, WM_GETTEXT, WM_GETTEXTLENGTH, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONUP,
    WM_PAINT, WM_SETFONT, WM_SETREDRAW,
    WINDOWPOS, WINDOW_STYLE, WNDPROC, WS_CHILD, WS_EX_CLIENTEDGE, WS_VISIBLE,
};

use super::app::{
    role_text, App, CatFilter, ConflictRole, EditSession, FilterState, SortMode, COL_ACTIONS,
    COL_COMMENT, COL_CONFLICT, COL_DOMAIN, COL_ENABLED, COL_IP, COL_SELECT,
};
use super::commands;
use super::create;
use super::draw;

// ---------- 填充 ----------

pub unsafe fn subclass_header(app: &mut App) {
    let header = HWND(SendMessageW(app.hwnd_records, 0x1000 + 31, WPARAM(0), LPARAM(0)).0 as *mut _);
    if header.0.is_null() { return; }
    app.hwnd_header = header;
    app.header_old_proc = SetWindowLongPtrW(header, GWLP_WNDPROC,
        header_proc as *const () as usize as isize);
}

#[repr(C)]
struct HeaderLayout { rect: *mut RECT, pos: *mut WINDOWPOS }

unsafe extern "system" fn header_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let lv = GetParent(hwnd).unwrap_or_default();
    let main = GetParent(lv).unwrap_or_default();
    let ptr = GetWindowLongPtrW(main, GWLP_USERDATA) as *mut App;
    if ptr.is_null() { return DefWindowProcW(hwnd, msg, wp, lp); }
    let app = &mut *ptr;
    if msg == WM_LBUTTONUP {
        let first_w = SendMessageW(lv, 0x1000 + 29, WPARAM(0), LPARAM(0)).0 as i32; // LVM_GETCOLUMNWIDTH
        if (lp.0 & 0xFFFF) as i16 as i32 <= first_w {
        let all_checked = !app.row_map.is_empty()
            && app.row_map.iter().all(|cell| app.selected_cells.contains(cell));
        if all_checked { app.selected_cells.clear(); }
        else { app.selected_cells.extend(app.row_map.iter().copied()); }
        let _ = InvalidateRect(lv, None, false);
        let _ = InvalidateRect(hwnd, None, false);
        return LRESULT(0);
        }
    }
    let old = app.header_old_proc;
    let result = if old != 0 { call_old(old, hwnd, msg, wp, lp) }
        else { DefWindowProcW(hwnd, msg, wp, lp) };
    if msg == 0x1200 + 5 && lp.0 != 0 { // HDM_LAYOUT
        let layout = &mut *(lp.0 as *mut HeaderLayout);
        if !layout.pos.is_null() {
            let desired = 36 * GetDpiForWindow(main).clamp(96, 120) as i32 / 96;
            let old_height = (*layout.pos).cy;
            (*layout.pos).cy = desired;
            if !layout.rect.is_null() {
                (*layout.rect).top += desired - old_height;
            }
        }
    }
    if msg == WM_PAINT {
        let first_w = SendMessageW(lv, 0x1000 + 29, WPARAM(0), LPARAM(0)).0 as i32;
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let hdc = GetDC(hwnd);
        let font_old = windows::Win32::Graphics::Gdi::SelectObject(hdc, app.font);
        let mut x = 0;
        for (col, label) in ["", "域名", "IP 地址", "备注", "启用", "冲突", "操作"].iter().enumerate() {
            let w = SendMessageW(lv, 0x1000 + 29, WPARAM(col), LPARAM(0)).0 as i32;
            let cell = RECT { left: x, top: 0, right: x + w, bottom: rc.bottom };
            draw::draw_cell_text(hdc, &cell, label, draw::C_TEXT,
                draw::C_BAND_BG, GetDpiForWindow(main).clamp(96, 120) as i32);
            x += w;
        }
        rc.right = first_w;
        let checked = !app.row_map.is_empty()
            && app.row_map.iter().all(|cell| app.selected_cells.contains(cell));
        draw::draw_checkbox(hdc, &rc, checked,
            GetDpiForWindow(main).clamp(96, 120) as i32);
        windows::Win32::Graphics::Gdi::SelectObject(hdc, font_old);
        let _ = ReleaseDC(hwnd, hdc);
    }
    result
}

pub unsafe fn populate_all(app: &mut App) {
    app.busy = true;
    app.refresh_conflicts();
    populate_records(app);
    refresh_conflict_panel(app);
    app.busy = false;
    refresh_chrome(app);
}

/// 全局平铺填充：按文件顺序展示所有方案的记录（UI 不再分组展示，
/// 但文件中既有的 `#Group:` 结构由引擎原样保留）
pub unsafe fn populate_records(app: &mut App) {
    let lv = app.hwnd_records;
    app.selected_cells.clear();
    let filter = app.filter.clone();
    SendMessageW(lv, WM_SETREDRAW, WPARAM(0), LPARAM(0));
    SendMessageW(lv, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));
    app.row_map.clear();

    // 收集全部记录单元格 → 文本/状态过滤 → 排序（仅显示序，文件顺序不变）
    let mut cells: Vec<(usize, usize)> = Vec::new();
    for (si, s) in app.doc.schemes.iter().enumerate() {
        for (ei, e) in s.entries.iter().enumerate() {
            if matches!(e, hostsmate::engine::Entry::Record(_)) {
                cells.push((si, ei));
            }
        }
    }
    cells.retain(|&cell| {
        app.record(cell.0, cell.1).map_or(false, |r| app.record_matches(r, cell, &filter))
    });
    match app.sort_mode {
        SortMode::FileOrder => {}
        SortMode::Domain => cells.sort_by_key(|&c| {
            app.record(c.0, c.1)
                .map(|r| r.domain.to_lowercase())
                .unwrap_or_default()
        }),
        SortMode::Ip => cells.sort_by_key(|&c| app.record(c.0, c.1).map(|r| r.ip).unwrap()),
    }

    for (row, &cell) in cells.iter().enumerate() {
        let Some((domain, ip, comment)) = app.record(cell.0, cell.1).map(|r| {
            (r.domain.clone(), r.ip.to_string(), r.comment.clone().unwrap_or_default())
        }) else {
            continue;
        };
        let row = row as i32;
        app.row_map.push(cell);
        let d = HSTRING::from(&domain);
        let ip = HSTRING::from(&ip);
        let c = HSTRING::from(&comment);
        let empty = HSTRING::from("");
        let mut item = LVITEMW {
            mask: LVIF_TEXT | LVIF_PARAM,
            iItem: row,
            pszText: std::mem::transmute(empty.as_ptr()),
            lParam: LPARAM(((cell.0 as u64) << 32 | cell.1 as u64) as isize),
            ..Default::default()
        };
        SendMessageW(lv, LVM_INSERTITEMW, WPARAM(0), LPARAM(&mut item as *mut _ as isize));
        for (sub, h) in [(COL_DOMAIN, &d), (COL_IP, &ip), (COL_COMMENT, &c)] {
            let mut si_item = LVITEMW {
                mask: LVIF_TEXT,
                iItem: row,
                iSubItem: sub as i32,
                pszText: std::mem::transmute(h.as_ptr()),
                ..Default::default()
            };
            SendMessageW(
                lv,
                LVM_SETITEMTEXTW,
                WPARAM(row as usize),
                LPARAM(&mut si_item as *mut _ as isize),
            );
        }
    }
    SendMessageW(lv, WM_SETREDRAW, WPARAM(1), LPARAM(0));
    let _ = InvalidateRect(lv, None, true);
}

pub unsafe fn refresh_chrome(app: &mut App) {
    create::set_title(app);
    // 头部/状态条/chips 均由主窗口 WM_PAINT 与自绘按钮承担，整体重绘即可
    let _ = InvalidateRect(app.hwnd_main, None, false);
}

/// 冲突筛选变更后刷新表格与 chips
pub unsafe fn refresh_conflict_panel(app: &mut App) {
    app.busy = true;
    populate_records(app);
    app.busy = false;
}

unsafe fn select_row(lv: HWND, index: usize) {
    let sel = LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0);
    let mut si = LVITEMW {
        mask: LVIF_STATE,
        state: sel,
        stateMask: sel,
        iItem: index as i32,
        ..Default::default()
    };
    SendMessageW(
        lv,
        0x1000 + 43, // LVM_SETITEMSTATE
        WPARAM(index),
        LPARAM(&mut si as *mut _ as isize),
    );
}

unsafe fn get_next_selected(lv: HWND, start: i32) -> i32 {
    let wp = if start < 0 { usize::MAX } else { start as usize };
    SendMessageW(
        lv,
        0x1000 + 12, // LVM_GETNEXTITEM
        WPARAM(wp),
        LPARAM(LVNI_SELECTED as isize),
    )
    .0 as i32
}

/// 首个选中行（无则 None）
pub unsafe fn first_selected_row(app: &App) -> Option<usize> {
    let row = get_next_selected(app.hwnd_records, -1);
    if row >= 0 { Some(row as usize) } else { None }
}

// ---------- 通知 ----------

pub unsafe fn on_notify(app: &mut App, lp: LPARAM) -> LRESULT {
    let nm = &*(lp.0 as *const NMHDR);
    if nm.hwndFrom == app.hwnd_records {
        match nm.code {
            NM_CLICK => record_click(app, lp),
            NM_DBLCLK => record_dblclk(app, lp),
            NM_CUSTOMDRAW => return record_custom_draw(app, lp),
            _ => {}
        }
    }
    LRESULT(0)
}

/// 点击「启用」列翻转开关；「操作」列命中编辑/删除小按钮
unsafe fn record_click(app: &mut App, lp: LPARAM) {
    let nmia = &*(lp.0 as *const NMITEMACTIVATE);
    if nmia.iItem < 0 {
        return;
    }
    let row = nmia.iItem as usize;
    let sub = nmia.iSubItem as usize;
    match sub {
        COL_SELECT => {
            if let Some(&cell) = app.row_map.get(row) {
                if !app.selected_cells.insert(cell) { app.selected_cells.remove(&cell); }
                SendMessageW(app.hwnd_records, LVM_REDRAWITEMS, WPARAM(row), LPARAM(row as isize));
            }
        }
        COL_ENABLED => toggle_row(app, row),
        COL_ACTIONS => {
            let Some(&cell) = app.row_map.get(row) else { return };
            // ptAction 为表客户区坐标；操作列两个小按钮各占一半
            let mut rc = RECT {
                left: LVIR_BOUNDS as i32,
                top: COL_ACTIONS as i32,
                ..Default::default()
            };
            SendMessageW(
                app.hwnd_records,
                0x1000 + 56, // LVM_GETSUBITEMRECT
                WPARAM(row),
                LPARAM(&mut rc as *mut _ as isize),
            );
            let dpi = GetDpiForWindow(app.hwnd_main).max(96) as i32;
            let u = dpi as f64 / 96.0;
            let bw = (26.0 * u) as i32;
            let gap = (6.0 * u) as i32;
            let total = bw * 2 + gap;
            let x0 = (rc.left + rc.right - total) / 2;
            if nmia.ptAction.x < x0 + bw {
                start_edit(app, row, COL_DOMAIN);
            } else {
                app.delete_one(cell);
                app.busy = true;
                populate_records(app);
                app.refresh_conflicts();
                app.busy = false;
                refresh_chrome(app);
            }
        }
        _ => {}
    }
}

unsafe fn record_dblclk(app: &mut App, lp: LPARAM) {
    let nmia = &*(lp.0 as *const NMITEMACTIVATE);
    if nmia.iItem >= 0 {
        let col = nmia.iSubItem as usize;
        if col <= COL_COMMENT {
            start_edit(app, nmia.iItem as usize, col);
        }
    }
}

/// 自绘「启用」列复选框；冲突行底色；禁用行文字置灰
unsafe fn record_custom_draw(app: &mut App, lp: LPARAM) -> LRESULT {
    let lvcd = &mut *(lp.0 as *mut NMLVCUSTOMDRAW);
    let stage = lvcd.nmcd.dwDrawStage.0;
    if stage == CDDS_PREPAINT.0 {
        return LRESULT(CDRF_NOTIFYITEMDRAW as isize);
    }
    if stage == CDDS_ITEMPREPAINT.0 {
        let row = lvcd.nmcd.dwItemSpec as usize;
        lvcd.clrTextBk = if row % 2 == 0 { draw::C_WHITE } else { draw::C_BAND_BG };
        lvcd.clrText = draw::C_TEXT;
        if let Some(r) = app.record_by_row(row) {
            if !r.enabled {
                lvcd.clrText = draw::C_DISABLED;
            }
        }
        return LRESULT((CDRF_NOTIFYSUBITEMDRAW | CDRF_NOTIFYPOSTPAINT) as isize);
    }
    if stage == CDDS_ITEMPOSTPAINT.0 {
        let row = lvcd.nmcd.dwItemSpec as usize;
        let mut rc = RECT { left: LVIR_BOUNDS as i32, top: COL_DOMAIN as i32,
            ..Default::default() };
        SendMessageW(app.hwnd_records, 0x1000 + 56, WPARAM(row),
            LPARAM(&mut rc as *mut _ as isize));
        rc.right = rc.left;
        rc.left = 0;
        let checked = app.row_map.get(row).is_some_and(|c| app.selected_cells.contains(c));
        draw::draw_checkbox(lvcd.nmcd.hdc, &rc, checked,
            GetDpiForWindow(app.hwnd_main).clamp(96, 120) as i32);
        return LRESULT(CDRF_DODEFAULT as isize);
    }
    if stage == CDDS_SUBITEM.0 | CDDS_ITEMPREPAINT.0 {
        let row = lvcd.nmcd.dwItemSpec as usize;
        let sub = lvcd.iSubItem as usize;
        // 控件对默认绘制的 subitem 不发 POSTPAINT，自绘必须在 PREPAINT 做并返回 SKIPDEFAULT
        if sub == COL_SELECT || sub > COL_ACTIONS {
            return LRESULT(CDRF_DODEFAULT as isize);
        }
        let mut rc = RECT {
            left: LVIR_BOUNDS as i32,
            top: sub as i32,
            ..Default::default()
        };
        SendMessageW(
            app.hwnd_records,
            0x1000 + 56, // LVM_GETSUBITEMRECT
            WPARAM(row),
            LPARAM(&mut rc as *mut _ as isize),
        );
        let dpi = GetDpiForWindow(app.hwnd_main).clamp(96, 120) as i32;
        let native_selected = SendMessageW(app.hwnd_records, 0x1000 + 44,
            WPARAM(row), LPARAM(LVIS_SELECTED.0 as isize)).0 as u32 & LVIS_SELECTED.0 != 0;
        let checked = app.row_map.get(row).is_some_and(|c| app.selected_cells.contains(c));
        let bg = if checked || native_selected { draw::C_BLUE_TINT }
            else if row % 2 == 0 { draw::C_WHITE } else { draw::C_BAND_BG };
        let record = app.record_by_row(row);
        let color = if record.is_some_and(|r| !r.enabled) { draw::C_DISABLED }
            else if sub == COL_COMMENT { draw::C_SUBTEXT } else { draw::C_TEXT };
        let content = match sub {
            COL_DOMAIN => record.map(|r| r.domain.clone()).unwrap_or_default(),
            COL_IP => record.map(|r| r.ip.to_string()).unwrap_or_default(),
            COL_COMMENT => record.and_then(|r| r.comment.clone()).unwrap_or_default(),
            _ => String::new(),
        };
        draw::draw_cell_text(lvcd.nmcd.hdc, &rc, &content, color, bg, dpi);
        match sub {
            COL_ENABLED => {
                let on = app.record_by_row(row).is_some_and(|r| r.enabled);
                draw::draw_toggle(lvcd.nmcd.hdc, &rc, on, dpi);
            }
            COL_CONFLICT => {
                let role = app.row_map.get(row).and_then(|c| app.roles.get(c)).copied();
                match role {
                    Some(r) => {
                        let (text, bg, fg) = match r {
                            ConflictRole::RealEffective | ConflictRole::RedundantKept => {
                                ("✓ 生效", draw::C_GREEN_BG, draw::C_GREEN_TEXT)
                            }
                            ConflictRole::RealOverridden => {
                                ("✗ 被覆盖", draw::C_RED_BG, draw::C_RED_TEXT)
                            }
                            ConflictRole::RedundantExtra => {
                                ("－ 冗余", draw::C_YELLOW_BG, draw::C_YELLOW_TEXT)
                            }
                        };
                        draw::draw_badge(lvcd.nmcd.hdc, &rc, text, bg, fg, dpi);
                    }
                    None => draw::draw_badge(lvcd.nmcd.hdc, &rc, "", draw::C_TOGGLE_GRAY, draw::C_TOGGLE_GRAY, dpi),
                }
            }
            COL_ACTIONS => {
                // 两个小操作按钮：编辑（蓝底铅笔）/ 删除（红底垃圾桶）
                let u = dpi as f64 / 96.0;
                let bw = (26.0 * u) as i32;
                let bh = (24.0 * u) as i32;
                let gap = (6.0 * u) as i32;
                let total = bw * 2 + gap;
                let mut x = (rc.left + rc.right - total) / 2;
                let y = (rc.top + rc.bottom - bh) / 2;
                draw::round_rect(
                    lvcd.nmcd.hdc,
                    &RECT { left: x, top: y, right: x + bw, bottom: y + bh },
                    (6.0 * u) as i32,
                    draw::C_BLUE_TINT,
                    None,
                );
                if let Some(ic) = app.op_dcs.first() {
                    draw::draw_icon(
                        lvcd.nmcd.hdc,
                        ic,
                        draw::OP_SRC_PX,
                        x + (bw - (18.0 * u) as i32) / 2,
                        y + (bh - (18.0 * u) as i32) / 2,
                        (18.0 * u) as i32,
                    );
                }
                x += bw + gap;
                draw::round_rect(
                    lvcd.nmcd.hdc,
                    &RECT { left: x, top: y, right: x + bw, bottom: y + bh },
                    (6.0 * u) as i32,
                    draw::C_RED_BG,
                    None,
                );
                if let Some(ic) = app.op_dcs.get(1) {
                    draw::draw_icon(
                        lvcd.nmcd.hdc,
                        ic,
                        draw::OP_SRC_PX,
                        x + (bw - (18.0 * u) as i32) / 2,
                        y + (bh - (18.0 * u) as i32) / 2,
                        (18.0 * u) as i32,
                    );
                }
            }
            _ => {}
        }
        return LRESULT(CDRF_SKIPDEFAULT as isize);
    }
    LRESULT(CDRF_DODEFAULT as isize)
}

// ---------- 增删改 ----------

pub unsafe fn new_record(app: &mut App) {
    let (s, entry) = app.add_record();
    app.busy = true;
    app.refresh_conflicts();
    populate_records(app);
    app.busy = false;

    // 新行被搜索/筛选隐藏时定位落空（unwrap_or(0) 会误编辑第一行既有记录），先还原为「全部」
    if !app.row_map.contains(&(s, entry)) {
        app.filter.clear();
        app.filter_state = FilterState::All;
        app.category = CatFilter::All;
        let _ = SetWindowTextW(app.hwnd_search, &HSTRING::from(""));
        app.busy = true;
        populate_records(app);
        refresh_conflict_panel(app);
        app.busy = false;
    }

    let row = app
        .row_map
        .iter()
        .position(|&c| c == (s, entry))
        .unwrap_or(0);
    select_row(app.hwnd_records, row);
    // 新行在文件末尾：必须滚入可视区，否则行内编辑框建在视口外，表象即“点击新建没有反应”
    SendMessageW(app.hwnd_records, LVM_ENSUREVISIBLE, WPARAM(row), LPARAM(0));
    refresh_chrome(app);
    start_edit(app, row, COL_DOMAIN);
}

pub unsafe fn delete_selected(app: &mut App) {
    if app.row_map.is_empty() {
        return;
    }
    let mut rows: Vec<usize> = app.row_map.iter().enumerate()
        .filter_map(|(i, cell)| app.selected_cells.contains(cell).then_some(i)).collect();
    let mut i = get_next_selected(app.hwnd_records, -1);
    while i >= 0 {
        if !rows.contains(&(i as usize)) { rows.push(i as usize); }
        i = get_next_selected(app.hwnd_records, i);
    }
    if rows.is_empty() {
        return;
    }
    let msg = format!("确认删除选中的 {} 条记录？", rows.len());
    if MessageBoxW(
        app.hwnd_main,
        &HSTRING::from(msg),
        w!("确认删除"),
        MB_YESNO | MB_ICONQUESTION,
    ) != IDYES
    {
        return;
    }
    let cells: Vec<(usize, usize)> = rows
        .iter()
        .filter_map(|r| app.row_map.get(*r).copied())
        .collect();
    app.delete_rows(cells);
    app.busy = true;
    populate_records(app);
    app.refresh_conflicts();
    refresh_conflict_panel(app);
    app.busy = false;
    refresh_chrome(app);
}

/// 右键「切换启用状态」（选中第一条）
pub unsafe fn toggle_selected(app: &mut App) {
    if let Some(row) = first_selected_row(app) {
        toggle_row(app, row);
    }
}

unsafe fn toggle_row(app: &mut App, row: usize) {
    if let Some(cell) = app.row_map.get(row).copied() {
        if let Some(r) = app.record_mut(cell.0, cell.1) {
            r.enabled = !r.enabled;
        }
        app.mark_dirty();
        app.busy = true;
        update_row(app, row);
        app.refresh_conflicts();
        refresh_conflict_panel(app);
        app.busy = false;
        refresh_chrome(app);
    }
}

/// 右键「置顶生效」（选中第一条）
pub unsafe fn pin_selected(app: &mut App) {
    let Some(row) = first_selected_row(app) else { return };
    let Some(&cell) = app.row_map.get(row) else { return };
    if app.pin_to_top(cell.0, cell.1) {
        app.busy = true;
        populate_records(app);
        app.refresh_conflicts();
        refresh_conflict_panel(app);
        app.busy = false;
        refresh_chrome(app);
    }
}

// ---------- 行内编辑 ----------

pub unsafe fn start_edit(app: &mut App, row: usize, col: usize) {
    if app.edit.is_some() {
        return;
    }
    let Some(cell) = app.row_map.get(row).copied() else {
        return;
    };
    let Some(r) = app.record(cell.0, cell.1) else {
        return;
    };
    let text = match col {
        COL_DOMAIN => r.domain.clone(),
        COL_IP => r.ip.to_string(),
        COL_COMMENT => r.comment.clone().unwrap_or_default(),
        _ => return,
    };
    let mut rc = RECT {
        left: LVIR_BOUNDS as i32,
        top: col as i32,
        ..Default::default()
    };
    SendMessageW(
        app.hwnd_records,
        0x1000 + 56, // LVM_GETSUBITEMRECT
        WPARAM(row),
        LPARAM(&mut rc as *mut _ as isize),
    );
    if rc.right - rc.left < 40 {
        rc.right = rc.left + 120;
    }
    let h = HSTRING::from(text);
    let hinst = GetModuleHandleW(None).unwrap_or_default();
    let edit = match CreateWindowExW(
        WS_EX_CLIENTEDGE,
        w!("EDIT"),
        &h,
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        rc.left,
        rc.top,
        rc.right - rc.left,
        rc.bottom - rc.top,
        app.hwnd_records,
        windows::Win32::UI::WindowsAndMessaging::HMENU::default(),
        hinst,
        None,
    ) {
        Ok(h) => h,
        Err(_) => return,
    };
    SendMessageW(edit, WM_SETFONT, WPARAM(app.font.0 as usize), LPARAM(0));
    let old = SetWindowLongPtrW(
        edit,
        GWLP_WNDPROC,
        edit_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT
            as usize as isize,
    );
    app.edit = Some(EditSession {
        hwnd_edit: edit,
        old_proc: old,
        row,
        scheme: cell.0,
        entry: cell.1,
        col,
    });
    let _ = SetFocus(edit);
    SendMessageW(edit, EM_SETSEL, WPARAM(0), LPARAM(-1));
}

unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let list = GetParent(hwnd).unwrap_or_default();
    let main = GetParent(list).unwrap_or_default();
    let ptr = GetWindowLongPtrW(main, GWLP_USERDATA) as *mut App;
    if ptr.is_null() {
        return LRESULT(0);
    }
    let app = &mut *ptr;
    let old = app.edit.as_ref().map_or(0, |e| e.old_proc);
    match msg {
        WM_KEYDOWN => match VIRTUAL_KEY(wp.0 as u16) {
            VK_RETURN => {
                commands::commit_edit(app);
                LRESULT(0)
            }
            VK_ESCAPE => {
                commands::cancel_edit(app);
                LRESULT(0)
            }
            _ => call_old(old, hwnd, msg, wp, lp),
        },
        WM_CHAR => {
            let c = wp.0 as u16;
            if c == 13 || c == 27 {
                LRESULT(0)
            } else {
                call_old(old, hwnd, msg, wp, lp)
            }
        }
        WM_KILLFOCUS => {
            if app.edit.is_some() && !app.busy {
                commands::commit_edit(app);
            }
            LRESULT(0)
        }
        _ => call_old(old, hwnd, msg, wp, lp),
    }
}

fn call_old(old: isize, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if old == 0 {
        return LRESULT(0);
    }
    let f: WNDPROC = unsafe { std::mem::transmute(old) };
    unsafe { CallWindowProcW(f, hwnd, msg, wp, lp) }
}

pub(crate) unsafe fn read_edit_text(hwnd: HWND) -> String {
    let len = SendMessageW(hwnd, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0)).0 as usize;
    if len == 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len + 1];
    SendMessageW(hwnd, WM_GETTEXT, WPARAM(len + 1), LPARAM(buf.as_mut_ptr() as isize));
    String::from_utf16_lossy(&buf[..len])
}

/// 校验并提交行内编辑；无效时提示并保持编辑态（commands.rs 调用）
pub(crate) unsafe fn apply_commit(app: &mut App) {
    let (row, scheme, entry, col, text) = {
        let Some(sess) = app.edit.as_ref() else { return };
        (
            sess.row,
            sess.scheme,
            sess.entry,
            sess.col,
            read_edit_text(sess.hwnd_edit),
        )
    };

    let invalid = match col {
        COL_DOMAIN => {
            let d = text.trim().to_string();
            if d.is_empty() {
                Some(w!("域名不能为空"))
            } else {
                if let Some(r) = app.record_mut(scheme, entry) {
                    r.domain = d;
                }
                None
            }
        }
        COL_IP => match text.trim().parse::<IpAddr>() {
            Ok(ip) => {
                if let Some(r) = app.record_mut(scheme, entry) {
                    r.ip = ip;
                }
                None
            }
            Err(_) => Some(w!("IP 不合法（支持 IPv4 / IPv6）")),
        },
        COL_COMMENT => {
            let c = text.trim_end().to_string();
            if let Some(r) = app.record_mut(scheme, entry) {
                r.comment = if c.is_empty() { None } else { Some(c) };
            }
            None
        }
        _ => None,
    };
    if let Some(msg) = invalid {
        app.busy = true;
        MessageBoxW(app.hwnd_main, msg, w!("无法修改"), MB_OK | MB_ICONERROR);
        app.busy = false;
        let focus = app.edit.as_ref().map_or(app.hwnd_records, |s| s.hwnd_edit);
        let _ = SetFocus(focus);
        return;
    }

    let sess = app.edit.take().expect("session 存在");
    let _ = DestroyWindow(sess.hwnd_edit);
    app.mark_dirty();
    app.busy = true;
    update_row(app, row);
    app.refresh_conflicts();
    refresh_conflict_panel(app);
    app.busy = false;
    refresh_chrome(app);
    let _ = SetFocus(app.hwnd_records);
}

pub(crate) unsafe fn cancel_edit_impl(app: &mut App) {
    if let Some(sess) = app.edit.take() {
        let _ = DestroyWindow(sess.hwnd_edit);
        let _ = SetFocus(app.hwnd_records);
    }
}

/// 刷新一行文本与冲突标注（提交编辑/翻转后）
unsafe fn update_row(app: &mut App, row: usize) {
    let Some(&cell) = app.row_map.get(row) else { return };
    let Some(r) = app.record(cell.0, cell.1) else { return };
    let conflict = app
        .roles
        .get(&cell)
        .map(|role| HSTRING::from(role_text(*role)))
        .unwrap_or_default();
    let texts = [
        (COL_DOMAIN, HSTRING::from(&r.domain)),
        (COL_IP, HSTRING::from(r.ip.to_string())),
        (
            COL_COMMENT,
            HSTRING::from(r.comment.clone().unwrap_or_default()),
        ),
        (COL_CONFLICT, conflict),
    ];
    for (sub, h) in texts {
        let mut si = LVITEMW {
            mask: LVIF_TEXT,
            iItem: row as i32,
            iSubItem: sub as i32,
            pszText: std::mem::transmute(h.as_ptr()),
            ..Default::default()
        };
        SendMessageW(
            app.hwnd_records,
            LVM_SETITEMTEXTW,
            WPARAM(row),
            LPARAM(&mut si as *mut _ as isize),
        );
    }
    SendMessageW(
        app.hwnd_records,
        LVM_REDRAWITEMS,
        WPARAM(row),
        LPARAM(row as isize),
    );
}
