//! System data acquisition. All metrics come from public Win32/NT APIs.
//!
//! Design constraints:
//!  * First frame must be cheap: one NtQuerySystemInformation call plus a few
//!    scalar APIs; no per-process handle open, no WMI, no registry sweep.
//!  * Expensive per-process details (image path, command line, signature,
//!    architecture) are filled in progressively, at most N per refresh.
//!  * Services / startup / sessions are refreshed on a slow cadence.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::ptr;

use crate::ffi::native::*;
use crate::ffi::*;
use crate::model::*;
use crate::table::HistoryTracker;

type NtQsiFn = unsafe extern "system" fn(u32, *mut c_void, u32, *mut u32) -> i32;
type NtQipFn = unsafe extern "system" fn(HANDLE, u32, *mut c_void, u32, *mut u32) -> i32;
type WtsEnumerateFn = unsafe extern "system" fn(HANDLE, u32, u32, *mut *mut WtsSessionInfoW, *mut u32) -> BOOL;
type WtsQueryFn = unsafe extern "system" fn(HANDLE, u32, i32, *mut *mut u16, *mut u32) -> BOOL;
type WtsFreeFn = unsafe extern "system" fn(*mut c_void);
type PdhOpenFn = unsafe extern "system" fn(PCWSTR, usize, *mut *mut c_void) -> u32;
type PdhAddFn = unsafe extern "system" fn(*mut c_void, PCWSTR, usize, *mut *mut c_void) -> u32;
type PdhCollectFn = unsafe extern "system" fn(*mut c_void) -> u32;
type PdhArrayFn = unsafe extern "system" fn(*mut c_void, u32, *mut u32, *mut u32, *mut u8) -> u32;

const SYSTEM_PROCESS_INFORMATION: u32 = 5;
const SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION: u32 = 8;
const STATUS_INFO_LENGTH_MISMATCH: i32 = unchecked_i32(0xC000_0004u32);
const fn unchecked_i32(v: u32) -> i32 {
    v as i32
}
const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
const ALL_PROCESSOR_GROUPS: u16 = 0xFFFF;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CorePerf {
    IdleTime: i64,
    KernelTime: i64,
    UserTime: i64,
    DpcTime: i64,
    InterruptTime: i64,
    InterruptCount: u32,
}

#[derive(Clone, Default)]
struct PrevProc {
    cpu_time: u64,
    read: u64,
    write: u64,
}

#[derive(Clone, Default)]
struct ProcDetail {
    create_time: i64,
    path: String,
    cmdline: String,
    signed: SignState,
    arch: String,
}

struct GpuMonitor {
    collect_fn: Option<PdhCollectFn>,
    array_fn: Option<PdhArrayFn>,
    query: *mut c_void,
    counter: *mut c_void,
    ok: bool,
}
impl Default for GpuMonitor {
    fn default() -> Self {
        GpuMonitor { collect_fn: None, array_fn: None, query: ptr::null_mut(), counter: ptr::null_mut(), ok: false }
    }
}
impl GpuMonitor {
    fn init() -> GpuMonitor {
        let mut g = GpuMonitor::default();
        unsafe {
            let open: Option<PdhOpenFn> = call_fn(dyn_proc("pdh.dll", "PdhOpenQueryW"));
            let add: Option<PdhAddFn> = call_fn(dyn_proc("pdh.dll", "PdhAddEnglishCounterW"));
            let collect_fn: Option<PdhCollectFn> = call_fn(dyn_proc("pdh.dll", "PdhCollectQueryData"));
            let array_fn: Option<PdhArrayFn> = call_fn(dyn_proc("pdh.dll", "PdhGetFormattedCounterArrayW"));
            let (open, add, collect_fn, array_fn) = match (open, add, collect_fn, array_fn) {
                (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
                _ => return g,
            };
            let mut q: *mut c_void = ptr::null_mut();
            if open(ptr::null(), 0, &mut q) != 0 || q.is_null() {
                return g;
            }
            let path = wide("\\GPU Engine(*)\\Utilization Percentage");
            let mut c: *mut c_void = ptr::null_mut();
            if add(q, path.as_ptr(), 0, &mut c) != 0 || c.is_null() {
                return g;
            }
            g.query = q;
            g.counter = c;
            g.collect_fn = Some(collect_fn);
            g.array_fn = Some(array_fn);
            g.ok = true;
        }
        g
    }
    /// Returns per-PID GPU utilisation. Fails silently to 0 when PDH or the
    /// GPU engine counters are not available (older drivers, remote sessions).
    fn sample(&self) -> HashMap<u32, f32> {
        let mut out = HashMap::new();
        if !self.ok {
            return out;
        }
        unsafe {
            let collect = self.collect_fn.unwrap();
            let array = self.array_fn.unwrap();
            if collect(self.query) != 0 {
                return out;
            }
            let mut bytes: u32 = 0;
            let mut items: u32 = 0;
            let r = array(self.counter, k::PDH_FMT_DOUBLE, &mut bytes, &mut items, ptr::null_mut());
            if r != k::PDH_MORE_DATA || bytes == 0 || bytes > 8 * 1024 * 1024 {
                return out;
            }
            let mut buf = vec![0u8; bytes as usize];
            let r = array(self.counter, k::PDH_FMT_DOUBLE, &mut bytes, &mut items, buf.as_mut_ptr());
            if r != 0 {
                return out;
            }
            let item_size = std::mem::size_of::<PdhFmtCounterValueItemW>();
            if item_size == 0 {
                return out;
            }
            for i in 0..items as usize {
                let off = i * item_size;
                if off + item_size > buf.len() {
                    break;
                }
                let item = &*(buf.as_ptr().add(off) as *const PdhFmtCounterValueItemW);
                if item.szName.is_null() || item.FmtValue.CStatus != k::PDH_CSTATUS_VALID_DATA {
                    continue;
                }
                let name = from_wide(item.szName);
                if let Some(pos) = name.find("pid_") {
                    let digits: String = name[pos + 4..].chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(pid) = digits.parse::<u32>() {
                        let v = item.FmtValue.as_f64() as f32;
                        *out.entry(pid).or_insert(0.0) += v;
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Collector
// ---------------------------------------------------------------------------

// Raw PDH/DWM handles are process-wide and only ever touched by the single
// collection thread, so moving the collector to that thread is sound.
unsafe impl Send for Collector {}

pub struct Collector {
    nt_qsi: Option<NtQsiFn>,
    nt_qip: Option<NtQipFn>,
    prev: HashMap<u32, PrevProc>,
    cpu_hist: HashMap<u32, std::collections::VecDeque<f32>>,
    prev_cores: Vec<(u64, u64)>,
    prev_wall: u64,
    prev_idle: u64,
    prev_total: u64,
    prev_net_in: u64,
    prev_net_out: u64,
    prev_net_stamp: u64,
    details: HashMap<u32, ProcDetail>,
    session_users: HashMap<u32, (String, String)>,
    services_cache: Vec<Service>,
    service_meta: HashMap<String, (String, String)>,
    startups_cache: Vec<StartupItem>,
    users_cache: Vec<UserSession>,
    cpu_name: String,
    cpu_mhz: u32,
    qpc_freq: f64,
    tick: u32,
    gpu: Option<GpuMonitor>,
}

impl Collector {
    pub fn new() -> Collector {
        let mut qpc: i64 = 0;
        unsafe { QueryPerformanceFrequency(&mut qpc) };
        let (cpu_name, cpu_mhz) = cpu_identity();
        Collector {
            nt_qsi: unsafe { call_fn(dyn_proc("ntdll.dll", "NtQuerySystemInformation")) },
            nt_qip: unsafe { call_fn(dyn_proc("ntdll.dll", "NtQueryInformationProcess")) },
            prev: HashMap::new(),
            cpu_hist: HashMap::new(),
            prev_cores: Vec::new(),
            prev_wall: 0,
            prev_idle: 0,
            prev_total: 0,
            prev_net_in: 0,
            prev_net_out: 0,
            prev_net_stamp: 0,
            details: HashMap::new(),
            session_users: HashMap::new(),
            services_cache: Vec::new(),
            service_meta: HashMap::new(),
            startups_cache: Vec::new(),
            users_cache: Vec::new(),
            cpu_name,
            cpu_mhz,
            qpc_freq: if qpc > 0 { qpc as f64 } else { 1.0 },
            tick: 0,
            gpu: None, // PDH is initialised off the startup path (frame 3+)
        }
    }

    fn qpc_now(&self) -> f64 {
        let mut v: i64 = 0;
        unsafe { QueryPerformanceCounter(&mut v) };
        v as f64 / self.qpc_freq * 1000.0
    }

    /// Produce one complete snapshot.
    pub fn collect(&mut self, cfg: &Config, history: &mut HistoryTracker) -> Snapshot {
        let t0 = self.qpc_now();
        self.tick = self.tick.wrapping_add(1);
        // Frame 1 must stay cheap: no SCM sweep, no registry enumeration.
        // Slow sources arrive on frame 2 (one refresh later) and then every 10.
        let slow = self.tick == 2 || (self.tick > 2 && self.tick % 10 == 0);

        let wall = wall_100ns();
        let dt_100ns = if self.prev_wall == 0 { 1000_000 } else { wall.saturating_sub(self.prev_wall).max(1) };
        let dt_secs = dt_100ns as f64 / 10_000_000.0;

        let (sys_cpu, cpu_cores, idle, total) = cpu_times(&mut self.prev_cores, self.prev_idle, self.prev_total, self.nt_qsi);
        self.prev_idle = idle;
        self.prev_total = total;

        let (mem, perf) = memory_info();
        let (net_in, net_out, adapters) = network_totals();
        let (net_recv, net_sent) = if self.prev_net_stamp == 0 {
            (0.0, 0.0)
        } else {
            let d = wall.saturating_sub(self.prev_net_stamp).max(1) as f64 / 10_000_000.0;
            (
                (net_in.wrapping_sub(self.prev_net_in) as f64 / d).max(0.0).min(1e12),
                (net_out.wrapping_sub(self.prev_net_out) as f64 / d).max(0.0).min(1e12),
            )
        };
        self.prev_net_in = net_in;
        self.prev_net_out = net_out;
        self.prev_net_stamp = wall;

        if slow {
            self.session_users = wts_users();
            self.services_cache = enum_services(&mut self.service_meta);
            self.startups_cache = enum_startups();
        }

        let app_pids = visible_window_pids();
        let gpu_map = if !cfg.gpu || cfg.low_power || self.tick <= 2 {
            HashMap::new()
        } else {
            if self.gpu.is_none() {
                self.gpu = Some(GpuMonitor::init());
            }
            self.gpu.as_ref().map(|g| g.sample()).unwrap_or_default()
        };
        let raw = self.raw_processes();
        let mut procs = Vec::with_capacity(raw.len());
        let mut next_prev: HashMap<u32, PrevProc> = HashMap::with_capacity(raw.len());
        let mut disk_read_sum: f64 = 0.0;
        let mut disk_write_sum: f64 = 0.0;

        for rp in raw {
            let cpu_time = rp.user_time.saturating_add(rp.kernel_time);
            let (cpu, dr, dw) = match self.prev.get(&rp.pid) {
                Some(p) if cpu_time >= p.cpu_time => {
                    let d_cpu = cpu_time - p.cpu_time;
                    let c = (d_cpu as f64 * 100.0 / (dt_100ns as f64 * logical_cpus() as f64)).min(100.0 * logical_cpus() as f64) as f32;
                    let r = (rp.read.saturating_sub(p.read) as f64 / dt_secs).max(0.0);
                    let w = (rp.write.saturating_sub(p.write) as f64 / dt_secs).max(0.0);
                    (c, r, w)
                }
                _ => (0.0, 0.0, 0.0),
            };
            disk_read_sum += dr;
            disk_write_sum += dw;
            next_prev.insert(rp.pid, PrevProc { cpu_time, read: rp.read, write: rp.write });
            let (path, cmdline, signed, arch, user) = match self.details.get(&rp.pid) {
                Some(d) if d.create_time == rp.create_time => (
                    d.path.clone(),
                    d.cmdline.clone(),
                    d.signed,
                    d.arch.clone(),
                    self.session_users.get(&rp.session_id).map(|(u, _)| u.clone()).unwrap_or_else(|| "-".into()),
                ),
                _ => (
                    String::new(),
                    String::new(),
                    SignState::Unknown,
                    String::new(),
                    self.session_users.get(&rp.session_id).map(|(u, _)| u.clone()).unwrap_or_else(|| "-".into()),
                ),
            };
            let is_app = app_pids.contains(&rp.pid);
            let hist = {
                let q = self.cpu_hist.entry(rp.pid).or_default();
                q.push_back(cpu);
                while q.len() > 16 {
                    q.pop_front();
                }
                q.iter().copied().collect::<Vec<f32>>()
            };
            procs.push(Process {
                pid: rp.pid,
                ppid: rp.ppid,
                name: rp.name,
                path,
                cmdline,
                user,
                cmd_loaded: false,
                cpu,
                cpu_time,
                mem: rp.working_set,
                private_bytes: rp.private_bytes,
                commit: rp.commit,
                virtual_bytes: rp.virtual_size,
                disk_read: rp.read,
                disk_write: rp.write,
                disk_read_rate: dr,
                disk_write_rate: dw,
                gpu: gpu_map.get(&rp.pid).copied().unwrap_or(0.0),
                net_conns: 0,
                handles: rp.handles,
                threads: rp.threads,
                session_id: rp.session_id,
                create_time: rp.create_time,
                priority: rp.priority,
                signed,
                is_app,
                status: if is_app { "应用".into() } else { "后台".into() },
                arch,
                cpu_hist: hist,
                ..Default::default()
            });
        }
        self.cpu_hist.retain(|pid, _| next_prev.contains_key(pid));
        self.prev = next_prev;
        self.prev_wall = wall;

        // Progressive enrichment: bounded work per refresh keeps first paint fast.
        let missing: Vec<(u32, i64)> = procs
            .iter()
            .filter(|p| self.details.get(&p.pid).map(|d| d.create_time != p.create_time).unwrap_or(true))
            .map(|p| (p.pid, p.create_time))
            .take(if self.tick <= 1 { 0 } else if cfg.low_power { 4 } else { 40 })
            .collect();
        for (pid, create_time) in missing {
            let d = load_detail(pid, create_time, self.nt_qip);
            self.details.insert(pid, d);
        }

        let uptime = unsafe { GetTickCount64() } / 1000;
        // Resolve each startup item against the live process table: creation
        // time gives "启动时间", the counters feed the impact estimate.
        for s in &mut self.startups_cache {
            if s.exe.is_empty() {
                s.exe = exe_from_command(&s.command);
            }
            match startup_process(&procs, &s.exe) {
                Some(p) => {
                    s.start_time = p.create_time;
                    s.cpu_secs = p.cpu_time as f64 / 10_000_000.0;
                    s.disk_bytes = p.disk_read.saturating_add(p.disk_write);
                    s.mem_bytes = p.mem;
                    let cpu_ratio = if uptime > 0 { s.cpu_secs / uptime as f64 } else { 0.0 };
                    let disk_rate = p.disk_read_rate + p.disk_write_rate;
                    let size = std::fs::metadata(&s.exe).map(|m| m.len()).unwrap_or(0);
                    s.impact_level = grade_impact(cpu_ratio, disk_rate, p.mem, size);
                }
                None => {
                    s.start_time = 0;
                    s.cpu_secs = 0.0;
                    s.disk_bytes = 0;
                    s.mem_bytes = 0;
                    // Not running: fall back to a static estimate from the
                    // image size so the column is still informative.
                    let size = if s.exe.is_empty() {
                        0
                    } else {
                        std::fs::metadata(&s.exe).map(|m| m.len()).unwrap_or(0)
                    };
                    s.impact_level = if size > 100 * 1024 * 1024 {
                        1
                    } else if size > 0 {
                        0
                    } else {
                        3
                    };
                }
            }
        }
        let mut sys = SysInfo {
            cpu: sys_cpu,
            cpu_cores,
            logical_processors: logical_cpus(),
            mem_total: mem.ullTotalPhys,
            mem_avail: mem.ullAvailPhys,
            mem_used: mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys),
            mem_load: mem.dwMemoryLoad,
            commit: (perf.CommitTotal * perf.PageSize) as u64,
            commit_limit: (perf.CommitLimit * perf.PageSize) as u64,
            cached: (perf.SystemCache * perf.PageSize) as u64,
            disk_read: disk_read_sum,
            disk_write: disk_write_sum,
            disk_active: ((disk_read_sum + disk_write_sum) / (20.0 * 1024.0 * 1024.0) * 100.0).min(100.0) as f32,
            net_recv,
            net_sent,
            net_total_recv: net_in,
            net_total_sent: net_out,
            handles: perf.HandleCount,
            threads: perf.ThreadCount,
            processes: perf.ProcessCount,
            uptime_secs: uptime,
            boot_unix: unix_now().saturating_sub(uptime),
            gpu: gpu_map.values().copied().sum::<f32>().min(100.0),
            page_size: perf.PageSize as u64,
            cpu_name: self.cpu_name.clone(),
            cpu_base_mhz: self.cpu_mhz,
            adapters,
            ..Default::default()
        };
        sys.disks = disks();
        let stamp = unix_now();
        let mut snap = Snapshot {
            stamp,
            procs,
            sys,
            services: self.services_cache.clone(),
            startups: self.startups_cache.clone(),
            users: Vec::new(),
            history: Vec::new(),
            collect_ms: 0.0,
        };
        snap.users = aggregate_users(&snap, &self.session_users);
        snap.history = history.ingest(&snap);
        snap.collect_ms = self.qpc_now() - t0;
        snap
    }
}

// ---------------------------------------------------------------------------
// Process enumeration via NtQuerySystemInformation
// ---------------------------------------------------------------------------

struct RawProc {
    pid: u32,
    ppid: u32,
    name: String,
    user_time: u64,
    kernel_time: u64,
    read: u64,
    write: u64,
    working_set: u64,
    private_bytes: u64,
    commit: u64,
    virtual_size: u64,
    handles: u32,
    threads: u32,
    session_id: u32,
    create_time: i64,
    priority: i32,
}

impl Collector {
    fn raw_processes(&self) -> Vec<RawProc> {
        let f = match self.nt_qsi {
            Some(f) => f,
            None => return Vec::new(),
        };
        let mut out = Vec::new();
        unsafe {
            let mut size: usize = 1 << 20;
            let mut buf: Vec<u8> = Vec::new();
            let mut ret: u32 = 0;
            for _ in 0..8 {
                buf.resize(size, 0);
                let st = f(SYSTEM_PROCESS_INFORMATION, buf.as_mut_ptr() as *mut c_void, size as u32, &mut ret);
                if st == 0 {
                    break;
                }
                if st != STATUS_INFO_LENGTH_MISMATCH {
                    return out;
                }
                size = (ret as usize + 0x10000).max(size * 2);
            }
            let used = (ret as usize).min(buf.len());
            let base = buf.as_ptr() as usize;
            let end = base + used;
            let mut off = 0usize;
            let item = std::mem::size_of::<SystemProcessInformation>();
            while off + item <= used {
                let p = &*(buf.as_ptr().add(off) as *const SystemProcessInformation);
                let name = read_image_name(p, base, end);
                let pid = p.UniqueProcessId as u32;
                let ppid = p.InheritedFromUniqueProcessId as u32;
                out.push(RawProc {
                    pid,
                    ppid,
                    name,
                    user_time: p.UserTime.max(0) as u64,
                    kernel_time: p.KernelTime.max(0) as u64,
                    read: p.ReadTransferCount.max(0) as u64,
                    write: p.WriteTransferCount.max(0) as u64,
                    working_set: p.WorkingSetSize as u64,
                    private_bytes: p.PrivatePageCount as u64,
                    commit: p.PagefileUsage as u64,
                    virtual_size: p.VirtualSize as u64,
                    handles: p.HandleCount,
                    threads: p.NumberOfThreads,
                    session_id: p.SessionId,
                    create_time: p.CreateTime,
                    priority: p.BasePriority,
                });
                if p.NextEntryOffset == 0 {
                    break;
                }
                off += p.NextEntryOffset as usize;
            }
        }
        out
    }
}

unsafe fn read_image_name(p: &SystemProcessInformation, base: usize, end: usize) -> String {
    let b = p.ImageName.Buffer as usize;
    let len = p.ImageName.Length as usize;
    if b == 0 || len == 0 || b < base || b + len > end {
        return String::from("[系统]");
    }
    from_wide_n(b as *const u16, len / 2)
}

fn load_detail(pid: u32, create_time: i64, nt_qip: Option<NtQipFn>) -> ProcDetail {
    let mut d = ProcDetail { create_time, ..Default::default() };
    unsafe {
        let h = OpenProcess(k::PROCESS_QUERY_LIMITED_INFORMATION | k::PROCESS_VM_READ, 0, pid);
        if h.is_null() {
            d.path = String::from("[拒绝访问]");
            d.cmdline = String::from("[拒绝访问]");
            d.arch = "-".into();
            return d;
        }
        let mut path = vec![0u16; k::MAX_PATH_W];
        let mut len = path.len() as u32;
        if QueryFullProcessImageNameW(h, 0, path.as_mut_ptr(), &mut len) != 0 {
            d.path = from_wide(path.as_ptr());
        }
        if let Some(qip) = nt_qip {
            let mut buf = vec![0u8; 8192];
            let mut ret: u32 = 0;
            if qip(h, PROCESS_COMMAND_LINE_INFORMATION, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut ret) == 0 {
                let us = &*(buf.as_ptr() as *const UnicodeString);
                let n = us.Length as usize / 2;
                let ptr = us.Buffer as usize;
                if ptr != 0 && n > 0 && ptr >= buf.as_ptr() as usize && ptr + n * 2 <= buf.as_ptr() as usize + buf.len() {
                    d.cmdline = from_wide_n(ptr as *const u16, n);
                }
            }
        }
        if d.cmdline.is_empty() {
            d.cmdline = d.path.clone();
        }
        let mut wow: BOOL = 0;
        d.arch = if IsWow64Process(h, &mut wow) != 0 && wow != 0 { "x86".into() } else { "x64".into() };
        d.signed = if d.path.is_empty() {
            SignState::Unknown
        } else if pe_has_signature(&d.path) {
            SignState::Signed
        } else {
            SignState::Unsigned
        };
        CloseHandle(h);
    }
    d
}

// ---------------------------------------------------------------------------
// CPU / memory / network / disks
// ---------------------------------------------------------------------------

fn logical_cpus() -> u32 {
    std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1)
}

fn wall_100ns() -> u64 {
    let mut ft = FILETIME::default();
    unsafe { GetSystemTimeAsFileTime(&mut ft) };
    ft.as_u64()
}

pub fn unix_now() -> u64 {
    (wall_100ns() / 10_000_000).saturating_sub(11_644_473_600)
}

fn cpu_times(prev_cores: &mut Vec<(u64, u64)>, prev_idle: u64, prev_total: u64, nt_qsi: Option<NtQsiFn>) -> (f32, Vec<f32>, u64, u64) {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    let i = idle.as_u64();
    let t = kernel.as_u64().saturating_add(user.as_u64());
    let sys_cpu = if prev_total > 0 && t > prev_total {
        let busy = (t - i).saturating_sub(prev_total.saturating_sub(prev_idle));
        (busy as f64 * 100.0 / (t - prev_total) as f64).min(100.0) as f32
    } else {
        0.0
    };

    let mut cores = Vec::new();
    if let Some(f) = nt_qsi {
        let n = logical_cpus() as usize;
        let size = n * std::mem::size_of::<CorePerf>();
        let mut buf = vec![0u8; size.max(64)];
        let mut ret: u32 = 0;
        unsafe {
            if f(SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut ret) == 0 {
                let item = std::mem::size_of::<CorePerf>();
                let count = (ret as usize / item).min(n);
                while prev_cores.len() < count {
                    prev_cores.push((0, 0));
                }
                for c in 0..count {
                    let p = &*(buf.as_ptr().add(c * item) as *const CorePerf);
                    let ci = p.IdleTime.max(0) as u64;
                    let ct = (p.KernelTime.max(0) + p.UserTime.max(0)) as u64;
                    let (pi, pt) = prev_cores[c];
                    let v = if pt > 0 && ct > pt {
                        let busy = (ct - ci).saturating_sub(pt.saturating_sub(pi));
                        (busy as f64 * 100.0 / (ct - pt) as f64).min(100.0) as f32
                    } else {
                        0.0
                    };
                    cores.push(v);
                    prev_cores[c] = (ci, ct);
                }
            }
        }
    }
    (sys_cpu, cores, i, t)
}

fn memory_info() -> (MemoryStatusEx, PerformanceInformation) {
    let mut m = MemoryStatusEx::default();
    let mut p = PerformanceInformation::default();
    p.cb = std::mem::size_of::<PerformanceInformation>() as u32;
    unsafe {
        GlobalMemoryStatusEx(&mut m);
        GetPerformanceInfo(&mut p, p.cb);
    }
    (m, p)
}

fn network_totals() -> (u64, u64, Vec<AdapterInfo>) {
    let mut size: u32 = 0;
    unsafe { GetIfTable(ptr::null_mut(), &mut size, 0) };
    if size == 0 || size > 4 * 1024 * 1024 {
        return (0, 0, Vec::new());
    }
    let mut buf = vec![0u8; size as usize];
    unsafe {
        if GetIfTable(buf.as_mut_ptr(), &mut size, 0) != 0 {
            return (0, 0, Vec::new());
        }
    }
    let count = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let row_size = std::mem::size_of::<MIB_IFROW>();
    let mut in_total: u64 = 0;
    let mut out_total: u64 = 0;
    let mut adapters = Vec::new();
    for i in 0..count.min(256) {
        let off = 4 + i * row_size;
        if off + row_size > buf.len() {
            break;
        }
        unsafe {
            let row = &*(buf.as_ptr().add(off) as *const MIB_IFROW);
            in_total = in_total.wrapping_add(row.dwInOctets as u64);
            out_total = out_total.wrapping_add(row.dwOutOctets as u64);
            let name = from_wide(row.wszName.as_ptr());
            if !name.is_empty() && adapters.len() < 12 {
                adapters.push(AdapterInfo { name, speed: row.dwSpeed as u64, ..Default::default() });
            }
        }
    }
    (in_total, out_total, adapters)
}

fn disks() -> Vec<DiskInfo> {
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
    let drive_strings = unsafe { from_wide_n(buf.as_ptr(), n.min(buf.len())) };
    let mut out = Vec::new();
    for part in drive_strings.split('\0').filter(|s| !s.is_empty()) {
        let w = wide(part);
        let dtype = unsafe { GetDriveTypeW(w.as_ptr()) };
        if dtype != 3 {
            continue; // DRIVE_FIXED only
        }
        let (mut free, mut total, mut free_total) = (0u64, 0u64, 0u64);
        unsafe {
            if GetDiskFreeSpaceExW(w.as_ptr(), &mut free, &mut total, &mut free_total) != 0 && total > 0 {
                out.push(DiskInfo { letter: part.trim_end_matches('\\').to_string(), total, free, ..Default::default() });
            }
        }
    }
    out
}

fn cpu_identity() -> (String, u32) {
    let sub = wide("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0");
    let mut h: HKEY = ptr::null_mut();
    let mut name = String::from("未知处理器");
    let mut mhz = 0u32;
    unsafe {
        if RegOpenKeyExW(k::HKEY_LOCAL_MACHINE as HKEY, sub.as_ptr(), 0, k::KEY_READ, &mut h) == 0 {
            name = reg_string(h, "ProcessorNameString").filter(|s| !s.is_empty()).unwrap_or(name);
            mhz = reg_dword(h, "~MHz");
            RegCloseKey(h);
        }
    }
    (name.trim().to_string(), mhz)
}

// ---------------------------------------------------------------------------
// WTS users
// ---------------------------------------------------------------------------

fn wts_users() -> HashMap<u32, (String, String)> {
    let mut out = HashMap::new();
    unsafe {
        let enumerate: Option<WtsEnumerateFn> = call_fn(dyn_proc("wtsapi32.dll", "WTSEnumerateSessionsW"));
        let query: Option<WtsQueryFn> = call_fn(dyn_proc("wtsapi32.dll", "WTSQuerySessionInformationW"));
        let free: Option<WtsFreeFn> = call_fn(dyn_proc("wtsapi32.dll", "WTSFreeMemory"));
        let (enumerate, query, free) = match (enumerate, query, free) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => return out,
        };
        let mut sessions: *mut WtsSessionInfoW = ptr::null_mut();
        let mut count: u32 = 0;
        if enumerate(ptr::null_mut(), 0, 1, &mut sessions, &mut count) == 0 || sessions.is_null() {
            return out;
        }
        let item = std::mem::size_of::<WtsSessionInfoW>();
        for i in 0..count as usize {
            let s = &*((sessions as *const u8).add(i * item) as *const WtsSessionInfoW);
            let mut user = wts_string(query, free, s.SessionId, k::WTSUserName);
            let domain = wts_string(query, free, s.SessionId, k::WTSDomainName);
            if user.is_empty() {
                user = format!("会话 {}", s.SessionId);
            }
            out.insert(s.SessionId, (user, domain));
        }
        free(sessions as *mut c_void);
    }
    out
}

unsafe fn wts_string(query: WtsQueryFn, free: WtsFreeFn, session: u32, class: i32) -> String {
    let mut buf: *mut u16 = ptr::null_mut();
    let mut bytes: u32 = 0;
    if query(ptr::null_mut(), session, class, &mut buf, &mut bytes) == 0 || buf.is_null() {
        return String::new();
    }
    let s = from_wide(buf);
    free(buf as *mut c_void);
    s
}

fn aggregate_users(snap: &Snapshot, names: &HashMap<u32, (String, String)>) -> Vec<UserSession> {
    let mut map: HashMap<u32, UserSession> = HashMap::new();
    for p in &snap.procs {
        let e = map.entry(p.session_id).or_insert_with(|| {
            let (name, domain) = names.get(&p.session_id).cloned().unwrap_or_else(|| (format!("会话 {}", p.session_id), String::new()));
            UserSession { name, domain, session_id: p.session_id, state: "活动".into(), ..Default::default() }
        });
        e.cpu += p.cpu;
        e.mem = e.mem.saturating_add(p.mem);
        e.disk += p.disk_read_rate + p.disk_write_rate;
        e.procs += 1;
    }
    let mut v: Vec<UserSession> = map.into_values().collect();
    v.sort_by(|a, b| a.session_id.cmp(&b.session_id));
    v
}

// ---------------------------------------------------------------------------
// Services / startup items (registry + SCM)
// ---------------------------------------------------------------------------

fn enum_services(cache: &mut HashMap<String, (String, String)>) -> Vec<Service> {
    let mut out = Vec::new();
    unsafe {
        let sc = OpenSCManagerW(ptr::null(), ptr::null(), k::SC_MANAGER_ENUMERATE_SERVICE);
        if sc.is_null() {
            return out;
        }
        let mut need: u32 = 0;
        let mut count: u32 = 0;
        let mut resume: u32 = 0;
        EnumServicesStatusExW(sc, 0, k::SERVICE_WIN32, k::SERVICE_STATE_ALL, ptr::null_mut(), 0, &mut need, &mut count, &mut resume, ptr::null());
        if need == 0 || need > 8 * 1024 * 1024 {
            CloseServiceHandle(sc);
            return out;
        }
        let mut buf = vec![0u8; need as usize];
        let ok = EnumServicesStatusExW(sc, 0, k::SERVICE_WIN32, k::SERVICE_STATE_ALL, buf.as_mut_ptr(), need, &mut need, &mut count, &mut resume, ptr::null());
        if ok != 0 {
            let item = std::mem::size_of::<EnumServiceStatusProcessW>();
            for i in 0..count as usize {
                let off = i * item;
                if off + item > buf.len() {
                    break;
                }
                let e = &*(buf.as_ptr().add(off) as *const EnumServiceStatusProcessW);
                let name = from_wide(e.lpServiceName);
                let display = from_wide(e.lpDisplayName);
                // Registry metadata for ~200 services is expensive the first
                // time; cache it by service name so slow frames stay cheap.
                let (start_type, description) = match cache.get(&name) {
                    Some(v) => v.clone(),
                    None => {
                        let v = service_registry_meta(&name);
                        cache.insert(name.clone(), v.clone());
                        v
                    }
                };
                out.push(Service {
                    name,
                    display,
                    pid: e.ServiceStatusProcess.dwProcessId,
                    state: e.ServiceStatusProcess.dwCurrentState,
                    start_type,
                    description,
                });
            }
        }
        CloseServiceHandle(sc);
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

fn service_registry_meta(name: &str) -> (String, String) {
    let sub = wide(&format!("SYSTEM\\CurrentControlSet\\Services\\{}", name));
    let mut h: HKEY = ptr::null_mut();
    let mut start = "未知".to_string();
    let mut desc = String::new();
    unsafe {
        if RegOpenKeyExW(k::HKEY_LOCAL_MACHINE as HKEY, sub.as_ptr(), 0, k::KEY_READ, &mut h) == 0 {
            start = match reg_dword(h, "Start") {
                0 | 1 => "系统",
                2 => "自动",
                3 => "手动",
                4 => "禁用",
                _ => "未知",
            }
            .to_string();
            desc = reg_string(h, "Description").unwrap_or_default();
            RegCloseKey(h);
        }
    }
    (start, desc)
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_ONCE_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce";
const APPROVED_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved";

fn enum_startups() -> Vec<StartupItem> {
    let sources: [(isize, &str, u32, &str, &str); 4] = [
        (k::HKEY_CURRENT_USER, RUN_KEY, 0, "HKCU\\Run", "Run"),
        (k::HKEY_LOCAL_MACHINE, RUN_KEY, k::KEY_WOW64_64KEY, "HKLM\\Run", "Run"),
        (k::HKEY_LOCAL_MACHINE, RUN_KEY, k::KEY_WOW64_32KEY, "HKLM\\Run32", "Run32"),
        (k::HKEY_CURRENT_USER, RUN_ONCE_KEY, 0, "HKCU\\RunOnce", "Run"),
    ];
    let mut out = Vec::new();
    let disabled = approved_disabled();
    for (root, sub, view, label, approved_sub) in sources {
        let w = wide(sub);
        let mut h: HKEY = ptr::null_mut();
        unsafe {
            if RegOpenKeyExW(root as HKEY, w.as_ptr(), 0, k::KEY_READ | view, &mut h) != 0 {
                continue;
            }
            for (name, command) in reg_enum_string_values(h) {
                let is_disabled = disabled.contains(&(approved_sub.to_string(), name.clone()));
                out.push(StartupItem {
                    name,
                    command,
                    source: label.to_string(),
                    enabled: !is_disabled,
                    publisher: "-".into(),
                    reg_key: if approved_sub == "Run32" { 2 } else { 0 },
                    exe: String::new(),
                    start_time: 0,
                    impact_level: 3,
                    cpu_secs: 0.0,
                    disk_bytes: 0,
                    mem_bytes: 0,
                });
            }
            RegCloseKey(h);
        }
    }
    // Startup folders
    for (dir, label) in startup_folders() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
                if ext != "lnk" && ext != "exe" && ext != "bat" {
                    continue;
                }
                let name = p.file_stem().and_then(|x| x.to_str()).unwrap_or("").to_string();
                if name.is_empty() {
                    continue;
                }
                let is_disabled = disabled.contains(&("StartupFolder".to_string(), p.file_name().and_then(|x| x.to_str()).unwrap_or("").to_string()));
                out.push(StartupItem {
                    name,
                    command: p.to_string_lossy().to_string(),
                    source: label.to_string(),
                    enabled: !is_disabled,
                    publisher: "-".into(),
                    reg_key: 4,
                    exe: String::new(),
                    start_time: 0,
                    impact_level: 3,
                    cpu_secs: 0.0,
                    disk_bytes: 0,
                    mem_bytes: 0,
                });
            }
        }
    }
    out
}

/// Extracts the executable path from a startup command line.
fn expand_env(cmd: &str) -> String {
    let mut out = cmd.to_string();
    for var in [
        "windir", "SystemRoot", "ProgramFiles", "ProgramFiles(x86)",
        "ProgramData", "APPDATA", "LOCALAPPDATA", "USERPROFILE",
    ] {
        if let Ok(v) = std::env::var(var) {
            out = out.replace(&format!("%{}%", var), &v);
            out = out.replace(&format!("%{}%", var.to_uppercase()), &v);
        }
    }
    out
}

pub fn exe_from_command(cmd: &str) -> String {
    let expanded = expand_env(cmd);
    let c = expanded.trim();
    if c.is_empty() {
        return String::new();
    }
    if let Some(rest) = c.strip_prefix('"') {
        return match rest.find('"') {
            Some(end) => rest[..end].to_string(),
            None => rest.to_string(),
        };
    }
    let first = c.split_whitespace().next().unwrap_or("");
    if first.to_ascii_lowercase().ends_with(".exe") {
        first.to_string()
    } else {
        String::new()
    }
}

fn startup_process<'a>(procs: &'a [Process], exe: &str) -> Option<&'a Process> {
    if exe.is_empty() {
        return None;
    }
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    procs.iter().find(|p| {
        p.name.eq_ignore_ascii_case(file) || (!p.path.is_empty() && p.path.eq_ignore_ascii_case(exe))
    })
}

/// Estimates startup impact. The OS score comes from diagnostic data that is
/// not publicly readable, so this is a documented heuristic over normalised
/// CPU time since boot, disk throughput, working set and image size.
fn grade_impact(cpu_ratio: f64, disk_rate: f64, mem: u64, size: u64) -> u8 {
    let mut score = 0i32;
    if cpu_ratio > 0.05 {
        score += 2;
    } else if cpu_ratio > 0.01 {
        score += 1;
    }
    if disk_rate > 1_000_000.0 {
        score += 2;
    } else if disk_rate > 100_000.0 {
        score += 1;
    }
    if mem > 300 * 1024 * 1024 {
        score += 2;
    } else if mem > 100 * 1024 * 1024 {
        score += 1;
    }
    if size > 50 * 1024 * 1024 {
        score += 1;
    }
    match score {
        0..=1 => 0,
        2..=3 => 1,
        _ => 2,
    }
}

// ---------------------------------------------------------------------------
// Static hardware inventory. Read once from the registry and cached: these
// values never change at runtime and must not sit in the per-second path.
// ---------------------------------------------------------------------------

pub struct HardwareInfo {
    pub board: String,
    pub bios: String,
    pub gpu: String,
    pub gpu_driver: String,
    pub disks: Vec<String>,
    pub phys_mem: u64,
    /// One entry per installed memory module, e.g. "DIMM 0  8 GB  1600 MHz".
    pub mem_modules: Vec<String>,
    /// SSD/HDM classification per physical drive, aligned with `disks`.
    pub disk_kinds: Vec<String>,
}

static HW: std::sync::OnceLock<HardwareInfo> = std::sync::OnceLock::new();

pub fn hardware() -> &'static HardwareInfo {
    HW.get_or_init(read_hardware)
}

fn reg_str(path: &str, name: &str) -> String {
    unsafe {
        let mut h: HKEY = ptr::null_mut();
        let wp = wide(path);
        if RegOpenKeyExW(k::HKEY_LOCAL_MACHINE as HKEY, wp.as_ptr(), 0, k::KEY_READ, &mut h) != 0 {
            return String::new();
        }
        let mut buf = [0u16; 512];
        let mut len = (buf.len() * 2) as u32;
        let mut ty = 0u32;
        let wn = wide(name);
        let r = RegQueryValueExW(
            h,
            wn.as_ptr(),
            ptr::null_mut(),
            &mut ty,
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        );
        RegCloseKey(h);
        if r != 0 || ty != 1 {
            return String::new();
        }
        from_wide(buf.as_ptr())
    }
}

fn reg_qword(path: &str, name: &str) -> u64 {
    unsafe {
        let mut h: HKEY = ptr::null_mut();
        let wp = wide(path);
        if RegOpenKeyExW(k::HKEY_LOCAL_MACHINE as HKEY, wp.as_ptr(), 0, k::KEY_READ, &mut h) != 0 {
            return 0;
        }
        let mut val: u64 = 0;
        let mut len = 8u32;
        let mut ty = 0u32;
        let wn = wide(name);
        let r = RegQueryValueExW(
            h,
            wn.as_ptr(),
            ptr::null_mut(),
            &mut ty,
            &mut val as *mut u64 as *mut u8,
            &mut len,
        );
        RegCloseKey(h);
        if r != 0 {
            return 0;
        }
        val
    }
}

/// Reads the string at 1-based index `n` from the string table that follows a
/// SMBIOS structure header.
fn smbios_str(header: &[u8], table: &[u8], hdr_off: usize, n: u8) -> String {
    if n == 0 {
        return String::new();
    }
    let strings = &table[hdr_off + header.len()..];
    let mut idx = 1u8;
    let mut start = 0usize;
    for (i, &b) in strings.iter().enumerate() {
        if b == 0 {
            if idx == n {
                let end = i;
                return String::from_utf8_lossy(&strings[start..end]).trim().to_string();
            }
            idx += 1;
            start = i + 1;
        }
    }
    String::new()
}

/// Trims a display string so a single detail line never overflows the panel.
fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max - 1).collect::<String>() + "\u{2026}"
}

/// Parses SMBIOS type 17 (Memory Device) entries from the raw 'RSMB' table.
fn read_memory_modules() -> Vec<String> {
    unsafe {
        const RSMB: u32 = 0x5253_4D42; // 'RSMB'
        let size = GetSystemFirmwareTable(RSMB, 0, ptr::null_mut(), 0);
        if size == 0 || size > 1 << 20 {
            return Vec::new();
        }
        let mut buf = vec![0u8; size as usize];
        let got = GetSystemFirmwareTable(RSMB, 0, buf.as_mut_ptr() as *mut c_void, size);
        if got == 0 {
            return Vec::new();
        }
        if buf.len() < 8 {
            return Vec::new();
        }
        let table = &buf[8..];
        let mut out: Vec<String> = Vec::new();
        let mut off = 0usize;
        while off + 4 <= table.len() {
            let t = table[off];
            let len = table[off + 1] as usize;
            if len < 4 || off + len > table.len() {
                break;
            }
            let hdr = &table[off..off + len];
            if t == 17 && len >= 0x17 {
                let raw = u16::from_le_bytes([hdr[0x0C], hdr[0x0D]]);
                let bytes = if raw == 0 {
                    0u64
                } else {
                    let unit = if raw & 0x8000 != 0 { 1u64 } else { 1024 };
                    ((raw & 0x7FFF) as u64) * unit
                };
                let speed = u16::from_le_bytes([hdr[0x15], hdr[0x16]]);
                if bytes > 0 {
                    let loc = smbios_str(hdr, table, off, hdr[0x10]);
                    let bank = smbios_str(hdr, table, off, hdr[0x11]);
                    let mut line = if !bank.is_empty() { bank } else { loc };
                    if line.is_empty() {
                        line = "内存".into();
                    }
                    line.push_str(&format!("  {}", fmt_bytes(bytes * 1024 * 1024)));
                    if speed > 0 {
                        line.push_str(&format!("  {} MHz", speed));
                    }
                    out.push(line);
                }
            }
            // Advance past the header and its double-NUL terminated strings.
            let mut end = off + len;
            while end + 1 < table.len() {
                if table[end] == 0 && table[end + 1] == 0 {
                    end += 2;
                    break;
                }
                end += 1;
            }
            off = end;
        }
        out
    }
}

/// SSD vs HDD via the seek-penalty query (no TRIM heuristics).
fn disk_kind(index: u32) -> String {
    unsafe {
        let path = wide(&format!("\\\\.\\PhysicalDrive{}", index));
        let h = CreateFileW(
            path.as_ptr(),
            0,
            0,
            ptr::null_mut(),
            3, // OPEN_EXISTING
            0,
            ptr::null_mut(),
        );
        if h.is_null() || h as isize == -1 {
            return String::new();
        }
        #[repr(C)]
        struct Query {
            property_id: u32,
            query_type: u32,
        }
        #[repr(C)]
        struct Penalty {
            version: u32,
            size: u32,
            incurs: i32,
        }
        let q = Query { property_id: 9, query_type: 0 }; // StorageDeviceSeekPenaltyProperty
        let mut p = Penalty { version: 0, size: 0, incurs: 0 };
        let mut ret = 0u32;
        let ok = DeviceIoControl(
            h,
            0x002D_1400, // IOCTL_STORAGE_QUERY_PROPERTY
            &q as *const Query as *const c_void,
            std::mem::size_of::<Query>() as u32,
            &mut p as *mut Penalty as *mut c_void,
            std::mem::size_of::<Penalty>() as u32,
            &mut ret,
            ptr::null_mut(),
        );
        CloseHandle(h);
        if ok == 0 {
            return String::new();
        }
        if p.incurs == 0 { "SSD".to_string() } else { "HDD".to_string() }
    }
}

fn read_hardware() -> HardwareInfo {
    let bios_key = "HARDWARE\\DESCRIPTION\\System\\BIOS";
    let board = format!(
        "{}  {}",
        reg_str(bios_key, "BaseBoardManufacturer"),
        reg_str(bios_key, "BaseBoardProduct")
    )
    .trim()
    .to_string();
    let board = short(&board, 30);
    let bios_ver = reg_str(bios_key, "BIOSVersion");
    let bios_year = {
        let d = reg_str(bios_key, "BIOSReleaseDate");
        d.chars().take(4).collect::<String>()
    };
    let bios = format!("{} {}", bios_ver, if bios_year.is_empty() { String::new() } else { format!("({})", bios_year) })
        .trim()
        .to_string();
    let bios = short(&bios, 30);

    let gpu_class =
        "SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let mut gpus: Vec<String> = Vec::new();
    let mut drivers: Vec<String> = Vec::new();
    for i in 0..4 {
        let sub = format!("{}\\{:04}", gpu_class, i);
        let desc = short(&reg_str(&sub, "DriverDesc"), 30);
        if desc.is_empty() {
            continue;
        }
        let mem = reg_qword(&sub, "HardwareInformation.qwMemorySize");
        let ver = reg_str(&sub, "DriverVersion");
        let date = reg_qword(&sub, "DriverDate");
        let mut line = desc;
        if mem > 0 {
            line.push_str(&format!("  {}", fmt_bytes(mem)));
        }
        gpus.push(line);
        let mut drv = String::new();
        if !ver.is_empty() {
            drv.push_str(&ver);
        }
        if date > 0 {
            if !drv.is_empty() {
                drv.push(' ');
            }
            drv.push_str(&format!("({})", short_time_string(date as i64)));
        }
        if !drv.is_empty() {
            drivers.push(drv);
        }
    }

    let disk_enum = "SYSTEM\\CurrentControlSet\\Services\\disk\\Enum";
    let mut disks: Vec<String> = Vec::new();
    for i in 0..4 {
        let id = reg_str(disk_enum, &i.to_string());
        if id.is_empty() {
            continue;
        }
        let friendly = reg_str(
            &format!("SYSTEM\\CurrentControlSet\\Enum\\{}", id),
            "FriendlyName",
        );
        disks.push(if friendly.is_empty() { id } else { friendly });
    }

    let mut kb = 0u64;
    unsafe {
        let _ = GetPhysicallyInstalledSystemMemory(&mut kb);
    }

    let disk_kinds: Vec<String> = (0..disks.len() as u32).map(disk_kind).collect();
    HardwareInfo {
        board,
        bios,
        gpu: gpus.join("   |   "),
        gpu_driver: drivers.join("   |   "),
        disks,
        phys_mem: kb * 1024,
        mem_modules: read_memory_modules(),
        disk_kinds,
    }
}

fn startup_folders() -> Vec<(String, &'static str)> {
    let mut v = Vec::new();
    if let Ok(a) = std::env::var("APPDATA") {
        v.push((format!("{}\\Microsoft\\Windows\\Start Menu\\Programs\\Startup", a), "启动文件夹(当前用户)"));
    }
    if let Ok(a) = std::env::var("ProgramData") {
        v.push((format!("{}\\Microsoft\\Windows\\Start Menu\\Programs\\StartUp", a), "启动文件夹(所有用户)"));
    }
    v
}

fn approved_disabled() -> HashSet<(String, String)> {
    let mut out = HashSet::new();
    for sub in ["Run", "Run32", "StartupFolder"] {
        let path = wide(&format!("{}\\{}", APPROVED_KEY, sub));
        let mut h: HKEY = ptr::null_mut();
        unsafe {
            if RegOpenKeyExW(k::HKEY_CURRENT_USER as HKEY, path.as_ptr(), 0, k::KEY_READ, &mut h) != 0 {
                continue;
            }
            for (name, data) in reg_enum_raw_values(h) {
                if data.first().map(|b| b & 1 == 1).unwrap_or(false) {
                    out.insert((sub.to_string(), name));
                }
            }
            RegCloseKey(h);
        }
    }
    out
}

unsafe fn reg_enum_string_values(h: HKEY) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for i in 0..2000 {
        let mut name = [0u16; 512];
        let mut name_len = 512u32;
        let mut ty = 0u32;
        let mut data = vec![0u8; 16 * 1024];
        let mut data_len = data.len() as u32;
        if RegEnumValueW(h, i, name.as_mut_ptr(), &mut name_len, ptr::null_mut(), &mut ty, data.as_mut_ptr(), &mut data_len) != 0 {
            break;
        }
        if ty != k::REG_SZ && ty != k::REG_EXPAND_SZ {
            continue;
        }
        out.push((from_wide(name.as_ptr()), from_wide(data.as_ptr() as *const u16)));
    }
    out
}

unsafe fn reg_enum_raw_values(h: HKEY) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for i in 0..2000 {
        let mut name = [0u16; 512];
        let mut name_len = 512u32;
        let mut ty = 0u32;
        let mut data = vec![0u8; 4096];
        let mut data_len = data.len() as u32;
        if RegEnumValueW(h, i, name.as_mut_ptr(), &mut name_len, ptr::null_mut(), &mut ty, data.as_mut_ptr(), &mut data_len) != 0 {
            break;
        }
        data.truncate(data_len as usize);
        out.push((from_wide(name.as_ptr()), data));
    }
    out
}

unsafe fn reg_string(h: HKEY, name: &str) -> Option<String> {
    let w = wide(name);
    let mut ty = 0u32;
    let mut size = 0u32;
    if RegQueryValueExW(h, w.as_ptr(), ptr::null_mut(), &mut ty, ptr::null_mut(), &mut size) != 0 || size == 0 || size > 1 << 20 {
        return None;
    }
    let mut buf = vec![0u8; size as usize + 2];
    if RegQueryValueExW(h, w.as_ptr(), ptr::null_mut(), &mut ty, buf.as_mut_ptr(), &mut size) != 0 {
        return None;
    }
    Some(from_wide(buf.as_ptr() as *const u16))
}

unsafe fn reg_dword(h: HKEY, name: &str) -> u32 {
    let w = wide(name);
    let mut ty = 0u32;
    let mut val = 0u32;
    let mut size = 4u32;
    if RegQueryValueExW(h, w.as_ptr(), ptr::null_mut(), &mut ty, &mut val as *mut u32 as *mut u8, &mut size) != 0 {
        return 0;
    }
    val
}

// ---------------------------------------------------------------------------
// Visible top-level windows -> "app" classification
// ---------------------------------------------------------------------------

unsafe extern "system" fn enum_windows_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
    let set = &mut *(lp as *mut HashSet<u32>);
    if IsWindowVisible(hwnd) == 0 {
        return TRUE;
    }
    if GetWindowTextLengthW(hwnd) == 0 {
        return TRUE;
    }
    let style = GetWindowLongW(hwnd, k::GWL_STYLE) as u32;
    if style & k::WS_POPUP != 0 {
        return TRUE;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid > 0 {
        set.insert(pid);
    }
    TRUE
}

fn visible_window_pids() -> HashSet<u32> {
    let mut set = HashSet::new();
    unsafe { EnumWindows(enum_windows_cb, &mut set as *mut HashSet<u32> as LPARAM) };
    set
}

// ---------------------------------------------------------------------------
// Time formatting helpers used by the table layer
// ---------------------------------------------------------------------------

pub fn file_time_string(ft: i64) -> String {
    if ft <= 0 {
        return "-".into();
    }
    let mut local = FILETIME::default();
    let mut st = SystemTime::default();
    unsafe {
        let src = ft_from_u64(ft as u64);
        FileTimeToLocalFileTime(&src, &mut local);
        if FileTimeToSystemTime(&local, &mut st) == 0 {
            return "-".into();
        }
    }
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond)
}

/// Compact local timestamp, e.g. "11-08 09:12".
pub fn short_time_string(ft: i64) -> String {
    if ft <= 0 {
        return "-".into();
    }
    let mut local = FILETIME::default();
    let mut st = SystemTime::default();
    unsafe {
        let src = ft_from_u64(ft as u64);
        FileTimeToLocalFileTime(&src, &mut local);
        if FileTimeToSystemTime(&local, &mut st) == 0 {
            return "-".into();
        }
    }
    format!("{:02}-{:02} {:02}:{:02}", st.wMonth, st.wDay, st.wHour, st.wMinute)
}

pub fn unix_short_string(secs: u64) -> String {
    if secs == 0 {
        return "-".into();
    }
    short_time_string(((secs + 11_644_473_600) * 10_000_000) as i64)
}

pub fn unix_time_string(secs: u64) -> String {
    if secs == 0 {
        return "-".into();
    }
    file_time_string(((secs + 11_644_473_600) * 10_000_000) as i64)
}

pub fn local_clock() -> String {
    let mut st = SystemTime::default();
    unsafe { GetLocalTime(&mut st) };
    format!("{:02}:{:02}:{:02}", st.wHour, st.wMinute, st.wSecond)
}
