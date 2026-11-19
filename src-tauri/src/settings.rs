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
        if let Some(value) = patch.system_proxy_prompted {
            next.system_proxy_prompted = value;
        }
        if let Some(value) = patch.decrypt_https {
            next.decrypt_https = value;
        }
        if let Some(value) = patch.allow_insecure_upstream {
            next.allow_insecure_upstream = value;
        }
        if let Some(value) = patch.upstream_proxy_enabled {
            next.upstream_proxy_enabled = value;
        }
        if let Some(value) = patch.upstream_proxy_url {
            next.upstream_proxy_url = value;
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
        if let Some(value) = patch.theme {
            next.theme = value;
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
        system_proxy_prompted: input.system_proxy_prompted,
        decrypt_https: input.decrypt_https,
        allow_insecure_upstream: input.allow_insecure_upstream,
        upstream_proxy_enabled: input.upstream_proxy_enabled,
        upstream_proxy_url: input.upstream_proxy_url.trim().to_string(),
        max_body_size: input.max_body_size.clamp(MIN_BODY_LIMIT, MAX_BODY_LIMIT),
        max_entries: input.max_entries.clamp(MIN_ENTRIES, MAX_ENTRIES),
        record_tunnels: input.record_tunnels,
        theme: if input.theme == "dark" {
            "dark".to_string()
        } else {
            "light".to_string()
        },
    }
}

fn clamp_u16(value: u16, min: u16, max: u16, fallback: u16) -> u16 {
    if value < min {
        fallback
    } else {
        value.min(max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_use_defaults() {
        let settings: AppSettings = serde_json::from_str("{}").unwrap();
        let normalized = normalize(settings);
        assert_eq!(normalized.theme, "light");
        assert!(!normalized.system_proxy_prompted);
        assert!(normalized.auto_system_proxy);
    }

    #[test]
    fn normalizes_theme_and_ranges() {
        let settings = AppSettings {
            theme: "DARK".to_string(),
            proxy_port: 80,
            max_entries: 1,
            ..Default::default()
        };
        let normalized = normalize(settings);
        assert_eq!(normalized.theme, "light");
        assert_eq!(normalized.proxy_port, 8888);
        assert_eq!(normalized.max_entries, MIN_ENTRIES);
    }
}
