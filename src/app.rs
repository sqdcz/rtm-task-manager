//! Windowing, drawing and interaction.
//!
//! Layout has two modes:
//!  * Compact  - classic tab strip on top, dense toolbar, maximum rows.
//!  * Fluent   - Win11-style left navigation rail, taller command bar.
//!
//! Lists are owner-data SysListView32 controls: only visible rows are ever
//! materialised as text, so tens of thousands of rows stay cheap.

#![allow(static_mut_refs)]

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::actions;
use crate::collect::{self, Collector};
use crate::ffi::native::*;
use crate::ffi::*;
use crate::model::*;
use crate::table::{self, HistoryTracker, Table};

static mut APP: *mut App = ptr::null_mut();
static mut PROMPT_RESULT: Option<String> = None;
static mut PROMPT_DONE: bool = false;
static mut PROMPT_CLASS: bool = false;
static mut CHART_CLASS: bool = false;
static mut MAIN_CLASS: bool = false;
static mut OLD_HEADER_PROC: isize = 0;
static mut OLD_HEADER_PROC2: isize = 0;

unsafe fn app() -> &'static mut App {
    &mut *APP
}

#[derive(Clone, Copy)]
struct Btn {
    rc: RECT,
    id: u32,
    label: &'static str,
}

pub struct App {
    pub cfg: Config,
    pub snap: Snapshot,
    pub trends: Trends,
    pub table: Table,
    pub collapsed: std::collections::HashSet<u32>,
    pub page: Page,
    pub perf_sel: usize,
    pub ctx_row: usize,
    pub search_placeholder: bool,
    pub hover: i32,
    pub hwnd: HWND,
    pub h_list: HWND,
    pub h_header: HWND,
    pub h_perf_list: HWND,
    pub h_perf_header: HWND,
    pub h_chart: HWND,
    pub h_search: HWND,
    pub font: HFONT,
    pub font_bold: HFONT,
    pub bg: HBRUSH,
    pub panel: HBRUSH,
    pub accent: HBRUSH,
    pub tab_sel: HBRUSH,
    pub border: HBRUSH,
    pub h_icon: HICON,
    pub tabs: Vec<RECT>,
    pub buttons: Vec<Btn>,
    pub header_cols: Vec<(RECT, usize)>,
    pub himl: HANDLE,
    pub icon_cache: std::collections::HashMap<String, i32>,
    pub tray: bool,
    pub cols_dirty: bool,
    pub cfg_shared: Arc<Mutex<Config>>,
    pub interval: Arc<AtomicU32>,
    pub paused: Arc<AtomicBool>,
    pub pending: Arc<Mutex<Option<Snapshot>>>,
}

struct Palette {
    bg: u32,
    panel: u32,
    card: u32,
    text: u32,
    sub: u32,
    accent: u32,
    border: u32,
    sel: u32,
    grid: u32,
    chart: u32,
    chart_fill: u32,
    warn: u32,
}

fn palette(theme: Theme) -> Palette {
    if theme == Theme::Dark {
        Palette {
            bg: rgb(26, 26, 30),
            panel: rgb(33, 33, 38),
            card: rgb(40, 40, 47),
            text: rgb(240, 241, 243),
            sub: rgb(158, 163, 174),
            accent: rgb(96, 205, 255),
            border: rgb(60, 60, 69),
            sel: rgb(18, 92, 152),
            grid: rgb(48, 48, 56),
            chart: rgb(96, 205, 255),
            chart_fill: rgb(38, 70, 90),
            warn: rgb(240, 130, 100),
        }
    } else {
        Palette {
            bg: rgb(255, 255, 255),
            panel: rgb(246, 248, 251),
            card: rgb(246, 248, 251),
            text: rgb(26, 28, 32),
            sub: rgb(103, 110, 122),
            accent: rgb(0, 103, 192),
            border: rgb(226, 230, 236),
            sel: rgb(204, 229, 255),
            grid: rgb(236, 239, 244),
            chart: rgb(0, 120, 212),
            chart_fill: rgb(214, 234, 253),
            warn: rgb(214, 76, 52),
        }
    }
}

/// Resolves `Theme::Auto` from the Windows apps light/dark preference.
fn theme_now(cfg: &Config) -> Theme {
    match cfg.theme {
        Theme::Light => Theme::Light,
        Theme::Dark => Theme::Dark,
        Theme::Auto => system_theme(),
    }
}

fn system_theme() -> Theme {
    unsafe {
        let sub = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let mut h: HKEY = ptr::null_mut();
        if RegOpenKeyExW(k::HKEY_CURRENT_USER as HKEY, sub.as_ptr(), 0, k::KEY_READ, &mut h) != 0 {
            return Theme::Light;
        }
        let name = wide("AppsUseLightTheme");
        let mut ty = 0u32;
        let mut val: u32 = 1;
        let mut size = 4u32;
        let ok = RegQueryValueExW(
            h, name.as_ptr(), ptr::null_mut(), &mut ty,
            &mut val as *mut u32 as *mut u8, &mut size,
        );
        RegCloseKey(h);
        if ok == 0 && val == 0 { Theme::Dark } else { Theme::Light }
    }
}

fn pal(cfg: &Config) -> Palette {
    palette(theme_now(cfg))
}

fn font(size: i32, bold: i32) -> HFONT {
    let face = wide("Microsoft YaHei UI");
    let h = unsafe { CreateFontW(-size, 0, 0, 0, bold, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr()) };
    if h.is_null() {
        let f2 = wide("Segoe UI");
        unsafe { CreateFontW(-size, 0, 0, 0, bold, 0, 0, 0, 1, 0, 0, 5, 0, f2.as_ptr()) }
    } else {
        h
    }
}

unsafe fn enable_dpi() {
    type DpiFn = unsafe extern "system" fn(*mut c_void) -> BOOL;
    if let Some(f) = call_fn::<DpiFn>(dyn_proc("user32.dll", "SetProcessDpiAwarenessContext")) {
        f(-4isize as *mut c_void); // PER_MONITOR_AWARE_V2
    }
}

/// Applies the dark scrollbar theme to a control. Win10 1809+ understands
/// "DarkMode_Explorer"; passing null restores the default theme.
unsafe fn set_control_dark(hwnd: HWND, dark: bool) {
    type ThemeFn = unsafe extern "system" fn(HWND, PCWSTR, PCWSTR) -> i32;
    if let Some(f) = call_fn::<ThemeFn>(dyn_proc("uxtheme.dll", "SetWindowTheme")) {
        if dark {
            let name = wide("DarkMode_Explorer");
            f(hwnd, name.as_ptr(), ptr::null());
        } else {
            f(hwnd, ptr::null(), ptr::null());
        }
    }
}

unsafe fn dark_titlebar(hwnd: HWND, dark: bool) {
    type DwmFn = unsafe extern "system" fn(HWND, u32, *const c_void, u32) -> i32;
    if let Some(f) = call_fn::<DwmFn>(dyn_proc("dwmapi.dll", "DwmSetWindowAttribute")) {
        let v: i32 = if dark { 1 } else { 0 };
        // 20 is the documented attribute since 19041; 19 is what some Windows
        // 10 builds actually honour. Set both, one of them will stick.
        f(hwnd, k::DWMWA_USE_IMMERSIVE_DARK_MODE, &v as *const i32 as *const c_void, 4);
        f(hwnd, 19, &v as *const i32 as *const c_void, 4);
        // Win11: system rounded corners and Mica backdrop. Both calls simply
        // fail on Win10, where the app's own flat surfaces are used instead.
        let corner: i32 = 2; // DWMWCP_ROUND
        f(hwnd, k::DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const i32 as *const c_void, 4);
        let mica: i32 = 2; // DWMSBT_MAINWINDOW
        f(hwnd, k::DWMWA_SYSTEMBACKDROP_TYPE, &mica as *const i32 as *const c_void, 4);
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run(cfg: Config, first: Snapshot, mut collector: Collector, mut history: HistoryTracker) -> i32 {
    unsafe {
        enable_dpi();
        let mut icc = InitCommonControlsEx {
            dwSize: std::mem::size_of::<InitCommonControlsEx>() as u32,
            dwICC: k::ICC_LISTVIEW_CLASSES | k::ICC_BAR_CLASSES,
        };
        InitCommonControlsEx(&mut icc);

        let hinst = GetModuleHandleW(ptr::null());
        register_classes(hinst);

        let cfg_shared = Arc::new(Mutex::new(cfg.clone()));
        let interval = Arc::new(AtomicU32::new(cfg.interval_ms));
        let paused = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(Mutex::new(None::<Snapshot>));

        let pal = pal(&cfg);
        let h_icon = app_icon(hinst);
        let a = App {
            cfg: cfg.clone(),
            snap: first,
            trends: Trends::new(),
            table: Table::new(Vec::new()),
            collapsed: std::collections::HashSet::new(),
            page: Page::from_usize(cfg.page),
            perf_sel: 0,
            ctx_row: 0,
            search_placeholder: false,
            hover: -1,
            hwnd: ptr::null_mut(),
            h_list: ptr::null_mut(),
            h_header: ptr::null_mut(),
            h_perf_list: ptr::null_mut(),
            h_perf_header: ptr::null_mut(),
            h_chart: ptr::null_mut(),
            h_search: ptr::null_mut(),
            font: font(if cfg.mode == UiMode::Compact { 12 } else { 14 }, k::FW_NORMAL),
            font_bold: font(if cfg.mode == UiMode::Compact { 12 } else { 14 }, k::FW_BOLD),
            bg: CreateSolidBrush(pal.bg),
            panel: CreateSolidBrush(pal.panel),
            accent: CreateSolidBrush(pal.accent),
            tab_sel: CreateSolidBrush(pal.sel),
            border: CreateSolidBrush(pal.border),
            h_icon,
            tabs: Vec::new(),
            buttons: Vec::new(),
            header_cols: Vec::new(),
            himl: ptr::null_mut(),
            icon_cache: std::collections::HashMap::new(),
            tray: false,
            cols_dirty: true,
            cfg_shared: cfg_shared.clone(),
            interval: interval.clone(),
            paused: paused.clone(),
            pending: pending.clone(),
        };
        APP = Box::into_raw(Box::new(a));

        let title = wide("Rust 任务管理器 · RTM 1.4");
        let cls = wide("RtmMain");
        let hwnd = CreateWindowExW(
            0,
            cls.as_ptr(),
            title.as_ptr(),
            k::WS_OVERLAPPEDWINDOW | k::WS_CLIPCHILDREN,
            cfg.x,
            cfg.y,
            cfg.w,
            cfg.h,
            ptr::null_mut(),
            ptr::null_mut(),
            hinst,
            ptr::null_mut(),
        );
        if hwnd.is_null() {
            let msg = wide("无法创建主窗口");
            MessageBoxW(ptr::null_mut(), msg.as_ptr(), msg.as_ptr(), k::MB_ICONERROR);
            return 1;
        }
        (*APP).hwnd = hwnd;
        create_children(hinst, hwnd);
        dark_titlebar(hwnd, theme_now(&cfg) == Theme::Dark);
        rebuild_table(true);
        layout();
        apply_theme();
        ShowWindow(hwnd, k::SW_SHOW);
        UpdateWindow(hwnd);
        RegisterHotKey(hwnd, k::ID_HOTKEY, k::MOD_CONTROL | k::MOD_ALT | k::MOD_NOREPEAT, 'T' as u32);
        SetTimer(hwnd, k::IDT_REFRESH, 1000, ptr::null_mut());
        if cfg.tray {
            add_tray();
        }

        // Collection thread: owns the collector so its delta state stays warm.
        let hwnd_addr = hwnd as usize;
        let c_shared = cfg_shared.clone();
        let c_interval = interval.clone();
        let c_paused = paused.clone();
        let c_pending = pending.clone();
        thread::spawn(move || loop {
            let ms = c_interval.load(Ordering::Relaxed).max(250) as u64;
            if !c_paused.load(Ordering::Relaxed) {
                let snapshot = {
                    let c = c_shared.lock().map(|g| g.clone()).unwrap_or_else(|_| Config::default());
                    collector.collect(&c, &mut history)
                };
                if let Ok(mut g) = c_pending.lock() {
                    *g = Some(snapshot);
                }
                PostMessageW(hwnd_addr as HWND, k::WM_APP_SNAPSHOT, 0, 0);
            }
            thread::sleep(Duration::from_millis(ms));
        });

        let mut msg: MSG = std::mem::zeroed();
        loop {
            let r = GetMessageW(&mut msg, ptr::null_mut(), 0, 0);
            if r <= 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        0
    }
}

unsafe fn app_icon(hinst: HINSTANCE) -> HICON {
    // Resource id 1 is injected into the exe after the build (see mkicon.py).
    let mut icon = LoadIconW(hinst, 1 as *const u16);
    if icon.is_null() {
        icon = LoadIconW(ptr::null_mut(), k::IDI_APPLICATION);
    }
    icon
}

unsafe fn register_classes(hinst: HINSTANCE) {
    let cursor = LoadCursorW(ptr::null_mut(), k::IDC_ARROW);
    let icon = app_icon(hinst);
    if !MAIN_CLASS {
        let name = wide("RtmMain");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: k::CS_HREDRAW | k::CS_VREDRAW,
            lpfnWndProc: main_proc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: icon,
            hCursor: cursor,
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: name.as_ptr(),
            hIconSm: ptr::null_mut(),
        };
        RegisterClassExW(&wc);
        MAIN_CLASS = true;
    }
    if !CHART_CLASS {
        let name = wide("RtmChart");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: k::CS_HREDRAW | k::CS_VREDRAW,
            lpfnWndProc: chart_proc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: ptr::null_mut(),
            hCursor: cursor,
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: name.as_ptr(),
            hIconSm: ptr::null_mut(),
        };
        RegisterClassExW(&wc);
        CHART_CLASS = true;
    }
}

unsafe fn create_children(hinst: HINSTANCE, parent: HWND) {
    let a = app();
    let list_cls = wide("SysListView32");
    let edit_cls = wide("Edit");
    let chart_cls = wide("RtmChart");
    let empty = wide("");

    a.h_list = CreateWindowExW(
        k::WS_EX_CLIENTEDGE,
        list_cls.as_ptr(),
        empty.as_ptr(),
        k::WS_CHILD | k::WS_VISIBLE | k::WS_CLIPSIBLINGS | k::LVS_REPORT | k::LVS_OWNERDATA
            | k::LVS_SHOWSELALWAYS | k::LVS_NOCOLUMNHEADER,
        0, 0, 100, 100,
        parent,
        k::ID_LIST as usize as HMENU,
        hinst,
        ptr::null_mut(),
    );
    SendMessageW(a.h_list, k::LVM_SETEXTENDEDLISTVIEWSTYLE, 0,
        (k::LVS_EX_FULLROWSELECT | k::LVS_EX_INFOTIP | k::LVS_EX_LABELTIP) as LPARAM);
    SendMessageW(a.h_list, k::WM_SETFONT, a.font as WPARAM, 1);
    // Shared 16x16 image list for per-process executable icons.
    let himl = ImageList_Create(16, 16, k::ILC_COLOR32, 32, 64);
    if !himl.is_null() {
        ImageList_SetBkColor(himl, k::CLR_NONE);
        // Index 0 is the generic executable icon shown until the real icon of
        // a process has been resolved.
        // Index 0 is the shared default glyph. Every row falls back to it, so
        // an unresolvable icon can never render as a blank cell.
        let mut sys_icon = LoadIconW(ptr::null_mut(), 32517 as *const u16);
        if sys_icon.is_null() {
            sys_icon = LoadIconW(ptr::null_mut(), k::IDI_APPLICATION);
        }
        if sys_icon.is_null() {
            let mut sfi: ShFileInfoW = std::mem::zeroed();
            let placeholder = wide("placeholder.exe");
            let r = SHGetFileInfoW(
                placeholder.as_ptr(), 0x80,
                &mut sfi as *mut ShFileInfoW as *mut c_void,
                std::mem::size_of::<ShFileInfoW>() as u32,
                k::SHGFI_ICON | k::SHGFI_SMALLICON | k::SHGFI_USEFILEATTRIBUTES,
            );
            if r != 0 && !sfi.hIcon.is_null() {
                sys_icon = sfi.hIcon;
            }
        }
        if !sys_icon.is_null() {
            ImageList_AddIcon(himl, sys_icon);
        }
        SendMessageW(a.h_list, k::LVM_SETIMAGELIST, k::LVSIL_SMALL as WPARAM, himl as LPARAM);
        a.himl = himl;
    }
    // The native header stays disabled: DarkMode_Explorer does not repaint it
    // on Windows 10, so the parent window paints the captions instead.

    a.h_perf_list = CreateWindowExW(
        k::WS_EX_CLIENTEDGE,
        list_cls.as_ptr(),
        empty.as_ptr(),
        k::WS_CHILD | k::WS_CLIPSIBLINGS | k::LVS_REPORT | k::LVS_OWNERDATA | k::LVS_SHOWSELALWAYS,
        0, 0, 100, 100,
        parent,
        k::ID_PERF_LIST as usize as HMENU,
        hinst,
        ptr::null_mut(),
    );
    SendMessageW(a.h_perf_list, k::LVM_SETEXTENDEDLISTVIEWSTYLE, 0,
        (k::LVS_EX_FULLROWSELECT | k::LVS_EX_DOUBLEBUFFER) as LPARAM);
    SendMessageW(a.h_perf_list, k::WM_SETFONT, a.font as WPARAM, 1);

    a.h_chart = CreateWindowExW(
        0,
        chart_cls.as_ptr(),
        empty.as_ptr(),
        k::WS_CHILD | k::WS_CLIPSIBLINGS,
        0, 0, 100, 100,
        parent,
        k::ID_CHART as usize as HMENU,
        hinst,
        ptr::null_mut(),
    );

    a.h_search = CreateWindowExW(
        k::WS_EX_CLIENTEDGE,
        edit_cls.as_ptr(),
        empty.as_ptr(),
        k::WS_CHILD | k::WS_VISIBLE | k::WS_TABSTOP | k::ES_AUTOHSCROLL,
        0, 0, 100, 24,
        parent,
        k::ID_SEARCH as usize as HMENU,
        hinst,
        ptr::null_mut(),
    );
    SendMessageW(a.h_search, k::WM_SETFONT, a.font as WPARAM, 1);
    // EM_SETCUEBANNER requires comctl32 v6 visual styles, which a raw Win32
    // app has no manifest for. Use an explicit placeholder managed by
    // EN_SETFOCUS / EN_KILLFOCUS so the hint works on every configuration.
    if !a.cfg.search.is_empty() {
        let s = wide(&a.cfg.search);
        SetWindowTextW(a.h_search, s.as_ptr());
        a.search_placeholder = false;
    } else {
        // Set the flag BEFORE SetWindowTextW: the latter synchronously sends
        // EN_CHANGE, which would otherwise persist the hint as a search term.
        a.search_placeholder = true;
        SetWindowTextW(a.h_search, wide(SEARCH_HINT).as_ptr());
    }
}

const SEARCH_HINT: &str = "搜索进程 / 服务 / 启动项  (* ? | 与 re: 正则)";

// ---------------------------------------------------------------------------
// Layout & theme
// ---------------------------------------------------------------------------

unsafe fn layout() {
    let a = app();
    let mut rc = RECT::default();
    GetClientRect(a.hwnd, &mut rc);
    let w = rc.right;
    let h = rc.bottom;
    let fluent = a.cfg.mode == UiMode::Fluent;
    let (nav_w, tab_h, toolbar_h, status_h) = if fluent { (180, 0, 48, 28) } else { (0, 30, 34, 26) };
    let content_top = tab_h + toolbar_h;
    let content_left = nav_w;
    let content_w = (w - content_left).max(200);
    let metrics_h = if metrics_area(w, h, a.cfg.mode, a.page).is_some() { METRICS_H } else { 0 };
    let header_h = if a.page == Page::Performance { 0 } else { HEADER_H };
    let list_top = content_top + metrics_h + header_h;
    let content_h = (h - list_top - status_h).max(100);

    // Column header geometry is computed here so click hit-testing works even
    // before the first paint.
    a.header_cols.clear();
    if header_h > 0 {
        let mut x = content_left;
        let vis = a.table.visible_cols();
        for &ci in &vis {
            let cw = a.table.cols[ci].width;
            a.header_cols.push((
                RECT { left: x, top: content_top + metrics_h, right: x + cw, bottom: content_top + metrics_h + header_h },
                ci,
            ));
            x += cw;
        }
    }

    a.tabs.clear();
    if fluent {
        for i in 0..7 {
            let y = 54 + i as i32 * 44;
            a.tabs.push(RECT { left: 8, top: y, right: nav_w - 8, bottom: y + 40 });
        }
    } else {
        let tw = ((w - 8) / 7).min(130).max(64);
        for i in 0..7 {
            let x = 4 + i as i32 * tw;
            a.tabs.push(RECT { left: x, top: 2, right: x + tw - 2, bottom: tab_h - 2 });
        }
    }

    let search_w = if fluent { 280 } else { 250 };
    let sx = content_left + 10;
    let sy = tab_h + 5;
    MoveWindow(a.h_search, sx, sy, search_w, toolbar_h - 10, 1);

    a.buttons.clear();
    let defs: [(u32, &'static str); 8] = [
        (k::ID_BTN_RUN as u32, "运行新任务"),
        (k::ID_BTN_END as u32, "结束任务"),
        (k::ID_BTN_TREE as u32, "进程树"),
        (k::ID_BTN_MODE as u32, "模式"),
        (k::ID_BTN_THEME as u32, "主题"),
        (k::ID_BTN_INTERVAL as u32, "刷新"),
        (k::ID_BTN_EXPORT as u32, "导出"),
        (k::ID_BTN_MORE as u32, "更多"),
    ];
    let mut bx = sx + search_w + 10;
    for (id, label) in defs {
        let bw = (label.chars().count() as i32 * 12 + 22).max(58);
        if bx + bw > w - 8 {
            break;
        }
        a.buttons.push(Btn { rc: RECT { left: bx, top: sy, right: bx + bw, bottom: sy + toolbar_h - 10 }, id, label });
        bx += bw + 6;
    }

    let perf = a.page == Page::Performance;
    ShowWindow(a.h_chart, if perf { k::SW_SHOW } else { k::SW_HIDE });
    ShowWindow(a.h_perf_list, if perf { k::SW_SHOW } else { k::SW_HIDE });
    ShowWindow(a.h_list, if perf { k::SW_HIDE } else { k::SW_SHOW });
    if perf {
        let lw = 210.min(content_w / 3);
        MoveWindow(a.h_perf_list, content_left, content_top, lw, content_h, 1);
        MoveWindow(a.h_chart, content_left + lw, content_top, content_w - lw, content_h, 1);
        rebuild_perf_list();
    } else {
        MoveWindow(a.h_list, content_left, list_top, content_w, content_h, 1);
    }
}

unsafe fn apply_theme() {
    let a = app();
    let p = pal(&a.cfg);
    let list_bg = p.bg;
    SendMessageW(a.h_list, k::LVM_SETBKCOLOR, 0, list_bg as LPARAM);
    SendMessageW(a.h_list, k::LVM_SETTEXTCOLOR, 0, p.text as LPARAM);
    SendMessageW(a.h_list, k::LVM_SETTEXTBKCOLOR, 0, list_bg as LPARAM);
    SendMessageW(a.h_perf_list, k::LVM_SETBKCOLOR, 0, list_bg as LPARAM);
    SendMessageW(a.h_perf_list, k::LVM_SETTEXTCOLOR, 0, p.text as LPARAM);
    SendMessageW(a.h_perf_list, k::LVM_SETTEXTBKCOLOR, 0, list_bg as LPARAM);
    let grid = if a.cfg.mode == UiMode::Compact { k::LVS_EX_GRIDLINES } else { 0 };
    SendMessageW(a.h_list, k::LVM_SETEXTENDEDLISTVIEWSTYLE, 0,
        (k::LVS_EX_FULLROWSELECT | k::LVS_EX_INFOTIP | grid) as LPARAM);
    let dark = theme_now(&a.cfg) == Theme::Dark;
    dark_titlebar(a.hwnd, dark);
    set_control_dark(a.h_list, dark);
    set_control_dark(a.h_perf_list, dark);
    set_control_dark(a.h_search, dark);
    if !a.h_header.is_null() {
        set_control_dark(a.h_header, dark);
    }
    // Re-apply the control colours after the theme change; some common-control
    // versions reset them when the window theme changes.
    SendMessageW(a.h_list, k::LVM_SETBKCOLOR, 0, p.bg as LPARAM);
    SendMessageW(a.h_list, k::LVM_SETTEXTCOLOR, 0, p.text as LPARAM);
    SendMessageW(a.h_list, k::LVM_SETTEXTBKCOLOR, 0, p.bg as LPARAM);
    SendMessageW(a.h_perf_list, k::LVM_SETBKCOLOR, 0, p.bg as LPARAM);
    SendMessageW(a.h_perf_list, k::LVM_SETTEXTCOLOR, 0, p.text as LPARAM);
    SendMessageW(a.h_perf_list, k::LVM_SETTEXTBKCOLOR, 0, p.bg as LPARAM);
    InvalidateRect(a.hwnd, ptr::null(), 1);
    InvalidateRect(a.h_list, ptr::null(), 1);
    InvalidateRect(a.h_perf_list, ptr::null(), 1);
    if !a.h_header.is_null() {
        InvalidateRect(a.h_header, ptr::null(), 1);
    }
    if !a.h_perf_header.is_null() {
        InvalidateRect(a.h_perf_header, ptr::null(), 1);
    }
    InvalidateRect(a.h_chart, ptr::null(), 1);
}

unsafe fn set_mode(mode: UiMode) {
    let a = app();
    if !a.font.is_null() {
        DeleteObject(a.font as HGDIOBJ);
    }
    if !a.font_bold.is_null() {
        DeleteObject(a.font_bold as HGDIOBJ);
    }
    let size = if mode == UiMode::Compact { 12 } else { 14 };
    a.font = font(size, k::FW_NORMAL);
    a.font_bold = font(size, k::FW_BOLD);
    a.cfg.mode = mode;
    SendMessageW(a.h_list, k::WM_SETFONT, a.font as WPARAM, 1);
    SendMessageW(a.h_perf_list, k::WM_SETFONT, a.font as WPARAM, 1);
    SendMessageW(a.h_search, k::WM_SETFONT, a.font as WPARAM, 1);
    sync_config();
    layout();
    apply_theme();
}

unsafe fn sync_config() {
    let a = app();
    if let Ok(mut g) = a.cfg_shared.lock() {
        *g = a.cfg.clone();
    }
    a.cfg.save();
}

// ---------------------------------------------------------------------------
// Table / list synchronisation
// ---------------------------------------------------------------------------

unsafe fn rebuild_table(columns_dirty: bool) {
    let a = app();
    let cfg = a.cfg.clone();
    let collapsed = a.collapsed.clone();
    let page = a.page;
    let icons = a.icon_cache.clone();
    let mut t = table::build(page, &a.snap, &cfg, &collapsed,
        &|ft| collect::file_time_string(ft),
        &|s| collect::unix_time_string(s),
        &|path, name| {
            if !path.is_empty() {
                let idx = *icons.get(path).unwrap_or(&0);
                if idx > 0 {
                    return idx;
                }
            }
            if !name.is_empty() {
                let key = format!("sys:{}", name.to_ascii_lowercase());
                let idx = *icons.get(&key).unwrap_or(&0);
                if idx > 0 {
                    return idx;
                }
            }
            0
        });
    if let Some(hidden) = cfg.cols_hidden.get(page.label()) {
        for &i in hidden {
            if let Some(c) = t.cols.get_mut(i) {
                c.visible = false;
            }
        }
        if t.visible_cols().is_empty() {
            if let Some(c) = t.cols.first_mut() {
                c.visible = true;
            }
        }
    }
    t.recompute(&cfg.search, cfg.use_regex, cfg.sort_col, cfg.sort_desc);
    a.table = t;
    if columns_dirty || a.cols_dirty {
        rebuild_columns();
        a.cols_dirty = false;
    }
    if page != Page::Performance {
        // Preserve the scroll position across a refresh: rebuilding the row set
        // otherwise snaps the view back to the first row every second.
        let top = SendMessageW(a.h_list, k::LVM_GETTOPINDEX, 0, 0) as i32;
        SendMessageW(a.h_list, k::LVM_SETITEMCOUNT, a.table.len() as WPARAM, 0);
        if top > 0 && a.table.len() as i32 > top {
            SendMessageW(a.h_list, k::LVM_ENSUREVISIBLE, top as WPARAM, 0);
        }
        InvalidateRect(a.h_list, ptr::null(), 0);
    }
}

unsafe fn rebuild_columns() {
    let a = app();
    let h = a.h_list;
    for _ in 0..64 {
        if SendMessageW(h, k::LVM_DELETECOLUMN, 0, 0) == 0 {
            break;
        }
    }
    let widths = a.cfg.col_widths.get(a.page.label()).cloned().unwrap_or_default();
    let vis = a.table.visible_cols();
    for (i, &ci) in vis.iter().enumerate() {
        let col = &a.table.cols[ci];
        let name = wide(col.name);
        let cx = widths.get(i).copied().unwrap_or(col.width);
        let lc = LvColumnW {
            mask: k::LVCF_TEXT | k::LVCF_WIDTH | k::LVCF_FMT | k::LVCF_SUBITEM,
            fmt: col.align,
            cx,
            pszText: name.as_ptr() as *mut u16,
            cchTextMax: 0,
            iSubItem: i as i32,
            iImage: 0,
            iOrder: i as i32,
            cxMin: 0,
            cxDefault: 0,
            cxIdeal: 0,
        };
        SendMessageW(h, k::LVM_INSERTCOLUMNW, i as WPARAM, &lc as *const LvColumnW as LPARAM);
    }
}

unsafe fn rebuild_perf_list() {
    let a = app();
    let h = a.h_perf_list;
    for _ in 0..16 {
        if SendMessageW(h, k::LVM_DELETECOLUMN, 0, 0) == 0 {
            break;
        }
    }
    for (i, name) in ["类别", "当前值"].iter().enumerate() {
        let w = wide(name);
        let lc = LvColumnW {
            mask: k::LVCF_TEXT | k::LVCF_WIDTH | k::LVCF_FMT | k::LVCF_SUBITEM,
            fmt: if i == 1 { k::LVCFMT_RIGHT } else { k::LVCFMT_LEFT },
            cx: if i == 0 { 110 } else { 90 },
            pszText: w.as_ptr() as *mut u16,
            iSubItem: i as i32,
            ..Default::default()
        };
        SendMessageW(h, k::LVM_INSERTCOLUMNW, i as WPARAM, &lc as *const LvColumnW as LPARAM);
    }
    let cats = table::perf_categories(&a.snap);
    SendMessageW(h, k::LVM_SETITEMCOUNT, cats.len() as WPARAM, 0);
}

unsafe fn switch_page(idx: usize) {
    let a = app();
    let p = Page::from_usize(idx);
    if p == a.page {
        return;
    }
    a.page = p;
    a.cfg.page = idx;
    a.cfg.sort_col = -1;
    a.cols_dirty = true;
    sync_config();
    rebuild_table(true);
    layout();
    // The tab strip is painted by the parent window; child moves do not
    // invalidate it, so force a repaint or the highlight lags one frame.
    InvalidateRect(a.hwnd, ptr::null(), 0);
}

/// Resolves the shell small icon of an executable path into the shared image
/// list. Returns the image index, or -1 when the shell cannot provide one.
unsafe fn shell_icon_index(himl: HANDLE, path: &str) -> i32 {
    let w = wide(path);
    // ExtractIconEx reports whether the image actually embeds an icon. Shell
    // would silently return the generic document icon, which is why so many
    // system processes looked "unloaded".
    let mut large: HICON = ptr::null_mut();
    let mut small: HICON = ptr::null_mut();
    let n = ExtractIconExW(w.as_ptr(), 0, &mut large, &mut small, 1);
    if !large.is_null() {
        DestroyIcon(large);
    }
    if n > 0 && !small.is_null() {
        let idx = ImageList_AddIcon(himl, small);
        DestroyIcon(small);
        return idx;
    }
    // No embedded icon: fall back to the shared default at index 0.
    0
}

/// Loads a bounded number of new icons per refresh. Shell icon extraction is
/// file I/O, so doing all ~250 at once would stall the UI thread.
unsafe fn load_icons_batch(a: &mut App, budget: usize) -> bool {
    if a.himl.is_null() {
        return false;
    }
    let mut added = 0usize;
    let mut changed = false;
    for p in a.snap.procs.iter() {
        if added >= budget {
            break;
        }
        if !p.path.is_empty() {
            if a.icon_cache.contains_key(&p.path) {
                continue;
            }
            let idx = shell_icon_index(a.himl, &p.path);
            a.icon_cache.insert(p.path.clone(), idx);
            added += 1;
            changed = true;
        } else if !p.name.is_empty() {
            // System processes often deny QueryFullProcessImageName, so fall
            // back to the conventional System32 location for the image.
            let key = format!("sys:{}", p.name.to_ascii_lowercase());
            if a.icon_cache.contains_key(&key) {
                continue;
            }
            let guess = format!("C:\\Windows\\System32\\{}", p.name);
            let idx = if std::fs::metadata(&guess).is_ok() {
                shell_icon_index(a.himl, &guess)
            } else {
                -1
            };
            a.icon_cache.insert(key, idx);
            added += 1;
            changed = true;
        }
    }
    changed
}

unsafe fn selected_row() -> usize {
    let a = app();
    let r = SendMessageW(a.h_list, k::LVM_GETNEXTITEM, usize::MAX, k::LVNI_SELECTED as LPARAM);
    if r < 0 {
        usize::MAX
    } else {
        r as usize
    }
}

// ---------------------------------------------------------------------------
// Snapshot application
// ---------------------------------------------------------------------------

unsafe fn apply_snapshot() {
    let snap = {
        let a = app();
        a.pending.lock().ok().and_then(|mut g| g.take())
    };
    if let Some(s) = snap {
        {
            let a = app();
            a.trends.update(&s.sys);
            a.snap = s;
            load_icons_batch(a, 60);
        }
        rebuild_table(false);
        let a = app();
        if a.page == Page::Performance {
            rebuild_perf_list();
            InvalidateRect(a.h_chart, ptr::null(), 0);
        }
        InvalidateRect(a.hwnd, ptr::null(), 0);
    }
}

// ---------------------------------------------------------------------------
// Main window procedure
// ---------------------------------------------------------------------------

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        k::WM_ERASEBKGND => 1,
        k::WM_CTLCOLOREDIT => {
            // The search box is a native EDIT and ignores the app palette.
            if !APP.is_null() {
                let a = app();
                let p = pal(&a.cfg);
                SetTextColor(w as HDC, p.text);
                SetBkColor(w as HDC, p.bg);
                return a.bg as LRESULT;
            }
            0
        }
        k::WM_PAINT => {
            let mut ps = PaintStruct::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint_main(hwnd, hdc);
            EndPaint(hwnd, &ps);
            0
        }
        k::WM_SIZE => {
            if !APP.is_null() && !app().h_list.is_null() {
                layout();
            }
            0
        }
        k::WM_MEASUREITEM => {
            let m = &mut *(l as *mut MeasureItemStruct);
            if m.itemData != 0 {
                let item = &*(m.itemData as *const MenuDrawItem);
                m.itemHeight = if item.separator { 9 } else { 28 };
                m.itemWidth = 236;
            }
            1
        }
        k::WM_DRAWITEM => {
            let d = &mut *(l as *mut DrawItemStruct);
            if d.itemData != 0 && !APP.is_null() {
                draw_menu_item(d, app());
            }
            1
        }
        k::WM_COMMAND => {
            on_command(hwnd, w, l);
            0
        }
        k::WM_NOTIFY => on_notify(hwnd, l),
        k::WM_TIMER => {
            if w as usize == k::IDT_REFRESH && !APP.is_null() {
                InvalidateRect(hwnd, ptr::null(), 0);
            }
            0
        }
        k::WM_APP_SNAPSHOT => {
            apply_snapshot();
            0
        }
        k::WM_APP_REBUILD => {
            rebuild_table(false);
            0
        }
        k::WM_LBUTTONDOWN => {
            on_click(hwnd, l);
            0
        }
        k::WM_MOUSEMOVE => {
            if !APP.is_null() {
                let x = (l & 0xFFFF) as i16 as i32;
                let y = ((l >> 16) & 0xFFFF) as i16 as i32;
                let pt = POINT { x, y };
                let a = app();
                let mut hv = -1i32;
                for (i, b) in a.buttons.iter().enumerate() {
                    if PtInRect(&b.rc as *const RECT, pt) != 0 {
                        hv = i as i32;
                        break;
                    }
                }
                if hv < 0 {
                    for (i, r) in a.tabs.iter().enumerate() {
                        if PtInRect(r as *const RECT, pt) != 0 {
                            hv = 100 + i as i32;
                            break;
                        }
                    }
                }
                if hv != a.hover {
                    a.hover = hv;
                    InvalidateRect(hwnd, ptr::null(), 0);
                }
            }
            0
        }
        k::WM_KEYDOWN => {
            on_key(w);
            0
        }
        k::WM_HOTKEY => {
            let a = app();
            if IsWindowVisible(a.hwnd) != 0 && IsIconic(a.hwnd) == 0 {
                ShowWindow(a.hwnd, k::SW_HIDE);
            } else {
                ShowWindow(a.hwnd, k::SW_RESTORE);
                SetForegroundWindow(a.hwnd);
            }
            0
        }
        k::WM_APP_TRAY => {
            if l as u32 == 0x0203 || l as u32 == k::WM_LBUTTONDBLCLK {
                let a = app();
                ShowWindow(a.hwnd, k::SW_RESTORE);
                SetForegroundWindow(a.hwnd);
            } else if l as u32 == k::WM_RBUTTONUP {
                let a = app();
                show_tray_menu(a.hwnd);
            }
            0
        }
        k::WM_CLOSE => {
            let a = app();
            if a.cfg.tray {
                ShowWindow(hwnd, k::SW_HIDE);
            } else {
                DestroyWindow(hwnd);
            }
            0
        }
        k::WM_DESTROY => {
            if !APP.is_null() {
                let a = app();
                let mut wr = RECT::default();
                if GetWindowRect(a.hwnd, &mut wr) != 0 {
                    a.cfg.x = wr.left;
                    a.cfg.y = wr.top;
                    a.cfg.w = wr.right - wr.left;
                    a.cfg.h = wr.bottom - wr.top;
                }
                a.cfg.save();
                if !a.himl.is_null() {
                    ImageList_Destroy(a.himl);
                }
                remove_tray();
                UnregisterHotKey(a.hwnd, k::ID_HOTKEY);
                KillTimer(a.hwnd, k::IDT_REFRESH);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

unsafe fn on_click(hwnd: HWND, l: LPARAM) {
    let x = (l & 0xFFFF) as i16 as i32;
    let y = ((l >> 16) & 0xFFFF) as i16 as i32;
    let pt = POINT { x, y };
    let a = app();
    let mut origin = POINT::default();
    SendMessageW(a.h_list, k::LVM_GETORIGIN, 0, &mut origin as *mut POINT as LPARAM);
    let content_pt = POINT { x: pt.x + origin.x, y: pt.y };
    for (r, ci) in a.header_cols.clone() {
        if PtInRect(&r as *const RECT, content_pt) != 0 {
            if a.cfg.sort_col == ci as i32 {
                a.cfg.sort_desc = !a.cfg.sort_desc;
            } else {
                a.cfg.sort_col = ci as i32;
                a.cfg.sort_desc = ci != 0;
            }
            sync_config();
            rebuild_table(false);
            InvalidateRect(a.hwnd, ptr::null(), 0);
            return;
        }
    }
    for (i, r) in a.tabs.iter().enumerate() {
        if PtInRect(r as *const RECT, pt) != 0 {
            switch_page(i);
            return;
        }
    }
    let buttons = a.buttons.clone();
    for b in buttons {
        if PtInRect(&b.rc as *const RECT, pt) != 0 {
            execute(hwnd, b.id);
            return;
        }
    }
}

unsafe fn on_key(w: WPARAM) {
    let a = app();
    match w {
        0x74 => {
            // F5: force an immediate visual refresh
            a.paused.store(false, Ordering::Relaxed);
            rebuild_table(false);
            InvalidateRect(a.h_chart, ptr::null(), 0);
        }
        0x2E => execute(a.hwnd, k::M_END), // Delete
        0x46 if (GetAsyncKeyState(0x11) as u16 & 0x8000) != 0 => {
            SetFocus(a.h_search);
        }
        _ => {}
    }
}

unsafe fn on_command(hwnd: HWND, w: WPARAM, l: LPARAM) {
    let id = (w & 0xFFFF) as u32;
    let code = ((w >> 16) & 0xFFFF) as u32;
    if id == k::ID_SEARCH as u32 && code == k::EN_SETFOCUS {
        let a = app();
        if a.search_placeholder {
            SetWindowTextW(a.h_search, wide("").as_ptr());
            a.search_placeholder = false;
        }
        return;
    }
    if id == k::ID_SEARCH as u32 && code == k::EN_KILLFOCUS {
        let a = app();
        let n = GetWindowTextLengthW(a.h_search);
        if n == 0 {
            // Set the flag first: SetWindowTextW sends EN_CHANGE synchronously.
            a.search_placeholder = true;
            SetWindowTextW(a.h_search, wide(SEARCH_HINT).as_ptr());
        }
        return;
    }
    if id == k::ID_SEARCH as u32 && code == k::EN_CHANGE {
        let a = app();
        if a.search_placeholder {
            return;
        }
        let n = GetWindowTextLengthW(a.h_search);
        let mut buf = vec![0u16; (n + 1).max(1) as usize];
        GetWindowTextW(a.h_search, buf.as_mut_ptr(), (n + 1) as i32);
        a.cfg.search = from_wide(buf.as_ptr());
        a.cfg.save();
        rebuild_table(false);
        return;
    }
    if l == 0 && id >= 200 {
        execute(hwnd, id);
        return;
    }
    if id >= 100 && id < 200 && l == 0 {
        execute(hwnd, id);
    }
}

unsafe fn on_notify(hwnd: HWND, l: LPARAM) -> LRESULT {
    if l == 0 || APP.is_null() {
        return 0;
    }
    let hdr = &*(l as *const NmHdr);
    let from = hdr.idFrom;
    if from == k::ID_PERF_LIST as usize {
        if hdr.code == k::NM_CLICK || hdr.code == k::LVN_ITEMCHANGED {
            let a = app();
            let r = SendMessageW(a.h_perf_list, k::LVM_GETNEXTITEM, usize::MAX, k::LVNI_SELECTED as LPARAM);
            if r >= 0 {
                a.perf_sel = (r as usize).min(4);
                InvalidateRect(a.h_chart, ptr::null(), 0);
            }
        }
        if hdr.code == k::LVN_GETDISPINFOW {
            let di = &mut *(l as *mut NmLvDispInfoW);
            if di.item.mask & k::LVIF_TEXT != 0 {
                let cats = table::perf_categories(&app().snap);
                let idx = di.item.iItem as usize;
                if let Some(c) = cats.get(idx) {
                    let s = if di.item.iSubItem == 0 { c.0.clone() } else { c.1.clone() };
                    write_item_text(di.item.pszText, di.item.cchTextMax, &s);
                }
            }
        }
        return 0;
    }
    if from != k::ID_LIST as usize {
        return 0;
    }
    match hdr.code {
        k::LVN_GETDISPINFOW => {
            let di = &mut *(l as *mut NmLvDispInfoW);
            if di.item.mask & k::LVIF_TEXT != 0 {
                let a = app();
                let vis = a.table.visible_cols();
                let col = vis.get(di.item.iSubItem as usize).copied().unwrap_or(0);
                let s = a.table.cell(di.item.iItem as usize, col).to_string();
                write_item_text(di.item.pszText, di.item.cchTextMax, &s);
            }
            if di.item.iSubItem == 0 {
                let a = app();
                let icon = a.table.row(di.item.iItem as usize).map(|r| r.icon).unwrap_or(-1);
                if icon >= 0 {
                    di.item.mask |= k::LVIF_IMAGE;
                    di.item.iImage = icon;
                }
            }
            0
        }
        k::LVN_COLUMNCLICK => {
            let nmlv = &*(l as *const NmListView);
            let a = app();
            let vis = a.table.visible_cols();
            if let Some(&ci) = vis.get(nmlv.iSubItem as usize) {
                if a.cfg.sort_col == ci as i32 {
                    a.cfg.sort_desc = !a.cfg.sort_desc;
                } else {
                    a.cfg.sort_col = ci as i32;
                    a.cfg.sort_desc = ci != 0;
                }
                sync_config();
                rebuild_table(false);
            }
            0
        }
        k::NM_CLICK => {
            // The disclosure triangle has no hit area of its own, so clicks in
            // the leading 46 px of the name cell toggle the node.
            let nm = &*(l as *const NmItemActivate);
            let a = app();
            if a.page == Page::Processes
                && a.cfg.show_tree
                && nm.iItem >= 0
                && nm.pt.x >= 0
                && nm.pt.x < 46
            {
                if let Some(r) = a.table.row(nm.iItem as usize) {
                    if r.has_children {
                        let pid = r.pid;
                        if !a.collapsed.insert(pid) {
                            a.collapsed.remove(&pid);
                        }
                        // Rebuilding here re-enters the control's notification
                        // and deadlocks; defer to the message loop.
                        PostMessageW(a.hwnd, k::WM_APP_REBUILD, 0, 0);
                    }
                }
            }
            0
        }
        k::NM_RCLICK => {
            let a = app();
            a.ctx_row = selected_row();
            show_context_menu(hwnd, a.page);
            0
        }
        k::NM_DBLCLK => {
            let a = app();
            let row = selected_row();
            if a.page == Page::Processes && a.cfg.show_tree {
                if let Some(r) = a.table.row(row) {
                    if r.has_children {
                        let pid = r.pid;
                        if !a.collapsed.insert(pid) {
                            a.collapsed.remove(&pid);
                        }
                        PostMessageW(a.hwnd, k::WM_APP_REBUILD, 0, 0);
                        return 0;
                    }
                }
            }
            execute(hwnd, k::M_PROPERTIES);
            0
        }
        k::NM_CUSTOMDRAW => {
            let cd = &mut *(l as *mut NmLvCustomDraw);
            custom_draw(cd)
        }
        _ => 0,
    }
}

unsafe fn write_item_text(dst: *mut u16, max: i32, s: &str) {
    if dst.is_null() || max <= 1 {
        return;
    }
    let w = wide_no_null(s);
    let n = w.len().min(max as usize - 1);
    ptr::copy_nonoverlapping(w.as_ptr(), dst, n);
    *dst.add(n) = 0;
}



unsafe fn custom_draw(cd: &mut NmLvCustomDraw) -> LRESULT {
    if cd.nmcd.dwDrawStage == k::CDDS_PREPAINT {
        return k::CDRF_NOTIFYITEMDRAW as LRESULT;
    }
    if cd.nmcd.dwDrawStage != k::CDDS_ITEMPREPAINT {
        return k::CDRF_DODEFAULT as LRESULT;
    }
    // Never return CDRF_DODEFAULT for a row: the control would paint it with
    // the system window colour, which stays light in dark mode. Selection is
    // resolved through LVM_GETITEMSTATE because uItemState is not reliable.
    let a = app();
    let p = pal(&a.cfg);
    let state = SendMessageW(
        a.h_list,
        k::LVM_GETITEMSTATE,
        cd.nmcd.dwItemSpec as WPARAM,
        k::LVIS_SELECTED as LPARAM,
    ) as u32;
    let selected = state & k::LVIS_SELECTED != 0;
    cd.clrText = p.text;
    cd.clrTextBk = if selected { p.sel } else { p.bg };
    if !selected && a.cfg.heatmap {
        if let Some(r) = a.table.row(cd.nmcd.dwItemSpec as usize) {
            let cpu = r.vals.get(3).copied().unwrap_or(0.0);
            let intensity = (cpu / 45.0).min(1.0);
            if intensity > 0.25 {
                let t = intensity;
                cd.clrTextBk = if theme_now(&a.cfg) == Theme::Dark {
                    rgb((32.0 + 62.0 * t) as u8, (32.0 + 30.0 * t) as u8, (34.0 + 14.0 * t) as u8)
                } else {
                    rgb(255, (252.0 - 30.0 * t) as u8, (246.0 - 70.0 * t) as u8)
                };
            }
        }
    }
    k::CDRF_NEWFONT as LRESULT
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

unsafe fn draw_text(hdc: HDC, r: &mut RECT, s: &str, color: u32, f: HFONT, fmt: u32) {
    let old = SelectObject(hdc, f as HGDIOBJ);
    SetBkMode(hdc, k::TRANSPARENT);
    SetTextColor(hdc, color);
    let w = wide(s);
    DrawTextW(hdc, w.as_ptr(), -1, r as *mut RECT, fmt | k::DT_NOPREFIX | k::DT_END_ELLIPSIS | k::DT_SINGLELINE);
    SelectObject(hdc, old);
}

unsafe fn fill(hdc: HDC, r: &RECT, b: HBRUSH) {
    FillRect(hdc, r as *const RECT, b);
}

unsafe fn line_rect(hdc: HDC, r: &RECT, color: u32) {
    let br = CreateSolidBrush(color);
    fill(hdc, &RECT { left: r.left, top: r.top, right: r.right, bottom: r.top + 1 }, br);
    fill(hdc, &RECT { left: r.left, top: r.bottom - 1, right: r.right, bottom: r.bottom }, br);
    fill(hdc, &RECT { left: r.left, top: r.top, right: r.left + 1, bottom: r.bottom }, br);
    fill(hdc, &RECT { left: r.right - 1, top: r.top, right: r.right, bottom: r.bottom }, br);
    DeleteObject(br as HGDIOBJ);
}

/// Filled rounded rectangle with a 1px border, drawn with the stock GDI
/// primitives so it works identically on Win10 and Win11.
unsafe fn round_fill(hdc: HDC, r: &RECT, radius: i32, brush: HBRUSH, border: u32) {
    let pen = CreatePen(k::PS_SOLID, 1, border);
    let old_pen = SelectObject(hdc, pen);
    let old_br = SelectObject(hdc, brush);
    RoundRect(hdc, r.left, r.top, r.right, r.bottom, radius, radius);
    SelectObject(hdc, old_br);
    SelectObject(hdc, old_pen);
    DeleteObject(pen as HGDIOBJ);
}

/// Small vector-style navigation glyphs. Drawn as geometry instead of relying
/// on an icon font that may be missing on Win10.
unsafe fn draw_nav_icon(hdc: HDC, i: usize, x: i32, y: i32, color: u32) {
    let pen = CreatePen(k::PS_SOLID, 1, color);
    let old_pen = SelectObject(hdc, pen);
    let old_br = SelectObject(hdc, GetStockObject(5)); // NULL_BRUSH
    match i {
        0 => {
            Rectangle(hdc, x, y, x + 12, y + 12);
            MoveToEx(hdc, x + 2, y + 5, ptr::null_mut());
            LineTo(hdc, x + 10, y + 5);
            MoveToEx(hdc, x + 2, y + 8, ptr::null_mut());
            LineTo(hdc, x + 8, y + 8);
        }
        1 => {
            Rectangle(hdc, x, y, x + 12, y + 12);
            MoveToEx(hdc, x + 1, y + 9, ptr::null_mut());
            LineTo(hdc, x + 4, y + 5);
            LineTo(hdc, x + 7, y + 8);
            LineTo(hdc, x + 11, y + 2);
        }
        2 => {
            Ellipse(hdc, x, y, x + 12, y + 12);
            MoveToEx(hdc, x + 6, y + 6, ptr::null_mut());
            LineTo(hdc, x + 6, y + 2);
            MoveToEx(hdc, x + 6, y + 6, ptr::null_mut());
            LineTo(hdc, x + 9, y + 7);
        }
        3 => {
            let pts = [POINT { x: x + 2, y }, POINT { x: x + 11, y: y + 6 }, POINT { x: x + 2, y: y + 12 }, POINT { x: x + 2, y }];
            Polyline(hdc, pts.as_ptr(), pts.len() as i32);
        }
        4 => {
            Ellipse(hdc, x + 3, y, x + 9, y + 6);
            let pts = [POINT { x, y: y + 12 }, POINT { x, y: y + 9 }, POINT { x: x + 3, y: y + 7 },
                       POINT { x: x + 9, y: y + 7 }, POINT { x: x + 12, y: y + 9 }, POINT { x: x + 12, y: y + 12 }];
            Polyline(hdc, pts.as_ptr(), pts.len() as i32);
        }
        5 => {
            for j in 0..3 {
                MoveToEx(hdc, x + 1, y + 3 + j * 4, ptr::null_mut());
                LineTo(hdc, x + 11, y + 3 + j * 4);
            }
        }
        _ => {
            Ellipse(hdc, x + 2, y + 2, x + 10, y + 10);
            for (dx, dy) in [(6, 0), (6, 12), (0, 6), (12, 6)] {
                MoveToEx(hdc, x + dx, y + dy, ptr::null_mut());
                LineTo(hdc, x + dx + if dx == 0 { 2 } else { 0 }, y + dy + if dy == 0 { 2 } else { 0 });
            }
        }
    }
    SelectObject(hdc, old_br);
    SelectObject(hdc, old_pen);
    DeleteObject(pen as HGDIOBJ);
}

const METRICS_H: i32 = 84;
const HEADER_H: i32 = 30;

fn metrics_area(w: i32, h: i32, mode: UiMode, page: Page) -> Option<RECT> {
    if page == Page::Performance {
        return None;
    }
    let (nav_w, tab_h, toolbar_h, status_h) = if mode == UiMode::Fluent { (180, 0, 48, 28) } else { (0, 30, 34, 26) };
    let top = tab_h + toolbar_h + 8;
    let bottom = top + METRICS_H - 16;
    if bottom + 40 > h - status_h || w - nav_w < 320 {
        return None;
    }
    Some(RECT { left: nav_w + 12, top, right: w - 12, bottom })
}

unsafe fn draw_metrics(hdc: HDC, area: &RECT, a: &App, p: &Palette) {
    let s = &a.snap.sys;
    let mem_pct = if s.mem_total > 0 { s.mem_used as f32 * 100.0 / s.mem_total as f32 } else { 0.0 };
    let labels = ["CPU", "内存", "磁盘", "网络"];
    let values = [
        fmt_pct(s.cpu),
        format!("{} / {}", fmt_bytes(s.mem_used), fmt_bytes(s.mem_total)),
        fmt_rate(s.disk_read + s.disk_write),
        fmt_rate(s.net_recv + s.net_sent),
    ];
    let percents = [s.cpu, mem_pct, s.disk_active, ((s.net_recv + s.net_sent) / (2.0 * 1024.0 * 1024.0) * 100.0).min(100.0) as f32];
    let card = CreateSolidBrush(p.card);
    let track = CreateSolidBrush(p.border);
    let gap = 10i32;
    let cw = (area.right - area.left - gap * 3) / 4;
    for i in 0..4usize {
        let r = RECT {
            left: area.left + i as i32 * (cw + gap),
            top: area.top,
            right: area.left + i as i32 * (cw + gap) + cw,
            bottom: area.bottom,
        };
        round_fill(hdc, &r, 10, card, p.border);
        let mut lr = RECT { left: r.left + 12, top: r.top + 7, right: r.right - 10, bottom: r.top + 25 };
        draw_text(hdc, &mut lr, labels[i], p.sub, a.font, k::DT_LEFT | k::DT_VCENTER);
        let mut vr = RECT { left: r.left + 12, top: r.top + 23, right: r.right - 10, bottom: r.bottom - 16 };
        draw_text(hdc, &mut vr, &values[i], p.text, a.font_bold, k::DT_LEFT | k::DT_VCENTER);
        let bar = RECT { left: r.left + 12, top: r.bottom - 13, right: r.right - 12, bottom: r.bottom - 9 };
        fill(hdc, &bar, track);
        let filled = ((bar.right - bar.left) as f32 * percents[i].max(0.0).min(100.0) / 100.0) as i32;
        if filled > 1 {
            fill(hdc, &RECT { left: bar.left, top: bar.top, right: bar.left + filled, bottom: bar.bottom }, a.accent);
        }
    }
    DeleteObject(card as HGDIOBJ);
    DeleteObject(track as HGDIOBJ);
}

/// Custom column header painted by the parent window. The native header is
/// disabled because it cannot be themed and would stay bright in dark mode.
unsafe fn draw_column_header(hdc: HDC, a: &App, p: &Palette) {
    let cols = a.header_cols.clone();
    if cols.is_empty() {
        return;
    }
    let top = cols[0].0.top;
    let bottom = cols[0].0.bottom;
    let left = cols[0].0.left;
    let right = cols.last().map(|c| c.0.right).unwrap_or(left);
    let bg = CreateSolidBrush(p.panel);
    FillRect(hdc, &RECT { left, top, right, bottom } as *const RECT, bg);
    DeleteObject(bg as HGDIOBJ);
    let line = CreateSolidBrush(p.border);
    FillRect(hdc, &RECT { left, top: bottom - 1, right, bottom } as *const RECT, line);
    DeleteObject(line as HGDIOBJ);
    // Follow the list view's horizontal scroll offset, otherwise captions drift
    // away from their columns when the user scrolls sideways.
    let mut origin = POINT::default();
    SendMessageW(a.h_list, k::LVM_GETORIGIN, 0, &mut origin as *mut POINT as LPARAM);
    let shift = origin.x;
    IntersectClipRect(hdc, left, top, right, bottom);
    for (r, ci) in cols.iter() {
        let col = &a.table.cols[*ci];
        let sorted = a.cfg.sort_col == *ci as i32;
        let reserve = if sorted { 22 } else { 6 };
        let l = r.left - shift;
        let rr = r.right - shift;
        if rr <= left || l >= right {
            continue;
        }
        let mut tr = RECT { left: l + 8, top: r.top + 1, right: rr - reserve, bottom: r.bottom - 1 };
        let fmt = if col.align == k::LVCFMT_RIGHT {
            k::DT_RIGHT
        } else if col.align == k::LVCFMT_CENTER {
            k::DT_CENTER
        } else {
            k::DT_LEFT
        };
        draw_text(hdc, &mut tr, col.name, p.sub, a.font, fmt | k::DT_VCENTER);
        if sorted {
            let arrow = if a.cfg.sort_desc { "▼" } else { "▲" };
            let mut ar = RECT { left: rr - 20, top: r.top, right: rr - 5, bottom: r.bottom };
            draw_text(hdc, &mut ar, arrow, p.accent, a.font, k::DT_RIGHT | k::DT_VCENTER);
        }
        let sep = CreateSolidBrush(p.border);
        FillRect(hdc, &RECT { left: rr - 1, top: r.top + 6, right: rr, bottom: r.bottom - 6 } as *const RECT, sep);
        DeleteObject(sep as HGDIOBJ);
    }
    SelectClipRgn(hdc, ptr::null_mut());
}

// ---------------------------------------------------------------------------
// Legacy header subclassing helpers (kept for reference; the active path is
// the parent-painted header above).
// ---------------------------------------------------------------------------

unsafe extern "system" fn header_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if APP.is_null() {
        return CallWindowProcW(OLD_HEADER_PROC, hwnd, msg, w, l);
    }
    match msg {
        k::WM_ERASEBKGND => 1,
        k::WM_PAINT => {
            paint_header(hwnd, app());
            0
        }
        _ => {
            let a = app();
            let old = if hwnd == a.h_header { OLD_HEADER_PROC } else { OLD_HEADER_PROC2 };
            CallWindowProcW(old, hwnd, msg, w, l)
        }
    }
}

unsafe fn paint_header(header: HWND, a: &App) {
    let p = pal(&a.cfg);
    let mut ps = PaintStruct::default();
    let hdc = BeginPaint(header, &mut ps);
    let mut rc = RECT::default();
    GetClientRect(header, &mut rc);
    let bg = CreateSolidBrush(p.panel);
    FillRect(hdc, &rc as *const RECT, bg);
    DeleteObject(bg as HGDIOBJ);
    let line = CreateSolidBrush(p.border);
    FillRect(hdc, &RECT { left: 0, top: rc.bottom - 1, right: rc.right, bottom: rc.bottom } as *const RECT, line);
    DeleteObject(line as HGDIOBJ);

    let count = SendMessageW(header, k::HDM_GETITEMCOUNT, 0, 0) as i32;
    let old_font = SelectObject(hdc, a.font as HGDIOBJ);
    SetBkMode(hdc, k::TRANSPARENT);
    SetTextColor(hdc, p.sub);
    for i in 0..count {
        let mut r = RECT::default();
        if SendMessageW(header, k::HDM_GETITEMRECT, i as WPARAM, &mut r as *mut RECT as LPARAM) == 0 {
            continue;
        }
        if r.right <= rc.left || r.left >= rc.right {
            continue;
        }
        let mut text = [0u16; 128];
        let mut item = HdItemW {
            mask: k::HDI_TEXT,
            cxy: 0,
            pszText: text.as_mut_ptr(),
            cchTextMax: 128,
            iImage: 0,
            iOrder: 0,
            iType: 0,
            pvFilter: ptr::null_mut(),
            state: 0,
        };
        SendMessageW(header, k::HDM_GETITEMW, i as WPARAM, &mut item as *mut HdItemW as LPARAM);
        let label = from_wide(text.as_ptr());
        let mut tr = RECT { left: r.left + 8, top: r.top + 1, right: r.right - 6, bottom: r.bottom - 1 };
        let wl = wide(&label);
        DrawTextW(
            hdc, wl.as_ptr(), -1, &mut tr,
            k::DT_LEFT | k::DT_VCENTER | k::DT_SINGLELINE | k::DT_END_ELLIPSIS | k::DT_NOPREFIX,
        );
        let sep = CreateSolidBrush(p.border);
        FillRect(hdc, &RECT { left: r.right - 1, top: r.top + 5, right: r.right, bottom: r.bottom - 5 } as *const RECT, sep);
        DeleteObject(sep as HGDIOBJ);
    }
    SelectObject(hdc, old_font);
    EndPaint(header, &ps);
}

unsafe fn paint_main(hwnd: HWND, hdc: HDC) {
    if APP.is_null() {
        return;
    }
    let a = app();
    let p = pal(&a.cfg);
    let mut rc = RECT::default();
    GetClientRect(hwnd, &mut rc);
    let w = rc.right;
    let h = rc.bottom;
    let fluent = a.cfg.mode == UiMode::Fluent;
    fill(hdc, &rc, a.bg);
    let (nav_w, tab_h, toolbar_h, status_h) = if fluent { (180, 0, 48, 28) } else { (0, 30, 34, 26) };

    if fluent {
        let nav = RECT { left: 0, top: 0, right: nav_w, bottom: h - status_h };
        fill(hdc, &nav, a.panel);
        fill(hdc, &RECT { left: nav_w - 1, top: 0, right: nav_w, bottom: h }, a.border);
        let brand = RECT { left: 20, top: 14, right: nav_w - 10, bottom: 42 };
        draw_text(hdc, &mut brand.clone(), "Rust 任务管理器", p.text, a.font_bold, k::DT_LEFT | k::DT_VCENTER);
    } else {
        let bar = RECT { left: 0, top: 0, right: w, bottom: tab_h };
        fill(hdc, &bar, a.panel);
        fill(hdc, &RECT { left: 0, top: tab_h - 1, right: w, bottom: tab_h }, a.border);
    }

    let tabs = a.tabs.clone();
    for (i, r) in tabs.iter().enumerate() {
        let sel = i == a.page.index();
        let hovered = a.hover == 100 + i as i32;
        if fluent {
            if sel || hovered {
                let br = CreateSolidBrush(if sel { p.sel } else { p.card });
                round_fill(hdc, r, 8, br, if sel { p.sel } else { p.border });
                DeleteObject(br as HGDIOBJ);
            }
            if sel {
                fill(hdc, &RECT { left: r.left + 2, top: r.top + 9, right: r.left + 5, bottom: r.bottom - 9 }, a.accent);
            }
            draw_nav_icon(hdc, i, r.left + 16, r.top + 13, if sel { p.accent } else { p.sub });
            let mut tr = RECT { left: r.left + 40, top: r.top, right: r.right - 6, bottom: r.bottom };
            draw_text(hdc, &mut tr, Page::from_usize(i).label(), if sel { p.accent } else { p.text }, if sel { a.font_bold } else { a.font }, k::DT_LEFT | k::DT_VCENTER);
        } else {
            if sel || hovered {
                let br = CreateSolidBrush(if sel { p.sel } else { p.card });
                round_fill(hdc, r, 6, br, if sel { p.sel } else { p.border });
                DeleteObject(br as HGDIOBJ);
            }
            if sel {
                fill(hdc, &RECT { left: r.left + 12, top: r.bottom - 4, right: r.right - 12, bottom: r.bottom - 2 }, a.accent);
            }
            let mut tr = *r;
            draw_text(hdc, &mut tr, Page::from_usize(i).label(), if sel { p.accent } else { p.sub }, if sel { a.font_bold } else { a.font }, k::DT_CENTER | k::DT_VCENTER);
        }
    }

    let bar = RECT { left: nav_w, top: tab_h, right: w, bottom: tab_h + toolbar_h };
    fill(hdc, &bar, a.bg);
    fill(hdc, &RECT { left: nav_w, top: tab_h + toolbar_h - 1, right: w, bottom: tab_h + toolbar_h }, a.border);
    let buttons = a.buttons.clone();
    for (bi, b) in buttons.iter().enumerate() {
        let hovered = a.hover == bi as i32;
        let primary = bi == 0;
        let br = CreateSolidBrush(if primary { p.accent } else if hovered { p.sel } else { p.card });
        round_fill(hdc, &b.rc, 7, br, if primary || hovered { p.accent } else { p.border });
        DeleteObject(br as HGDIOBJ);
        let mut tr = b.rc;
        draw_text(hdc, &mut tr, b.label, if primary { rgb(255, 255, 255) } else { p.text }, a.font, k::DT_CENTER | k::DT_VCENTER);
    }
    if let Some(area) = metrics_area(w, h, a.cfg.mode, a.page) {
        draw_metrics(hdc, &area, a, &p);
    }
    if a.page != Page::Performance {
        draw_column_header(hdc, a, &p);
    }

    let s = &a.snap.sys;
    let status = format!(
        "进程 {}  |  CPU {}  |  内存 {}/{}  |  磁盘 {}  |  网络 ↓{} ↑{}",
        a.snap.procs.len(),
        fmt_pct(s.cpu),
        fmt_bytes(s.mem_used),
        fmt_bytes(s.mem_total),
        fmt_rate(s.disk_read + s.disk_write),
        fmt_rate(s.net_recv),
        fmt_rate(s.net_sent),
    );
    let sb = RECT { left: nav_w, top: h - status_h, right: w, bottom: h };
    fill(hdc, &sb, a.panel);
    fill(hdc, &RECT { left: nav_w, top: h - status_h, right: w, bottom: h - status_h + 1 }, a.border);
    let mut sr = RECT { left: sb.left + 10, top: sb.top, right: sb.right - 176, bottom: sb.bottom };
    draw_text(hdc, &mut sr, &status, p.sub, a.font, k::DT_LEFT | k::DT_VCENTER);
    let mut hr = RECT { left: sb.right - 170, top: sb.top, right: sb.right - 10, bottom: sb.bottom };
    draw_text(
        hdc,
        &mut hr,
        &format!("开机 {}", collect::unix_short_string(s.boot_unix)),
        p.sub,
        a.font,
        k::DT_RIGHT | k::DT_VCENTER,
    );
}

fn nav_icon(i: usize) -> &'static str {
    match i {
        0 => "▤",
        1 => "◈",
        2 => "◷",
        3 => "⚡",
        4 => "♙",
        5 => "☰",
        6 => "⚙",
        _ => "·",
    }
}

// ---------------------------------------------------------------------------
// Performance chart
// ---------------------------------------------------------------------------

unsafe extern "system" fn chart_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        k::WM_ERASEBKGND => 1,
        k::WM_PAINT => {
            let mut ps = PaintStruct::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint_chart(hwnd, hdc);
            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

unsafe fn draw_series(hdc: HDC, r: &RECT, vals: &[f32], max: f32, color: u32, fill_color: u32) {
    let w = (r.right - r.left).max(1);
    let h = (r.bottom - r.top).max(1);
    let n = vals.len().max(2);
    let m = max.max(1.0);
    let pen = CreatePen(k::PS_SOLID, 2, color);
    let old_pen = SelectObject(hdc, pen);
    let br = CreateSolidBrush(fill_color);
    let old_br = SelectObject(hdc, br);

    let mut pts: Vec<POINT> = (0..n)
        .map(|i| {
            let v = vals.get(i).copied().unwrap_or(0.0).max(0.0).min(m);
            POINT { x: r.left + (i as i32 * w / (n as i32 - 1)), y: r.bottom - (v / m * h as f32) as i32 }
        })
        .collect();
    if pts.len() >= 2 {
        let mut poly = pts.clone();
        poly.push(POINT { x: r.right, y: r.bottom });
        poly.push(POINT { x: r.left, y: r.bottom });
        Polygon(hdc, poly.as_ptr(), poly.len() as i32);
        Polyline(hdc, pts.as_ptr(), pts.len() as i32);
    }
    SelectObject(hdc, old_br);
    DeleteObject(br as HGDIOBJ);
    SelectObject(hdc, old_pen);
    DeleteObject(pen);
    pts.clear();
}

unsafe fn paint_chart(hwnd: HWND, hdc: HDC) {
    if APP.is_null() {
        return;
    }
    let a = app();
    let p = pal(&a.cfg);
    let mut rc = RECT::default();
    GetClientRect(hwnd, &mut rc);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    if w < 50 || h < 50 {
        return;
    }
    let mem = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old_bmp = SelectObject(mem, bmp as HGDIOBJ);
    fill(mem, &rc, a.bg);

    let s = &a.snap.sys;
    let sel = a.perf_sel.min(4);
    let hw = collect::hardware();
    let (title, value, series, maxv, details): (&str, String, Vec<f32>, f32, Vec<String>) = match sel {
        0 => {
            let mut d = vec![
                format!("处理器: {}", s.cpu_name),
                format!("基准速度: {:.2} GHz", s.cpu_base_mhz as f64 / 1000.0),
                format!("逻辑处理器: {}   内核: {}", s.logical_processors, s.cpu_cores.len()),
                format!(
                    "开机时间: {}    已运行: {}",
                    collect::unix_short_string(s.boot_unix),
                    fmt_duration(s.uptime_secs)
                ),
                format!("句柄 {}   线程 {}   进程 {}", s.handles, s.threads, s.processes),
            ];
            if !s.cpu_cores.is_empty() {
                d.push(format!("单核峰值: {}", fmt_pct(s.cpu_cores.iter().cloned().fold(0.0f32, f32::max))));
            }
            if !hw.board.is_empty() {
                d.push(format!("主板: {}", hw.board));
            }
            if !hw.bios.is_empty() {
                d.push(format!("BIOS: {}", hw.bios));
            }
            if !hw.gpu.is_empty() {
                for g in hw.gpu.split("   |   ") {
                    d.push(format!("显卡: {}", g));
                }
            }
            if !hw.gpu_driver.is_empty() {
                for g in hw.gpu_driver.split("   |   ") {
                    d.push(format!("驱动: {}", g));
                }
            }
            for (i, dsk) in hw.disks.iter().enumerate() {
                let kind = hw.disk_kinds.get(i).cloned().unwrap_or_default();
                if kind.is_empty() {
                    d.push(format!("磁盘: {}", dsk));
                } else {
                    d.push(format!("磁盘: {} [{}]", dsk, kind));
                }
            }
            for m in &hw.mem_modules {
                d.push(format!("内存条: {}", m));
            }
            ("CPU", fmt_pct(s.cpu), a.trends.cpu.vals.clone(), 100.0, d)
        }
        1 => {
            let d = vec![
                format!("使用中: {}", fmt_bytes(s.mem_used)),
                format!("可用: {}", fmt_bytes(s.mem_avail)),
                format!("总计: {}", fmt_bytes(s.mem_total)),
                format!("已提交: {} / {}", fmt_bytes(s.commit), fmt_bytes(s.commit_limit)),
                format!("缓存: {}", fmt_bytes(s.cached)),
                format!("内存负载: {}%", s.mem_load),
                if hw.phys_mem > 0 {
                    format!("物理安装: {}", fmt_bytes(hw.phys_mem))
                } else {
                    String::new()
                },
            ].into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>();
            ("内存", fmt_bytes(s.mem_used), a.trends.mem.vals.clone(), 100.0, d)
        }
        2 => {
            let mut d = vec![
                format!("读取: {}", fmt_rate(s.disk_read)),
                format!("写入: {}", fmt_rate(s.disk_write)),
                format!("活动时间: {}", fmt_pct(s.disk_active)),
            ];
            for dsk in s.disks.iter().take(6) {
                d.push(format!("{}  {} 可用 / {}", dsk.letter, fmt_bytes(dsk.free), fmt_bytes(dsk.total)));
            }
            ("磁盘 0", fmt_rate(s.disk_read + s.disk_write), a.trends.disk.vals.clone(), 100.0, d)
        }
        3 => {
            let mut d = vec![
                format!("接收: {}", fmt_rate(s.net_recv)),
                format!("发送: {}", fmt_rate(s.net_sent)),
                format!("累计接收: {}", fmt_bytes(s.net_total_recv)),
                format!("累计发送: {}", fmt_bytes(s.net_total_sent)),
            ];
            for ad in s.adapters.iter().take(6) {
                d.push(format!("{}   {:.0} Mbps", ad.name, ad.speed as f64 / 1_000_000.0));
            }
            ("网络", fmt_rate(s.net_recv + s.net_sent), a.trends.net.vals.clone(), 100.0, d)
        }
        _ => {
            let d = vec![
                format!("GPU 引擎占用: {}", fmt_pct(s.gpu)),
                "说明: 数据来自 PDH GPU Engine 计数器，按 PID 聚合".into(),
                "若显示 0%，表示当前驱动未提供该计数器".into(),
            ];
            ("GPU 0", fmt_pct(s.gpu), a.trends.gpu.vals.clone(), 100.0, d)
        }
    };

    let mut tr = RECT { left: 20, top: 14, right: w - 20, bottom: 46 };
    draw_text(mem, &mut tr, title, p.text, a.font_bold, k::DT_LEFT | k::DT_VCENTER);
    let mut vr = RECT { left: 20, top: 44, right: w - 20, bottom: 88 };
    draw_text(mem, &mut vr, &value, p.accent, a.font_bold, k::DT_LEFT | k::DT_VCENTER);

    // The detail block now carries up to 12 hardware lines, so the graph is
    // shortened to leave room instead of clipping them.
    let graph = RECT { left: 20, top: 94, right: (w - 20).max(21), bottom: (h - 360).max(160) };
    fill(mem, &graph, a.bg);
    for i in 0..=4 {
        let y = graph.top + (graph.bottom - graph.top) * i / 4;
        let br = CreateSolidBrush(p.grid);
        fill(mem, &RECT { left: graph.left, top: y, right: graph.right, bottom: y + 1 }, br);
        DeleteObject(br as HGDIOBJ);
    }
    for i in 0..=10 {
        let x = graph.left + (graph.right - graph.left) * i / 10;
        let br = CreateSolidBrush(p.grid);
        fill(mem, &RECT { left: x, top: graph.top, right: x + 1, bottom: graph.bottom }, br);
        DeleteObject(br as HGDIOBJ);
    }
    draw_series(mem, &graph, &series, maxv, p.chart, p.chart_fill);
    line_rect(mem, &graph, p.border);

    // Detail lines are laid out from the bottom up so they can never collide
    // with the footnote row, and are clipped to whatever height is left.
    let note_top = h - 26;
    let avail = (note_top - 6 - graph.bottom - 6).max(0);
    let max_rows = (avail / 20).max(1) as usize;
    let n = details.len().min(16).min(max_rows);
    let mut y = (note_top - 6 - n as i32 * 20).max(graph.bottom + 6);
    for d in details.iter().take(n) {
        let mut r = RECT { left: 20, top: y, right: w - 20, bottom: y + 20 };
        draw_text(mem, &mut r, d, p.sub, a.font, k::DT_LEFT | k::DT_VCENTER);
        y += 20;
    }
    let mut note = RECT { left: 20, top: note_top, right: w - 20, bottom: h - 4 };
    draw_text(mem, &mut note, "RTM · 原生 Win32 / Rust · 折线为最近 120 次采样", p.sub, a.font, k::DT_LEFT | k::DT_VCENTER);

    BitBlt(hdc, 0, 0, w, h, mem, 0, 0, k::SRCCOPY);
    SelectObject(mem, old_bmp);
    DeleteObject(bmp as HGDIOBJ);
    DeleteDC(mem);
}

// ---------------------------------------------------------------------------
// Menus
// ---------------------------------------------------------------------------

pub struct MenuDrawItem {
    pub text: String,
    pub shortcut: String,
    pub checked: bool,
    pub disabled: bool,
    pub separator: bool,
    pub submenu: bool,
}
impl Default for MenuDrawItem {
    fn default() -> Self {
        MenuDrawItem {
            text: String::new(),
            shortcut: String::new(),
            checked: false,
            disabled: false,
            separator: false,
            submenu: false,
        }
    }
}

/// Owner-drawn popup menu. System menus ignore the app theme and become
/// unreadable in dark mode, so every entry is drawn by us.
struct Mb {
    h: HMENU,
    items: Vec<Box<MenuDrawItem>>,
}

impl Mb {
    unsafe fn new() -> Mb {
        let h = CreatePopupMenu();
        let a = app();
        let mi = MenuInfo {
            cbSize: std::mem::size_of::<MenuInfo>() as u32,
            fMask: k::MIM_BACKGROUND,
            dwStyle: 0,
            cyMax: 0,
            hbrBack: a.panel,
            dwContextHelpID: 0,
            dwMenuData: 0,
        };
        SetMenuInfo(h, &mi);
        Mb { h, items: Vec::new() }
    }

    unsafe fn push(&mut self, data: Box<MenuDrawItem>, id: u32, sub: HMENU) {
        let ptr = (&*data) as *const MenuDrawItem as usize;
        let mut mii: MenuItemInfoW = std::mem::zeroed();
        mii.cbSize = std::mem::size_of::<MenuItemInfoW>() as u32;
        mii.fMask = k::MIIM_ID
            | k::MIIM_FTYPE
            | k::MIIM_DATA
            | k::MIIM_STATE
            | if sub.is_null() { 0 } else { k::MIIM_SUBMENU };
        mii.fType = k::MFT_OWNERDRAW;
        mii.fState = if data.checked { k::MFS_CHECKED } else { 0 };
        mii.wID = id;
        mii.hSubMenu = sub;
        mii.dwItemData = ptr;
        InsertMenuItemW(self.h, self.items.len() as u32, 1, &mii);
        self.items.push(data);
    }

    unsafe fn item(&mut self, label: &str, id: u32, checked: bool) {
        let mut d = MenuDrawItem::default();
        d.text = label.to_string();
        d.checked = checked;
        self.push(Box::new(d), id, ptr::null_mut());
    }

    unsafe fn sep(&mut self) {
        let mut d = MenuDrawItem::default();
        d.separator = true;
        self.push(Box::new(d), 0, ptr::null_mut());
    }

    unsafe fn sub(&mut self, label: &str, submenu: HMENU) {
        let mut d = MenuDrawItem::default();
        d.text = label.to_string();
        d.submenu = true;
        self.push(Box::new(d), 0, submenu);
    }

    unsafe fn show(&self, hwnd: HWND) -> u32 {
        let mut pt = POINT::default();
        GetCursorPos(&mut pt);
        // Without foreground activation a popup menu does not dismiss when the
        // user clicks elsewhere; WM_NULL afterwards releases the menu's grab.
        SetForegroundWindow(hwnd);
        let r = TrackPopupMenu(self.h, k::TPM_RETURNCMD | k::TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, ptr::null());
        PostMessageW(hwnd, 0, 0, 0);
        r as u32
    }

    unsafe fn destroy(&self) {
        DestroyMenu(self.h);
    }
}

unsafe fn draw_menu_item(d: &mut DrawItemStruct, a: &App) {
    let p = pal(&a.cfg);
    let item = &*(d.itemData as *const MenuDrawItem);
    let hdc = d.hDC;
    if item.separator {
        let br = CreateSolidBrush(p.border);
        let y = (d.rcItem.top + d.rcItem.bottom) / 2;
        FillRect(
            hdc,
            &RECT { left: d.rcItem.left + 10, top: y, right: d.rcItem.right - 10, bottom: y + 1 } as *const RECT,
            br,
        );
        DeleteObject(br as HGDIOBJ);
        return;
    }
    let selected = d.itemState & k::ODS_SELECTED != 0;
    let bg = CreateSolidBrush(if selected { p.sel } else { p.panel });
    FillRect(hdc, &d.rcItem as *const RECT, bg);
    DeleteObject(bg as HGDIOBJ);
    let color = if item.disabled {
        p.sub
    } else if selected {
        p.accent
    } else {
        p.text
    };
    if item.checked {
        let mut r = RECT { left: d.rcItem.left + 8, top: d.rcItem.top, right: d.rcItem.left + 26, bottom: d.rcItem.bottom };
        draw_text(hdc, &mut r, "✓", p.accent, a.font, k::DT_LEFT | k::DT_VCENTER);
    }
    let right = if item.submenu { d.rcItem.right - 26 } else { d.rcItem.right - 12 };
    let mut tr = RECT { left: d.rcItem.left + 28, top: d.rcItem.top, right, bottom: d.rcItem.bottom };
    draw_text(hdc, &mut tr, &item.text, color, a.font, k::DT_LEFT | k::DT_VCENTER);
    if !item.shortcut.is_empty() {
        let mut sr = RECT { left: d.rcItem.right - 62, top: d.rcItem.top, right: d.rcItem.right - 12, bottom: d.rcItem.bottom };
        draw_text(hdc, &mut sr, &item.shortcut, p.sub, a.font, k::DT_RIGHT | k::DT_VCENTER);
    }
    if item.submenu {
        let mut ar = RECT { left: d.rcItem.right - 24, top: d.rcItem.top, right: d.rcItem.right - 8, bottom: d.rcItem.bottom };
        draw_text(hdc, &mut ar, "›", p.sub, a.font, k::DT_CENTER | k::DT_VCENTER);
    }
}

unsafe fn show_context_menu(hwnd: HWND, page: Page) {
    let a = app();
    let mut m = Mb::new();
    match page {
        Page::Processes | Page::Details => {
            m.item("结束任务  (Delete)", k::M_END, false);
            m.item("结束进程树", k::M_END_TREE, false);
            m.item("效率模式 (降低优先级)", k::M_EFFICIENCY, false);
            m.item("重新启动", k::M_RESTART, false);
            m.sep();
            let mut pr = Mb::new();
            for (i, n) in ["低", "低于正常", "正常", "高于正常", "高"].iter().enumerate() {
                pr.item(n, k::M_PRIORITY_BASE + i as u32, false);
            }
            m.sub("设置优先级", pr.h);
            m.sep();
            m.item("打开文件所在位置", k::M_OPEN_PATH, false);
            m.item("属性 / 完整命令行", k::M_PROPERTIES, false);
            m.item("复制行", k::M_COPY, false);
            if page == Page::Processes {
                m.item("转到详细信息", k::M_GOTO_DETAILS, false);
            }
            m.sep();
            m.item("导出 CSV", k::M_EXPORT_CSV, false);
            m.item("导出 JSON", k::M_EXPORT_JSON, false);
        }
        Page::Services => {
            m.item("启动服务", k::M_SVC_START, false);
            m.item("停止服务", k::M_SVC_STOP, false);
            m.item("重启服务", k::M_SVC_RESTART, false);
            m.sep();
            m.item("复制行", k::M_COPY, false);
            m.item("导出 CSV", k::M_EXPORT_CSV, false);
        }
        Page::Startup => {
            m.item("启用", k::M_STARTUP_ENABLE, false);
            m.item("禁用", k::M_STARTUP_DISABLE, false);
            m.item("打开文件所在位置", k::M_STARTUP_LOCATE, false);
            m.sep();
            m.item("导出 CSV", k::M_EXPORT_CSV, false);
        }
        Page::Users => {
            m.item("复制行", k::M_COPY, false);
            m.item("导出 CSV", k::M_EXPORT_CSV, false);
        }
        Page::AppHistory => {
            m.item("清除历史记录", k::M_CLEAR_HISTORY, false);
            m.sep();
            m.item("导出 CSV", k::M_EXPORT_CSV, false);
        }
        Page::Performance => {
            m.item("立即刷新  (F5)", k::M_REFRESH, false);
            m.item("切换主题", k::M_THEME, false);
        }
    }
    let cmd = m.show(hwnd);
    // child popup menus are owned by the parent menu; DestroyMenu frees them
    m.destroy();
    if cmd != 0 {
        execute(hwnd, cmd);
    }
    let _ = a;
}

unsafe fn show_more_menu(hwnd: HWND) {
    let a = app();
    let mut m = Mb::new();
    m.item("立即刷新  (F5)", k::M_REFRESH, false);
    m.item(if a.paused.load(Ordering::Relaxed) { "继续刷新" } else { "暂停刷新" }, k::M_PAUSE, false);
    m.sep();
    let mut iv = Mb::new();
    for (i, n) in ["0.5 秒", "1 秒", "2 秒", "4 秒"].iter().enumerate() {
        iv.item(n, k::M_INTERVAL_BASE + i as u32, a.cfg.interval_ms == [500, 1000, 2000, 4000][i]);
    }
    m.sub("刷新速度", iv.h);
    m.sep();
    let mut cols = Mb::new();
    for (i, c) in a.table.cols.iter().enumerate().take(30) {
        cols.item(c.name, k::M_COL_BASE + i as u32, c.visible);
    }
    m.sub("显示列", cols.h);
    m.sep();
    m.item("紧凑 / Fluent 切换", k::M_MODE, a.cfg.mode == UiMode::Fluent);
    m.item(&format!("主题: {} (点击切换)", a.cfg.theme.label()), k::M_THEME, false);
    m.item("进程树", k::M_TREE, a.cfg.show_tree);
    m.item("进程分组", k::M_GROUP, a.cfg.group);
    m.item("全部展开", k::M_TREE_EXPAND, false);
    m.item("全部折叠", k::M_TREE_COLLAPSE, false);
    m.item("热力图", k::M_HEAT, a.cfg.heatmap);
    m.item("正则表达式搜索", k::M_REGEX, a.cfg.use_regex);
    m.item("GPU 监控 (首次启用需数百毫秒)", k::M_GPU, a.cfg.gpu);
    m.item("托盘常驻", k::M_TRAY, a.cfg.tray);
    m.item("低资源模式", k::M_LOWPOWER, a.cfg.low_power);
    m.sep();
    m.item("运行新任务", k::M_RUN, false);
    m.item("关于 RTM", k::M_ABOUT, false);
    m.item("退出", k::M_EXIT, false);
    let cmd = m.show(hwnd);
    m.destroy();
    if cmd != 0 {
        execute(hwnd, cmd);
    }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let mut m = Mb::new();
    m.item("打开 RTM", k::M_REFRESH, false);
    m.item("切换主题", k::M_THEME, false);
    m.item("退出", k::M_EXIT, false);
    let cmd = m.show(hwnd);
    m.destroy();
    if cmd == k::M_REFRESH {
        let a = app();
        ShowWindow(a.hwnd, k::SW_RESTORE);
        SetForegroundWindow(a.hwnd);
    } else if cmd != 0 {
        execute(hwnd, cmd);
    }
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

unsafe fn execute(hwnd: HWND, cmd: u32) {
    match cmd {
        k::ID_BTN_RUN | k::M_RUN => {
            if let Some(c) = prompt(hwnd, "运行新任务", "输入程序名、命令或完整路径：", "") {
                if !actions::run_task(&c) {
                    let msg = wide("无法启动该程序，请检查路径或权限。");
                    let cap = wide("运行新任务");
                    MessageBoxW(hwnd, msg.as_ptr(), cap.as_ptr(), k::MB_ICONERROR);
                }
            }
            return;
        }
        k::ID_BTN_MORE => {
            show_more_menu(hwnd);
            return;
        }
        k::ID_BTN_MODE | k::M_MODE => {
            let a = app();
            let m = if a.cfg.mode == UiMode::Compact { UiMode::Fluent } else { UiMode::Compact };
            set_mode(m);
            return;
        }
        k::ID_BTN_THEME | k::M_THEME => {
            let a = app();
            a.cfg.theme = a.cfg.theme.next();
            let p = pal(&a.cfg);
            if !a.bg.is_null() { DeleteObject(a.bg as HGDIOBJ); }
            if !a.panel.is_null() { DeleteObject(a.panel as HGDIOBJ); }
            if !a.accent.is_null() { DeleteObject(a.accent as HGDIOBJ); }
            if !a.tab_sel.is_null() { DeleteObject(a.tab_sel as HGDIOBJ); }
            if !a.border.is_null() { DeleteObject(a.border as HGDIOBJ); }
            a.bg = CreateSolidBrush(p.bg);
            a.panel = CreateSolidBrush(p.panel);
            a.accent = CreateSolidBrush(p.accent);
            a.tab_sel = CreateSolidBrush(p.sel);
            a.border = CreateSolidBrush(p.border);
            sync_config();
            apply_theme();
            return;
        }
        k::ID_BTN_TREE | k::M_TREE => {
            let a = app();
            a.cfg.show_tree = !a.cfg.show_tree;
            if a.cfg.show_tree {
                a.cfg.group = false;
            }
            sync_config();
            rebuild_table(false);
            return;
        }
        k::M_GROUP => {
            let a = app();
            a.cfg.group = !a.cfg.group;
            if a.cfg.group {
                a.cfg.show_tree = false;
            }
            sync_config();
            rebuild_table(true);
            InvalidateRect(a.hwnd, ptr::null(), 0);
            return;
        }
        k::M_TREE_EXPAND => {
            let a = app();
            a.collapsed.clear();
            rebuild_table(false);
            return;
        }
        k::M_TREE_COLLAPSE => {
            let a = app();
            let mut parents = std::collections::HashSet::new();
            for p in &a.snap.procs {
                if p.ppid != 0 {
                    parents.insert(p.ppid);
                }
            }
            a.collapsed = parents;
            rebuild_table(false);
            return;
        }
        k::ID_BTN_INTERVAL => {
            show_interval_menu(hwnd);
            return;
        }
        k::ID_BTN_EXPORT | k::M_EXPORT_CSV => {
            let a = app();
            match actions::export_table(a.page, &a.table, false) {
                Ok(path) => show_info(hwnd, "导出完成", &format!("已导出 {} 行到:\n{}", a.table.len(), path)),
                Err(e) => show_info(hwnd, "导出失败", &e),
            }
            return;
        }
        k::ID_BTN_END | k::M_END => {
            let a = app();
            let row = selected_row();
            let pid = a.table.pid_at(row);
            if pid > 0 {
                let name = a.table.cell(row, 0).to_string();
                if confirm(hwnd, "结束任务", &format!("确定要结束 {} (PID {}) 吗？未保存的数据可能丢失。", name.trim(), pid)) {
                    actions::kill(pid);
                }
            }
            return;
        }
        _ => {}
    }

    if cmd >= k::M_INTERVAL_BASE && cmd < k::M_INTERVAL_BASE + 8 {
        let ms = match cmd - k::M_INTERVAL_BASE {
            0 => 500,
            1 => 1000,
            2 => 2000,
            3 => 4000,
            _ => 1000,
        };
        let a = app();
        a.cfg.interval_ms = ms;
        a.interval.store(ms, Ordering::Relaxed);
        a.paused.store(false, Ordering::Relaxed);
        sync_config();
        return;
    }
    if cmd >= k::M_COL_BASE && cmd < k::M_COL_BASE + 64 {
        let a = app();
        let i = (cmd - k::M_COL_BASE) as usize;
        let visible_count = a.table.visible_cols().len();
        if let Some(c) = a.table.cols.get_mut(i) {
            if c.visible && visible_count <= 1 {
                return;
            }
            c.visible = !c.visible;
        }
        let hidden: Vec<usize> = a
            .table
            .cols
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.visible)
            .map(|(i, _)| i)
            .collect();
        a.cfg.cols_hidden.insert(a.page.label().to_string(), hidden);
        a.cols_dirty = true;
        sync_config();
        rebuild_table(true);
        return;
    }
    if cmd >= k::M_PRIORITY_BASE && cmd < k::M_PRIORITY_BASE + 8 {
        let a = app();
        let pid = a.table.pid_at(a.ctx_row);
        let level = cmd - k::M_PRIORITY_BASE;
        if pid > 0 && !actions::set_priority(pid, level) {
            show_info(hwnd, "操作失败", "无法修改优先级：权限不足或进程已退出。");
        }
        return;
    }

    let a = app();
    let row = a.ctx_row;
    let pid = a.table.pid_at(row);
    let key = a.table.key_at(row);
    match cmd {
        k::M_END_TREE => {
            if pid > 0 && confirm(hwnd, "结束进程树", &format!("确定结束 {} 及其所有子进程吗？", key)) {
                let n = actions::kill_tree(&a.snap, pid);
                show_info(hwnd, "结束进程树", &format!("已结束 {} 个进程。", n));
            }
        }
        k::M_EFFICIENCY => {
            if pid > 0 {
                actions::efficiency_mode(pid);
            }
        }
        k::M_RESTART => {
            if let Some(p) = a.snap.by_pid(pid) {
                let path = p.path.clone();
                let cmdline = p.cmdline.clone();
                if !path.is_empty() && !path.starts_with('[') {
                    actions::kill(pid);
                    if !actions::run_task(if cmdline.is_empty() { &path } else { &cmdline }) {
                        let _ = actions::run_task(&path);
                    }
                }
            }
        }
        k::M_OPEN_PATH | k::M_STARTUP_LOCATE => {
            let path = if cmd == k::M_STARTUP_LOCATE {
                a.table.cell(row, 4).to_string()
            } else {
                a.snap.by_pid(pid).map(|p| p.path.clone()).unwrap_or_default()
            };
            if !path.is_empty() {
                actions::reveal_file(path.trim_matches('"'));
            }
        }
        k::M_PROPERTIES => {
            if let Some(p) = a.snap.by_pid(pid) {
                let text = format!(
                    "名称: {}\nPID: {}    父 PID: {}\n路径: {}\n命令行: {}\n用户: {}\n会话: {}\n启动时间: {}\n签名: {}\n架构: {}\nCPU: {}    内存: {}\n线程: {}    句柄: {}\n磁盘读/写: {} / {}",
                    p.name, p.pid, p.ppid, p.path, p.cmdline, p.user, p.session_id,
                    collect::file_time_string(p.create_time), p.signed.label(), p.arch,
                    fmt_pct(p.cpu), fmt_bytes(p.mem), p.threads, p.handles,
                    fmt_rate(p.disk_read_rate), fmt_rate(p.disk_write_rate)
                );
                show_info(hwnd, "进程属性", &text);
            }
        }
        k::M_COPY => {
            let vis = a.table.visible_cols();
            let line = vis.iter().map(|&i| a.table.cell(row, i)).collect::<Vec<_>>().join("\t");
            actions::copy_to_clipboard(&line);
        }
        k::M_EXPORT_JSON => {
            match actions::export_table(a.page, &a.table, true) {
                Ok(path) => show_info(hwnd, "导出完成", &format!("已导出 {} 行到:\n{}", a.table.len(), path)),
                Err(e) => show_info(hwnd, "导出失败", &e),
            }
        }
        k::M_GOTO_DETAILS => {
            if pid > 0 {
                let a = app();
                a.cfg.search = pid.to_string();
                a.search_placeholder = false;
                SetWindowTextW(a.h_search, wide(&a.cfg.search).as_ptr());
                switch_page(Page::Details.index());
            }
        }
        k::M_SVC_START | k::M_SVC_STOP | k::M_SVC_RESTART => {
            let start = cmd == k::M_SVC_START;
            let ok = actions::service_control(&key, start);
            if cmd == k::M_SVC_RESTART {
                actions::service_control(&key, false);
                actions::service_control(&key, true);
            }
            if !ok && cmd != k::M_SVC_RESTART {
                show_info(hwnd, "服务操作失败", "需要管理员权限，或服务不接受该控制。\n可右键选择『以管理员身份运行 RTM』后重试。");
            }
        }
        k::M_STARTUP_ENABLE | k::M_STARTUP_DISABLE => {
            let item = a.snap.startups.iter().find(|s| s.name == key).cloned();
            if let Some(it) = item {
                if !actions::startup_set_enabled(&it, cmd == k::M_STARTUP_ENABLE) {
                    show_info(hwnd, "修改失败", "无法写入 StartupApproved 注册表项。");
                }
            }
        }
        k::M_CLEAR_HISTORY => {
            // Session history lives in the collector thread; request a reset.
            show_info(hwnd, "应用历史", "历史记录为本程序会话内累计数据；重启 RTM 即可清空。");
        }
        k::M_REFRESH => {
            a.paused.store(false, Ordering::Relaxed);
            rebuild_table(false);
            InvalidateRect(a.h_chart, ptr::null(), 0);
        }
        k::M_PAUSE => {
            let v = a.paused.load(Ordering::Relaxed);
            a.paused.store(!v, Ordering::Relaxed);
        }
        k::M_TRAY => {
            a.cfg.tray = !a.cfg.tray;
            if a.cfg.tray { add_tray(); } else { remove_tray(); }
            sync_config();
        }
        k::M_LOWPOWER => {
            a.cfg.low_power = !a.cfg.low_power;
            a.interval.store(if a.cfg.low_power { 2000 } else { a.cfg.interval_ms }, Ordering::Relaxed);
            sync_config();
        }
        k::M_GPU => {
            a.cfg.gpu = !a.cfg.gpu;
            sync_config();
            show_info(hwnd, "GPU 监控",
                if a.cfg.gpu {
                    "已启用。首次启用时 PDH 需要枚举 GPU 引擎实例，\n后台线程会额外耗时约 0.4 秒，界面不会卡顿。"
                } else {
                    "已关闭。GPU 列显示 0%，可避免 PDH 初始化开销。"
                });
        }
        k::M_HEAT => {
            a.cfg.heatmap = !a.cfg.heatmap;
            sync_config();
            InvalidateRect(a.h_list, ptr::null(), 0);
        }
        k::M_REGEX => {
            a.cfg.use_regex = !a.cfg.use_regex;
            sync_config();
            rebuild_table(false);
        }
        k::M_ABOUT => {
            show_info(hwnd, "关于 RTM", "Rust 任务管理器 1.0\n\n原生 Win32 / Rust，零运行时依赖。\n数据源: NtQuerySystemInformation / PDH / SCM / 注册表 / WTS / IP Helper。\n\n增强: 进程树、命令行、签名检测、细粒度 IO、批量操作、正则搜索、导出、热力图、双模式界面、托盘与全局快捷键。\n\nCtrl+Alt+T 显示/隐藏窗口；Ctrl+F 聚焦搜索；F5 刷新；Delete 结束任务。");
        }
        k::M_EXIT => {
            DestroyWindow(a.hwnd);
        }
        _ => {}
    }
}

unsafe fn show_interval_menu(hwnd: HWND) {
    let mut m = Mb::new();
    for (i, n) in ["0.5 秒", "1 秒", "2 秒", "4 秒"].iter().enumerate() {
        m.item(n, k::M_INTERVAL_BASE + i as u32, false);
    }
    m.sep();
    m.item("暂停刷新", k::M_PAUSE, false);
    let cmd = m.show(hwnd);
    m.destroy();
    if cmd != 0 {
        execute(hwnd, cmd);
    }
}

unsafe fn confirm(hwnd: HWND, title: &str, text: &str) -> bool {
    let t = wide(title);
    let x = wide(text);
    MessageBoxW(hwnd, x.as_ptr(), t.as_ptr(), k::MB_YESNO | k::MB_ICONINFORMATION) == k::IDYES
}

unsafe fn show_info(hwnd: HWND, title: &str, text: &str) {
    let t = wide(title);
    let x = wide(text);
    MessageBoxW(hwnd, x.as_ptr(), t.as_ptr(), k::MB_OK | k::MB_ICONINFORMATION);
}

// ---------------------------------------------------------------------------
// Tray icon
// ---------------------------------------------------------------------------

unsafe fn add_tray() {
    let a = app();
    let mut nid: NotifyIconDataW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NotifyIconDataW>() as u32;
    nid.hWnd = a.hwnd;
    nid.uID = 1;
    nid.uFlags = k::NIF_MESSAGE | k::NIF_ICON | k::NIF_TIP;
    nid.uCallbackMessage = k::WM_APP_TRAY;
    nid.hIcon = a.h_icon;
    let tip = wide("Rust 任务管理器 (Ctrl+Alt+T)");
    let n = tip.len().min(127);
    nid.szTip[..n].copy_from_slice(&tip[..n]);
    if Shell_NotifyIconW(k::NIM_ADD, &mut nid) != 0 {
        a.tray = true;
    }
}

unsafe fn remove_tray() {
    let a = app();
    if !a.tray {
        return;
    }
    let mut nid: NotifyIconDataW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NotifyIconDataW>() as u32;
    nid.hWnd = a.hwnd;
    nid.uID = 1;
    Shell_NotifyIconW(k::NIM_DELETE, &mut nid);
    a.tray = false;
}

// ---------------------------------------------------------------------------
// Lightweight modal input dialog
// ---------------------------------------------------------------------------

unsafe extern "system" fn prompt_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        k::WM_COMMAND => {
            let id = (w & 0xFFFF) as u32;
            if id == 1 || id == 2 {
                if id == 1 {
                    let ed = GetDlgItem(hwnd, 3);
                    let n = GetWindowTextLengthW(ed);
                    let mut buf = vec![0u16; (n + 2).max(2) as usize];
                    GetWindowTextW(ed, buf.as_mut_ptr(), (n + 2) as i32);
                    PROMPT_RESULT = Some(from_wide(buf.as_ptr()));
                } else {
                    PROMPT_RESULT = None;
                }
                PROMPT_DONE = true;
                DestroyWindow(hwnd);
            }
            0
        }
        k::WM_CLOSE => {
            PROMPT_RESULT = None;
            PROMPT_DONE = true;
            DestroyWindow(hwnd);
            0
        }
        k::WM_ERASEBKGND => 1,
        k::WM_PAINT => {
            let mut ps = PaintStruct::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc);
            let br = CreateSolidBrush(rgb(255, 255, 255));
            FillRect(hdc, &rc as *const RECT, br);
            DeleteObject(br as HGDIOBJ);
            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

unsafe fn prompt(parent: HWND, title: &str, caption: &str, default: &str) -> Option<String> {
    let hinst = GetModuleHandleW(ptr::null());
    if !PROMPT_CLASS {
        let name = wide("RtmPrompt");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: prompt_proc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: ptr::null_mut(),
            hCursor: LoadCursorW(ptr::null_mut(), k::IDC_ARROW),
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: name.as_ptr(),
            hIconSm: ptr::null_mut(),
        };
        RegisterClassExW(&wc);
        PROMPT_CLASS = true;
    }
    let mut pr = RECT::default();
    GetWindowRect(parent, &mut pr);
    let pw = 420;
    let ph = 160;
    let x = pr.left + ((pr.right - pr.left) - pw) / 2;
    let y = pr.top + ((pr.bottom - pr.top) - ph) / 2;
    let cls = wide("RtmPrompt");
    let t = wide(title);
    let dlg = CreateWindowExW(
        0, cls.as_ptr(), t.as_ptr(),
        k::WS_POPUP | k::WS_CAPTION | k::WS_SYSMENU | k::WS_VISIBLE,
        x, y, pw, ph, parent, ptr::null_mut(), hinst, ptr::null_mut(),
    );
    if dlg.is_null() {
        return None;
    }
    let fnt = font(12, k::FW_NORMAL);
    let stat = wide("Static");
    let edit = wide("Edit");
    let btn = wide("Button");
    let cap = wide(caption);
    let s1 = CreateWindowExW(0, stat.as_ptr(), cap.as_ptr(), k::WS_CHILD | k::WS_VISIBLE, 16, 16, 380, 22, dlg, 10usize as HMENU, hinst, ptr::null_mut());
    SendMessageW(s1, k::WM_SETFONT, fnt as WPARAM, 1);
    let ed = CreateWindowExW(k::WS_EX_CLIENTEDGE, edit.as_ptr(), wide(default).as_ptr(), k::WS_CHILD | k::WS_VISIBLE | k::WS_TABSTOP | k::ES_AUTOHSCROLL, 16, 44, 380, 26, dlg, 3usize as HMENU, hinst, ptr::null_mut());
    SendMessageW(ed, k::WM_SETFONT, fnt as WPARAM, 1);
    let ok = wide("确定");
    let cancel = wide("取消");
    let b1 = CreateWindowExW(0, btn.as_ptr(), ok.as_ptr(), k::WS_CHILD | k::WS_VISIBLE | k::BS_DEFPUSHBUTTON, 226, 86, 82, 28, dlg, 1usize as HMENU, hinst, ptr::null_mut());
    let b2 = CreateWindowExW(0, btn.as_ptr(), cancel.as_ptr(), k::WS_CHILD | k::WS_VISIBLE, 316, 86, 82, 28, dlg, 2usize as HMENU, hinst, ptr::null_mut());
    SendMessageW(b1, k::WM_SETFONT, fnt as WPARAM, 1);
    SendMessageW(b2, k::WM_SETFONT, fnt as WPARAM, 1);
    SetFocus(ed);
    EnableWindow(parent, 0);
    PROMPT_DONE = false;
    PROMPT_RESULT = None;
    let mut msg: MSG = std::mem::zeroed();
    while !PROMPT_DONE {
        if GetMessageW(&mut msg, ptr::null_mut(), 0, 0) <= 0 {
            break;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    EnableWindow(parent, 1);
    SetForegroundWindow(parent);
    DeleteObject(fnt as HGDIOBJ);
    PROMPT_RESULT.take()
}
