//! 自动接管 Windows 系统代理。
//!
//! 启动代理时备份当前设置并指向 127.0.0.1:端口，停止代理或退出应用时恢复。
//! 备份文件在应用数据目录下，应用异常退出后下次启动会自动恢复。

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::models::{now_ms, SystemProxyState};

/// Windows「Internet 选项」中的系统代理配置位置
pub const INTERNET_SETTINGS_KEY: &str =
    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";

const VALUE_NAMES: [&str; 4] = [
    "ProxyEnable",
    "ProxyServer",
    "ProxyOverride",
    "AutoConfigURL",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegistryValue {
    #[serde(rename = "type")]
    pub value_type: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProxyBackup {
    #[serde(rename = "createdAt")]
    created_at: u64,
    /// 我们接管时写入的地址，用于判断当前系统代理是否仍由本应用控制
    #[serde(rename = "ourServer")]
    our_server: String,
    values: HashMap<String, RegistryValue>,
}

pub struct SystemProxyManager {
    backup_file: PathBuf,
    active: bool,
    server: String,
    last_error: String,
}

impl SystemProxyManager {
    pub fn new(backup_file: PathBuf) -> Self {
        Self {
            backup_file,
            active: false,
            server: String::new(),
            last_error: String::new(),
        }
    }

    fn supported(&self) -> bool {
        cfg!(windows)
    }

    pub fn get_status(&self) -> SystemProxyState {
        SystemProxyState {
            supported: self.supported(),
            active: self.active,
            server: self.server.clone(),
            error: if self.last_error.is_empty() {
                None
            } else {
                Some(self.last_error.clone())
            },
        }
    }

    /// 将系统代理指向本机代理端口；重复调用只会更新端口
    pub fn apply(&mut self, port: u16) -> SystemProxyState {
        if !self.supported() {
            return self.get_status();
        }
        let server = format!("127.0.0.1:{port}");
        self.last_error.clear();

        if self.active {
            if self.server != server {
                if !write_registry_value(INTERNET_SETTINGS_KEY, "ProxyEnable", "REG_DWORD", "1")
                    || !write_registry_value(INTERNET_SETTINGS_KEY, "ProxyServer", "REG_SZ", &server)
                {
                    self.last_error = "自动设置系统代理失败：写入注册表失败".to_string();
                } else {
                    notify_system_proxy_change();
                    self.server = server;
                }
            }
            return self.get_status();
        }

        // 处理上次异常退出留下的备份，避免把「已修改后的值」当成用户原值
        if self.backup_file.exists() {
            self.restore();
            if self.backup_file.exists() {
                self.last_error = "上次的系统代理设置尚未恢复，已取消本次接管".to_string();
                return self.get_status();
            }
        }

        let backup = ProxyBackup {
            created_at: now_ms(),
            our_server: server.clone(),
            values: self.snapshot(),
        };
        if let Some(parent) = self.backup_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(&backup) {
            let _ = std::fs::write(&self.backup_file, text);
        }

        let written = write_registry_value(INTERNET_SETTINGS_KEY, "ProxyEnable", "REG_DWORD", "1")
            && write_registry_value(INTERNET_SETTINGS_KEY, "ProxyServer", "REG_SZ", &server);
        if !written {
            self.last_error = "自动设置系统代理失败：写入注册表失败".to_string();
            if let Some(backup) = self.read_backup() {
                self.restore_backup(&backup);
            }
            self.active = false;
            self.server.clear();
            return self.get_status();
        }

        // PAC 脚本优先级高于手动代理，自动接管期间先移除
        delete_registry_value(INTERNET_SETTINGS_KEY, "AutoConfigURL");
        notify_system_proxy_change();
        self.active = true;
        self.server = server;
        self.get_status()
    }

    /// 恢复接管前的系统代理设置
    pub fn restore(&mut self) -> SystemProxyState {
        if !self.supported() {
            return self.get_status();
        }
        self.last_error.clear();

        if !self.backup_file.exists() {
            self.active = false;
            self.server.clear();
            return self.get_status();
        }

        let Some(backup) = self.read_backup() else {
            self.remove_backup();
            self.active = false;
            self.server.clear();
            return self.get_status();
        };

        let expected_server = if self.server.is_empty() {
            backup.our_server.clone()
        } else {
            self.server.clone()
        };
        let current_enabled = is_proxy_enabled(read_registry_value(INTERNET_SETTINGS_KEY, "ProxyEnable"));
        let current_server = read_registry_value(INTERNET_SETTINGS_KEY, "ProxyServer")
            .map(|value| value.value)
            .unwrap_or_default();

        // 用户在运行期间手动关闭或改掉了系统代理，不再用旧值覆盖
        if !current_enabled || current_server != expected_server {
            self.remove_backup();
            self.active = false;
            self.server.clear();
            return self.get_status();
        }

        self.restore_backup(&backup);
        self.active = false;
        self.server.clear();
        self.get_status()
    }

    /// 应用启动时调用：如果上次是异常退出，则先把系统代理恢复为用户原值
    pub fn recover(&mut self) {
        if !self.supported() || !self.backup_file.exists() {
            return;
        }
        let Some(backup) = self.read_backup() else {
            self.remove_backup();
            return;
        };
        let current_enabled = is_proxy_enabled(read_registry_value(INTERNET_SETTINGS_KEY, "ProxyEnable"));
        let current_server = read_registry_value(INTERNET_SETTINGS_KEY, "ProxyServer")
            .map(|value| value.value)
            .unwrap_or_default();
        // 系统代理已被其他程序改过，不能贸然覆盖
        if !current_enabled || current_server != backup.our_server {
            self.remove_backup();
            return;
        }
        self.restore_backup(&backup);
        println!("[system-proxy] 已恢复上次异常退出时的系统代理设置");
    }

    fn snapshot(&self) -> HashMap<String, RegistryValue> {
        let mut values = HashMap::new();
        for name in VALUE_NAMES {
            if let Some(value) = read_registry_value(INTERNET_SETTINGS_KEY, name) {
                values.insert(name.to_string(), value);
            }
        }
        values
    }

    fn read_backup(&self) -> Option<ProxyBackup> {
        let text = std::fs::read_to_string(&self.backup_file).ok()?;
        serde_json::from_str::<ProxyBackup>(&text).ok()
    }

    fn restore_backup(&mut self, backup: &ProxyBackup) {
        for name in ["ProxyEnable", "ProxyServer", "AutoConfigURL"] {
            match backup.values.get(name) {
                Some(value) => {
                    write_registry_value(
                        INTERNET_SETTINGS_KEY,
                        name,
                        &value.value_type,
                        &value.value,
                    );
                }
                None => {
                    delete_registry_value(INTERNET_SETTINGS_KEY, name);
                }
            }
        }
        notify_system_proxy_change();
        self.remove_backup();
        self.active = false;
        self.server.clear();
    }

    fn remove_backup(&self) {
        let _ = std::fs::remove_file(&self.backup_file);
    }
}

/// 通知 WinINet 刷新代理配置，让已打开的程序尽快读取新设置
pub fn notify_system_proxy_change() -> bool {
    if !cfg!(windows) {
        return false;
    }
    let script = [
        "$sig = '[DllImport(\"wininet.dll\", SetLastError=true)] public static extern bool InternetSetOption(IntPtr h, int o, IntPtr b, int l);'",
        "Add-Type -MemberDefinition $sig -Name FreeCharlesWinInet -Namespace FreeCharles",
        "[void][FreeCharles.FreeCharlesWinInet]::InternetSetOption([IntPtr]::Zero, 39, [IntPtr]::Zero, 0)",
        "[void][FreeCharles.FreeCharlesWinInet]::InternetSetOption([IntPtr]::Zero, 37, [IntPtr]::Zero, 0)",
    ]
    .join("; ");

    let status = hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    matches!(status, Ok(value) if value.success())
}

/// 读取指定注册表值，不存在时返回 None
pub fn read_registry_value(key: &str, name: &str) -> Option<RegistryValue> {
    if !cfg!(windows) {
        return None;
    }
    let output = hidden_command("reg")
        .args(["query", key, "/v", name])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let trimmed = line.trim();
        let mut parts = trimmed.split_whitespace();
        let Some(field) = parts.next() else {
            continue;
        };
        if !field.eq_ignore_ascii_case(name) {
            continue;
        }
        let Some(value_type) = parts.next() else {
            continue;
        };
        if !value_type.to_uppercase().starts_with("REG_") {
            continue;
        }
        let value = parts.collect::<Vec<_>>().join(" ");
        return Some(RegistryValue {
            value_type: value_type.to_uppercase(),
            value,
        });
    }
    None
}

fn write_registry_value(key: &str, name: &str, value_type: &str, value: &str) -> bool {
    let normalized = normalize_registry_value(value_type, value);
    let result = hidden_command("reg")
        .args(["add", key, "/v", name, "/t", value_type, "/d", &normalized, "/f"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    !matches!(result, Ok(status) if !status.success())
}

fn delete_registry_value(key: &str, name: &str) -> bool {
    let result = hidden_command("reg")
        .args(["delete", key, "/v", name, "/f"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    matches!(result, Ok(status) if status.success())
}

fn is_proxy_enabled(value: Option<RegistryValue>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let normalized = value.value.trim().to_lowercase();
    normalized == "1" || normalized == "0x1"
}

fn normalize_registry_value(value_type: &str, value: &str) -> String {
    if !value_type.eq_ignore_ascii_case("REG_DWORD") {
        return value.to_string();
    }
    let trimmed = value.trim();
    let parsed = if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16)
    } else {
        trimmed.parse::<u32>()
    };
    parsed.map(|value| value.to_string()).unwrap_or_else(|_| "0".to_string())
}

#[cfg(windows)]
fn hidden_command(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(windows))]
fn hidden_command(program: &str) -> Command {
    Command::new(program)
}
