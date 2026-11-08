//! Low-level Win32 constants, native structures that `windows-sys` does not
//! expose, UTF-16 helpers and a small dynamic-import loader.
//!
//! Numeric constants live in the `k` module so they never collide with the
//! generated bindings' identically named constants.

#![allow(dead_code, non_snake_case, non_camel_case_types, non_upper_case_globals)]

use std::ffi::{c_void, CString};

pub type HANDLE = *mut c_void;
pub type HWND = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type HMODULE = *mut c_void;
pub type HMENU = *mut c_void;
pub type HDC = *mut c_void;
pub type HFONT = *mut c_void;
pub type HBRUSH = *mut c_void;
pub type HBITMAP = *mut c_void;
pub type HGDIOBJ = *mut c_void;
pub type HICON = *mut c_void;
pub type HCURSOR = *mut c_void;
pub type HKEY = *mut c_void;
pub type SC_HANDLE = *mut c_void;
pub type BOOL = i32;
pub type DWORD = u32;
pub type ULONG = u32;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type COLORREF = u32;
pub type PCWSTR = *const u16;
pub type PWSTR = *mut u16;
pub type PCSTR = *const u8;
pub type LPVOID = *mut c_void;
pub type LPCVOID = *const c_void;
pub type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

pub const TRUE: i32 = 1;
pub const FALSE: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FILETIME {
    pub dwLowDateTime: u32,
    pub dwHighDateTime: u32,
}
impl FILETIME {
    pub fn as_u64(&self) -> u64 {
        ((self.dwHighDateTime as u64) << 32) | self.dwLowDateTime as u64
    }
}
pub fn ft_from_u64(v: u64) -> FILETIME {
    FILETIME { dwLowDateTime: v as u32, dwHighDateTime: (v >> 32) as u32 }
}

// ---------------------------------------------------------------------------
// Numeric Win32 constants (stable ABI values)
// ---------------------------------------------------------------------------
pub mod k {
    // Window messages
    pub const WM_CREATE: u32 = 0x0001;
    pub const WM_DESTROY: u32 = 0x0002;
    pub const WM_SIZE: u32 = 0x0005;
    pub const WM_SETFOCUS: u32 = 0x0007;
    pub const WM_PAINT: u32 = 0x000F;
    pub const WM_CLOSE: u32 = 0x0010;
    pub const WM_ERASEBKGND: u32 = 0x0014;
    pub const WM_SETFONT: u32 = 0x0030;
    pub const WM_GETMINMAXINFO: u32 = 0x0024;
    pub const WM_NOTIFY: u32 = 0x004E;
    pub const WM_COMMAND: u32 = 0x0111;
    pub const WM_TIMER: u32 = 0x0113;
    pub const WM_HSCROLL: u32 = 0x0114;
    pub const WM_VSCROLL: u32 = 0x0115;
    pub const WM_CTLCOLORSTATIC: u32 = 0x0138;
    pub const WM_CTLCOLOREDIT: u32 = 0x0133;
    pub const WM_KEYDOWN: u32 = 0x0100;
    pub const WM_CHAR: u32 = 0x0102;
    pub const WM_LBUTTONDOWN: u32 = 0x0201;
    pub const WM_LBUTTONUP: u32 = 0x0202;
    pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
    pub const WM_RBUTTONUP: u32 = 0x0205;
    pub const WM_MOUSEMOVE: u32 = 0x0200;
    pub const WM_MOUSEWHEEL: u32 = 0x020A;
    pub const WM_APP: u32 = 0x8000;

    // Window styles
    pub const WS_OVERLAPPED: u32 = 0x0000_0000;
    pub const WS_POPUP: u32 = 0x8000_0000;
    pub const WS_CHILD: u32 = 0x4000_0000;
    pub const WS_MINIMIZE: u32 = 0x2000_0000;
    pub const WS_VISIBLE: u32 = 0x1000_0000;
    pub const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
    pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
    pub const WS_MAXIMIZE: u32 = 0x0100_0000;
    pub const WS_CAPTION: u32 = 0x00C0_0000;
    pub const WS_BORDER: u32 = 0x0080_0000;
    pub const WS_DLGFRAME: u32 = 0x0040_0000;
    pub const WS_VSCROLL: u32 = 0x0020_0000;
    pub const WS_HSCROLL: u32 = 0x0010_0000;
    pub const WS_SYSMENU: u32 = 0x0008_0000;
    pub const WS_THICKFRAME: u32 = 0x0004_0000;
    pub const WS_MINIMIZEBOX: u32 = 0x0002_0000;
    pub const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
    pub const WS_TABSTOP: u32 = 0x0001_0000;
    pub const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
    pub const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
    pub const WS_EX_COMPOSITED: u32 = 0x0200_0000;
    pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;

    // ShowWindow
    pub const SW_HIDE: i32 = 0;
    pub const SW_SHOW: i32 = 5;
    pub const SW_RESTORE: i32 = 9;

    // Edit control messages
    pub const EM_SETSEL: u32 = 0x00B1;
    pub const EM_GETSEL: u32 = 0x00B0;
    pub const WM_GETTEXT: u32 = 0x000D;
    pub const WM_SETTEXT: u32 = 0x000C;
    pub const WM_GETTEXTLENGTH: u32 = 0x000E;
    pub const EN_CHANGE: u32 = 0x0300;
    pub const EN_SETFOCUS: u32 = 0x0100;
    pub const EN_KILLFOCUS: u32 = 0x0200;

    // ListView
    pub const LVM_FIRST: u32 = 0x1000;
    pub const LVM_SETBKCOLOR: u32 = LVM_FIRST + 1;
    pub const LVM_SETTEXTCOLOR: u32 = LVM_FIRST + 2;
    pub const LVM_SETTEXTBKCOLOR: u32 = LVM_FIRST + 3;
    pub const LVM_GETITEMCOUNT: u32 = LVM_FIRST + 4;
    pub const LVM_DELETEALLITEMS: u32 = LVM_FIRST + 9;
    pub const LVM_GETNEXTITEM: u32 = LVM_FIRST + 12;
    pub const LVM_GETITEMRECT: u32 = LVM_FIRST + 14;
    pub const LVM_ENSUREVISIBLE: u32 = LVM_FIRST + 19;
    pub const LVM_SETCOLUMNWIDTH: u32 = LVM_FIRST + 30;
    pub const LVM_GETCOLUMNWIDTH: u32 = LVM_FIRST + 29;
    pub const LVM_SETITEMSTATE: u32 = LVM_FIRST + 43;
    pub const LVM_GETITEMSTATE: u32 = LVM_FIRST + 44;
    pub const LVM_SETITEMCOUNT: u32 = LVM_FIRST + 47;
    pub const LVM_SORTITEMS: u32 = LVM_FIRST + 48;
    pub const LVM_GETSELECTEDCOUNT: u32 = LVM_FIRST + 50;
    pub const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = LVM_FIRST + 54;
    pub const LVM_GETEXTENDEDLISTVIEWSTYLE: u32 = LVM_FIRST + 55;
    pub const LVM_INSERTCOLUMNW: u32 = LVM_FIRST + 97;
    pub const LVM_GETHEADER: u32 = LVM_FIRST + 31;
    pub const LVM_DELETECOLUMN: u32 = LVM_FIRST + 28;
    pub const ES_AUTOHSCROLL: u32 = 0x0080;
    pub const EM_SETCUEBANNER: u32 = 0x1501;
    pub const BS_DEFPUSHBUTTON: u32 = 0x0001;
    pub const CS_HREDRAW: u32 = 0x0002;
    pub const CS_VREDRAW: u32 = 0x0001;
    pub const ICC_LISTVIEW_CLASSES: u32 = 0x0001;
    pub const ICC_BAR_CLASSES: u32 = 0x0004;
    pub const COLOR_WINDOW: i32 = 5;
    pub const ES_LEFT: u32 = 0x0000;

    // Owner-draw menus and header subclassing
    pub const WM_DRAWITEM: u32 = 0x002B;
    pub const WM_MEASUREITEM: u32 = 0x002C;
    pub const HDM_FIRST: u32 = 0x1200;
    pub const HDM_GETITEMCOUNT: u32 = HDM_FIRST;
    pub const HDM_GETITEMW: u32 = HDM_FIRST + 11;
    pub const HDM_GETITEMRECT: u32 = HDM_FIRST + 7;
    pub const HDI_TEXT: u32 = 0x0002;
    pub const MIIM_STATE: u32 = 0x0000_0001;
    pub const MIIM_ID: u32 = 0x0000_0002;
    pub const MIIM_DATA: u32 = 0x0000_0020;
    pub const MIIM_FTYPE: u32 = 0x0000_0100;
    pub const MIIM_SUBMENU: u32 = 0x0000_0004;
    pub const MIM_BACKGROUND: u32 = 0x0000_0002;
    pub const MFT_OWNERDRAW: u32 = 0x0000_0100;
    pub const MFS_CHECKED: u32 = 0x0000_0008;
    pub const MFS_DISABLED: u32 = 0x0000_0003;
    pub const ODS_SELECTED: u32 = 0x0000_0001;
    pub const ODS_DISABLED: u32 = 0x0000_0004;
    pub const ODS_CHECKED: u32 = 0x0000_0008;
    pub const M_TREE_EXPAND: u32 = 436;
    pub const M_TREE_COLLAPSE: u32 = 437;
    pub const M_STARTUP_REFRESH: u32 = 438;
    pub const SHGFI_ICON: u32 = 0x0000_0100;
    pub const ILC_COLOR32: u32 = 0x0000_0020;
    pub const ILC_MASK: u32 = 0x0000_0001;
    pub const SHGFI_SMALLICON: u32 = 0x0000_0001;
    pub const SHGFI_USEFILEATTRIBUTES: u32 = 0x0000_0010;
    pub const I_IMAGENONE: i32 = -2;
    pub const LVM_SETIMAGELIST: u32 = LVM_FIRST + 3;
    pub const LVIF_IMAGE_FLAG: u32 = 0x0002;
    pub const LVM_SETITEM: u32 = LVM_FIRST + 76;
    pub const LVM_GETTOPINDEX: u32 = LVM_FIRST + 39;
    pub const LVM_GETORIGIN: u32 = LVM_FIRST + 41;
    pub const LVM_GETCOUNTPERPAGE: u32 = LVM_FIRST + 40;
    pub const LVM_REDRAWITEMS: u32 = LVM_FIRST + 21;

    pub const LVS_ICON: u32 = 0x0000;
    pub const LVS_REPORT: u32 = 0x0001;
    pub const LVS_SINGLESEL: u32 = 0x0004;
    pub const LVS_SHOWSELALWAYS: u32 = 0x0008;
    pub const LVS_SORTASCENDING: u32 = 0x0010;
    pub const LVS_ALIGNTOP: u32 = 0x0000;
    pub const LVS_OWNERDATA: u32 = 0x1000;
    pub const LVS_NOSORTHEADER: u32 = 0x8000;
    pub const LVS_NOCOLUMNHEADER: u32 = 0x4000;
    pub const LVS_EX_GRIDLINES: u32 = 0x0000_0001;
    pub const LVS_EX_SUBITEMIMAGES: u32 = 0x0000_0002;
    pub const LVS_EX_CHECKBOXES: u32 = 0x0000_0004;
    pub const LVS_EX_FULLROWSELECT: u32 = 0x0000_0020;
    pub const LVS_EX_HEADERDRAGDROP: u32 = 0x0000_0010;
    pub const LVS_EX_DOUBLEBUFFER: u32 = 0x0001_0000;
    pub const LVS_EX_INFOTIP: u32 = 0x0000_0400;
    pub const LVS_EX_LABELTIP: u32 = 0x0000_4000;

    pub const LVIF_TEXT: u32 = 0x0001;
    pub const LVIF_IMAGE: u32 = 0x0002;
    pub const LVIF_PARAM: u32 = 0x0004;
    pub const LVIF_STATE: u32 = 0x0008;
    pub const LVIF_INDENT: u32 = 0x0010;
    pub const LVCF_FMT: u32 = 0x0001;
    pub const LVCF_WIDTH: u32 = 0x0002;
    pub const LVCF_TEXT: u32 = 0x0004;
    pub const LVCF_SUBITEM: u32 = 0x0008;
    pub const LVCFMT_LEFT: i32 = 0;
    pub const LVCFMT_RIGHT: i32 = 1;
    pub const LVCFMT_CENTER: i32 = 2;
    pub const LVNI_SELECTED: usize = 0x0002;
    pub const LVNI_ALL: usize = 0x0000;
    pub const LVIS_SELECTED: u32 = 0x0002;
    pub const LVIS_FOCUSED: u32 = 0x0001;
    pub const LVSIL_SMALL: u32 = 1;
    pub const LVSIL_STATE: u32 = 2;
    pub const CLR_NONE: u32 = 0xFFFF_FFFF;
    pub const CLR_DEFAULT: u32 = 0xFF00_0000;

    // Notifications (negative codes stored as two's complement in NMHDR.code)
    pub const NM_FIRST: u32 = 0;
    pub const NM_CLICK: u32 = 0xFFFF_FFFE;
    pub const NM_DBLCLK: u32 = 0xFFFF_FFFD; // -2
    pub const NM_RCLICK: u32 = 0xFFFF_FFFB; // -5
    pub const NM_CUSTOMDRAW: u32 = 0xFFFF_FFF4; // -12
    pub const LVN_FIRST: u32 = 0;
    pub const LVN_ITEMCHANGED: u32 = 0xFFFF_FF9B; // -101
    pub const LVN_COLUMNCLICK: u32 = 0xFFFF_FF94; // -108
    pub const LVN_GETDISPINFOW: u32 = 0xFFFF_FF4F; // -177
    pub const LVN_ODFINDITEMW: u32 = 0xFFFF_FF4D; // -179
    pub const LVN_KEYDOWN: u32 = 0xFFFF_FFA5; // -155
    pub const CDDS_PREPAINT: u32 = 0x0000_0001;
    pub const CDDS_ITEMPREPAINT: u32 = 0x0001_0001;
    pub const CDDS_SUBITEM: u32 = 0x0002_0000;
    pub const CDDS_ITEM: u32 = 0x0001_0000;
    pub const CDDS_POSTPAINT: u32 = 0x0000_0002;
    pub const CDRF_NOTIFYPOSTPAINT: usize = 0x0000_0010;
    pub const CDRF_SKIPDEFAULT: usize = 0x0000_0004;
    pub const ILD_NORMAL: u32 = 0x0000_0000;
    pub const CDIS_SELECTED: u32 = 0x0001;
    pub const CDRF_DODEFAULT: usize = 0x0000_0000;
    pub const CDRF_NOTIFYITEMDRAW: usize = 0x0000_0020;
    pub const CDRF_NOTIFYSUBITEMDRAW: usize = 0x0000_0020;
    pub const CDRF_NEWFONT: usize = 0x0000_0002;

    // GDI / text
    pub const DT_LEFT: u32 = 0x0000;
    pub const DT_CENTER: u32 = 0x0001;
    pub const DT_RIGHT: u32 = 0x0002;
    pub const DT_VCENTER: u32 = 0x0004;
    pub const DT_SINGLELINE: u32 = 0x0020;
    pub const DT_END_ELLIPSIS: u32 = 0x8000;
    pub const DT_NOPREFIX: u32 = 0x0800;
    pub const TRANSPARENT: i32 = 1;
    pub const OPAQUE: i32 = 2;
    pub const PS_SOLID: i32 = 0;
    pub const SRCCOPY: u32 = 0x00CC_0020;
    pub const FW_NORMAL: i32 = 400;
    pub const FW_BOLD: i32 = 700;
    pub const DEFAULT_CHARSET: u8 = 1;
    pub const OUT_DEFAULT_PRECIS: u8 = 0;
    pub const CLIP_DEFAULT_PRECIS: u8 = 0;
    pub const CLEARTYPE_QUALITY: u8 = 5;
    pub const DEFAULT_PITCH: u8 = 0;
    pub const FF_DONTCARE: u8 = 0;
    pub const WHITE_BRUSH: i32 = 0;
    pub const DC_BRUSH: i32 = 18;
    pub const GWLP_USERDATA: i32 = -21;
    pub const GWLP_WNDPROC: i32 = -4;

    // Menu / track popup
    pub const MF_STRING: u32 = 0x0000;
    pub const MF_SEPARATOR: u32 = 0x0800;
    pub const MF_POPUP: u32 = 0x0010;
    pub const MF_CHECKED: u32 = 0x0008;
    pub const MF_UNCHECKED: u32 = 0x0000;
    pub const MF_DISABLED: u32 = 0x0002;
    pub const MF_GRAYED: u32 = 0x0001;
    pub const MF_BYCOMMAND: u32 = 0x0000;
    pub const TPM_LEFTBUTTON: u32 = 0x0000;
    pub const TPM_RIGHTBUTTON: u32 = 0x0002;
    pub const TPM_RETURNCMD: u32 = 0x0100;
    pub const TPM_LEFTALIGN: u32 = 0x0000;
    pub const TPM_TOPALIGN: u32 = 0x0000;

    // Tray / hotkey
    pub const NIM_ADD: u32 = 0;
    pub const NIM_MODIFY: u32 = 1;
    pub const NIM_DELETE: u32 = 2;
    pub const NIF_MESSAGE: u32 = 0x0001;
    pub const NIF_ICON: u32 = 0x0002;
    pub const NIF_TIP: u32 = 0x0004;
    pub const NIF_INFO: u32 = 0x0010;
    pub const MOD_ALT: u32 = 0x0001;
    pub const MOD_CONTROL: u32 = 0x0002;
    pub const MOD_SHIFT: u32 = 0x0004;
    pub const MOD_NOREPEAT: u32 = 0x4000;
    pub const WM_HOTKEY: u32 = 0x0312;
    pub const IDI_APPLICATION: *const u16 = 32512 as *const u16;
    pub const IDC_ARROW: *const u16 = 32512 as *const u16;
    pub const IDC_HAND: *const u16 = 32649 as *const u16;
    pub const IMAGE_ICON: u32 = 1;
    pub const LR_SHARED: u32 = 0x0000_8000;
    pub const LR_DEFAULTSIZE: u32 = 0x0000_0040;
    pub const SM_CXSMICON: i32 = 49;
    pub const SM_CYSMICON: i32 = 50;

    // Registry
    pub const HKEY_CLASSES_ROOT: isize = 0x8000_0000;
    pub const HKEY_CURRENT_USER: isize = 0x8000_0001;
    pub const HKEY_LOCAL_MACHINE: isize = 0x8000_0002;
    pub const HKEY_USERS: isize = 0x8000_0003;
    pub const KEY_READ: u32 = 0x0002_0019;
    pub const KEY_WRITE: u32 = 0x0002_0006;
    pub const KEY_ALL_ACCESS: u32 = 0x000F_003F;
    pub const KEY_WOW64_64KEY: u32 = 0x0100;
    pub const KEY_WOW64_32KEY: u32 = 0x0200;
    pub const REG_SZ: u32 = 1;
    pub const REG_EXPAND_SZ: u32 = 2;
    pub const REG_BINARY: u32 = 3;
    pub const REG_DWORD: u32 = 4;
    pub const ERROR_SUCCESS: i32 = 0;
    pub const ERROR_MORE_DATA: i32 = 234;
    pub const ERROR_NO_MORE_ITEMS: i32 = 259;

    // Process / thread
    pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    pub const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
    pub const PROCESS_VM_READ: u32 = 0x0010;
    pub const PROCESS_TERMINATE: u32 = 0x0001;
    pub const PROCESS_SET_INFORMATION: u32 = 0x0200;
    pub const PROCESS_SET_QUOTA: u32 = 0x0100;
    pub const STILL_ACTIVE: u32 = 259;
    pub const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    pub const MAX_PATH: usize = 260;
    pub const MAX_PATH_W: usize = 32768;
    pub const INVALID_HANDLE_VALUE: isize = -1;
    pub const TOKEN_QUERY: u32 = 0x0008;

    // Services
    pub const SC_MANAGER_ALL_ACCESS: u32 = 0x000F_003F;
    pub const SC_MANAGER_ENUMERATE_SERVICE: u32 = 0x0004;
    pub const SERVICE_WIN32: u32 = 0x0000_0030;
    pub const SERVICE_DRIVER: u32 = 0x0000_000B;
    pub const SERVICE_STATE_ALL: u32 = 0x0000_0003;
    pub const SERVICE_QUERY_CONFIG: u32 = 0x0001;
    pub const SERVICE_QUERY_STATUS: u32 = 0x0004;
    pub const SERVICE_START: u32 = 0x0010;
    pub const SERVICE_STOP: u32 = 0x0020;
    pub const SERVICE_CONTROL_STOP: u32 = 0x0001;
    pub const SERVICE_STOPPED: u32 = 0x0001;
    pub const SERVICE_RUNNING: u32 = 0x0004;
    pub const SERVICE_AUTO_START: u32 = 0x0002;
    pub const SERVICE_DEMAND_START: u32 = 0x0003;
    pub const SERVICE_DISABLED: u32 = 0x0004;

    // WTS
    pub const WTS_CURRENT_SERVER_HANDLE: isize = 0;
    pub const WTSUserName: i32 = 5;
    pub const WTSDomainName: i32 = 7;
    pub const WTSConnectState: i32 = 8;
    pub const WTSActive: i32 = 0;
    pub const WTSConnected: i32 = 1;

    // PDH
    pub const PDH_FMT_DOUBLE: u32 = 0x0000_0200;
    pub const PDH_MORE_DATA: u32 = 0x8000_07D2;
    pub const PDH_VALID_DATA: u32 = 0;
    pub const PDH_CSTATUS_VALID_DATA: u32 = 0;

    // Misc
    pub const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
    pub const GENERIC_READ: u32 = 0x8000_0000;
    pub const GENERIC_WRITE: u32 = 0x4000_0000;
    pub const FILE_SHARE_READ: u32 = 0x0000_0001;
    pub const OPEN_EXISTING: u32 = 3;
    pub const MB_OK: u32 = 0;
    pub const MB_ICONINFORMATION: u32 = 0x0040;
    pub const MB_ICONERROR: u32 = 0x0010;
    pub const MB_YESNO: u32 = 0x0004;
    pub const IDYES: i32 = 6;
    pub const GWL_STYLE: i32 = -16;
    pub const SWP_NOZORDER: u32 = 0x0004;
    pub const SWP_NOACTIVATE: u32 = 0x0010;
    pub const SWP_FRAMECHANGED: u32 = 0x0020;
    pub const RDW_INVALIDATE: u32 = 0x0001;
    pub const RDW_ALLCHILDREN: u32 = 0x0080;
    pub const RDW_UPDATENOW: u32 = 0x0100;
    pub const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    pub const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    pub const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    pub const DWMWA_CAPTION_COLOR: u32 = 35;
    pub const DWMWA_MICA_EFFECT: u32 = 1029;
    pub const DT_CALCRECT: u32 = 0x0400;

    // Application-defined messages / control ids
    pub const WM_APP_SNAPSHOT: u32 = WM_APP + 1;
    pub const WM_APP_TRAY: u32 = WM_APP + 2;
    pub const WM_APP_REFRESH: u32 = WM_APP + 3;
    pub const WM_APP_REBUILD: u32 = WM_APP + 10;
    pub const IDT_REFRESH: usize = 1;
    pub const IDT_BOOT: usize = 2;
    pub const ID_HOTKEY: i32 = 1;
    pub const ID_SEARCH: u32 = 100;
    pub const ID_LIST: u32 = 101;
    pub const ID_CHART: u32 = 102;
    pub const ID_PERF_LIST: u32 = 103;
    pub const ID_STATUS: u32 = 104;
    pub const ID_BTN_RUN: u32 = 110;
    pub const ID_BTN_END: u32 = 111;
    pub const ID_BTN_MODE: u32 = 112;
    pub const ID_BTN_THEME: u32 = 113;
    pub const ID_BTN_INTERVAL: u32 = 114;
    pub const ID_BTN_EXPORT: u32 = 115;
    pub const ID_BTN_MORE: u32 = 116;
    pub const ID_BTN_TREE: u32 = 117;

    // Context-menu / toolbar commands
    pub const M_END: u32 = 200;
    pub const M_END_TREE: u32 = 201;
    pub const M_RESTART: u32 = 202;
    pub const M_OPEN_PATH: u32 = 203;
    pub const M_COPY: u32 = 204;
    pub const M_PROPERTIES: u32 = 205;
    pub const M_PRIORITY_BASE: u32 = 210;
    pub const M_AFFINITY: u32 = 230;
    pub const M_EXPORT_CSV: u32 = 240;
    pub const M_EXPORT_JSON: u32 = 241;
    pub const M_SVC_START: u32 = 250;
    pub const M_SVC_STOP: u32 = 251;
    pub const M_SVC_RESTART: u32 = 252;
    pub const M_STARTUP_ENABLE: u32 = 260;
    pub const M_STARTUP_DISABLE: u32 = 261;
    pub const M_STARTUP_LOCATE: u32 = 262;
    pub const M_GOTO_DETAILS: u32 = 270;
    pub const M_GOTO_SERVICES: u32 = 271;
    pub const M_DUMP: u32 = 272;
    pub const M_COL_BASE: u32 = 300;
    pub const M_MODE: u32 = 400;
    pub const M_THEME: u32 = 401;
    pub const M_INTERVAL_BASE: u32 = 410;
    pub const M_TRAY: u32 = 430;
    pub const M_LOWPOWER: u32 = 431;
    pub const M_GPU: u32 = 435;
    pub const M_GROUP: u32 = 439;
    pub const M_TREE: u32 = 432;
    pub const M_HEAT: u32 = 433;
    pub const M_REGEX: u32 = 434;
    pub const M_CLEAR_HISTORY: u32 = 440;
    pub const M_ABOUT: u32 = 450;
    pub const M_EXIT: u32 = 451;
    pub const M_RUN: u32 = 452;
    pub const M_REFRESH: u32 = 453;
    pub const M_PAUSE: u32 = 454;
    pub const M_EFFICIENCY: u32 = 455;
    pub const M_DISCONNECT: u32 = 456;
}

// ---------------------------------------------------------------------------
// UTF-16 helpers
// ---------------------------------------------------------------------------

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn wide_no_null(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Reads a NUL-terminated UTF-16 string.
pub unsafe fn from_wide(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
        if len > 1 << 20 {
            break;
        }
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

/// Reads a UTF-16 string of known length in UTF-16 code units.
pub unsafe fn from_wide_n(p: *const u16, len: usize) -> String {
    if p.is_null() || len == 0 {
        return String::new();
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

pub fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

// ---------------------------------------------------------------------------
// Dynamic imports (for ntdll APIs and optional system DLLs)
// ---------------------------------------------------------------------------

/// Loads a DLL once per process and returns the address of an exported symbol.
/// The returned value is a raw function address; callers transmute it to the
/// correct `extern "system" fn` signature. Never freed: avoids lifetime bugs.
pub unsafe fn dyn_proc(dll: &str, name: &str) -> usize {
    let dll_w = wide(dll);
    let h = native::LoadLibraryW(dll_w.as_ptr());
    if h.is_null() {
        return 0;
    }
    let cname = match CString::new(name) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let p = native::GetProcAddress(h, cname.as_ptr() as *const u8);
    if p.is_null() {
        0
    } else {
        p as usize
    }
}

pub unsafe fn call_fn<T: Copy>(addr: usize) -> Option<T> {
    if addr == 0 {
        return None;
    }
    Some(std::mem::transmute_copy::<usize, T>(&addr))
}

// ---------------------------------------------------------------------------
// Native structures not provided by windows-sys
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UnicodeString {
    pub Length: u16,
    pub MaximumLength: u16,
    pub _pad: u32,
    pub Buffer: *mut u16,
}
impl Default for UnicodeString {
    fn default() -> Self {
        UnicodeString { Length: 0, MaximumLength: 0, _pad: 0, Buffer: std::ptr::null_mut() }
    }
}

/// SYSTEM_PROCESS_INFORMATION (Windows 10 x64 layout).
/// Only the prefix up to OtherTransferCount is declared; thread records follow.
#[repr(C)]
pub struct SystemProcessInformation {
    pub NextEntryOffset: u32,
    pub NumberOfThreads: u32,
    pub WorkingSetPrivateSize: i64,
    pub HardFaultCount: u32,
    pub NumberOfThreadsHighWatermark: u32,
    pub CycleTime: u64,
    pub CreateTime: i64,
    pub UserTime: i64,
    pub KernelTime: i64,
    pub ImageName: UnicodeString,
    pub BasePriority: i32,
    pub UniqueProcessId: *mut c_void,
    pub InheritedFromUniqueProcessId: *mut c_void,
    pub HandleCount: u32,
    pub SessionId: u32,
    pub UniqueProcessKey: usize,
    pub PeakVirtualSize: usize,
    pub VirtualSize: usize,
    pub PageFaultCount: u32,
    pub PeakWorkingSetSize: usize,
    pub WorkingSetSize: usize,
    pub QuotaPeakPagedPoolUsage: usize,
    pub QuotaPagedPoolUsage: usize,
    pub QuotaPeakNonPagedPoolUsage: usize,
    pub QuotaNonPagedPoolUsage: usize,
    pub PagefileUsage: usize,
    pub PeakPagefileUsage: usize,
    pub PrivatePageCount: usize,
    pub ReadOperationCount: i64,
    pub WriteOperationCount: i64,
    pub OtherOperationCount: i64,
    pub ReadTransferCount: i64,
    pub WriteTransferCount: i64,
    pub OtherTransferCount: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemoryStatusEx {
    pub dwLength: u32,
    pub dwMemoryLoad: u32,
    pub ullTotalPhys: u64,
    pub ullAvailPhys: u64,
    pub ullTotalPageFile: u64,
    pub ullAvailPageFile: u64,
    pub ullTotalVirtual: u64,
    pub ullAvailVirtual: u64,
    pub ullAvailExtendedVirtual: u64,
}
impl Default for MemoryStatusEx {
    fn default() -> Self {
        MemoryStatusEx {
            dwLength: std::mem::size_of::<MemoryStatusEx>() as u32,
            dwMemoryLoad: 0,
            ullTotalPhys: 0,
            ullAvailPhys: 0,
            ullTotalPageFile: 0,
            ullAvailPageFile: 0,
            ullTotalVirtual: 0,
            ullAvailVirtual: 0,
            ullAvailExtendedVirtual: 0,
        }
    }
}

#[repr(C)]
#[derive(Default)]
pub struct PerformanceInformation {
    pub cb: u32,
    pub CommitTotal: usize,
    pub CommitLimit: usize,
    pub CommitPeak: usize,
    pub PhysicalTotal: usize,
    pub PhysicalAvailable: usize,
    pub SystemCache: usize,
    pub KernelTotal: usize,
    pub KernelPaged: usize,
    pub KernelNonpaged: usize,
    pub PageSize: usize,
    pub HandleCount: u32,
    pub ProcessCount: u32,
    pub ThreadCount: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct ServiceStatusProcess {
    pub dwServiceType: u32,
    pub dwCurrentState: u32,
    pub dwControlsAccepted: u32,
    pub dwWin32ExitCode: u32,
    pub dwServiceSpecificExitCode: u32,
    pub dwCheckPoint: u32,
    pub dwWaitHint: u32,
    pub dwProcessId: u32,
    pub dwServiceFlags: u32,
}

#[repr(C)]
pub struct EnumServiceStatusProcessW {
    pub lpServiceName: *mut u16,
    pub lpDisplayName: *mut u16,
    pub ServiceStatusProcess: ServiceStatusProcess,
}

#[repr(C)]
#[derive(Default)]
pub struct WtsSessionInfoW {
    pub SessionId: u32,
    pub pWinStationName: *mut u16,
    pub State: i32,
}

#[repr(C)]
#[derive(Default)]
pub struct PdhFmtCounterValue {
    pub CStatus: u32,
    pub _pad: u32,
    pub value: [u8; 8],
}
impl PdhFmtCounterValue {
    pub fn as_f64(&self) -> f64 {
        f64::from_ne_bytes(self.value)
    }
}

#[repr(C)]
pub struct PdhFmtCounterValueItemW {
    pub szName: *mut u16,
    pub FmtValue: PdhFmtCounterValue,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NotifyIconDataW {
    pub cbSize: u32,
    pub hWnd: HWND,
    pub uID: u32,
    pub uFlags: u32,
    pub uCallbackMessage: u32,
    pub hIcon: *mut c_void,
    pub szTip: [u16; 128],
    pub dwState: u32,
    pub dwStateMask: u32,
    pub szInfo: [u16; 256],
    pub uVersion: u32,
    pub szInfoTitle: [u16; 64],
    pub dwInfoFlags: u32,
    pub guidItem: [u8; 16],
    pub hBalloonIcon: *mut c_void,
}

#[repr(C)]
#[derive(Default)]
pub struct LvColumnW {
    pub mask: u32,
    pub fmt: i32,
    pub cx: i32,
    pub pszText: *mut u16,
    pub cchTextMax: i32,
    pub iSubItem: i32,
    pub iImage: i32,
    pub iOrder: i32,
    pub cxMin: i32,
    pub cxDefault: i32,
    pub cxIdeal: i32,
}

#[repr(C)]
#[derive(Default)]
pub struct NmHdr {
    pub hwndFrom: HWND,
    pub idFrom: usize,
    pub code: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct NmListView {
    pub hdr: NmHdr,
    pub iItem: i32,
    pub iSubItem: i32,
    pub uNewState: u32,
    pub uOldState: u32,
    pub uChanged: u32,
    pub ptAction: POINT,
    pub lParam: isize,
}

#[repr(C)]
#[derive(Default)]
pub struct NmCustomDraw {
    pub hdr: NmHdr,
    pub dwDrawStage: u32,
    pub hdc: HDC,
    pub rc: RECT,
    pub dwItemSpec: usize,
    pub uItemState: u32,
    pub lItemlParam: isize,
}

#[repr(C)]
pub struct NmLvCustomDraw {
    pub nmcd: NmCustomDraw,
    pub clrText: u32,
    pub clrTextBk: u32,
    pub iSubItem: i32,
    pub dwItemType: u32,
    pub clrFace: u32,
    pub iIconEffect: i32,
    pub iIconPhase: i32,
    pub iPartId: i32,
    pub iStateId: i32,
    pub rcText: RECT,
    pub uAlign: u32,
}

/// LVITEMW / NMLVDISPINFOW used by owner-data list views.
#[repr(C)]
#[derive(Default)]
pub struct LvItemW {
    pub mask: u32,
    pub iItem: i32,
    pub iSubItem: i32,
    pub state: u32,
    pub stateMask: u32,
    pub pszText: *mut u16,
    pub cchTextMax: i32,
    pub iImage: i32,
    pub lParam: isize,
    pub iIndent: i32,
    pub iGroupId: i32,
    pub cColumns: u32,
    pub puColumns: *mut u32,
    pub piColFmt: *mut i32,
    pub iGroup: i32,
}

#[repr(C)]
pub struct NmLvDispInfoW {
    pub hdr: NmHdr,
    pub item: LvItemW,
}

#[repr(C)]
#[derive(Default)]
pub struct LogFontW {
    pub lfHeight: i32,
    pub lfWidth: i32,
    pub lfEscapement: i32,
    pub lfOrientation: i32,
    pub lfWeight: i32,
    pub lfItalic: u8,
    pub lfUnderline: u8,
    pub lfStrikeOut: u8,
    pub lfCharSet: u8,
    pub lfOutPrecision: u8,
    pub lfClipPrecision: u8,
    pub lfQuality: u8,
    pub lfPitchAndFamily: u8,
    pub lfFaceName: [u16; 32],
}

#[repr(C)]
#[derive(Default)]
pub struct InitCommonControlsEx {
    pub dwSize: u32,
    pub dwICC: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct PaintStruct {
    pub hdc: HDC,
    pub fErase: i32,
    pub rcPaint: RECT,
    pub fRestore: i32,
    pub fIncUpdate: i32,
    pub rgbReserved: [u8; 32],
}

#[repr(C)]
pub struct HdItemW {
    pub mask: u32,
    pub cxy: i32,
    pub pszText: *mut u16,
    pub cchTextMax: i32,
    pub iImage: i32,
    pub iOrder: i32,
    pub iType: u32,
    pub pvFilter: *mut c_void,
    pub state: u32,
}

#[repr(C)]
pub struct MeasureItemStruct {
    pub CtlType: u32,
    pub CtlID: u32,
    pub itemID: u32,
    pub itemWidth: u32,
    pub itemHeight: u32,
    pub itemData: usize,
}

#[repr(C)]
pub struct DrawItemStruct {
    pub CtlType: u32,
    pub CtlID: u32,
    pub itemID: u32,
    pub itemAction: u32,
    pub itemState: u32,
    pub hwndItem: HWND,
    pub hDC: HDC,
    pub rcItem: RECT,
    pub itemData: usize,
}

#[repr(C)]
pub struct NmItemActivate {
    pub hdr: NmHdr,
    pub iItem: i32,
    pub iSubItem: i32,
    pub uNewState: u32,
    pub uOldState: u32,
    pub uChanged: u32,
    pub pt: POINT,
    pub lParam: isize,
}

#[repr(C)]
pub struct ShFileInfoW {
    pub hIcon: HICON,
    pub iIcon: i32,
    pub dwAttributes: u32,
    pub szDisplayName: [u16; 260],
    pub szTypeName: [u16; 80],
}

#[repr(C)]
pub struct MenuInfo {
    pub cbSize: u32,
    pub fMask: u32,
    pub dwStyle: u32,
    pub cyMax: u32,
    pub hbrBack: HBRUSH,
    pub dwContextHelpID: u32,
    pub dwMenuData: usize,
}

#[repr(C)]
pub struct MenuItemInfoW {
    pub cbSize: u32,
    pub fMask: u32,
    pub fType: u32,
    pub fState: u32,
    pub wID: u32,
    pub hSubMenu: HMENU,
    pub hbmpChecked: *mut c_void,
    pub hbmpUnchecked: *mut c_void,
    pub dwItemData: usize,
    pub dwTypeData: *mut u16,
    pub cch: i32,
    pub hbmpItem: *mut c_void,
}

#[repr(C)]
#[derive(Default)]
pub struct SystemTime {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}

// ---------------------------------------------------------------------------
// PE Authenticode presence check (no trust-chain validation, no COM)
// ---------------------------------------------------------------------------

/// Returns true when the PE image contains a non-empty Security data directory.
pub fn pe_has_signature(path: &str) -> bool {
    use std::io::Read;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut head = [0u8; 4096];
    let n = match f.read(&mut head) {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n < 0x40 || head[0] != b'M' || head[1] != b'Z' {
        return false;
    }
    let pe = i32::from_le_bytes(head[0x3C..0x40].try_into().unwrap()) as usize;
    if pe + 24 + 4 > n || head[pe] != b'P' || head[pe + 1] != b'E' {
        return false;
    }
    let opt = pe + 24;
    if opt + 2 > n {
        return false;
    }
    let magic = u16::from_le_bytes([head[opt], head[opt + 1]]);
    let (num_off, dir_off) = match magic {
        0x20b => (opt + 108, opt + 112), // PE32+
        0x10b => (opt + 92, opt + 96),  // PE32
        _ => return false,
    };
    if num_off + 4 > n {
        return false;
    }
    let count = u32::from_le_bytes(head[num_off..num_off + 4].try_into().unwrap()) as usize;
    let sec = dir_off + 4 * 8; // index 4 = IMAGE_DIRECTORY_ENTRY_SECURITY
    if count < 5 || sec + 8 > n {
        return false;
    }
    let size = u32::from_le_bytes(head[sec + 4..sec + 8].try_into().unwrap());
    size > 0
}

// ---------------------------------------------------------------------------
// Direct Win32 ABI declarations.
//
// These link against the import libraries shipped inside the Rust GNU
// toolchain (lib/rustlib/.../lib/self-contained). No generated bindings, no
// build script and no external binutils are required. APIs from DLLs that are
// not part of that set (ntdll, pdh, wtsapi32, dwmapi) are loaded dynamically
// through `dyn_proc`.
// ---------------------------------------------------------------------------
pub mod native {
    use super::*;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct WNDCLASSEXW {
        pub cbSize: u32,
        pub style: u32,
        pub lpfnWndProc: WNDPROC,
        pub cbClsExtra: i32,
        pub cbWndExtra: i32,
        pub hInstance: HINSTANCE,
        pub hIcon: HICON,
        pub hCursor: HCURSOR,
        pub hbrBackground: HBRUSH,
        pub lpszMenuName: PCWSTR,
        pub lpszClassName: PCWSTR,
        pub hIconSm: HICON,
    }

    #[repr(C)]
    pub struct MSG {
        pub hwnd: HWND,
        pub message: u32,
        pub wParam: WPARAM,
        pub lParam: LPARAM,
        pub time: DWORD,
        pub pt: POINT,
        pub lPrivate: DWORD,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct SIZE {
        pub cx: i32,
        pub cy: i32,
    }

    #[repr(C)]
    pub struct STARTUPINFOW {
        pub cb: u32,
        pub lpReserved: PWSTR,
        pub lpDesktop: PWSTR,
        pub lpTitle: PWSTR,
        pub dwX: u32,
        pub dwY: u32,
        pub dwXSize: u32,
        pub dwYSize: u32,
        pub dwXCountChars: u32,
        pub dwYCountChars: u32,
        pub dwFillAttribute: u32,
        pub dwFlags: u32,
        pub wShowWindow: u16,
        pub cbReserved2: u16,
        pub lpReserved2: *mut u8,
        pub hStdInput: HANDLE,
        pub hStdOutput: HANDLE,
        pub hStdError: HANDLE,
    }
    impl Default for STARTUPINFOW {
        fn default() -> Self {
            unsafe { std::mem::zeroed() }
        }
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct PROCESS_INFORMATION {
        pub hProcess: HANDLE,
        pub hThread: HANDLE,
        pub dwProcessId: u32,
        pub dwThreadId: u32,
    }

    /// MIB_IFROW (GetIfTable). 860 bytes, 4-byte aligned.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MIB_IFROW {
        pub wszName: [u16; 256],
        pub dwIndex: u32,
        pub dwType: u32,
        pub dwMtu: u32,
        pub dwSpeed: u32,
        pub dwPhysAddrLen: u32,
        pub bPhysAddr: [u8; 8],
        pub dwAdminStatus: u32,
        pub dwOperStatus: u32,
        pub dwLastChange: u32,
        pub dwInOctets: u32,
        pub dwInUcastPkts: u32,
        pub dwInNUcastPkts: u32,
        pub dwInDiscards: u32,
        pub dwInErrors: u32,
        pub dwInUnknownProtos: u32,
        pub dwOutOctets: u32,
        pub dwOutUcastPkts: u32,
        pub dwOutNUcastPkts: u32,
        pub dwOutDiscards: u32,
        pub dwOutErrors: u32,
        pub dwOutQLen: u32,
        pub dwDescrLen: u32,
        pub bDescr: [u8; 256],
    }
    impl Default for MIB_IFROW {
        fn default() -> Self {
            unsafe { std::mem::zeroed() }
        }
    }
    #[repr(C)]
    pub struct MIB_IFTABLE {
        pub dwNumEntries: u32,
        pub table: [MIB_IFROW; 1],
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetModuleHandleW(lpModuleName: PCWSTR) -> HINSTANCE;
        pub fn LoadLibraryW(lpLibFileName: PCWSTR) -> HMODULE;
        pub fn GetProcAddress(hModule: HMODULE, lpProcName: PCSTR) -> *mut c_void;
        pub fn FreeLibrary(hLibModule: HMODULE) -> BOOL;
        pub fn CloseHandle(hObject: HANDLE) -> BOOL;
        pub fn GetLastError() -> u32;
        pub fn SetLastError(dwErrCode: u32);
        pub fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: BOOL, dwProcessId: u32) -> HANDLE;
        pub fn TerminateProcess(hProcess: HANDLE, uExitCode: u32) -> BOOL;
        pub fn QueryFullProcessImageNameW(hProcess: HANDLE, dwFlags: u32, lpExeName: PWSTR, lpdwSize: *mut u32) -> BOOL;
        pub fn GetProcessTimes(hProcess: HANDLE, lpCreationTime: *mut FILETIME, lpExitTime: *mut FILETIME, lpKernelTime: *mut FILETIME, lpUserTime: *mut FILETIME) -> BOOL;
        pub fn GetPriorityClass(hProcess: HANDLE) -> u32;
        pub fn SetPriorityClass(hProcess: HANDLE, dwPriorityClass: u32) -> BOOL;
        pub fn GetCurrentProcess() -> HANDLE;
        pub fn GetCurrentProcessId() -> u32;
        pub fn GetExitCodeProcess(hProcess: HANDLE, lpExitCode: *mut u32) -> BOOL;
        pub fn ProcessIdToSessionId(dwProcessId: u32, pSessionId: *mut u32) -> BOOL;
        pub fn CreateProcessW(lpApplicationName: PCWSTR, lpCommandLine: PWSTR, lpProcessAttributes: *mut c_void, lpThreadAttributes: *mut c_void, bInheritHandles: BOOL, dwCreationFlags: u32, lpEnvironment: LPVOID, lpCurrentDirectory: PCWSTR, lpStartupInfo: *mut STARTUPINFOW, lpProcessInformation: *mut PROCESS_INFORMATION) -> BOOL;
        pub fn GetSystemTimes(lpIdleTime: *mut FILETIME, lpKernelTime: *mut FILETIME, lpUserTime: *mut FILETIME) -> BOOL;
        pub fn GetTickCount64() -> u64;
        pub fn GlobalMemoryStatusEx(lpBuffer: *mut MemoryStatusEx) -> BOOL;
        pub fn GetPhysicallyInstalledSystemMemory(total: *mut u64) -> BOOL;
        pub fn GetSystemFirmwareTable(signature: u32, tableId: u32, pBuffer: *mut c_void, size: u32) -> u32;
        pub fn CreateFileW(name: PCWSTR, access: u32, share: u32, sa: *mut c_void, disp: u32, flags: u32, tmpl: HANDLE) -> HANDLE;
        pub fn DeviceIoControl(hDevice: HANDLE, code: u32, in_: *const c_void, inSize: u32, out: *mut c_void, outSize: u32, ret: *mut u32, ov: *mut c_void) -> BOOL;
        pub fn FileTimeToLocalFileTime(lpFileTime: *const FILETIME, lpLocalFileTime: *mut FILETIME) -> BOOL;
        pub fn FileTimeToSystemTime(lpFileTime: *const FILETIME, lpSystemTime: *mut SystemTime) -> BOOL;
        pub fn GetSystemTimeAsFileTime(lpSystemTimeAsFileTime: *mut FILETIME);
        pub fn GetLocalTime(lpSystemTime: *mut SystemTime);
        pub fn GetLogicalDriveStringsW(nBufferLength: u32, lpBuffer: PWSTR) -> u32;
        pub fn GetDriveTypeW(lpRootPathName: PCWSTR) -> u32;
        pub fn GetDiskFreeSpaceExW(lpDirectoryName: PCWSTR, lpFreeBytesAvailableToCaller: *mut u64, lpTotalNumberOfBytes: *mut u64, lpTotalNumberOfFreeBytes: *mut u64) -> BOOL;
        pub fn GetTempPathW(nBufferLength: u32, lpBuffer: PWSTR) -> u32;
        pub fn Sleep(dwMilliseconds: u32);
        pub fn QueryPerformanceCounter(lpPerformanceCount: *mut i64) -> BOOL;
        pub fn QueryPerformanceFrequency(lpFrequency: *mut i64) -> BOOL;
        pub fn GetComputerNameW(lpBuffer: PWSTR, nSize: *mut u32) -> BOOL;
        pub fn GetModuleFileNameW(hModule: HMODULE, lpFilename: PWSTR, nSize: u32) -> u32;
        pub fn IsWow64Process(hProcess: HANDLE, Wow64Process: *mut BOOL) -> BOOL;
        pub fn GetSystemInfo(lpSystemInfo: *mut c_void);
        pub fn GetActiveProcessorCount(GroupNumber: u16) -> u32;
        pub fn GlobalAlloc(uFlags: u32, dwBytes: usize) -> HANDLE;
        pub fn GlobalLock(hMem: HANDLE) -> LPVOID;
        pub fn GlobalUnlock(hMem: HANDLE) -> BOOL;
        pub fn GlobalFree(hMem: HANDLE) -> HANDLE;
        pub fn SetProcessAffinityMask(hProcess: HANDLE, dwProcessAffinityMask: usize) -> BOOL;
    }

    #[link(name = "psapi")]
    extern "system" {
        pub fn GetPerformanceInfo(pPerformanceInformation: *mut PerformanceInformation, cb: u32) -> BOOL;
    }

    #[link(name = "user32")]
    extern "system" {
        pub fn RegisterClassExW(lpwcx: *const WNDCLASSEXW) -> u16;
        pub fn CreateWindowExW(dwExStyle: u32, lpClassName: PCWSTR, lpWindowName: PCWSTR, dwStyle: u32, X: i32, Y: i32, nWidth: i32, nHeight: i32, hWndParent: HWND, hMenu: HMENU, hInstance: HINSTANCE, lpParam: LPVOID) -> HWND;
        pub fn DefWindowProcW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
        pub fn GetMessageW(lpMsg: *mut MSG, hWnd: HWND, wMsgFilterMin: u32, wMsgFilterMax: u32) -> BOOL;
        pub fn TranslateMessage(lpMsg: *const MSG) -> BOOL;
        pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
        pub fn PostQuitMessage(nExitCode: i32);
        pub fn PostMessageW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> BOOL;
        pub fn SendMessageW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
        pub fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> BOOL;
        pub fn UpdateWindow(hWnd: HWND) -> BOOL;
        pub fn DestroyWindow(hWnd: HWND) -> BOOL;
        pub fn SetWindowPos(hWnd: HWND, hWndInsertAfter: HWND, X: i32, Y: i32, cx: i32, cy: i32, uFlags: u32) -> BOOL;
        pub fn MoveWindow(hWnd: HWND, X: i32, Y: i32, nWidth: i32, nHeight: i32, bRepaint: BOOL) -> BOOL;
        pub fn GetClientRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;
        pub fn GetWindowRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;
        pub fn SetWindowTextW(hWnd: HWND, lpString: PCWSTR) -> BOOL;
        pub fn GetWindowTextW(hWnd: HWND, lpString: PWSTR, nMaxCount: i32) -> i32;
        pub fn GetWindowTextLengthW(hWnd: HWND) -> i32;
        pub fn SetFocus(hWnd: HWND) -> HWND;
        pub fn GetFocus() -> HWND;
        pub fn EnableWindow(hWnd: HWND, bEnable: BOOL) -> BOOL;
        pub fn InvalidateRect(hWnd: HWND, lpRect: *const RECT, bErase: BOOL) -> BOOL;
        pub fn RedrawWindow(hWnd: HWND, lprcUpdate: *const RECT, hrgn: *mut c_void, flags: u32) -> BOOL;
        pub fn GetDC(hWnd: HWND) -> HDC;
        pub fn ReleaseDC(hWnd: HWND, hDC: HDC) -> i32;
        pub fn BeginPaint(hWnd: HWND, lpPaint: *mut PaintStruct) -> HDC;
        pub fn EndPaint(hWnd: HWND, lpPaint: *const PaintStruct) -> BOOL;
        pub fn SetTimer(hWnd: HWND, nIDEvent: usize, uElapse: u32, lpTimerFunc: *mut c_void) -> usize;
        pub fn KillTimer(hWnd: HWND, nIDEvent: usize) -> BOOL;
        pub fn SetForegroundWindow(hWnd: HWND) -> BOOL;
        pub fn GetForegroundWindow() -> HWND;
        pub fn IsWindowVisible(hWnd: HWND) -> BOOL;
        pub fn IsIconic(hWnd: HWND) -> BOOL;
        pub fn GetCursorPos(lpPoint: *mut POINT) -> BOOL;
        pub fn ScreenToClient(hWnd: HWND, lpPoint: *mut POINT) -> BOOL;
        pub fn ClientToScreen(hWnd: HWND, lpPoint: *mut POINT) -> BOOL;
        pub fn PtInRect(lprc: *const RECT, pt: POINT) -> BOOL;
        pub fn TrackPopupMenu(hMenu: HMENU, uFlags: u32, x: i32, y: i32, nReserved: i32, hWnd: HWND, prcRect: *const RECT) -> i32;
        pub fn CreatePopupMenu() -> HMENU;
        pub fn AppendMenuW(hMenu: HMENU, uFlags: u32, uIDNewItem: usize, lpNewItem: PCWSTR) -> BOOL;
        pub fn DestroyMenu(hMenu: HMENU) -> BOOL;
        pub fn CheckMenuItem(hMenu: HMENU, uIDCheckItem: u32, uCheck: u32) -> u32;
        pub fn GetSubMenu(hMenu: HMENU, nPos: i32) -> HMENU;
        pub fn GetMenuItemCount(hMenu: HMENU) -> i32;
        pub fn MessageBoxW(hWnd: HWND, lpText: PCWSTR, lpCaption: PCWSTR, uType: u32) -> i32;
        pub fn LoadIconW(hInstance: HINSTANCE, lpIconName: PCWSTR) -> HICON;
        pub fn LoadCursorW(hInstance: HINSTANCE, lpCursorName: PCWSTR) -> HCURSOR;
        pub fn LoadImageW(hInst: HINSTANCE, name: PCWSTR, type_: u32, cx: i32, cy: i32, fuLoad: u32) -> HANDLE;
        pub fn DestroyIcon(hIcon: HICON) -> BOOL;
        pub fn GetSystemMetrics(nIndex: i32) -> i32;
        pub fn GetDlgItem(hDlg: HWND, nIDDlgItem: i32) -> HWND;
        pub fn SetDlgItemTextW(hDlg: HWND, nIDDlgItem: i32, lpString: PCWSTR) -> BOOL;
        pub fn GetDlgItemTextW(hDlg: HWND, nIDDlgItem: i32, lpString: PWSTR, cchMax: i32) -> u32;
        pub fn RegisterHotKey(hWnd: HWND, id: i32, fsModifiers: u32, vk: u32) -> BOOL;
        pub fn UnregisterHotKey(hWnd: HWND, id: i32) -> BOOL;
        pub fn SetWindowLongPtrW(hWnd: HWND, nIndex: i32, dwNewLong: isize) -> isize;
        pub fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> isize;
        pub fn GetClassNameW(hWnd: HWND, lpClassName: PWSTR, nMaxCount: i32) -> i32;
        pub fn GetWindowThreadProcessId(hWnd: HWND, lpdwProcessId: *mut u32) -> u32;
        pub fn EnumWindows(lpEnumFunc: unsafe extern "system" fn(HWND, LPARAM) -> BOOL, lParam: LPARAM) -> BOOL;
        pub fn GetWindowLongW(hWnd: HWND, nIndex: i32) -> i32;
        pub fn GetAsyncKeyState(vKey: i32) -> i16;
        pub fn OpenClipboard(hWndNewOwner: HWND) -> BOOL;
        pub fn EmptyClipboard() -> BOOL;
        pub fn SetClipboardData(uFormat: u32, hMem: HANDLE) -> HANDLE;
        pub fn CloseClipboard() -> BOOL;
        pub fn SetMenu(hWnd: HWND, hMenu: HMENU) -> BOOL;
        pub fn DrawMenuBar(hWnd: HWND) -> BOOL;
        pub fn SetCursor(hCursor: HCURSOR) -> HCURSOR;
        pub fn IsWindow(hWnd: HWND) -> BOOL;
        pub fn FillRect(hDC: HDC, lprc: *const RECT, hbr: HBRUSH) -> i32;
        pub fn DrawTextW(hDC: HDC, lpchText: PCWSTR, cchText: i32, lprc: *mut RECT, dwDTFormat: u32) -> i32;
        pub fn GetSysColor(nIndex: i32) -> u32;
        pub fn GetSysColorBrush(nIndex: i32) -> HBRUSH;
        pub fn SetCapture(hWnd: HWND) -> HWND;
        pub fn ReleaseCapture() -> BOOL;
        pub fn GetWindowPlacement(hWnd: HWND, lpwndpl: *mut c_void) -> BOOL;
        pub fn SetWindowPlacement(hWnd: HWND, lpwndpl: *const c_void) -> BOOL;
        pub fn AdjustWindowRectEx(lpRect: *mut RECT, dwStyle: u32, bMenu: BOOL, dwExStyle: u32) -> BOOL;
        pub fn MapWindowPoints(hWndFrom: HWND, hWndTo: HWND, lpPoints: *mut POINT, cPoints: u32) -> i32;
        pub fn UnregisterClassW(lpClassName: PCWSTR, hInstance: HINSTANCE) -> BOOL;
        pub fn CallWindowProcW(lpPrevWndFunc: isize, hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
        pub fn InsertMenuItemW(hMenu: HMENU, item: u32, fByPosition: BOOL, lpmi: *const MenuItemInfoW) -> BOOL;
        pub fn SetMenuInfo(hMenu: HMENU, lpcmi: *const MenuInfo) -> BOOL;
        pub fn GetGuiResources(hProcess: HANDLE, uiFlags: u32) -> u32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        pub fn CreateFontW(cHeight: i32, cWidth: i32, cEscapement: i32, cOrientation: i32, cWeight: i32, bItalic: u32, bUnderline: u32, bStrikeOut: u32, iCharSet: u32, iOutPrecision: u32, iClipPrecision: u32, iQuality: u32, iPitchAndFamily: u32, pszFaceName: PCWSTR) -> HFONT;
        pub fn CreateFontIndirectW(lplf: *const LogFontW) -> HFONT;
        pub fn CreateSolidBrush(crColor: COLORREF) -> HBRUSH;
        pub fn CreatePen(iStyle: i32, cWidth: i32, crColor: COLORREF) -> HGDIOBJ;
        pub fn SelectObject(hdc: HDC, h: HGDIOBJ) -> HGDIOBJ;
        pub fn DeleteObject(ho: HGDIOBJ) -> BOOL;
        pub fn CreateCompatibleDC(hdc: HDC) -> HDC;
        pub fn CreateCompatibleBitmap(hdc: HDC, cx: i32, cy: i32) -> HBITMAP;
        pub fn BitBlt(hdc: HDC, x: i32, y: i32, cx: i32, cy: i32, hdcSrc: HDC, x1: i32, y1: i32, rop: u32) -> BOOL;
        pub fn SetTextColor(hdc: HDC, crColor: COLORREF) -> COLORREF;
        pub fn SetBkColor(hdc: HDC, crColor: COLORREF) -> COLORREF;
        pub fn SetBkMode(hdc: HDC, mode: i32) -> i32;
        pub fn Rectangle(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32) -> BOOL;
        pub fn RoundRect(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32, w: i32, h: i32) -> BOOL;
        pub fn Ellipse(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32) -> BOOL;
        pub fn MoveToEx(hdc: HDC, x: i32, y: i32, lppt: *mut POINT) -> BOOL;
        pub fn LineTo(hdc: HDC, x: i32, y: i32) -> BOOL;
        pub fn Polyline(hdc: HDC, ppt: *const POINT, cpt: i32) -> BOOL;
        pub fn GetStockObject(i: i32) -> HGDIOBJ;
        pub fn DeleteDC(hdc: HDC) -> BOOL;
        pub fn SetDCBrushColor(hdc: HDC, crColor: COLORREF) -> COLORREF;
        pub fn GetTextExtentPoint32W(hdc: HDC, lpString: PCWSTR, c: i32, psizl: *mut SIZE) -> BOOL;
        pub fn GetDeviceCaps(hdc: HDC, nIndex: i32) -> i32;
        pub fn CreateRoundRectRgn(x1: i32, y1: i32, x2: i32, y2: i32, w: i32, h: i32) -> *mut c_void;
        pub fn Polygon(hdc: HDC, ppt: *const POINT, cpt: i32) -> BOOL;
        pub fn PatBlt(hdc: HDC, x: i32, y: i32, w: i32, h: i32, rop: u32) -> BOOL;
        pub fn SetTextAlign(hdc: HDC, align: u32) -> u32;
        pub fn IntersectClipRect(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32) -> i32;
        pub fn SelectClipRgn(hdc: HDC, hrgn: *mut c_void) -> i32;
    }

    #[link(name = "comctl32")]
    extern "system" {
        pub fn InitCommonControlsEx(picce: *const InitCommonControlsEx) -> BOOL;
        pub fn ImageList_Create(cx: i32, cy: i32, flags: u32, cInitial: i32, cGrow: i32) -> HANDLE;
        pub fn ImageList_Destroy(himl: HANDLE) -> BOOL;
        pub fn ImageList_AddIcon(himl: HANDLE, hicon: HANDLE) -> i32;
        pub fn ImageList_SetBkColor(himl: HANDLE, clrBk: u32) -> u32;
        pub fn ImageList_Draw(himl: HANDLE, i: i32, hdcDst: HDC, x: i32, y: i32, fStyle: u32) -> BOOL;
    }

    #[link(name = "shell32")]
    extern "system" {
        pub fn Shell_NotifyIconW(dwMessage: u32, lpData: *mut NotifyIconDataW) -> BOOL;
        pub fn ShellExecuteW(hwnd: HWND, lpOperation: PCWSTR, lpFile: PCWSTR, lpParameters: PCWSTR, lpDirectory: PCWSTR, nShowCmd: i32) -> HINSTANCE;
        pub fn SHGetFileInfoW(pszPath: PCWSTR, dwFileAttributes: u32, psfi: *mut c_void, cbFileInfo: u32, uFlags: u32) -> usize;
        pub fn ExtractIconExW(lpszFile: PCWSTR, nIconIndex: i32, phiconLarge: *mut HICON, phiconSmall: *mut HICON, nIcons: u32) -> u32;
    }

    #[link(name = "advapi32")]
    extern "system" {
        pub fn RegOpenKeyExW(hKey: HKEY, lpSubKey: PCWSTR, ulOptions: u32, samDesired: u32, phkResult: *mut HKEY) -> i32;
        pub fn RegCloseKey(hKey: HKEY) -> i32;
        pub fn RegQueryValueExW(hKey: HKEY, lpValueName: PCWSTR, lpReserved: *mut u32, lpType: *mut u32, lpData: *mut u8, lpcbData: *mut u32) -> i32;
        pub fn RegEnumValueW(hKey: HKEY, dwIndex: u32, lpValueName: PWSTR, lpcchValueName: *mut u32, lpReserved: *mut u32, lpType: *mut u32, lpData: *mut u8, lpcbData: *mut u32) -> i32;
        pub fn RegSetValueExW(hKey: HKEY, lpValueName: PCWSTR, Reserved: u32, dwType: u32, lpData: *const u8, cbData: u32) -> i32;
        pub fn RegDeleteValueW(hKey: HKEY, lpValueName: PCWSTR) -> i32;
        pub fn RegCreateKeyExW(hKey: HKEY, lpSubKey: PCWSTR, Reserved: u32, lpClass: PCWSTR, dwOptions: u32, samDesired: u32, lpSecurityAttributes: *mut c_void, phkResult: *mut HKEY, lpdwDisposition: *mut u32) -> i32;
        pub fn OpenSCManagerW(lpMachineName: PCWSTR, lpDatabaseName: PCWSTR, dwDesiredAccess: u32) -> SC_HANDLE;
        pub fn EnumServicesStatusExW(hSCManager: SC_HANDLE, InfoLevel: i32, dwServiceType: u32, dwServiceState: u32, lpServices: *mut u8, cbBufSize: u32, pcbBytesNeeded: *mut u32, lpServicesReturned: *mut u32, lpResumeHandle: *mut u32, pszGroupName: PCWSTR) -> BOOL;
        pub fn OpenServiceW(hSCManager: SC_HANDLE, lpServiceName: PCWSTR, dwDesiredAccess: u32) -> SC_HANDLE;
        pub fn StartServiceW(hService: SC_HANDLE, dwNumServiceArgs: u32, lpServiceArgVectors: *const PCWSTR) -> BOOL;
        pub fn ControlService(hService: SC_HANDLE, dwControl: u32, lpServiceStatus: *mut c_void) -> BOOL;
        pub fn CloseServiceHandle(hSCObject: SC_HANDLE) -> BOOL;
        pub fn QueryServiceConfigW(hService: SC_HANDLE, lpServiceConfig: *mut u8, cbBufSize: u32, pcbBytesNeeded: *mut u32) -> BOOL;
        pub fn QueryServiceConfig2W(hService: SC_HANDLE, dwInfoLevel: u32, lpBuffer: *mut u8, cbBufSize: u32, pcbBytesNeeded: *mut u32) -> BOOL;
        pub fn ChangeServiceConfigW(hService: SC_HANDLE, dwServiceType: u32, dwStartType: u32, dwErrorControl: u32, lpBinaryPathName: PCWSTR, lpLoadOrderGroup: PCWSTR, lpdwTagId: *mut u32, lpDependencies: PCWSTR, lpServiceStartName: PCWSTR, lpPassword: PCWSTR, lpDisplayName: PCWSTR) -> BOOL;
    }

    #[link(name = "iphlpapi")]
    extern "system" {
        pub fn GetIfTable(pIfTable: *mut u8, pdwSize: *mut u32, bOrder: BOOL) -> u32;
        pub fn GetExtendedTcpTable(pTcpTable: *mut c_void, pdwSize: *mut u32, bOrder: BOOL, ulAf: u32, TableClass: i32, Reserved: u32) -> u32;
        pub fn GetExtendedUdpTable(pUdpTable: *mut c_void, pdwSize: *mut u32, bOrder: BOOL, ulAf: u32, TableClass: i32, Reserved: u32) -> u32;
    }
}
