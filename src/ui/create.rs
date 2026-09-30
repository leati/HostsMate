//! 控件创建：自绘按钮组（工具栏/筛选 chips/排序）、无框搜索框、记录表（行高/列）
//! 按钮外观全部由 draw.rs 在 WM_DRAWITEM 中按设计稿绘制

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateFontIndirectW, GetStockObject, HFONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    ImageList_Create, LVM_INSERTCOLUMNW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETIMAGELIST,
    WC_LISTVIEWW, ILC_COLOR,
    LVCOLUMNW, LVCF_FMT, LVCF_SUBITEM, LVCF_TEXT, LVCF_WIDTH, LVCFMT_LEFT, LVS_EX_DOUBLEBUFFER,
    LVS_EX_FULLROWSELECT, LVS_EX_GRIDLINES, LVS_REPORT, LVS_SHOWSELALWAYS, LVSIL_SMALL,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SendMessageW, SetWindowTextW, SystemParametersInfoW, HMENU,
    NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, WINDOW_STYLE, WM_SETFONT,
    BS_OWNERDRAW, ES_AUTOHSCROLL, WS_CHILD, WS_TABSTOP, WS_VISIBLE,
};

use super::app::{
    App, CAT_ALL, CAT_BASE, CAT_FALLBACK, COL_COMMENT, COL_CONFLICT, COL_DOMAIN, COL_ENABLED,
    COL_IP, COL_ACTIONS, COL_SELECT, IDC_RECORDS, IDM_ABOUT, IDM_DELETE, IDM_EXPORT, IDM_IMPORT,
    IDM_NEW, IDM_SAVE, IDM_SETTINGS, IDM_SORT, IDM_STATUS,
};
use super::draw::build_icon_dcs;

pub const BTN_STRIP: &[u8] = include_bytes!("../../btn_icons.bin");
pub const OP_STRIP: &[u8] = include_bytes!("../../op_icons.bin");
pub const CAT_STRIP: &[u8] = include_bytes!("../../cat_icons.bin");
pub const BTN_ICON_PX: i32 = 24;
pub const OP_ICON_PX: i32 = 20;
pub const BTN_COUNT: usize = 7;
pub const OP_COUNT: usize = 2;

/// 工具栏按钮定义（id, 文本, 图标序号, 主按钮）
pub const TB_DEFS: [(i32, &str, usize, bool); 7] = [
    (IDM_NEW, "新建", 0, true),
    (IDM_DELETE, "删除", 1, false),
    (IDM_SAVE, "保存", 2, false),
    (IDM_IMPORT, "导入", 3, false),
    (IDM_EXPORT, "导出", 4, false),
    (IDM_SETTINGS, "设置", 5, false),
    (IDM_ABOUT, "关于", 6, false),
];

/// 分类筛选（仅改变视图，分类根据域名及备注推断，不写入 hosts 文件）：
/// chips 随设置里的自定义分类动态生成（全部 + 各分类 + 兜底「自定义」）
pub unsafe fn build_chips(parent: HWND, app: &mut App) -> windows::core::Result<()> {
    let hinst = HINSTANCE(GetModuleHandleW(None)?.0);
    let mut defs: Vec<(i32, String)> = vec![(CAT_ALL, "全部".to_string())];
    for (i, cat) in app.settings.rules.cats.iter().enumerate() {
        defs.push((CAT_BASE + i as i32, cat.name.clone()));
    }
    defs.push((CAT_FALLBACK, "自定义".to_string()));
    for (id, text) in defs {
        let h = CreateWindowExW(
            Default::default(),
            w!("BUTTON"),
            &HSTRING::from(text),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
            0,
            0,
            0,
            0,
            parent,
            HMENU(id as usize as *mut core::ffi::c_void),
            hinst,
            None,
        )?;
        SendMessageW(h, WM_SETFONT, WPARAM(app.font.0 as usize), LPARAM(0));
        app.hwnd_chips.push((h, id));
    }
    Ok(())
}

/// 分类增删后重建 chips（设置窗保存时调用），随后重排布局
pub unsafe fn rebuild_chips(app: &mut App) {
    for (h, _) in &app.hwnd_chips {
        let _ = DestroyWindow(*h);
    }
    app.hwnd_chips.clear();
    if build_chips(app.hwnd_main, app).is_err() {
        return;
    }
    super::wnd::layout(app);
}

/// 创建全部子控件与字体/图标资源
pub unsafe fn create_children(parent: HWND, app: &mut App) -> windows::core::Result<()> {
    let hinst = HINSTANCE(GetModuleHandleW(None)?.0);
    let dpi = GetDpiForWindow(parent).clamp(96, 120) as i32;

    // 字体：系统消息字体 + 头部标题（加粗大字）+ 副标题（小灰字）
    let mut ncm = NONCLIENTMETRICSW::default();
    ncm.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
    app.font = if SystemParametersInfoW(
        SPI_GETNONCLIENTMETRICS,
        std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
        Some(&mut ncm as *mut _ as _),
        windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    )
    .is_ok()
    {
        CreateFontIndirectW(&ncm.lfMessageFont)
    } else {
        HFONT(GetStockObject(windows::Win32::Graphics::Gdi::DEFAULT_GUI_FONT).0)
    };
    app.header_font = windows::Win32::Graphics::Gdi::CreateFontW(
        -19 * dpi / 96,
        0,
        0,
        0,
        700,
        0,
        0,
        0,
        windows::Win32::Graphics::Gdi::DEFAULT_CHARSET.0 as u32,
        windows::Win32::Graphics::Gdi::OUT_DEFAULT_PRECIS.0 as u32,
        windows::Win32::Graphics::Gdi::CLIP_DEFAULT_PRECIS.0 as u32,
        windows::Win32::Graphics::Gdi::CLEARTYPE_QUALITY.0 as u32,
        0,
        w!("Segoe UI"),
    );
    app.sub_font = windows::Win32::Graphics::Gdi::CreateFontW(
        -12 * dpi / 96,
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        windows::Win32::Graphics::Gdi::DEFAULT_CHARSET.0 as u32,
        windows::Win32::Graphics::Gdi::OUT_DEFAULT_PRECIS.0 as u32,
        windows::Win32::Graphics::Gdi::CLIP_DEFAULT_PRECIS.0 as u32,
        windows::Win32::Graphics::Gdi::CLEARTYPE_QUALITY.0 as u32,
        0,
        w!("Segoe UI"),
    );

    // 图标资源（内存 DC 常驻）
    app.btn_dcs = build_icon_dcs(BTN_STRIP, BTN_COUNT, BTN_ICON_PX);
    app.op_dcs = build_icon_dcs(OP_STRIP, OP_COUNT, OP_ICON_PX);
    app.cat_dcs = build_icon_dcs(CAT_STRIP, 5, 32);

    // 工具栏按钮（自绘药丸）
    for (id, text, _, _) in TB_DEFS {
        let h = CreateWindowExW(
            Default::default(),
            w!("BUTTON"),
            &HSTRING::from(text),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
            0,
            0,
            0,
            0,
            parent,
            HMENU(id as usize as *mut core::ffi::c_void),
            hinst,
            None,
        )?;
        SendMessageW(h, WM_SETFONT, WPARAM(app.font.0 as usize), LPARAM(0));
        app.hwnd_tbtns.push((h, id));
    }

    // 分类筛选（全部 + 自定义分类 + 兜底「自定义」）
    build_chips(parent, app)?;

    // 排序下拉按钮
    let sort = CreateWindowExW(
        Default::default(),
        w!("BUTTON"),
        w!("排序"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
        0,
        0,
        0,
        0,
        parent,
        HMENU(IDM_SORT as usize as *mut core::ffi::c_void),
        hinst,
        None,
    )?;
    SendMessageW(sort, WM_SETFONT, WPARAM(app.font.0 as usize), LPARAM(0));
    app.hwnd_sort = sort;

    let status = CreateWindowExW(
        Default::default(), w!("BUTTON"), w!("全部状态"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
        0, 0, 0, 0, parent,
        HMENU(IDM_STATUS as usize as *mut core::ffi::c_void), hinst, None,
    )?;
    SendMessageW(status, WM_SETFONT, WPARAM(app.font.0 as usize), LPARAM(0));
    app.hwnd_status = status;

    // 搜索框（无边框，父窗口画胶囊 + 放大镜 + Ctrl+F 提示）
    let search = CreateWindowExW(
        Default::default(),
        w!("EDIT"),
        PCWSTR::null(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        0,
        0,
        0,
        0,
        parent,
        HMENU(super::app::IDC_SEARCH as usize as *mut core::ffi::c_void),
        hinst,
        None,
    )?;
    SendMessageW(search, WM_SETFONT, WPARAM(app.font.0 as usize), LPARAM(0));
    {
        let cue = HSTRING::from("搜索域名 / IP / 备注…");
        SendMessageW(search, 0x1501 /* EM_SETCUEBANNER */, WPARAM(1), LPARAM(cue.as_ptr() as isize));
    }
    app.hwnd_search = search;

    // 记录表
    let records = create_record_list(parent, hinst, dpi)?;
    app.hwnd_records = records;
    super::table::subclass_header(app);
    Ok(())
}

unsafe fn create_record_list(parent: HWND, hinst: HINSTANCE, dpi: i32) -> windows::core::Result<HWND> {
    let lv = CreateWindowExW(
        Default::default(),
        WC_LISTVIEWW,
        PCWSTR::null(),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(LVS_REPORT | LVS_SHOWSELALWAYS),
        0,
        0,
        600,
        100,
        parent,
        HMENU(IDC_RECORDS as usize as *mut core::ffi::c_void),
        hinst,
        None,
    )?;
    let ex = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_GRIDLINES;
    SendMessageW(lv, LVM_SETEXTENDEDLISTVIEWSTYLE, WPARAM(0), LPARAM(ex as isize));

    // 行高：挂一个空 ImageList，高度即行高（设计稿为宽松行距）
    let himl = ImageList_Create(1, 27 * dpi / 96, ILC_COLOR, 1, 1);
    SendMessageW(lv, LVM_SETIMAGELIST, WPARAM(LVSIL_SMALL as usize), LPARAM(himl.0 as isize));
    // Header image list controls header height without changing the table font.
    let header = HWND(SendMessageW(lv, 0x1000 + 31, WPARAM(0), LPARAM(0)).0 as *mut _); // LVM_GETHEADER
    let head_iml = ImageList_Create(1, 31 * dpi / 96, ILC_COLOR, 1, 1);
    SendMessageW(header, 0x1200 + 8, WPARAM(0), LPARAM(head_iml.0 as isize)); // HDM_SETIMAGELIST

    insert_column(lv, COL_SELECT, w!(""), 48);
    insert_column(lv, COL_DOMAIN, w!("域名"), 300);
    insert_column(lv, COL_IP, w!("IP 地址"), 180);
    insert_column(lv, COL_COMMENT, w!("备注"), 300);
    insert_column(lv, COL_ENABLED, w!("启用"), 70);
    insert_column(lv, COL_CONFLICT, w!("冲突"), 90);
    insert_column(lv, COL_ACTIONS, w!("操作"), 100);
    Ok(lv)
}

unsafe fn insert_column(lv: HWND, index: usize, text: PCWSTR, width: i32) {
    let mut col = LVCOLUMNW {
        mask: LVCF_FMT | LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
        fmt: LVCFMT_LEFT,
        cx: width,
        pszText: std::mem::transmute(text.as_ptr()),
        iSubItem: index as i32,
        ..Default::default()
    };
    SendMessageW(
        lv,
        LVM_INSERTCOLUMNW,
        WPARAM(index),
        LPARAM(&mut col as *mut _ as isize),
    );
}

/// 窗口标题（脏前缀）
pub unsafe fn set_title(app: &App) {
    let prefix = if app.dirty { "* " } else { "" };
    let t = HSTRING::from(format!("{}Hosts 文件编辑器", prefix));
    let _ = SetWindowTextW(app.hwnd_main, &t);
}
