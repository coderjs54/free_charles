//! FreeCharles —— Charles 风格的桌面 HTTP 抓包代理（Tauri 版）。

mod body;
mod cert;
mod commands;
mod har;
mod logger;
mod menu;
mod models;
mod proxy;
mod settings;
mod store;
mod system_proxy;

use std::sync::{Arc, Mutex, MutexGuard};

use tauri::{AppHandle, Emitter, Manager, RunEvent};

use crate::cert::HttpsCa;
use crate::models::{
    SystemProxyState, APP_VERSION, EVENT_COMMAND, EVENT_ERROR, EVENT_SYSTEM_PROXY,
};
use crate::proxy::{CaptureProxy, ProxyOptions};
use crate::settings::SettingsStore;
use crate::store::SessionStore;
use crate::system_proxy::SystemProxyManager;

/// 全局应用状态：设置、系统代理、会话存储、抓包代理与本地 CA
pub struct AppState {
    pub settings: Mutex<SettingsStore>,
    pub system_proxy: Mutex<SystemProxyManager>,
    pub store: Arc<Mutex<SessionStore>>,
    pub proxy: CaptureProxy,
    pub ca: Arc<HttpsCa>,
}

impl AppState {
    pub fn store(&self) -> MutexGuard<'_, SessionStore> {
        self.store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn settings(&self) -> MutexGuard<'_, SettingsStore> {
        self.settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn system_proxy(&self) -> MutexGuard<'_, SystemProxyManager> {
        self.system_proxy
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// 把运行期错误同时写入控制台并广播给界面，避免只写日志导致用户无从排查
pub fn report_error(app: &AppHandle, message: &str) {
    logger::log_error(message);
    let _ = app.emit(EVENT_ERROR, message.to_string());
}

/// 根据「代理是否运行 + 是否允许自动接管 + 是否已征得同意」同步 Windows 系统代理
pub fn sync_system_proxy(app: &AppHandle) -> SystemProxyState {
    let Some(state) = app.try_state::<AppState>() else {
        return SystemProxyState {
            supported: false,
            active: false,
            server: String::new(),
            error: None,
        };
    };

    let settings = state.settings().get();
    let status = state.proxy.status();
    let result = {
        let mut manager = state.system_proxy();
        if settings.auto_system_proxy && settings.system_proxy_prompted && status.running {
            manager.apply(status.port)
        } else {
            manager.restore()
        }
    };

    let _ = app.emit(EVENT_SYSTEM_PROXY, &result);
    if let Some(error) = &result.error {
        report_error(app, error);
    } else if result.active {
        logger::log_info(&format!("系统代理已接管：{}", result.server));
    } else {
        logger::log_info("系统代理已恢复");
    }
    result
}

fn bootstrap(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    logger::init(data_dir.join("logs"));
    logger::log_info(&format!(
        "应用启动 v{APP_VERSION} · Tauri {} · {} {}",
        tauri::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    ));

    let settings_store = SettingsStore::new(data_dir.join("settings.json"));
    let mut system_proxy = SystemProxyManager::new(data_dir.join("system-proxy-backup.json"));
    // 上次运行如果是崩溃退出，先把系统代理恢复成用户原值
    system_proxy.recover();

    let settings = settings_store.get();
    let sink = proxy::make_event_sink(handle.clone());
    let store = Arc::new(Mutex::new(SessionStore::new(
        Some(sink),
        settings.max_entries,
    )));
    // 本地 HTTPS 解密 CA：启动时尝试加载已有根证书，不主动生成
    let ca = Arc::new(HttpsCa::new(data_dir.join("https-ca")));
    ca.warmup();
    let proxy = CaptureProxy::new(
        store.clone(),
        handle.clone(),
        ProxyOptions {
            port: settings.proxy_port,
            allow_lan: settings.allow_lan,
            max_body_size: settings.max_body_size,
            record_tunnels: settings.record_tunnels,
            recording: true,
            decrypt_https: settings.decrypt_https,
            allow_insecure_upstream: settings.allow_insecure_upstream,
        },
        ca.clone(),
    );

    app.manage(AppState {
        settings: Mutex::new(settings_store),
        system_proxy: Mutex::new(system_proxy),
        store,
        proxy: proxy.clone(),
        ca,
    });

    menu::install(&handle)?;
    handle.on_menu_event(move |app, event| {
        let id = event.id().as_ref().to_string();
        match id.as_str() {
            "about" => menu::show_about(app),
            "open-logs" => menu::open_logs(app),
            "export-diagnostics" => menu::export_diagnostics(app),
            // 走 AppHandle::exit 而不是 PostQuitMessage，确保退出时恢复系统代理
            "exit" => app.exit(0),
            _ => {
                let _ = app.emit(EVENT_COMMAND, id);
            }
        }
    });

    if settings.auto_start_proxy {
        let error_handle = handle.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = proxy.start().await {
                report_error(&error_handle, &format!("自动启动代理失败：{error}"));
            }
        });
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            bootstrap(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_proxy,
            commands::stop_proxy,
            commands::get_proxy_status,
            commands::set_recording,
            commands::list_entries,
            commands::clear_session,
            commands::remove_entry,
            commands::get_entry,
            commands::get_entry_body,
            commands::repeat_entry,
            commands::compose,
            commands::get_settings,
            commands::get_system_proxy,
            commands::quit_app,
            commands::get_https_ca_info,
            commands::ensure_https_ca,
            commands::install_https_ca,
            commands::reveal_https_ca,
            commands::update_settings,
            commands::save_session,
            commands::open_session,
            commands::export_har,
            commands::save_body,
            commands::suggest_body_file_name
        ])
        .build(tauri::generate_context!())
        .expect("error while building FreeCharles")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                // 先恢复系统代理，再关闭本地监听，避免退出后浏览器还指向已关闭的端口
                if let Some(state) = app.try_state::<AppState>() {
                    state.system_proxy().restore();
                    state.proxy.stop_now(true);
                }
            }
        });
}
