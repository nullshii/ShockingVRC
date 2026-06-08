use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use log::{Level, LevelFilter, Log, Metadata, Record};

const MAX_LOG_LINES: usize = 2000;

pub type LogBuffer = Arc<Mutex<VecDeque<String>>>;

struct TuiLogger {
    buffer: LogBuffer,
    level: LevelFilter,
}

impl Log for TuiLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let tag = match record.level() {
            Level::Error => "ERROR",
            Level::Warn => "WARN ",
            Level::Info => "INFO ",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        };
        let line = format!("[{tag}] {}", record.args());
        if let Ok(mut buf) = self.buffer.lock() {
            for sub in line.split('\n') {
                if sub.is_empty() {
                    continue;
                }
                if buf.back().is_some_and(|last| last == sub) {
                    continue;
                }
                buf.push_back(sub.to_string());
                while buf.len() > MAX_LOG_LINES {
                    buf.pop_front();
                }
            }
        }
    }

    fn flush(&self) {}
}

fn level_from_env(default_filter: &str) -> LevelFilter {
    let raw = std::env::var("RUST_LOG").unwrap_or_else(|_| default_filter.to_string());
    raw.split(',')
        .filter_map(|part| {
            let token = part.rsplit('=').next().unwrap_or(part).trim();
            token.parse::<LevelFilter>().ok()
        })
        .last()
        .unwrap_or(LevelFilter::Info)
}

pub fn init(default_filter: &str) -> LogBuffer {
    let buffer: LogBuffer = Arc::new(Mutex::new(VecDeque::with_capacity(256)));
    let level = level_from_env(default_filter);
    let logger = TuiLogger {
        buffer: Arc::clone(&buffer),
        level,
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
    }
    buffer
}
