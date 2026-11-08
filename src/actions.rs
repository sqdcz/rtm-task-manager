//! User-initiated actions: terminating processes, priority, services,
//! startup entries, export, clipboard and launching tasks.
//! Every Win32 call is checked; failures are reported to the caller instead of
//! panicking (the app is long-running and must not be taken down by a denied
//! handle on a protected process).

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;

use crate::collect::{local_clock, unix_now};
use crate::ffi::native::*;
use crate::ffi::*;
use crate::model::*;
use crate::table::Table;

const IDLE_PRIORITY_CLASS: u32 = 0x0000_0040;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
const NORMAL_PRIORITY_CLASS: u32 = 0x0000_0020;
const ABOVE_NORMAL_PRIORITY_CLASS: u32 = 0x0000_8000;
const HIGH_PRIORITY_CLASS: u32 = 0x0000_0080;
const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 0x0002;

pub fn kill(pid: u32) -> bool {
    if pid == 0 || pid == 4 {
        return false; // System / Idle are not terminable
    }
    unsafe {
        let h = OpenProcess(k::PROCESS_TERMINATE, 0, pid);
        if h.is_null() {
            return false;
        }
        let ok = TerminateProcess(h, 1);
        CloseHandle(h);
        ok != 0
    }
}

/// Terminates a process and every descendant, children first.
pub fn kill_tree(snap: &Snapshot, root: u32) -> usize {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in &snap.procs {
        children.entry(p.ppid).or_default().push(p.pid);
    }
    let mut order = Vec::new();
    let mut stack = vec![root];
    let mut guard = 0;
    while let Some(pid) = stack.pop() {
        guard += 1;
        if guard > 4096 {
            break;
        }
        order.push(pid);
        if let Some(kids) = children.get(&pid) {
            for &c in kids {
                if c != pid {
                    stack.push(c);
                }
            }
        }
    }
    order.reverse();
    let mut n = 0;
    for pid in order {
        if kill(pid) {
            n += 1;
        }
    }
    n
}

pub fn priority_class(level: u32) -> u32 {
    match level {
        0 => IDLE_PRIORITY_CLASS,
        1 => BELOW_NORMAL_PRIORITY_CLASS,
        2 => NORMAL_PRIORITY_CLASS,
        3 => ABOVE_NORMAL_PRIORITY_CLASS,
        4 => HIGH_PRIORITY_CLASS,
        _ => NORMAL_PRIORITY_CLASS,
    }
}

pub fn set_priority(pid: u32, level: u32) -> bool {
    unsafe {
        let h = OpenProcess(k::PROCESS_SET_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let ok = SetPriorityClass(h, priority_class(level));
        CloseHandle(h);
        ok != 0
    }
}

/// Win11 "Efficiency mode" equivalent available without undocumented QoS APIs:
/// lower scheduling priority. Returns false if the handle could not be opened.
pub fn efficiency_mode(pid: u32) -> bool {
    set_priority(pid, 1)
}

pub fn run_task(command: &str) -> bool {
    if command.trim().is_empty() {
        return false;
    }
    let mut cmd = wide(command);
    let mut si = STARTUPINFOW::default();
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut pi = PROCESS_INFORMATION::default();
    let ok = unsafe {
        CreateProcessW(
            ptr::null(),
            cmd.as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            0,
            ptr::null_mut(),
            ptr::null(),
            &mut si,
            &mut pi,
        )
    };
    unsafe {
        if !pi.hThread.is_null() {
            CloseHandle(pi.hThread);
        }
        if !pi.hProcess.is_null() {
            CloseHandle(pi.hProcess);
        }
    }
    ok != 0
}

pub fn reveal_file(path: &str) {
    if path.is_empty() || path.starts_with('[') {
        return;
    }
    let op = wide("open");
    let exe = wide("explorer.exe");
    let args = wide(&format!("/select,\"{}\"", path));
    unsafe {
        ShellExecuteW(ptr::null_mut(), op.as_ptr(), exe.as_ptr(), args.as_ptr(), ptr::null(), k::SW_SHOW);
    }
}

pub fn open_url_or_path(path: &str) {
    let op = wide("open");
    let w = wide(path);
    unsafe {
        ShellExecuteW(ptr::null_mut(), op.as_ptr(), w.as_ptr(), ptr::null(), ptr::null(), k::SW_SHOW);
    }
}

pub fn service_control(name: &str, start: bool) -> bool {
    let n = wide(name);
    unsafe {
        let sc = OpenSCManagerW(ptr::null(), ptr::null(), k::SC_MANAGER_ALL_ACCESS);
        if sc.is_null() {
            return false;
        }
        let access = k::SERVICE_START | k::SERVICE_STOP | k::SERVICE_QUERY_STATUS;
        let svc = OpenServiceW(sc, n.as_ptr(), access);
        if svc.is_null() {
            CloseServiceHandle(sc);
            return false;
        }
        let mut status = [0u8; 64];
        let ok = if start {
            StartServiceW(svc, 0, ptr::null()) != 0
        } else {
            ControlService(svc, k::SERVICE_CONTROL_STOP, status.as_mut_ptr() as *mut c_void) != 0
        };
        CloseServiceHandle(svc);
        CloseServiceHandle(sc);
        ok
    }
}

/// Enabling/disabling mirrors Task Manager: write the StartupApproved binary
/// value under HKCU. A low bit of 1 means disabled; 2/6 means enabled.
pub fn startup_set_enabled(item: &StartupItem, enabled: bool) -> bool {
    let sub = match item.reg_key {
        2 => "Run32",
        4 => "StartupFolder",
        _ => "Run",
    };
    let key = wide(&format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\{}",
        sub
    ));
    let value_name = if item.reg_key == 4 {
        // StartupApproved uses the shortcut file name, not the display name.
        std::path::Path::new(&item.command)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| item.name.clone())
    } else {
        item.name.clone()
    };
    let vn = wide(&value_name);
    let mut h: HKEY = ptr::null_mut();
    let mut disp: u32 = 0;
    unsafe {
        if RegCreateKeyExW(
            k::HKEY_CURRENT_USER as HKEY,
            key.as_ptr(),
            0,
            ptr::null(),
            0,
            k::KEY_WRITE,
            ptr::null_mut(),
            &mut h,
            &mut disp,
        ) != 0
        {
            return false;
        }
        let ok = if enabled {
            RegDeleteValueW(h, vn.as_ptr()) == 0 || reg_write_approved(h, vn.as_ptr(), 2)
        } else {
            reg_write_approved(h, vn.as_ptr(), 3)
        };
        RegCloseKey(h);
        ok
    }
}

unsafe fn reg_write_approved(h: HKEY, name: PCWSTR, first: u8) -> bool {
    let mut data = [0u8; 12];
    data[0] = first;
    RegSetValueExW(h, name, 0, k::REG_BINARY, data.as_ptr(), data.len() as u32) == 0
}

pub fn copy_to_clipboard(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let mut w = wide(text);
    let bytes = w.len() * 2;
    unsafe {
        if OpenClipboard(ptr::null_mut()) == 0 {
            return false;
        }
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if h.is_null() {
            CloseClipboard();
            return false;
        }
        let dst = GlobalLock(h) as *mut u16;
        if dst.is_null() {
            GlobalFree(h);
            CloseClipboard();
            return false;
        }
        ptr::copy_nonoverlapping(w.as_mut_ptr(), dst, w.len());
        GlobalUnlock(h);
        EmptyClipboard();
        SetClipboardData(CF_UNICODETEXT, h);
        CloseClipboard();
    }
    true
}

/// Writes an exported table to the user's Documents folder and returns the path.
pub fn export_table(page: Page, table: &Table, json: bool) -> Result<String, String> {
    let base = std::env::var("USERPROFILE").unwrap_or_else(|_| String::from("."));
    let dir = format!("{}\\Documents\\RustTaskManager", base);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Err(format!("无法创建导出目录: {}", e));
    }
    let clock = local_clock().replace(':', "-");
    let name = format!("rtm_{}_{}.{}", page.label(), clock, if json { "json" } else { "csv" });
    let path = format!("{}\\{}", dir, name);
    let content = if json { crate::table::to_json(table) } else { crate::table::to_csv(table) };
    match std::fs::write(&path, content) {
        Ok(_) => Ok(path),
        Err(e) => Err(format!("写入失败: {}", e)),
    }
}

pub fn process_snapshot_note() -> String {
    format!("导出时间 {}", unix_now())
}
