//! Table model for all seven pages: column definitions, row materialisation,
//! sorting, filtering, virtual-list view indices, process tree and history
//! accumulation. This module is pure Rust (no Win32 calls).

use std::collections::{HashMap, HashSet};

use crate::ffi::k::{LVCFMT_CENTER, LVCFMT_LEFT, LVCFMT_RIGHT};
use crate::model::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColKind {
    Text,
    Num,
    Pct,
    Bytes,
    Rate,
    Time,
}

#[derive(Clone)]
pub struct Column {
    pub name: &'static str,
    pub width: i32,
    pub align: i32,
    pub kind: ColKind,
    pub visible: bool,
    pub heat: bool,
}

#[derive(Clone, Default)]
pub struct Row {
    pub cells: Vec<String>,
    pub vals: Vec<f64>,
    pub pid: u32,
    pub key: String,
    pub depth: u8,
    pub has_children: bool,
    pub expanded: bool,
    pub cpu_hist: Vec<f32>,
    /// Small image-list index for the executable icon, -1 when unavailable.
    pub icon: i32,
}

pub struct Table {
    pub cols: Vec<Column>,
    pub rows: Vec<Row>,
    pub view: Vec<usize>,
    pub max_vals: Vec<f64>,
    pub sort_col: usize,
    pub sort_desc: bool,
    /// Grouped tables keep their build order (group headings must stay put).
    pub grouped: bool,
}

impl Table {
    pub fn new(cols: Vec<Column>) -> Table {
        let n = cols.len();
        Table {
            cols,
            rows: Vec::new(),
            view: Vec::new(),
            max_vals: vec![0.0; n],
            sort_col: 0,
            sort_desc: true,
            grouped: false,
        }
    }

    /// Number of currently visible columns (in display order).
    pub fn visible_cols(&self) -> Vec<usize> {
        self.cols.iter().enumerate().filter(|(_, c)| c.visible).map(|(i, _)| i).collect()
    }

    pub fn recompute(&mut self, query: &str, use_regex: bool, sort_col: i32, sort_desc: bool) {
        let n = self.cols.len();
        self.max_vals = vec![0.0; n];
        for r in &self.rows {
            for (i, v) in r.vals.iter().enumerate().take(n) {
                if *v > self.max_vals[i] {
                    self.max_vals[i] = *v;
                }
            }
        }
        let mut view: Vec<usize> = (0..self.rows.len())
            .filter(|&i| {
                query.trim().is_empty()
                    || self.rows[i].cells.iter().any(|c| matches_filter(c, query, use_regex))
            })
            .collect();
        if self.grouped {
            // Heading rows must not move when the user sorts.
            self.view = view;
            self.sort_col = if sort_col < 0 { 0 } else { sort_col as usize };
            self.sort_desc = sort_desc;
            return;
        }
        let col = if sort_col < 0 { 0 } else { (sort_col as usize).min(n.saturating_sub(1)) };
        let textual = n > 0 && self.cols[col].kind == ColKind::Text;
        view.sort_by(|&a, &b| {
            let ra = &self.rows[a];
            let rb = &self.rows[b];
            let ord = if textual || col >= ra.vals.len() || col >= rb.vals.len() {
                let sa = ra.cells.get(col).map(|s| s.to_lowercase()).unwrap_or_default();
                let sb = rb.cells.get(col).map(|s| s.to_lowercase()).unwrap_or_default();
                sa.cmp(&sb)
            } else {
                ra.vals[col]
                    .partial_cmp(&rb.vals[col])
                    .unwrap_or(std::cmp::Ordering::Equal)
            };
            if sort_desc { ord.reverse() } else { ord }
        });
        self.view = view;
        self.sort_col = col;
        self.sort_desc = sort_desc;
    }

    pub fn len(&self) -> usize {
        self.view.len()
    }
    pub fn row(&self, display_index: usize) -> Option<&Row> {
        self.view.get(display_index).and_then(|&i| self.rows.get(i))
    }
    pub fn row_mut(&mut self, display_index: usize) -> Option<&mut Row> {
        let idx = *self.view.get(display_index)?;
        self.rows.get_mut(idx)
    }
    pub fn pid_at(&self, display_index: usize) -> u32 {
        self.row(display_index).map(|r| r.pid).unwrap_or(0)
    }
    pub fn key_at(&self, display_index: usize) -> String {
        self.row(display_index).map(|r| r.key.clone()).unwrap_or_default()
    }
    pub fn cell(&self, display_index: usize, col: usize) -> &str {
        self.row(display_index)
            .and_then(|r| r.cells.get(col))
            .map(|s| s.as_str())
            .unwrap_or("")
    }
}

// ---------------------------------------------------------------------------
// Column sets
// ---------------------------------------------------------------------------

macro_rules! cols {
    ($( ($name:expr, $w:expr, $align:expr, $kind:expr, $vis:expr, $heat:expr) ),* $(,)?) => {
        vec![ $( Column { name: $name, width: $w, align: $align, kind: $kind, visible: $vis, heat: $heat } ),* ]
    };
}

pub fn columns(page: Page) -> Vec<Column> {
    use ColKind::*;
    match page {
        Page::Processes => cols![
            ("名称", 264, LVCFMT_LEFT, Text, true, false),
            ("PID", 62, LVCFMT_RIGHT, Num, true, false),
            ("状态", 68, LVCFMT_LEFT, Text, true, false),
            ("CPU", 132, LVCFMT_RIGHT, Pct, true, true),
            ("内存", 92, LVCFMT_RIGHT, Bytes, true, true),
            ("磁盘", 94, LVCFMT_RIGHT, Rate, true, true),
            ("网络", 80, LVCFMT_RIGHT, Num, true, true),
            ("GPU", 62, LVCFMT_RIGHT, Pct, true, true),
            ("句柄", 70, LVCFMT_RIGHT, Num, false, false),
            ("线程", 70, LVCFMT_RIGHT, Num, false, false),
            ("父进程", 90, LVCFMT_RIGHT, Num, false, false),
            ("用户", 120, LVCFMT_LEFT, Text, false, false),
            ("会话", 60, LVCFMT_RIGHT, Num, false, false),
            ("命令行", 260, LVCFMT_LEFT, Text, false, false),
            ("路径", 230, LVCFMT_LEFT, Text, false, false),
            ("签名", 82, LVCFMT_CENTER, Text, false, false),
            ("启动时间", 130, LVCFMT_LEFT, Text, false, false),
            ("CPU 时间", 100, LVCFMT_RIGHT, Time, false, false),
            ("架构", 60, LVCFMT_CENTER, Text, false, false),
        ],
        Page::Details => cols![
            ("名称", 172, LVCFMT_LEFT, Text, true, false),
            ("PID", 62, LVCFMT_RIGHT, Num, true, false),
            ("状态", 64, LVCFMT_LEFT, Text, true, false),
            ("用户名", 108, LVCFMT_LEFT, Text, true, false),
            ("CPU", 60, LVCFMT_RIGHT, Pct, true, true),
            ("工作集", 88, LVCFMT_RIGHT, Bytes, true, true),
            ("专用字节", 88, LVCFMT_RIGHT, Bytes, false, false),
            ("提交大小", 88, LVCFMT_RIGHT, Bytes, true, false),
            ("虚拟内存", 88, LVCFMT_RIGHT, Bytes, false, false),
            ("句柄", 64, LVCFMT_RIGHT, Num, true, false),
            ("线程", 62, LVCFMT_RIGHT, Num, true, false),
            ("优先级", 70, LVCFMT_RIGHT, Num, false, false),
            ("父 PID", 68, LVCFMT_RIGHT, Num, true, false),
            ("会话 ID", 70, LVCFMT_RIGHT, Num, false, false),
            ("磁盘读", 92, LVCFMT_RIGHT, Rate, false, false),
            ("磁盘写", 92, LVCFMT_RIGHT, Rate, false, false),
            ("命令行", 240, LVCFMT_LEFT, Text, false, false),
            ("路径", 220, LVCFMT_LEFT, Text, false, false),
            ("签名", 84, LVCFMT_CENTER, Text, false, false),
            ("启动时间", 130, LVCFMT_LEFT, Text, false, false),
            ("CPU 时间", 100, LVCFMT_RIGHT, Time, false, false),
            ("架构", 64, LVCFMT_CENTER, Text, false, false),
        ],
        Page::AppHistory => cols![
            ("名称", 240, LVCFMT_LEFT, Text, true, false),
            ("CPU 时间", 130, LVCFMT_RIGHT, Time, true, true),
            ("磁盘累计", 130, LVCFMT_RIGHT, Bytes, true, true),
            ("网络活动", 120, LVCFMT_RIGHT, Num, true, true),
            ("启动次数", 100, LVCFMT_RIGHT, Num, true, false),
            ("首次记录", 150, LVCFMT_LEFT, Text, true, false),
            ("最近活动", 150, LVCFMT_LEFT, Text, true, false),
            ("说明", 260, LVCFMT_LEFT, Text, false, false),
        ],
        Page::Startup => cols![
            ("名称", 190, LVCFMT_LEFT, Text, true, false),
            ("发布者", 120, LVCFMT_LEFT, Text, true, false),
            ("状态", 72, LVCFMT_CENTER, Text, true, false),
            ("启动影响(估算)", 116, LVCFMT_CENTER, Text, true, false),
            ("启动时间", 150, LVCFMT_LEFT, Text, true, false),
            ("命令", 280, LVCFMT_LEFT, Text, true, false),
            ("来源", 140, LVCFMT_LEFT, Text, true, false),
        ],
        Page::Users => cols![
            ("用户", 220, LVCFMT_LEFT, Text, true, false),
            ("会话 ID", 90, LVCFMT_RIGHT, Num, true, false),
            ("状态", 100, LVCFMT_LEFT, Text, true, false),
            ("CPU", 80, LVCFMT_RIGHT, Pct, true, true),
            ("内存", 110, LVCFMT_RIGHT, Bytes, true, true),
            ("磁盘", 110, LVCFMT_RIGHT, Rate, true, true),
            ("进程数", 90, LVCFMT_RIGHT, Num, true, false),
        ],
        Page::Services => cols![
            ("名称", 220, LVCFMT_LEFT, Text, true, false),
            ("显示名称", 230, LVCFMT_LEFT, Text, true, false),
            ("PID", 68, LVCFMT_RIGHT, Num, true, false),
            ("状态", 100, LVCFMT_CENTER, Text, true, false),
            ("启动类型", 110, LVCFMT_LEFT, Text, true, false),
            ("描述", 320, LVCFMT_LEFT, Text, false, false),
        ],
        Page::Performance => cols![
            ("类别", 220, LVCFMT_LEFT, Text, true, false),
            ("当前值", 160, LVCFMT_RIGHT, Text, true, false),
        ],
    }
}

// ---------------------------------------------------------------------------
// Row builders
// ---------------------------------------------------------------------------

/// Direction indicator derived from the last 6 CPU samples. Unicode block
/// elements were tried first but Microsoft YaHei UI renders U+2581..U+2588 as
/// a solid bar, so a glyph that exists in the font is used instead.
fn trend_arrow(v: &[f32]) -> &'static str {
    if v.len() < 6 {
        return "";
    }
    let n = v.len();
    let recent: f32 = v[n - 3..].iter().sum::<f32>() / 3.0;
    let earlier: f32 = v[n - 6..n - 3].iter().sum::<f32>() / 3.0;
    if recent > earlier + 0.5 {
        " ▲"
    } else if recent + 0.5 < earlier {
        " ▼"
    } else {
        ""
    }
}

fn blank_row(n: usize) -> Row {
    Row { cells: vec![String::new(); n], vals: vec![0.0; n], ..Default::default() }
}

fn put(r: &mut Row, i: usize, s: impl Into<String>, v: f64) {
    if i < r.cells.len() {
        r.cells[i] = s.into();
        r.vals[i] = v;
    }
}

fn state_label(s: u32) -> &'static str {
    match s {
        1 => "已停止",
        2 => "正在启动",
        3 => "正在停止",
        4 => "正在运行",
        5 => "正在继续",
        6 => "正在暂停",
        7 => "已暂停",
        _ => "未知",
    }
}

fn priority_label(p: i32) -> &'static str {
    match p {
        p if p >= 24 => "实时",
        p if p >= 13 => "高",
        p if p >= 9 => "高于正常",
        p if p >= 6 => "高于正常",
        p if p <= 4 => "低",
        _ => "正常",
    }
}

pub fn build(
    page: Page,
    snap: &Snapshot,
    cfg: &Config,
    collapsed: &HashSet<u32>,
    time_fmt: &dyn Fn(i64) -> String,
    stamp_fmt: &dyn Fn(u64) -> String,
    icon_of: &dyn Fn(&str, &str) -> i32,
) -> Table {
    let cols = columns(page);
    let n = cols.len();
    let mut t = Table::new(cols);
    t.grouped = cfg.group && page == Page::Processes;
    match page {
        Page::Processes => build_processes(&mut t, snap, cfg, collapsed, time_fmt, icon_of, n),
        Page::Details => build_details(&mut t, snap, time_fmt, icon_of, n),
        Page::AppHistory => build_history(&mut t, snap, stamp_fmt, n),
        Page::Startup => build_startup(&mut t, snap, time_fmt, n),
        Page::Users => build_users(&mut t, snap, n),
        Page::Services => build_services(&mut t, snap, n),
        Page::Performance => {}
    }
    t
}

/// Depth-first process tree. Roots are PID 0/4 and processes whose parent is
/// no longer alive. Collapsed nodes keep their own row but hide descendants.
fn tree_order(procs: &[Process], collapsed: &HashSet<u32>) -> Vec<(usize, u8, bool, bool)> {
    let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
    let ids: HashSet<u32> = procs.iter().map(|p| p.pid).collect();
    let mut roots = Vec::new();
    for (i, p) in procs.iter().enumerate() {
        if p.pid < 8 || !ids.contains(&p.ppid) || p.ppid == p.pid {
            roots.push(i);
        } else {
            children.entry(p.ppid).or_default().push(i);
        }
    }
    for list in children.values_mut() {
        list.sort_by(|&a, &b| procs[b].cpu.partial_cmp(&procs[a].cpu).unwrap_or(std::cmp::Ordering::Equal));
    }
    roots.sort_by(|&a, &b| procs[a].name.to_lowercase().cmp(&procs[b].name.to_lowercase()));
    let mut out = Vec::with_capacity(procs.len());
    let mut visited = HashSet::new();
    fn walk(
        i: usize,
        depth: u8,
        procs: &[Process],
        children: &HashMap<u32, Vec<usize>>,
        collapsed: &HashSet<u32>,
        visited: &mut HashSet<u32>,
        out: &mut Vec<(usize, u8, bool, bool)>,
    ) {
        let p = &procs[i];
        if !visited.insert(p.pid) || depth > 12 {
            return;
        }
        let kids = children.get(&p.pid);
        let has_children = kids.map(|k| !k.is_empty()).unwrap_or(false);
        let expanded = !collapsed.contains(&p.pid);
        out.push((i, depth, has_children, expanded));
        if has_children && expanded {
            if let Some(kids) = kids {
                for &c in kids {
                    walk(c, depth.saturating_add(1), procs, children, collapsed, visited, out);
                }
            }
        }
    }
    for i in roots {
        walk(i, 0, procs, &children, collapsed, &mut visited, &mut out);
    }
    // Any process not reached (cycles, protected parents) is appended as root.
    for (i, p) in procs.iter().enumerate() {
        if !visited.contains(&p.pid) {
            out.push((i, 0, false, true));
        }
    }
    out
}

fn sort_group(idxs: &mut Vec<usize>, snap: &Snapshot, cfg: &Config) {
    let col = cfg.sort_col;
    idxs.sort_by(|&a, &b| {
        let pa = &snap.procs[a];
        let pb = &snap.procs[b];
        let ord = match col {
            1 => pa.pid.cmp(&pb.pid),
            3 => pa.cpu.partial_cmp(&pb.cpu).unwrap_or(std::cmp::Ordering::Equal),
            4 => pa.mem.cmp(&pb.mem),
            5 => (pa.disk_read_rate + pa.disk_write_rate)
                .partial_cmp(&(pb.disk_read_rate + pb.disk_write_rate))
                .unwrap_or(std::cmp::Ordering::Equal),
            _ => pa.name.to_lowercase().cmp(&pb.name.to_lowercase()),
        };
        if cfg.sort_desc { ord.reverse() } else { ord }
    });
}

fn push_group_row(t: &mut Table, p: &Process, time_fmt: &dyn Fn(i64) -> String, icon_of: &dyn Fn(&str, &str) -> i32, n: usize) {
    let mut r = blank_row(n);
    put(&mut r, 0, p.name.clone(), 0.0);
    put(&mut r, 1, p.pid.to_string(), p.pid as f64);
    put(&mut r, 2, p.status.clone(), 0.0);
    put(&mut r, 3, format!("{}{}", fmt_pct(p.cpu), trend_arrow(&p.cpu_hist)), p.cpu as f64);
    put(&mut r, 4, fmt_bytes(p.mem), p.mem as f64);
    let disk = p.disk_read_rate + p.disk_write_rate;
    put(&mut r, 5, fmt_rate(disk), disk);
    put(&mut r, 6, if p.net_conns > 0 { format!("{} 连接", p.net_conns) } else { "0".into() }, p.net_conns as f64);
    put(&mut r, 7, if p.gpu > 0.05 { fmt_pct(p.gpu) } else { "0%".into() }, p.gpu as f64);
    put(&mut r, 8, p.handles.to_string(), p.handles as f64);
    put(&mut r, 9, p.threads.to_string(), p.threads as f64);
    put(&mut r, 10, p.ppid.to_string(), p.ppid as f64);
    put(&mut r, 11, p.user.clone(), 0.0);
    put(&mut r, 12, p.session_id.to_string(), p.session_id as f64);
    put(&mut r, 13, p.cmdline.clone(), 0.0);
    put(&mut r, 14, p.path.clone(), 0.0);
    put(&mut r, 15, p.signed.label(), 0.0);
    put(&mut r, 16, time_fmt(p.create_time), 0.0);
    put(&mut r, 17, fmt_cpu_time(p.cpu_time), p.cpu_time as f64);
    put(&mut r, 18, p.arch.clone(), 0.0);
    r.pid = p.pid;
    r.key = p.name.clone();
    r.cpu_hist = p.cpu_hist.clone();
    r.icon = icon_of(&p.path, &p.name);
    t.rows.push(r);
}

fn push_group(
    t: &mut Table,
    title: &str,
    idxs: &[usize],
    snap: &Snapshot,
    time_fmt: &dyn Fn(i64) -> String,
    icon_of: &dyn Fn(&str, &str) -> i32,
    n: usize,
) {
    if idxs.is_empty() {
        return;
    }
    let mut h = blank_row(n);
    put(&mut h, 0, format!("{}  ({})", title, idxs.len()), 0.0);
    h.icon = -1;
    h.pid = 0;
    t.rows.push(h);
    for &i in idxs {
        push_group_row(t, &snap.procs[i], time_fmt, icon_of, n);
    }
}

fn build_processes(t: &mut Table, snap: &Snapshot, cfg: &Config, collapsed: &HashSet<u32>, time_fmt: &dyn Fn(i64) -> String, icon_of: &dyn Fn(&str, &str) -> i32, n: usize) {
    if cfg.group {
        let mut apps: Vec<usize> = Vec::new();
        let mut bgs: Vec<usize> = Vec::new();
        for (i, p) in snap.procs.iter().enumerate() {
            if p.is_app {
                apps.push(i);
            } else {
                bgs.push(i);
            }
        }
        sort_group(&mut apps, snap, cfg);
        sort_group(&mut bgs, snap, cfg);
        push_group(t, "应用", &apps, snap, time_fmt, icon_of, n);
        push_group(t, "后台进程", &bgs, snap, time_fmt, icon_of, n);
        return;
    }
    let order: Vec<(usize, u8, bool, bool)> = if cfg.show_tree {
        tree_order(&snap.procs, collapsed)
    } else {
        (0..snap.procs.len()).map(|i| (i, 0, false, true)).collect()
    };
    for (idx, depth, has_children, expanded) in order {
        let p = &snap.procs[idx];
        let mut r = blank_row(n);
        let marker = if has_children {
            if expanded { "▼  " } else { "▶  " }
        } else if cfg.show_tree {
            "   "
        } else {
            ""
        };
        // Clamp the visual indent: some protected parents produce deep chains
        // and a 10-level indent would push the name out of the column.
        let indent = "  ".repeat((depth as usize).min(2));
        let name = format!("{}{}{}{}", indent, marker, if p.is_app { "◆ " } else { "" }, p.name);
        put(&mut r, 0, name, 0.0);
        let disk = p.disk_read_rate + p.disk_write_rate;
        put(&mut r, 1, p.pid.to_string(), p.pid as f64);
        put(&mut r, 2, p.status.clone(), 0.0);
        put(&mut r, 3, format!("{}{}", fmt_pct(p.cpu), trend_arrow(&p.cpu_hist)), p.cpu as f64);
        put(&mut r, 4, fmt_bytes(p.mem), p.mem as f64);
        put(&mut r, 5, fmt_rate(disk), disk);
        put(&mut r, 6, if p.net_conns > 0 { format!("{} 连接", p.net_conns) } else { "0".into() }, p.net_conns as f64);
        put(&mut r, 7, if p.gpu > 0.05 { fmt_pct(p.gpu) } else { "0%".into() }, p.gpu as f64);
        put(&mut r, 8, p.handles.to_string(), p.handles as f64);
        put(&mut r, 9, p.threads.to_string(), p.threads as f64);
        put(&mut r, 10, p.ppid.to_string(), p.ppid as f64);
        put(&mut r, 11, p.user.clone(), 0.0);
        put(&mut r, 12, p.session_id.to_string(), p.session_id as f64);
        put(&mut r, 13, p.cmdline.clone(), 0.0);
        put(&mut r, 14, p.path.clone(), 0.0);
        put(&mut r, 15, p.signed.label(), 0.0);
        put(&mut r, 16, time_fmt(p.create_time), 0.0);
        put(&mut r, 17, fmt_cpu_time(p.cpu_time), p.cpu_time as f64);
        put(&mut r, 18, p.arch.clone(), 0.0);
        r.pid = p.pid;
        r.key = p.name.clone();
        r.depth = depth;
        r.has_children = has_children;
        r.expanded = expanded;
        r.cpu_hist = p.cpu_hist.clone();
        r.icon = icon_of(&p.path, &p.name);
        t.rows.push(r);
    }
}

fn build_details(t: &mut Table, snap: &Snapshot, time_fmt: &dyn Fn(i64) -> String, icon_of: &dyn Fn(&str, &str) -> i32, n: usize) {
    for p in &snap.procs {
        let mut r = blank_row(n);
        put(&mut r, 0, p.name.clone(), 0.0);
        put(&mut r, 1, p.pid.to_string(), p.pid as f64);
        put(&mut r, 2, p.status.clone(), 0.0);
        put(&mut r, 3, p.user.clone(), 0.0);
        put(&mut r, 4, if p.cpu > 0.0 { fmt_pct(p.cpu) } else { "0%".into() }, p.cpu as f64);
        put(&mut r, 5, fmt_bytes(p.mem), p.mem as f64);
        put(&mut r, 6, fmt_bytes(p.private_bytes), p.private_bytes as f64);
        put(&mut r, 7, fmt_bytes(p.commit), p.commit as f64);
        put(&mut r, 8, fmt_bytes(p.virtual_bytes), p.virtual_bytes as f64);
        put(&mut r, 9, p.handles.to_string(), p.handles as f64);
        put(&mut r, 10, p.threads.to_string(), p.threads as f64);
        put(&mut r, 11, priority_label(p.priority), p.priority as f64);
        put(&mut r, 12, p.ppid.to_string(), p.ppid as f64);
        put(&mut r, 13, p.session_id.to_string(), p.session_id as f64);
        put(&mut r, 14, fmt_rate(p.disk_read_rate), p.disk_read_rate);
        put(&mut r, 15, fmt_rate(p.disk_write_rate), p.disk_write_rate);
        put(&mut r, 16, p.cmdline.clone(), 0.0);
        put(&mut r, 17, p.path.clone(), 0.0);
        put(&mut r, 18, p.signed.label(), 0.0);
        put(&mut r, 19, time_fmt(p.create_time), 0.0);
        put(&mut r, 20, fmt_cpu_time(p.cpu_time), p.cpu_time as f64);
        put(&mut r, 21, p.arch.clone(), 0.0);
        r.pid = p.pid;
        r.key = p.name.clone();
        r.icon = icon_of(&p.path, &p.name);
        t.rows.push(r);
    }
}

fn build_history(t: &mut Table, snap: &Snapshot, stamp_fmt: &dyn Fn(u64) -> String, n: usize) {
    for h in &snap.history {
        let mut r = blank_row(n);
        put(&mut r, 0, h.name.clone(), 0.0);
        put(&mut r, 1, fmt_cpu_time((h.cpu_time_secs * 10_000_000.0) as u64), h.cpu_time_secs);
        put(&mut r, 2, fmt_bytes(h.disk_bytes), h.disk_bytes as f64);
        put(&mut r, 3, format!("{} 次连接", h.net_conns), h.net_conns as f64);
        put(&mut r, 4, h.launches.to_string(), h.launches as f64);
        put(&mut r, 5, stamp_fmt(h.first_seen), 0.0);
        put(&mut r, 6, stamp_fmt(h.last_seen), 0.0);
        put(&mut r, 7, "本程序会话内累计；系统级历史需读取 DUSM 数据库", 0.0);
        r.key = h.name.clone();
        t.rows.push(r);
    }
}

fn build_startup(t: &mut Table, snap: &Snapshot, time_fmt: &dyn Fn(i64) -> String, n: usize) {
    for s in &snap.startups {
        let mut r = blank_row(n);
        put(&mut r, 0, s.name.clone(), 0.0);
        put(&mut r, 1, s.publisher.clone(), 0.0);
        put(&mut r, 2, if s.enabled { "已启用" } else { "已禁用" }, if s.enabled { 1.0 } else { 0.0 });
        let (label, val) = match s.impact_level {
            0 => ("低", 0.0),
            1 => ("中", 1.0),
            2 => ("高", 2.0),
            _ => ("—", -1.0),
        };
        put(&mut r, 3, label, val);
        put(
            &mut r,
            4,
            if s.start_time > 0 { time_fmt(s.start_time) } else { "未运行".to_string() },
            0.0,
        );
        put(&mut r, 5, s.command.clone(), 0.0);
        put(&mut r, 6, s.source.clone(), 0.0);
        r.key = s.name.clone();
        t.rows.push(r);
    }
}

fn build_users(t: &mut Table, snap: &Snapshot, n: usize) {
    for u in &snap.users {
        let mut r = blank_row(n);
        let label = if u.domain.is_empty() { u.name.clone() } else { format!("{}\\{}", u.domain, u.name) };
        put(&mut r, 0, label.clone(), 0.0);
        put(&mut r, 1, u.session_id.to_string(), u.session_id as f64);
        put(&mut r, 2, u.state.clone(), 0.0);
        put(&mut r, 3, fmt_pct(u.cpu), u.cpu as f64);
        put(&mut r, 4, fmt_bytes(u.mem), u.mem as f64);
        put(&mut r, 5, fmt_rate(u.disk), u.disk);
        put(&mut r, 6, u.procs.to_string(), u.procs as f64);
        r.key = label;
        t.rows.push(r);
    }
}

fn build_services(t: &mut Table, snap: &Snapshot, n: usize) {
    for s in &snap.services {
        let mut r = blank_row(n);
        put(&mut r, 0, s.name.clone(), 0.0);
        put(&mut r, 1, s.display.clone(), 0.0);
        put(&mut r, 2, if s.pid > 0 { s.pid.to_string() } else { "-".into() }, s.pid as f64);
        put(&mut r, 3, state_label(s.state), s.state as f64);
        put(&mut r, 4, s.start_type.clone(), 0.0);
        put(&mut r, 5, s.description.clone(), 0.0);
        r.pid = s.pid;
        r.key = s.name.clone();
        t.rows.push(r);
    }
}

/// Rows for the left-hand category list of the Performance page.
pub fn perf_categories(snap: &Snapshot) -> Vec<(String, String, f32)> {
    let s = &snap.sys;
    let mem_pct = if s.mem_total > 0 { s.mem_used as f32 * 100.0 / s.mem_total as f32 } else { 0.0 };
    let disk = s.disks.first();
    vec![
        ("CPU".into(), fmt_pct(s.cpu), s.cpu),
        ("内存".into(), fmt_pct(mem_pct), mem_pct),
        ("磁盘 0".into(), disk.map(|d| fmt_rate(d.read + d.write)).unwrap_or_else(|| fmt_rate(s.disk_read + s.disk_write)), s.disk_active),
        ("网络".into(), fmt_rate(s.net_recv + s.net_sent), 0.0),
        ("GPU 0".into(), fmt_pct(s.gpu), s.gpu),
    ]
}

// ---------------------------------------------------------------------------
// Session-local app history tracker
// ---------------------------------------------------------------------------

pub struct HistoryTracker {
    entries: HashMap<String, HistoryEntry>,
    prev: HashMap<u32, (String, u64, u64)>,
    cleared: bool,
}
impl Default for HistoryTracker {
    fn default() -> Self {
        HistoryTracker { entries: HashMap::new(), prev: HashMap::new(), cleared: false }
    }
}
impl HistoryTracker {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.prev.clear();
        self.cleared = true;
    }
    pub fn ingest(&mut self, snap: &Snapshot) -> Vec<HistoryEntry> {
        let mut live: HashMap<u32, (String, u64, u64)> = HashMap::new();
        let now = snap.stamp;
        for p in &snap.procs {
            let cur = (p.name.clone(), p.cpu_time, p.disk_read.saturating_add(p.disk_write));
            live.insert(p.pid, cur.clone());
            let is_new = !self.prev.contains_key(&p.pid);
            let (d_cpu, d_disk) = match self.prev.get(&p.pid) {
                Some((old_name, old_cpu, old_disk)) if *old_name == p.name => (
                    p.cpu_time.saturating_sub(*old_cpu),
                    (p.disk_read.saturating_add(p.disk_write)).saturating_sub(*old_disk),
                ),
                _ => (0, 0),
            };
            let e = self.entries.entry(p.name.clone()).or_insert_with(|| HistoryEntry {
                name: p.name.clone(),
                first_seen: now,
                ..Default::default()
            });
            e.cpu_time_secs += d_cpu as f64 / 10_000_000.0;
            e.disk_bytes = e.disk_bytes.saturating_add(d_disk);
            e.net_conns = e.net_conns.max(p.net_conns);
            e.last_seen = now;
            if is_new {
                e.launches = e.launches.saturating_add(1);
            }
        }
        self.prev = live;
        self.cleared = false;
        let mut v: Vec<HistoryEntry> = self.entries.values().cloned().collect();
        v.sort_by(|a, b| b.cpu_time_secs.partial_cmp(&a.cpu_time_secs).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(400);
        v
    }
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

pub fn to_csv(t: &Table) -> String {
    let cols = t.visible_cols();
    let mut s = String::from("\u{FEFF}");
    s.push_str(&cols.iter().map(|&i| csv_escape(t.cols[i].name)).collect::<Vec<_>>().join(","));
    s.push('\n');
    for &ri in &t.view {
        let r = &t.rows[ri];
        s.push_str(&cols.iter().map(|&i| csv_escape(r.cells.get(i).map(|x| x.as_str()).unwrap_or(""))).collect::<Vec<_>>().join(","));
        s.push('\n');
    }
    s
}

pub fn to_json(t: &Table) -> String {
    let cols = t.visible_cols();
    let mut s = String::from("[\n");
    for (n, &ri) in t.view.iter().enumerate() {
        let r = &t.rows[ri];
        s.push_str("  {");
        let fields: Vec<String> = cols
            .iter()
            .map(|&i| format!("\"{}\": \"{}\"", json_escape(t.cols[i].name), json_escape(r.cells.get(i).map(|x| x.as_str()).unwrap_or(""))))
            .collect();
        s.push_str(&fields.join(", "));
        s.push('}');
        if n + 1 < t.view.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("]\n");
    s
}
