//! 构建与 Charles 类似的桌面菜单；菜单项通过事件通知前端执行命令。
//!
//! 注意：菜单项不注册原生快捷键（accelerator）。Windows 下 Tauri 会把快捷键消息
//! 交给 `TranslateAcceleratorW` 处理，但目标句柄是 WebView2 子窗口，WM_COMMAND
//! 到不了菜单所在的宿主窗口，结果是按键被吞掉且菜单事件不触发。
//! 因此快捷键统一在前端 keydown 里处理（见 `src/App.vue` 的 SHORTCUTS）。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, FilePath, MessageDialogKind};

use crate::logger;
use crate::models::{APP_NAME, APP_VERSION};
use crate::AppState;

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let file_menu = Submenu::with_items(
        app,
        "文件",
        true,
        &[
            &MenuItem::with_id(app, "compose", "新建请求", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open-session", "打开会话…", true, None::<&str>)?,
            &MenuItem::with_id(app, "save-session", "保存会话…", true, None::<&str>)?,
            &MenuItem::with_id(app, "export-har", "导出 HAR…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open-settings", "设置…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            // 用自定义项而不是 PredefinedMenuItem::quit，确保退出时能恢复系统代理
            &MenuItem::with_id(app, "exit", "退出", true, None::<&str>)?,
        ],
    )?;

    let session_menu = Submenu::with_items(
        app,
        "会话",
        true,
        &[
            &MenuItem::with_id(app, "clear-session", "清空会话", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "toggle-recording",
                "开始 / 暂停记录",
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(app, "focus-filter", "查找请求", true, None::<&str>)?,
        ],
    )?;

    let proxy_menu = Submenu::with_items(
        app,
        "代理",
        true,
        &[
            &MenuItem::with_id(app, "toggle-proxy", "启动 / 停止代理", true, None::<&str>)?,
            &MenuItem::with_id(app, "copy-proxy", "复制代理地址", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open-settings", "代理设置…", true, None::<&str>)?,
        ],
    )?;

    let help_menu = Submenu::with_items(
        app,
        "帮助",
        true,
        &[
            &MenuItem::with_id(app, "open-logs", "打开日志目录", true, None::<&str>)?,
            &MenuItem::with_id(
                app,
                "export-diagnostics",
                "导出诊断信息…",
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "about", format!("关于 {APP_NAME}"), true, None::<&str>)?,
        ],
    )?;

    let menu = Menu::with_items(app, &[&file_menu, &session_menu, &proxy_menu, &help_menu])?;
    app.set_menu(menu)?;
    Ok(())
}

/// 打开日志目录；未初始化日志时给出提示
pub fn open_logs(app: &AppHandle) {
    match logger::directory() {
        Some(path) => logger::open_directory(&path),
        None => {
            app.dialog()
                .message("日志目录尚未初始化")
                .title("打开日志目录")
                .kind(MessageDialogKind::Warning)
                .show(|_| {});
        }
    }
}

/// 导出包含版本、系统信息、代理状态、设置与最近日志的诊断文本
pub fn export_diagnostics(app: &AppHandle) {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let app_handle = app.clone();
    app.dialog()
        .file()
        .set_title("导出诊断信息")
        .set_file_name(format!("freecharles-diagnostics-{stamp}.txt"))
        .add_filter("文本文件", &["txt"])
        .save_file(move |path| {
            let Some(path) = path.and_then(|value: FilePath| value.into_path().ok()) else {
                return;
            };
            let content = build_diagnostics(&app_handle);
            match std::fs::write(&path, content) {
                Ok(()) => {
                    app_handle
                        .dialog()
                        .message(format!("诊断信息已导出到：{}", path.display()))
                        .title("导出诊断信息")
                        .kind(MessageDialogKind::Info)
                        .show(|_| {});
                }
                Err(error) => {
                    app_handle
                        .dialog()
                        .message(format!("导出失败：{error}"))
                        .title("导出诊断信息")
                        .kind(MessageDialogKind::Error)
                        .show(|_| {});
                }
            }
        });
}

fn build_diagnostics(app: &AppHandle) -> String {
    let state = app.try_state::<AppState>();
    let proxy_status = state
        .as_ref()
        .map(|value| value.proxy.status())
        .map(|value| serde_json::to_string_pretty(&value).unwrap_or_default())
        .unwrap_or_else(|| "(状态未知)".to_string());
    let settings = state
        .as_ref()
        .map(|value| redact_settings(&value.settings().get()))
        .unwrap_or_else(|| "(设置未知)".to_string());
    let logs = logger::read_diagnostic_logs();
    [
        format!("{APP_NAME} {APP_VERSION} 诊断信息"),
        format!(
            "导出时间: {}",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z")
        ),
        format!(
            "系统: {} {} (Tauri {})",
            std::env::consts::OS,
            std::env::consts::ARCH,
            tauri::VERSION
        ),
        format!(
            "日志目录: {}",
            logger::directory()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "(未初始化)".to_string())
        ),
        String::new(),
        "== 代理状态 ==".to_string(),
        proxy_status,
        String::new(),
        "== 当前设置 ==".to_string(),
        settings,
        String::new(),
        "== 最近日志 ==".to_string(),
        if logs.is_empty() {
            "(暂无日志)".to_string()
        } else {
            logs
        },
    ]
    .join("\n")
}

/// 隐藏上游代理地址中的密码，避免导出诊断时泄漏凭据
fn redact_settings(settings: &crate::models::AppSettings) -> String {
    let mut value = serde_json::to_value(settings).unwrap_or_default();
    if let Some(serde_json::Value::String(url)) = value.get_mut("upstreamProxyUrl") {
        if let Some(redacted) = redact_proxy_url(url) {
            *url = redacted;
        }
    }
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

/// 支持 `http://user:pass@host` 与 `user:pass@host` 两种写法
fn redact_proxy_url(url: &str) -> Option<String> {
    let (prefix, rest) = match url.find("://") {
        Some(index) => (&url[..index + 3], &url[index + 3..]),
        None => ("", url),
    };
    let at = rest.find('@')?;
    let credentials = &rest[..at];
    let colon = credentials.find(':')?;
    Some(format!(
        "{prefix}{}:***@{}",
        &credentials[..colon],
        &rest[at + 1..]
    ))
}

/// 关于对话框：展示版本、能力说明与当前 HTTPS 解密状态
pub fn show_about(app: &AppHandle) {
    let detail = format!(
        "{APP_NAME} {APP_VERSION}\n\n\
         Charles 风格的桌面 HTTP / HTTPS 抓包代理。\n\n\
         · HTTP 请求直接转发并抓取\n\
         · HTTPS 默认以 CONNECT 隧道透传；开启解密后由本地 CA 做 TLS 中间人，可查看明文内容\n\
         · 支持 Structure / Sequence 视图、请求重放、Compose、会话保存与 HAR 导出\n\
         · 可自动接管 Windows 系统代理，退出时恢复\n\n\
         HTTPS 解密：{}\n\n\
         Tauri {} · Rust {}",
        https_state_text(app),
        tauri::VERSION,
        std::env::consts::ARCH
    );
    app.dialog()
        .message(detail)
        .title(format!("关于 {APP_NAME}"))
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}

/// 汇总当前 HTTPS 解密状态，便于用户判断是否需要安装根证书
fn https_state_text(app: &AppHandle) -> String {
    let Some(state) = app.try_state::<crate::AppState>() else {
        return "状态未知".to_string();
    };
    let enabled = state.settings().get().decrypt_https;
    let generated = state.ca.generated();
    match (enabled, generated) {
        (true, true) => "已开启（根证书已就绪）".to_string(),
        (true, false) => "已开启，但尚未生成根证书，请在设置中生成并安装".to_string(),
        (false, true) => "未开启（根证书已生成，可在设置中开启）".to_string(),
        (false, false) => "未开启（可在设置中生成并信任根证书后开启）".to_string(),
    }
}
