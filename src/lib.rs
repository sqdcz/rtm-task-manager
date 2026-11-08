//! Rust data engine exposed as a C ABI library for the WinUI 3 front end.
//!
//! The engine owns all system data acquisition. The UI layer only receives a
//! UTF-8 JSON snapshot and calls back for actions. This keeps all Win32/NT
//! interaction in Rust and all XAML/WinRT interaction in C#.

mod actions;
mod collect;
mod ffi;
mod model;
mod table;

use std::ffi::{c_char, c_void, CString};
use std::sync::Mutex;

use collect::Collector;
use model::{json_escape, Config, Snapshot};
use table::HistoryTracker;

struct Engine {
    collector: Collector,
    history: HistoryTracker,
}

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

fn with_engine<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Engine) -> R,
{
    match ENGINE.lock() {
        Ok(mut g) => {
            if g.is_none() {
                *g = Some(Engine { collector: Collector::new(), history: HistoryTracker::default() });
            }
            Some(f(g.as_mut()?))
        }
        Err(_) => None,
    }
}

fn hist_json(h: &[f32]) -> String {
    h.iter().map(|v| format!("{:.1}", v)).collect::<Vec<_>>().join(",")
}

fn num(v: f64) -> String {
    if v.is_finite() {
        format!("{:.3}", v)
    } else {
        "0".to_string()
    }
}

fn snapshot_json(s: &Snapshot) -> String {
    let sys = &s.sys;
    let mut o = String::with_capacity(64 * 1024);
    o.push_str(&format!(
        "{{\"cpu\":{},\"memTotal\":{},\"memUsed\":{},\"memAvail\":{},\"diskRead\":{},\"diskWrite\":{},\"netRecv\":{},\"netSent\":{},\"handles\":{},\"threads\":{},\"procCount\":{},\"collectMs\":{},\"uptime\":{},\"cpuName\":\"{}\",\"procs\":[",
        num(sys.cpu as f64),
        sys.mem_total,
        sys.mem_used,
        sys.mem_avail,
        num(sys.disk_read),
        num(sys.disk_write),
        num(sys.net_recv),
        num(sys.net_sent),
        sys.handles,
        sys.threads,
        s.procs.len(),
        num(s.collect_ms),
        sys.uptime_secs,
        json_escape(&sys.cpu_name),
    ));
    for (i, p) in s.procs.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!(
            "{{\"pid\":{},\"ppid\":{},\"name\":\"{}\",\"cpu\":{},\"mem\":{},\"disk\":{},\"gpu\":{},\"handles\":{},\"threads\":{},\"status\":\"{}\",\"path\":\"{}\",\"cmd\":\"{}\",\"signed\":\"{}\",\"user\":\"{}\",\"hist\":[{}]}}",
            p.pid,
            p.ppid,
            json_escape(&p.name),
            num(p.cpu as f64),
            p.mem,
            num(p.disk_read_rate + p.disk_write_rate),
            num(p.gpu as f64),
            p.handles,
            p.threads,
            json_escape(&p.status),
            json_escape(&p.path),
            json_escape(&p.cmdline),
            json_escape(p.signed.label()),
            json_escape(&p.user),
            hist_json(&p.cpu_hist),
        ));
    }
    o.push_str(&format!("],\"serviceCount\":{},\"startupCount\":{},\"userCount\":{}}}",
        s.services.len(), s.startups.len(), s.users.len()));
    o
}

/// Returns a freshly allocated UTF-8 JSON snapshot. The caller must release it
/// with `rtm_free_str`. Returns null on failure.
#[no_mangle]
pub extern "C" fn rtm_snapshot_json() -> *mut c_char {
    let json = with_engine(|e| {
        let cfg = Config::default();
        let snap = e.collector.collect(&cfg, &mut e.history);
        snapshot_json(&snap)
    });
    match json.and_then(|s| CString::new(s).ok()) {
        Some(s) => s.into_raw(),
        None => std::ptr::null_mut(),
    }
}

/// Frees a string returned by `rtm_snapshot_json`.
#[no_mangle]
pub extern "C" fn rtm_free_str(p: *mut c_char) {
    if !p.is_null() {
        unsafe {
            drop(CString::from_raw(p));
        }
    }
}

/// Terminates a single process. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn rtm_kill(pid: u32) -> i32 {
    if actions::kill(pid) { 1 } else { 0 }
}

/// Terminates a process and all of its descendants. Returns the number killed.
#[no_mangle]
pub extern "C" fn rtm_kill_tree(pid: u32) -> i32 {
    let n = with_engine(|e| {
        // Build a lightweight snapshot only when the tree action is requested.
        let cfg = Config::default();
        let snap = e.collector.collect(&cfg, &mut e.history);
        actions::kill_tree(&snap, pid)
    })
    .unwrap_or(0);
    n as i32
}

/// Sets process priority. level: 0=idle 1=below normal 2=normal 3=above normal 4=high.
#[no_mangle]
pub extern "C" fn rtm_set_priority(pid: u32, level: u32) -> i32 {
    if actions::set_priority(pid, level) { 1 } else { 0 }
}

/// Returns the engine ABI version (bump whenever the JSON schema changes).
#[no_mangle]
pub extern "C" fn rtm_abi_version() -> u32 {
    1
}

// Keep the C ABI honest about the calling convention used for the opaque
// pointer helpers; this symbol is used by interop smoke tests.
#[no_mangle]
pub extern "C" fn rtm_noop(_p: *mut c_void) {}
