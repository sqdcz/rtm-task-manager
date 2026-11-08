#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Debug builds keep a console so panics are visible; release builds are pure GUI.

mod actions;
mod app;
mod collect;
mod ffi;
mod model;
mod table;

use collect::Collector;
use model::Config;
use table::HistoryTracker;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let path = std::env::var("TEMP")
            .map(|p| format!("{}\\rtm_panic.log", p))
            .unwrap_or_else(|_| "rtm_panic.log".to_string());
        let _ = std::fs::write(path, format!("RTM panic at {}\n{}\n", chrono_stamp(), info));
    }));

    let cfg = Config::load();
    let mut history = HistoryTracker::default();
    let mut collector = Collector::new();
    let first = collector.collect(&cfg, &mut history);
    let code = app::run(cfg, first, collector, history);
    std::process::exit(code);
}

fn chrono_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs().to_string()).unwrap_or_default()
}
