//! 简单的文件日志：写入应用数据目录下的 logs/main.log，超过 2MB 自动轮转。
//!
//! 日志写入失败必须静默降级，不能影响代理与界面主流程。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 单个日志文件的最大字节数，超过后轮转为 main.1.log
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
/// 导出诊断文件时附带最近多少字节的日志
const DIAGNOSTIC_LOG_BYTES: usize = 256 * 1024;

struct LoggerState {
    directory: PathBuf,
    file: PathBuf,
}

static STATE: OnceLock<Mutex<Option<LoggerState>>> = OnceLock::new();

fn state() -> &'static Mutex<Option<LoggerState>> {
    STATE.get_or_init(|| Mutex::new(None))
}

/// 初始化日志目录；旧日志过大时先轮转。
pub fn init(directory: PathBuf) {
    let file = directory.join("main.log");
    if let Err(error) = fs::create_dir_all(&directory) {
        eprintln!("[FreeCharles] 初始化日志目录失败: {error}");
        return;
    }
    if let Ok(metadata) = fs::metadata(&file) {
        if metadata.len() > MAX_LOG_BYTES {
            let backup = directory.join("main.1.log");
            let _ = fs::remove_file(&backup);
            let _ = fs::rename(&file, &backup);
        }
    }
    if let Ok(mut guard) = state().lock() {
        *guard = Some(LoggerState { directory, file });
    }
}

/// 当前日志目录，供「打开日志目录」使用。
pub fn directory() -> Option<PathBuf> {
    state()
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|state| state.directory.clone()))
}

fn write_line(level: &str, message: &str) {
    let line = format!(
        "[{}] [{}] {}\n",
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z"),
        level,
        message
    );
    let Ok(guard) = state().lock() else {
        return;
    };
    let Some(state) = guard.as_ref() else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.file)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn log_info(message: &str) {
    println!("[FreeCharles] {message}");
    write_line("INFO", message);
}

#[allow(dead_code)]
pub fn log_warn(message: &str) {
    println!("[FreeCharles] {message}");
    write_line("WARN", message);
}

pub fn log_error(message: &str) {
    eprintln!("[FreeCharles] {message}");
    write_line("ERROR", message);
}

/// 读取最近一段日志，供诊断导出使用。
pub fn read_recent(max_bytes: usize) -> String {
    let Some(directory) = directory() else {
        return String::new();
    };
    let file = directory.join("main.log");
    let Ok(content) = fs::read_to_string(&file) else {
        return String::new();
    };
    if content.len() <= max_bytes {
        return content;
    }
    let mut start = content.len() - max_bytes;
    // 保证按 UTF-8 字符边界截取，避免中文日志被截断成乱码
    while start < content.len() && !content.is_char_boundary(start) {
        start += 1;
    }
    content[start..].to_string()
}

pub fn read_diagnostic_logs() -> String {
    read_recent(DIAGNOSTIC_LOG_BYTES)
}

/// 在资源管理器中打开日志目录（Windows 用 explorer，其他平台用 xdg-open）。
pub fn open_directory(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}
