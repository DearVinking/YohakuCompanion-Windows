//! 微型滚动文件日志（data 目录，≤1 MiB × 2 轮转）。诊断刻意最小化。

use log::{LevelFilter, Log, Metadata, Record};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_ROTATIONS: usize = 2;

struct FileLogger {
    path: Mutex<PathBuf>,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut path = self.path.lock().unwrap();
        if let Ok(meta) = std::fs::metadata(&*path) {
            if meta.len() > MAX_FILE_BYTES {
                for i in (1..MAX_ROTATIONS).rev() {
                    let _ = std::fs::rename(rotated_name(&path, i), rotated_name(&path, i + 1));
                }
                let _ = std::fs::rename(&*path, rotated_name(&path, 1));
            }
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&*path)
        {
            let _ = writeln!(
                file,
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f"),
                record.level(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

fn rotated_name(base: &PathBuf, index: usize) -> PathBuf {
    let mut name = base.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{index}"));
    base.with_file_name(name)
}

/// 初始化全局日志（重复调用无效果）。
pub fn init(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir);
    let logger = FileLogger {
        path: Mutex::new(dir.join("app.log")),
    };
    let _ = log::set_boxed_logger(Box::new(logger));
    log::set_max_level(LevelFilter::Info);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_naming() {
        let base = PathBuf::from("/tmp/x/app.log");
        assert_eq!(rotated_name(&base, 1), PathBuf::from("/tmp/x/app.log.1"));
        assert_eq!(rotated_name(&base, 2), PathBuf::from("/tmp/x/app.log.2"));
    }
}
