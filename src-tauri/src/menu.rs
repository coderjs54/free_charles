//! 构建与 Charles 类似的桌面菜单；菜单项通过事件通知前端执行命令。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::models::{APP_NAME, APP_VERSION};

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let file_menu = Submenu::with_items(
        app,
        "文件",
        true,
        &[
            &MenuItem::with_id(app, "compose", "新建请求…", true, Some("CmdOrCtrl+N"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open-session", "打开会话…", true, Some("CmdOrCtrl+O"))?,
            &MenuItem::with_id(app, "save-session", "保存会话…", true, Some("CmdOrCtrl+S"))?,
            &MenuItem::with_id(
                app,
                "export-har",
                "导出 HAR…",
                true,
                Some("CmdOrCtrl+Shift+E"),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open-settings", "设置…", true, Some("CmdOrCtrl+,"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some("退出"))?,
        ],
    )?;

    let edit_menu = Submenu::with_items(
        app,
        "编辑",
        true,
        &[
            &PredefinedMenuItem::undo(app, Some("撤销"))?,
            &PredefinedMenuItem::redo(app, Some("重做"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some("剪切"))?,
            &PredefinedMenuItem::copy(app, Some("复制"))?,
            &PredefinedMenuItem::paste(app, Some("粘贴"))?,
            &PredefinedMenuItem::select_all(app, Some("全选"))?,
        ],
    )?;

    let session_menu = Submenu::with_items(
        app,
        "会话",
        true,
        &[
            &MenuItem::with_id(app, "clear-session", "清空会话", true, Some("CmdOrCtrl+K"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "toggle-recording",
                "开始 / 暂停记录",
                true,
                Some("CmdOrCtrl+Shift+R"),
            )?,
            &MenuItem::with_id(app, "focus-filter", "查找请求", true, Some("CmdOrCtrl+F"))?,
        ],
    )?;

    let proxy_menu = Submenu::with_items(
        app,
        "代理",
        true,
        &[
            &MenuItem::with_id(
                app,
                "toggle-proxy",
                "启动 / 停止代理",
                true,
                Some("CmdOrCtrl+Shift+P"),
            )?,
            &MenuItem::with_id(app, "copy-proxy", "复制代理地址", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open-settings", "代理设置…", true, None::<&str>)?,
        ],
    )?;

    let view_menu = Submenu::with_items(
        app,
        "视图",
        true,
        &[
            &MenuItem::with_id(app, "reload", "重新加载", true, Some("CmdOrCtrl+R"))?,
            &MenuItem::with_id(app, "devtools", "开发者工具", true, Some("F12"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::fullscreen(app, Some("全屏"))?,
        ],
    )?;

    let help_menu = Submenu::with_items(
        app,
        "帮助",
        true,
        &[&MenuItem::with_id(
            app,
            "about",
            format!("关于 {APP_NAME}"),
            true,
            None::<&str>,
        )?],
    )?;

    let menu = Menu::with_items(
        app,
        &[
            &file_menu,
            &edit_menu,
            &session_menu,
            &proxy_menu,
            &view_menu,
            &help_menu,
        ],
    )?;
    app.set_menu(menu)?;
    Ok(())
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
