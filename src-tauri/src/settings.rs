//! settings.json 持久化，带字段校验与范围钳制。

use std::path::{Path, PathBuf};

use crate::models::{
    AppSettings, AppSettingsPatch, MAX_BODY_LIMIT, MAX_ENTRIES, MIN_BODY_LIMIT, MIN_ENTRIES,
};

pub struct SettingsStore {
    path: PathBuf,
    settings: AppSettings,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        let settings = load_settings(&path);
        Self { path, settings }
    }

    pub fn get(&self) -> AppSettings {
        self.settings.clone()
    }

    pub fn update(&mut self, patch: AppSettingsPatch) -> AppSettings {
        let mut next = self.settings.clone();
        if let Some(value) = patch.proxy_port {
            next.proxy_port = value;
        }
        if let Some(value) = patch.auto_start_proxy {
            next.auto_start_proxy = value;
        }
        if let Some(value) = patch.allow_lan {
            next.allow_lan = value;
        }
        if let Some(value) = patch.auto_system_proxy {
            next.auto_system_proxy = value;
        }
        if let Some(value) = patch.decrypt_https {
            next.decrypt_https = value;
        }
        if let Some(value) = patch.allow_insecure_upstream {
            next.allow_insecure_upstream = value;
        }
        if let Some(value) = patch.max_body_size {
            next.max_body_size = value;
        }
        if let Some(value) = patch.max_entries {
            next.max_entries = value;
        }
        if let Some(value) = patch.record_tunnels {
            next.record_tunnels = value;
        }
        self.settings = normalize(next);
        self.save();
        self.get()
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.settings) {
            // 配置写入失败不影响主流程
            let _ = std::fs::write(&self.path, text);
        }
    }
}

fn load_settings(path: &Path) -> AppSettings {
    let Ok(text) = std::fs::read_to_string(path) else {
        return AppSettings::default();
    };
    // 兼容被其他编辑器写入 BOM 的配置文件
    let text = text.trim_start_matches('\u{feff}');
    match serde_json::from_str::<AppSettings>(text) {
        Ok(settings) => normalize(settings),
        Err(_) => AppSettings::default(),
    }
}

fn normalize(input: AppSettings) -> AppSettings {
    AppSettings {
        proxy_port: clamp_u16(input.proxy_port, 1024, 65535, 8888),
        auto_start_proxy: input.auto_start_proxy,
        allow_lan: input.allow_lan,
        auto_system_proxy: input.auto_system_proxy,
        decrypt_https: input.decrypt_https,
        allow_insecure_upstream: input.allow_insecure_upstream,
        max_body_size: input.max_body_size.clamp(MIN_BODY_LIMIT, MAX_BODY_LIMIT),
        max_entries: input.max_entries.clamp(MIN_ENTRIES, MAX_ENTRIES),
        record_tunnels: input.record_tunnels,
    }
}

fn clamp_u16(value: u16, min: u16, max: u16, fallback: u16) -> u16 {
    if value < min {
        fallback
    } else {
        value.min(max)
    }
}
