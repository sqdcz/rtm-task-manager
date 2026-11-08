//! Data model, formatting helpers, configuration persistence and export helpers.
//! This module is deliberately free of any Win32 calls so it stays trivially testable.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiMode {
    Compact,
    Fluent,
}
impl Default for UiMode {
    fn default() -> Self {
        UiMode::Fluent
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Auto,
    Light,
    Dark,
}
impl Default for Theme {
    fn default() -> Self {
        Theme::Auto
    }
}
impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Theme::Auto => "跟随系统",
            Theme::Light => "浅色",
            Theme::Dark => "深色",
        }
    }
    pub fn next(self) -> Theme {
        match self {
            Theme::Auto => Theme::Light,
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Auto,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignState {
    Unknown,
    Signed,
    Unsigned,
}
impl Default for SignState {
    fn default() -> Self {
        SignState::Unknown
    }
}
impl SignState {
    pub fn label(self) -> &'static str {
        match self {
            SignState::Unknown => "-",
            SignState::Signed => "已签名",
            SignState::Unsigned => "未签名",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Processes = 0,
    Performance = 1,
    AppHistory = 2,
    Startup = 3,
    Users = 4,
    Details = 5,
    Services = 6,
}
impl Page {
    pub fn from_usize(i: usize) -> Page {
        match i {
            0 => Page::Processes,
            1 => Page::Performance,
            2 => Page::AppHistory,
            3 => Page::Startup,
            4 => Page::Users,
            5 => Page::Details,
            6 => Page::Services,
            _ => Page::Processes,
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn label(self) -> &'static str {
        match self {
            Page::Processes => "进程",
            Page::Performance => "性能",
            Page::AppHistory => "应用历史",
            Page::Startup => "启动应用",
            Page::Users => "用户",
            Page::Details => "详细信息",
            Page::Services => "服务",
        }
    }
    pub fn all() -> [Page; 7] {
        [
            Page::Processes,
            Page::Performance,
            Page::AppHistory,
            Page::Startup,
            Page::Users,
            Page::Details,
            Page::Services,
        ]
    }
    pub fn has_list(self) -> bool {
        self != Page::Performance
    }
}

// ---------------------------------------------------------------------------
// Snapshot data
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub path: String,
    pub cmdline: String,
    pub user: String,
    pub cmd_loaded: bool,
    pub cpu: f32,
    pub cpu_time: u64,
    pub mem: u64,
    pub private_bytes: u64,
    pub commit: u64,
    pub virtual_bytes: u64,
    pub disk_read: u64,
    pub disk_write: u64,
    pub disk_read_rate: f64,
    pub disk_write_rate: f64,
    pub gpu: f32,
    pub net_conns: u32,
    pub handles: u32,
    pub threads: u32,
    pub session_id: u32,
    pub create_time: i64,
    pub priority: i32,
    pub signed: SignState,
    pub depth: u8,
    pub has_children: bool,
    pub expanded: bool,
    pub is_app: bool,
    pub status: String,
    pub arch: String,
    /// Recent CPU utilisation, oldest first, for the inline sparkline.
    pub cpu_hist: Vec<f32>,
}

#[derive(Clone, Default)]
pub struct SysInfo {
    pub cpu: f32,
    pub cpu_cores: Vec<f32>,
    pub logical_processors: u32,
    pub mem_total: u64,
    pub mem_avail: u64,
    pub mem_used: u64,
    pub mem_load: u32,
    pub commit: u64,
    pub commit_limit: u64,
    pub cached: u64,
    pub disk_read: f64,
    pub disk_write: f64,
    pub disk_active: f32,
    pub net_recv: f64,
    pub net_sent: f64,
    pub net_total_recv: u64,
    pub net_total_sent: u64,
    pub handles: u32,
    pub threads: u32,
    pub processes: u32,
    pub uptime_secs: u64,
    /// Unix timestamp of the last boot (now - uptime).
    pub boot_unix: u64,
    pub gpu: f32,
    pub page_size: u64,
    pub cpu_name: String,
    pub cpu_base_mhz: u32,
    pub disks: Vec<DiskInfo>,
    pub adapters: Vec<AdapterInfo>,
}

#[derive(Clone, Default)]
pub struct DiskInfo {
    pub letter: String,
    pub total: u64,
    pub free: u64,
    pub active: f32,
    pub read: f64,
    pub write: f64,
}

#[derive(Clone, Default)]
pub struct AdapterInfo {
    pub name: String,
    pub recv: f64,
    pub sent: f64,
    pub speed: u64,
}

#[derive(Clone, Default)]
pub struct Service {
    pub name: String,
    pub display: String,
    pub pid: u32,
    pub state: u32,
    pub start_type: String,
    pub description: String,
}

#[derive(Clone, Default)]
pub struct StartupItem {
    pub name: String,
    pub command: String,
    pub source: String,
    pub enabled: bool,
    pub publisher: String,
    pub reg_key: u8, // 0..3 registry locations, 4 = folder
    /// Resolved executable path (from the command line), empty when unknown.
    pub exe: String,
    /// Process creation time (FILETIME); 0 when the item is not running.
    pub start_time: i64,
    /// 0 = low, 1 = medium, 2 = high, 3 = unknown (estimated, not the OS score).
    pub impact_level: u8,
    pub cpu_secs: f64,
    pub disk_bytes: u64,
    pub mem_bytes: u64,
}

#[derive(Clone, Default)]
pub struct UserSession {
    pub name: String,
    pub domain: String,
    pub session_id: u32,
    pub state: String,
    pub cpu: f32,
    pub mem: u64,
    pub disk: f64,
    pub procs: u32,
}

#[derive(Clone, Default)]
pub struct HistoryEntry {
    pub name: String,
    pub cpu_time_secs: f64,
    pub disk_bytes: u64,
    pub net_conns: u32,
    pub first_seen: u64,
    pub last_seen: u64,
    pub launches: u32,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub stamp: u64,
    pub procs: Vec<Process>,
    pub sys: SysInfo,
    pub services: Vec<Service>,
    pub startups: Vec<StartupItem>,
    pub users: Vec<UserSession>,
    pub history: Vec<HistoryEntry>,
    pub collect_ms: f64,
}

impl Snapshot {
    pub fn by_pid(&self, pid: u32) -> Option<&Process> {
        self.procs.iter().find(|p| p.pid == pid)
    }
}

// ---------------------------------------------------------------------------
// Trend buffer
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct Trend {
    pub vals: Vec<f32>,
    pub cap: usize,
}
impl Trend {
    pub fn new(cap: usize) -> Self {
        Trend { vals: Vec::with_capacity(cap), cap }
    }
    pub fn push(&mut self, v: f32) {
        if self.cap == 0 {
            return;
        }
        if self.vals.len() >= self.cap {
            self.vals.remove(0);
        }
        self.vals.push(v);
    }
    pub fn last(&self) -> f32 {
        *self.vals.last().unwrap_or(&0.0)
    }
}

#[derive(Default)]
pub struct Trends {
    pub cpu: Trend,
    pub mem: Trend,
    pub disk: Trend,
    pub net: Trend,
    pub gpu: Trend,
    pub per_core: Vec<Trend>,
}
impl Trends {
    pub fn new() -> Self {
        Trends {
            cpu: Trend::new(120),
            mem: Trend::new(120),
            disk: Trend::new(120),
            net: Trend::new(120),
            gpu: Trend::new(120),
            per_core: Vec::new(),
        }
    }
    pub fn update(&mut self, s: &SysInfo) {
        self.cpu.push(s.cpu);
        self.mem.push(if s.mem_total > 0 {
            s.mem_used as f32 * 100.0 / s.mem_total as f32
        } else {
            0.0
        });
        self.disk.push(s.disk_active);
        self.net.push(if s.net_recv + s.net_sent > 0.0 {
            50.0
        } else {
            0.0
        });
        self.gpu.push(s.gpu);
        while self.per_core.len() < s.cpu_cores.len() {
            self.per_core.push(Trend::new(120));
        }
        for (i, v) in s.cpu_cores.iter().enumerate() {
            self.per_core[i].push(*v);
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Config {
    pub schema: u32,
    pub mode: UiMode,
    pub theme: Theme,
    pub interval_ms: u32,
    pub low_power: bool,
    pub gpu: bool,
    pub tray: bool,
    pub page: usize,
    pub sort_col: i32,
    pub sort_desc: bool,
    pub search: String,
    pub use_regex: bool,
    pub show_tree: bool,
    pub group: bool,
    pub heatmap: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub col_widths: HashMap<String, Vec<i32>>,
    pub cols_hidden: HashMap<String, Vec<usize>>,
}
impl Default for Config {
    fn default() -> Self {
        Config {
            schema: 3,
            mode: UiMode::Fluent,
            theme: Theme::Auto,
            interval_ms: 1000,
            low_power: false,
            gpu: false,
            tray: true,
            page: 0,
            sort_col: -1,
            sort_desc: true,
            search: String::new(),
            use_regex: false,
            show_tree: true,
            group: false,
            heatmap: true,
            x: 120,
            y: 100,
            w: 1100,
            h: 680,
            col_widths: HashMap::new(),
            cols_hidden: HashMap::new(),
        }
    }
}

impl Config {
    fn path() -> PathBuf {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        base.join("RustTaskManager")
    }
    pub fn file() -> PathBuf {
        Config::path().join("rtm.ini")
    }
    pub fn load() -> Config {
        let mut c = Config::default();
        let text = match fs::read_to_string(Config::file()) {
            Ok(t) => t,
            Err(_) => return c,
        };
        let mut schema = 0u32;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let (k, v) = match line.split_once('=') {
                Some(p) => p,
                None => continue,
            };
            let k = k.trim();
            let v = v.trim();
            match k {
                "schema" => schema = v.parse().unwrap_or(0),
                "mode" => c.mode = if v == "fluent" { UiMode::Fluent } else { UiMode::Compact },
                "theme" => c.theme = match v {
                    "dark" => Theme::Dark,
                    "light" => Theme::Light,
                    _ => Theme::Auto,
                },
                "interval" => c.interval_ms = v.parse().unwrap_or(1000).clamp(250, 10000),
                "low_power" => c.low_power = v == "1",
                "gpu" => c.gpu = v == "1",
                "tray" => c.tray = v == "1",
                "page" => c.page = v.parse::<usize>().unwrap_or(0).min(6),
                "sort_col" => c.sort_col = v.parse().unwrap_or(-1),
                "sort_desc" => c.sort_desc = v == "1",
                "search" => c.search = v.to_string(),
                "regex" => c.use_regex = v == "1",
                "tree" => c.show_tree = v == "1",
                "group" => c.group = v == "1",
                "heatmap" => c.heatmap = v == "1",
                "x" => c.x = v.parse().unwrap_or(120),
                "y" => c.y = v.parse().unwrap_or(100),
                "w" => c.w = v.parse().unwrap_or(1100).max(640),
                "h" => c.h = v.parse().unwrap_or(680).max(400),
                _ => {
                    if let Some(page) = k.strip_prefix("cols.") {
                        let widths: Vec<i32> = v
                            .split(',')
                            .filter_map(|x| x.parse::<i32>().ok())
                            .collect();
                        if !widths.is_empty() {
                            c.col_widths.insert(page.to_string(), widths);
                        }
                    } else if let Some(page) = k.strip_prefix("hidden.") {
                        let idx: Vec<usize> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                        c.cols_hidden.insert(page.to_string(), idx);
                    }
                }
            }
        }
        // Older configs predate the Fluent default, system theme support and
        // the placeholder-aware search box. Migrate once. A hint accidentally
        // persisted by v2 is cleaned up as well.
        if c.search.contains("与 re: 正则") {
            c.search.clear();
        }
        if schema < 3 {
            c.mode = UiMode::Fluent;
            c.theme = Theme::Auto;
            c.page = 0;
            c.schema = 3;
        }
        // Grouping and the process tree are mutually exclusive; the tree wins
        // so expanding children always works after an upgrade.
        if c.group && c.show_tree {
            c.group = false;
        }
        c
    }
    pub fn save(&self) {
        let dir = Config::path();
        let _ = fs::create_dir_all(&dir);
        let mut s = String::new();
        s.push_str("# Rust Task Manager configuration\n");
        s.push_str("schema=3\n");
        s.push_str(&format!("mode={}\n", if self.mode == UiMode::Fluent { "fluent" } else { "compact" }));
        s.push_str(&format!("theme={}\n", match self.theme {
            Theme::Dark => "dark",
            Theme::Light => "light",
            Theme::Auto => "auto",
        }));
        s.push_str(&format!("interval={}\n", self.interval_ms));
        s.push_str(&format!("low_power={}\n", self.low_power as i32));
        s.push_str(&format!("gpu={}\n", self.gpu as i32));
        s.push_str(&format!("tray={}\n", self.tray as i32));
        s.push_str(&format!("page={}\n", self.page));
        s.push_str(&format!("sort_col={}\n", self.sort_col));
        s.push_str(&format!("sort_desc={}\n", self.sort_desc as i32));
        s.push_str(&format!("search={}\n", self.search.replace('\n', " ")));
        s.push_str(&format!("regex={}\n", self.use_regex as i32));
        s.push_str(&format!("tree={}\n", self.show_tree as i32));
        s.push_str(&format!("group={}\n", self.group as i32));
        s.push_str(&format!("heatmap={}\n", self.heatmap as i32));
        s.push_str(&format!("x={}\ny={}\nw={}\nh={}\n", self.x, self.y, self.w, self.h));
        for (page, widths) in &self.col_widths {
            let joined = widths.iter().map(|w| w.to_string()).collect::<Vec<_>>().join(",");
            s.push_str(&format!("cols.{}={}\n", page, joined));
        }
        for (page, hidden) in &self.cols_hidden {
            let joined = hidden.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
            s.push_str(&format!("hidden.{}={}\n", page, joined));
        }
        let file = Config::file();
        let tmp = file.with_extension("tmp");
        if fs::write(&tmp, s).is_ok() {
            let _ = fs::rename(&tmp, &file);
        }
    }
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

pub fn fmt_bytes(v: u64) -> String {
    const KB: f64 = 1024.0;
    let f = v as f64;
    if f < KB {
        format!("{} B", v)
    } else if f < KB * KB {
        format!("{:.0} KB", f / KB)
    } else if f < KB * KB * KB {
        format!("{:.1} MB", f / (KB * KB))
    } else {
        format!("{:.2} GB", f / (KB * KB * KB))
    }
}

pub fn fmt_rate(v: f64) -> String {
    const KB: f64 = 1024.0;
    if v < KB {
        format!("{:.0} B/秒", v)
    } else if v < KB * KB {
        format!("{:.1} KB/秒", v / KB)
    } else {
        format!("{:.1} MB/秒", v / (KB * KB))
    }
}

pub fn fmt_pct(v: f32) -> String {
    format!("{:.1}%", v.max(0.0))
}

pub fn fmt_duration(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{}:{:02}:{:02}:{:02}", d, h, m, s)
    } else {
        format!("{:02}:{:02}:{:02}", h, m, s)
    }
}

pub fn fmt_cpu_time(ticks_100ns: u64) -> String {
    let secs = ticks_100ns / 10_000_000;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

pub fn short_path(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

pub fn csv_escape(s: &str) -> String {
    if s.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Matches a filter expression against text. Supports `|` alternatives,
/// `*` / `?` wildcards, and a `re:` prefix for real regular expressions.
pub fn matches_filter(haystack_ci: &str, expr: &str, use_regex: bool) -> bool {
    let expr = expr.trim();
    if expr.is_empty() {
        return true;
    }
    for alt in expr.split('|') {
        let alt = alt.trim();
        if alt.is_empty() {
            continue;
        }
        if use_regex || alt.starts_with("re:") {
            let pat = alt.strip_prefix("re:").unwrap_or(alt);
            if let Ok(re) = regex_lite::Regex::new(&format!("(?i){}", pat)) {
                if re.is_match(haystack_ci) {
                    return true;
                }
            }
        } else if wildcard(haystack_ci, alt) {
            return true;
        }
    }
    false
}

fn wildcard_ci(text: &str, pat: &str) -> bool {
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let p: Vec<char> = pat.to_lowercase().chars().collect();
    let (mut i, mut j, mut star, mut mark) = (0usize, 0usize, None, 0usize);
    while i < t.len() {
        if j < p.len() && (p[j] == '?' || p[j] == t[i]) {
            i += 1;
            j += 1;
        } else if j < p.len() && p[j] == '*' {
            star = Some(j);
            mark = i;
            j += 1;
        } else if let Some(s) = star {
            j = s + 1;
            mark += 1;
            i = mark;
        } else {
            return false;
        }
    }
    while j < p.len() && p[j] == '*' {
        j += 1;
    }
    j == p.len()
}

pub fn wildcard(text: &str, pat: &str) -> bool {
    let needle = format!("*{}*", pat);
    wildcard_ci(text, &needle)
}
