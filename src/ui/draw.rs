//! 自绘原语：药丸按钮 / 徽章 / 开关 / 图标 Alpha 混合 / 圆角矩形（设计稿还原用）

use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    AlphaBlend, CreateCompatibleDC, CreateDIBSection, CreatePen, CreateSolidBrush,
    DeleteObject, DrawTextW, Ellipse, FillRect, GetDC, GetStockObject, ReleaseDC, RoundRect, SelectObject,
    SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    DIB_RGB_COLORS, HBITMAP, HDC, AC_SRC_ALPHA, AC_SRC_OVER, DT_CENTER, DT_END_ELLIPSIS,
    DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, NULL_PEN, PS_SOLID,
    TRANSPARENT,
};
use windows::Win32::UI::Controls::{ODS_DISABLED, ODS_SELECTED};

// —— 设计稿色板（COLORREF = 0x00BBGGRR）——
pub const C_PRIMARY: COLORREF = COLORREF(0x00F5_7317); // #1773F5
pub const C_PRIMARY_DARK: COLORREF = COLORREF(0x00E9_5D0F); // #0F5DE9
pub const C_BLUE_TINT: COLORREF = COLORREF(0x00FF_F6EF); // #EFF6FF
pub const C_BORDER: COLORREF = COLORREF(0x00E1_D5CB); // #CBD5E1
pub const C_BORDER_LIGHT: COLORREF = COLORREF(0x00F0_E8E2); // #E2E8F0
pub const C_TEXT: COLORREF = COLORREF(0x003B_291E); // #1E293B
pub const C_SUBTEXT: COLORREF = COLORREF(0x008B_7464); // #64748B
pub const C_DISABLED: COLORREF = COLORREF(0x00B8_A394); // #94A3B8
pub const C_GREEN: COLORREF = COLORREF(0x0052_A82F); // #2FA852
pub const C_GREEN_BG: COLORREF = COLORREF(0x00E7_FCDC); // #DCFCE7
pub const C_GREEN_TEXT: COLORREF = COLORREF(0x0003_8015); // #15803D
pub const C_RED_BG: COLORREF = COLORREF(0x00E2_E2FE); // #FEE2E2
pub const C_RED_TEXT: COLORREF = COLORREF(0x001C_1CB9); // #B91C1C
pub const C_YELLOW_BG: COLORREF = COLORREF(0x00C3_F9FE); // #FEF9C3
pub const C_YELLOW_TEXT: COLORREF = COLORREF(0x0007_62A1); // #A16207
pub const C_TOGGLE_GRAY: COLORREF = COLORREF(0x00E1_D5CB); // #CBD5E1
pub const C_WHITE: COLORREF = COLORREF(0x00FF_FFFF);
pub const C_BAND_BG: COLORREF = COLORREF(0x00FC_FAF8); // #F8FAFC

/// 由 BGRA（自上而下）像素条带构建常驻内存 DC（每图标一个，生命周期 = 进程）
pub unsafe fn build_icon_dcs(strip: &[u8], count: usize, px: i32) -> Vec<(HDC, HBITMAP)> {
    let screen = GetDC(None);
    let mut out = Vec::new();
    let stride = (px * px * 4) as usize;
    for i in 0..count {
        let Some(bgra) = strip.get(i * stride..(i + 1) * stride) else {
            break;
        };
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = px;
        bmi.bmiHeader.biHeight = -px; // 自上而下
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        if let Ok(hbmp) = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            // AlphaBlend expects premultiplied BGRA. The embedded strips contain straight alpha.
            let pixels = std::slice::from_raw_parts_mut(bits as *mut u8, bgra.len());
            pixels.copy_from_slice(bgra);
            for p in pixels.chunks_exact_mut(4) {
                let a = p[3] as u16;
                p[0] = ((p[0] as u16 * a + 127) / 255) as u8;
                p[1] = ((p[1] as u16 * a + 127) / 255) as u8;
                p[2] = ((p[2] as u16 * a + 127) / 255) as u8;
            }
            let mem = CreateCompatibleDC(screen);
            SelectObject(mem, hbmp);
            out.push((mem, hbmp));
        }
    }
    let _ = ReleaseDC(None, screen);
    out
}

/// 把内存 DC 中的图标以指定尺寸 Alpha 混合到目标
pub unsafe fn draw_icon(hdc: HDC, icon: &(HDC, HBITMAP), px: i32, x: i32, y: i32, size: i32) {
    let bf = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = AlphaBlend(hdc, x, y, size, size, icon.0, 0, 0, px, px, bf);
}

pub unsafe fn round_rect(hdc: HDC, rc: &RECT, radius: i32, fill: COLORREF, border: Option<COLORREF>) {
    let brush = CreateSolidBrush(fill);
    let pen = CreatePen(PS_SOLID, 1, border.unwrap_or(fill));
    let old_b = SelectObject(hdc, brush);
    let old_p = SelectObject(hdc, pen);
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius * 2, radius * 2);
    SelectObject(hdc, old_b);
    SelectObject(hdc, old_p);
    let _ = DeleteObject(brush);
    let _ = DeleteObject(pen);
}

unsafe fn text_w(hdc: HDC, text: &str) -> i32 {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut sz = windows::Win32::Foundation::SIZE::default();
    let _ = windows::Win32::Graphics::Gdi::GetTextExtentPoint32W(hdc, &mut wide, &mut sz);
    sz.cx
}

unsafe fn text_out_center(hdc: HDC, rc: &RECT, text: &str, color: COLORREF) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, color);
    let mut r = *rc;
    let _ = DrawTextW(hdc, &mut wide, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
}

/// 药丸按钮绘制规格
pub struct PillSpec<'a> {
    pub text: &'a str,
    /// btn 图标条带中的序号
    pub icon: Option<usize>,
    pub primary: bool,
    pub selected: bool,
    pub pressed: bool,
    pub disabled: bool,
    /// 文本前的彩色圆点（筛选 chips）
    pub dot: Option<COLORREF>,
    /// 圆角半径
    pub radius: i32,
}

/// 按设计稿绘制药丸按钮（WM_DRAWITEM）
pub unsafe fn draw_pill(
    hdc: HDC,
    rc: &RECT,
    spec: PillSpec,
    btn_dcs: &[(HDC, HBITMAP)],
    dpi: i32,
) {
    let (fill, border, textc) = if spec.primary {
        if spec.pressed {
            (C_PRIMARY_DARK, C_PRIMARY_DARK, C_WHITE)
        } else {
            (C_PRIMARY, C_PRIMARY, C_WHITE)
        }
    } else if spec.selected {
        if spec.pressed {
            (C_PRIMARY_DARK, C_PRIMARY_DARK, C_WHITE)
        } else {
            (C_PRIMARY, C_PRIMARY, C_WHITE)
        }
    } else if spec.pressed {
        (C_BLUE_TINT, C_PRIMARY, C_TEXT)
    } else if spec.disabled {
        (C_WHITE, C_BORDER_LIGHT, C_DISABLED)
    } else {
        (C_WHITE, C_BORDER, C_TEXT)
    };
    round_rect(hdc, rc, spec.radius, fill, Some(border));
    if spec.disabled {
        return; // 禁用态只画底
    }

    let u = dpi as f64 / 96.0;
    let icon_size = (21.0 * u) as i32;
    let gap = (8.0 * u) as i32;
    let tw = text_w(hdc, spec.text);
    let mut total = tw;
    if spec.icon.is_some() {
        total += icon_size + gap;
    }
    if spec.dot.is_some() {
        total += (12.0 * u) as i32 + gap;
    }
    let cy = (rc.top + rc.bottom) / 2;
    let mut x = (rc.left + rc.right - total) / 2;

    if let Some(dot) = spec.dot {
        let r = (5.0 * u) as i32;
        let brush = CreateSolidBrush(dot);
        let old = SelectObject(hdc, brush);
        let _ = Ellipse(hdc, x, cy - r, x + 2 * r, cy + r);
        SelectObject(hdc, old);
        let _ = DeleteObject(brush);
        x += (12.0 * u) as i32 + gap;
    }
    if let Some(idx) = spec.icon {
        if spec.primary && idx == 0 {
            let pen = CreatePen(PS_SOLID, (2.0 * u) as i32, C_WHITE);
            let old = SelectObject(hdc, pen);
            let iy = cy;
            let ix = x + icon_size / 2;
            let arm = icon_size * 2 / 5;
            let _ = windows::Win32::Graphics::Gdi::MoveToEx(hdc, ix - arm, iy, None);
            let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, ix + arm, iy);
            let _ = windows::Win32::Graphics::Gdi::MoveToEx(hdc, ix, iy - arm, None);
            let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, ix, iy + arm);
            SelectObject(hdc, old);
            let _ = DeleteObject(pen);
        } else if let Some(ic) = btn_dcs.get(idx) {
            draw_icon(hdc, ic, BTN_SRC_PX, x, cy - icon_size / 2, icon_size);
        }
        x += icon_size + gap;
    }
    let tr = RECT {
        left: x,
        top: rc.top,
        right: x + tw,
        bottom: rc.bottom,
    };
    text_out_center(hdc, &tr, spec.text, textc);
}

pub const BTN_SRC_PX: i32 = 24;
pub const OP_SRC_PX: i32 = 20;

/// 状态徽章（圆角小底 + 居中文本），无内容时画灰短横
pub unsafe fn draw_badge(hdc: HDC, rc: &RECT, text: &str, bg: COLORREF, fg: COLORREF, dpi: i32) {
    if text.is_empty() {
        let cy = (rc.top + rc.bottom) / 2;
        let w = (22.0 * dpi as f64 / 96.0) as i32;
        let r = RECT {
            left: (rc.left + rc.right - w) / 2,
            top: cy - w / 2,
            right: (rc.left + rc.right + w) / 2,
            bottom: cy + w / 2,
        };
        round_rect(hdc, &r, w / 2, COLORREF(0x00F1_F3F5), None);
        text_out_center(hdc, &r, "−", C_SUBTEXT);
        return;
    }
    let tw = text_w(hdc, text);
    let padx = (10.0 * dpi as f64 / 96.0) as i32;
    let h = rc.bottom - rc.top - (8.0 * dpi as f64 / 96.0) as i32;
    let w = tw + 2 * padx;
    let x = (rc.left + rc.right - w) / 2;
    let y = (rc.top + rc.bottom - h) / 2;
    let pill = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    round_rect(hdc, &pill, h / 2, bg, None);
    let tr = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    text_out_center(hdc, &tr, text, fg);
}

/// 开关滑块（启用列）
pub unsafe fn draw_toggle(hdc: HDC, rc: &RECT, on: bool, dpi: i32) {
    let h = (20.0 * dpi as f64 / 96.0) as i32;
    let w = (40.0 * dpi as f64 / 96.0) as i32;
    let x = (rc.left + rc.right - w) / 2;
    let y = (rc.top + rc.bottom - h) / 2;
    let pill = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    round_rect(hdc, &pill, h / 2, if on { C_GREEN } else { C_TOGGLE_GRAY }, None);
    let knob = h - (4.0 * dpi as f64 / 96.0) as i32;
    let kx = if on {
        x + w - knob - (2.0 * dpi as f64 / 96.0) as i32
    } else {
        x + (2.0 * dpi as f64 / 96.0) as i32
    };
    let kr = RECT {
        left: kx,
        top: y + (2.0 * dpi as f64 / 96.0) as i32,
        right: kx + knob,
        bottom: y + (2.0 * dpi as f64 / 96.0) as i32 + knob,
    };
    let brush = CreateSolidBrush(C_WHITE);
    let pen = GetStockObject(NULL_PEN);
    let old_b = SelectObject(hdc, brush);
    let old_p = SelectObject(hdc, pen);
    let _ = Ellipse(hdc, kr.left, kr.top, kr.right, kr.bottom);
    SelectObject(hdc, old_b);
    SelectObject(hdc, old_p);
    let _ = DeleteObject(brush);
}

pub unsafe fn draw_checkbox(hdc: HDC, rc: &RECT, checked: bool, dpi: i32) {
    let s = (17 * dpi / 96).max(14);
    let x = (rc.left + rc.right - s) / 2;
    let y = (rc.top + rc.bottom - s) / 2;
    let box_rc = RECT { left: x, top: y, right: x + s, bottom: y + s };
    round_rect(hdc, &box_rc, 3 * dpi / 96, if checked { C_PRIMARY } else { C_WHITE },
        Some(if checked { C_PRIMARY } else { C_BORDER }));
    if checked {
        let pen = CreatePen(PS_SOLID, (2 * dpi / 96).max(1), C_WHITE);
        let old = SelectObject(hdc, pen);
        let _ = windows::Win32::Graphics::Gdi::MoveToEx(hdc, x + s / 4, y + s / 2, None);
        let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, x + s * 2 / 5, y + s * 3 / 4);
        let _ = windows::Win32::Graphics::Gdi::LineTo(hdc, x + s * 4 / 5, y + s / 4);
        SelectObject(hdc, old);
        let _ = DeleteObject(pen);
    }
}

/// 分类 chip 绘制：index 0 = 全部（无图标）；base_w 为 96 DPI 设计宽度，窄窗口时退化为纯文字
pub unsafe fn draw_category_chip(hdc: HDC, rc: &RECT, label: &str, count: usize,
    index: usize, selected: bool, dpi: i32, icons: &[(HDC, HBITMAP)], base_w: i32) {
    let sc = |v: i32| v * dpi / 96;
    round_rect(hdc, rc, sc(9), if selected { C_PRIMARY } else { C_WHITE },
        Some(if selected { C_PRIMARY } else { C_BORDER_LIGHT }));
    // 窄窗口优先保留可读的分类名称，避免图标、数量和文字互相盖住。
    if rc.right - rc.left < sc(base_w - 5) {
        text_out_center(hdc, rc, label, if selected { C_WHITE } else { C_TEXT });
        return;
    }
    if index > 0 {
        if let Some(icon) = icons.get(index - 1) {
            let s = sc(23);
            draw_icon(hdc, icon, 32, rc.left + sc(10),
                (rc.top + rc.bottom - s) / 2, s);
        }
    }
    let count_w = if count >= 100 { 38 } else { 30 };
    let badge = RECT { left: rc.right - count_w - sc(8), top: rc.top + sc(9),
        right: rc.right - sc(8), bottom: rc.bottom - sc(9) };
    round_rect(hdc, &badge, sc(5), if selected { C_BLUE_TINT } else { C_BAND_BG }, None);
    text_out_center(hdc, &badge, &count.to_string(), if selected { C_PRIMARY } else { C_SUBTEXT });
    let label_rc = RECT { left: rc.left + if index == 0 { sc(13) } else { sc(42) }, top: rc.top,
        right: badge.left - sc(1), bottom: rc.bottom };
    text_out_center(hdc, &label_rc, label, if selected { C_WHITE } else { C_TEXT });
}

pub unsafe fn draw_cell_text(hdc: HDC, rc: &RECT, content: &str, foreground: COLORREF,
    background: COLORREF, dpi: i32) {
    let brush = CreateSolidBrush(background);
    let fill = RECT { left: rc.left, top: rc.top, right: rc.right - 1,
        bottom: rc.bottom - 1 };
    let _ = FillRect(hdc, &fill, brush);
    let _ = DeleteObject(brush);
    if content.is_empty() { return; }
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, foreground);
    let mut text: Vec<u16> = content.encode_utf16().collect();
    let mut area = RECT { left: rc.left + 14 * dpi / 96, top: rc.top,
        right: rc.right - 4 * dpi / 96, bottom: rc.bottom };
    let _ = DrawTextW(hdc, &mut text, &mut area,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
}

/// 从 DRAWITEMSTRUCT 取按下/禁用状态
pub fn states(item_state: u32) -> (bool, bool) {
    let pressed = item_state & ODS_SELECTED.0 != 0;
    let disabled = item_state & ODS_DISABLED.0 != 0;
    (pressed, disabled)
}

/// 文本像素宽度（窗口状态条排版用）
pub unsafe fn text_width(hdc: HDC, text: &str) -> i32 {
    text_w(hdc, text)
}
