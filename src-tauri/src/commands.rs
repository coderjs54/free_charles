//! Tauri 命令：等价于 Electron 版本主进程注册的 IPC handler。

use tauri::{AppHandle, Manager};

use crate::body::{decode_buffer, render_body};
use crate::har::build_har;
use crate::models::{
    AppSettings, AppSettingsPatch, ComposePayload, EntryDetail, EntryMeta, HttpsCaInfo,
    ProxyStatus, SaveBodyResult, SaveResult, SessionFileData, SystemProxyState, EVENT_LOADED,
    EVENT_SETTINGS,
};
use crate::proxy::ProxyOptionsPatch;
use crate::AppState;

fn build_detail(state: &AppState, id: &str) -> Option<EntryDetail> {
    let snapshot = state.store().snapshot(id)?;
    let request_body = render_body(
        snapshot.request_body.as_ref(),
        snapshot.request_body_size,
        snapshot.request_truncated,
        &snapshot.meta.request_mime,
        snapshot.request_encoding.as_deref(),
    );
    let response_body = render_body(
        snapshot.response_body.as_ref(),
        snapshot.response_body_size,
        snapshot.response_truncated,
        &snapshot.meta.response_mime,
        snapshot.response_encoding.as_deref(),
    );
    Some(snapshot.into_detail(request_body, response_body))
}

#[tauri::command]
pub async fn start_proxy(app: AppHandle, port: Option<u16>) -> ProxyStatus {
    let state = app.state::<AppState>();
    if let Some(port) = port {
        state.proxy.update_options(ProxyOptionsPatch {
            port: Some(port),
            ..Default::default()
        });
    }
    // 具体错误已经通过代理状态事件广播，这里统一返回最新状态
    let _ = state.proxy.start().await;
    state.proxy.status()
}

#[tauri::command]
pub async fn stop_proxy(app: AppHandle) -> ProxyStatus {
    let state = app.state::<AppState>();
    state.proxy.stop(false).await;
    state.proxy.status()
}

#[tauri::command]
pub fn get_proxy_status(app: AppHandle) -> ProxyStatus {
    app.state::<AppState>().proxy.status()
}

#[tauri::command]
pub fn set_recording(app: AppHandle, recording: bool) -> ProxyStatus {
    app.state::<AppState>().proxy.set_recording(recording)
}

#[tauri::command]
pub fn list_entries(app: AppHandle) -> Vec<EntryMeta> {
    app.state::<AppState>().store().list()
}

#[tauri::command]
pub fn clear_session(app: AppHandle) {
    app.state::<AppState>().store().clear();
}

#[tauri::command]
pub fn remove_entry(app: AppHandle, id: String) {
    app.state::<AppState>().store().remove(&id);
}

#[tauri::command]
pub fn get_entry(app: AppHandle, id: String) -> Option<EntryDetail> {
    build_detail(&app.state::<AppState>(), &id)
}

#[tauri::command]
pub fn repeat_entry(app: AppHandle, id: String) -> Result<EntryMeta, String> {
    app.state::<AppState>().proxy.resend(&id)
}

#[tauri::command]
pub fn compose(app: AppHandle, payload: ComposePayload) -> Result<EntryMeta, String> {
    app.state::<AppState>().proxy.compose(payload)
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppSettings {
    app.state::<AppState>().settings().get()
}

#[tauri::command]
pub fn get_system_proxy(app: AppHandle) -> SystemProxyState {
    app.state::<AppState>().system_proxy().get_status()
}

/// 前端快捷键触发的退出；与菜单「退出」一致，走 app.exit 以便恢复系统代理
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn get_https_ca_info(app: AppHandle) -> HttpsCaInfo {
    app.state::<AppState>().ca.info()
}

/// 生成（或加载）本地 HTTPS 根证书
#[tauri::command]
pub fn ensure_https_ca(app: AppHandle) -> HttpsCaInfo {
    app.state::<AppState>().ca.ensure()
}

/// 把根证书安装到当前用户的信任存储
#[tauri::command]
pub fn install_https_ca(app: AppHandle) -> HttpsCaInfo {
    app.state::<AppState>().ca.install()
}

/// 在资源管理器中定位根证书文件
#[tauri::command]
pub fn reveal_https_ca(app: AppHandle) -> HttpsCaInfo {
    let state = app.state::<AppState>();
    let info = state.ca.info();
    if let Some(path) = &info.cert_path {
        reveal_in_file_manager(path);
    }
    info
}

fn reveal_in_file_manager(path: &str) {
    #[cfg(windows)]
    {
        // explorer /select 会直接定位到文件
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    patch: AppSettingsPatch,
) -> Result<AppSettings, String> {
    let (previous, next) = {
        let state = app.state::<AppState>();
        let previous = state.settings().get();
        let next = state.settings().update(patch);
        state.store().set_max_entries(next.max_entries);
        state.proxy.update_options(ProxyOptionsPatch {
            port: Some(next.proxy_port),
            allow_lan: Some(next.allow_lan),
            max_body_size: Some(next.max_body_size),
            record_tunnels: Some(next.record_tunnels),
            decrypt_https: Some(next.decrypt_https),
            allow_insecure_upstream: Some(next.allow_insecure_upstream),
        });
        let _ = tauri::Emitter::emit(&app, EVENT_SETTINGS, &next);
        (previous, next)
    };

    let need_restart = {
        let state = app.state::<AppState>();
        let status = state.proxy.status();
        status.running
            && (previous.proxy_port != next.proxy_port || previous.allow_lan != next.allow_lan)
    };
    if need_restart {
        let _ = app.state::<AppState>().proxy.start().await;
    }
    // 处理「自动接管系统代理」开关变化；端口变化时也会在这里再次对齐
    crate::sync_system_proxy(&app);
    Ok(next)
}

#[tauri::command]
pub fn save_session(app: AppHandle, path: String) -> Result<SaveResult, String> {
    let data = app.state::<AppState>().store().export_session();
    let count = data.entries.len();
    let text =
        serde_json::to_string_pretty(&data).map_err(|error| format!("会话序列化失败：{error}"))?;
    std::fs::write(&path, text).map_err(|error| format!("保存会话失败：{error}"))?;
    Ok(SaveResult { path, count })
}

#[tauri::command]
pub fn open_session(app: AppHandle, path: String) -> Result<SaveResult, String> {
    let text = std::fs::read_to_string(&path)
        .map_err(|_| "会话文件读取失败，请确认文件是否存在".to_string())?;
    // 兼容带 BOM 的会话文件
    let text = text.trim_start_matches('\u{feff}');
    let data = serde_json::from_str::<SessionFileData>(text)
        .map_err(|_| "会话文件解析失败，请确认文件内容完整".to_string())?;
    let state = app.state::<AppState>();
    let metas = state.store().import_session(data)?;
    let count = metas.len();
    let _ = tauri::Emitter::emit(&app, EVENT_LOADED, &metas);
    Ok(SaveResult { path, count })
}

#[tauri::command]
pub fn export_har(app: AppHandle, path: String) -> Result<SaveResult, String> {
    let state = app.state::<AppState>();
    let ids: Vec<String> = state
        .store()
        .list()
        .into_iter()
        .filter(|meta| !meta.tunnel && meta.method != "CONNECT")
        .map(|meta| meta.id)
        .collect();
    let mut details = Vec::new();
    for id in &ids {
        if let Some(detail) = build_detail(&state, id) {
            details.push(detail);
        }
    }
    if details.is_empty() {
        return Err("当前会话没有可导出的 HTTP 请求".to_string());
    }
    let har = build_har(&details);
    let count = details.len();
    let text =
        serde_json::to_string_pretty(&har).map_err(|error| format!("HAR 序列化失败：{error}"))?;
    std::fs::write(&path, text).map_err(|error| format!("导出 HAR 失败：{error}"))?;
    Ok(SaveResult { path, count })
}

#[tauri::command]
pub fn save_body(
    app: AppHandle,
    id: String,
    side: String,
    path: String,
) -> Result<SaveBodyResult, String> {
    let state = app.state::<AppState>();
    let (buffer, encoding) = {
        let store = state.store();
        let tx = store.tx(&id).ok_or_else(|| "请求已不存在".to_string())?;
        if side == "request" {
            (tx.captured_request(), tx.request_encoding.clone())
        } else {
            (tx.captured_response(), tx.response_encoding.clone())
        }
    };
    let Some(buffer) = buffer.filter(|value| !value.is_empty()) else {
        return Err("当前没有可保存的内容".to_string());
    };
    let decoded = decode_buffer(buffer.as_ref(), encoding.as_deref());
    std::fs::write(&path, &decoded.data).map_err(|error| format!("保存内容失败：{error}"))?;
    Ok(SaveBodyResult {
        path,
        size: decoded.data.len(),
    })
}

/// 保存内容时的建议文件名，由前端拿去展示系统保存对话框
#[tauri::command]
pub fn suggest_body_file_name(app: AppHandle, id: String, side: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    let store = state.store();
    let tx = store.tx(&id).ok_or_else(|| "请求已不存在".to_string())?;
    let mime = if side == "request" {
        &tx.meta.request_mime
    } else {
        &tx.meta.response_mime
    };
    Ok(file_name_from_url(
        &tx.meta.url,
        extension_from_mime(mime),
        if side == "request" {
            "request"
        } else {
            "response"
        },
    ))
}

fn extension_from_mime(mime: &str) -> &'static str {
    let value = mime.to_lowercase();
    if value.contains("json") {
        return "json";
    }
    if value.contains("html") {
        return "html";
    }
    if value.contains("xml") {
        return "xml";
    }
    if value.contains("css") {
        return "css";
    }
    if value.contains("javascript") {
        return "js";
    }
    if value.contains("csv") {
        return "csv";
    }
    if value.contains("png") {
        return "png";
    }
    if value.contains("jpeg") || value.contains("jpg") {
        return "jpg";
    }
    if value.contains("gif") {
        return "gif";
    }
    if value.contains("webp") {
        return "webp";
    }
    if value.contains("svg") {
        return "svg";
    }
    if value.contains("pdf") {
        return "pdf";
    }
    if value.contains("zip") {
        return "zip";
    }
    if value.contains("plain") {
        return "txt";
    }
    "bin"
}

fn file_name_from_url(url: &str, extension: &str, side: &str) -> String {
    // 取路径最后一个非空分段作为文件名
    let mut name = url::Url::parse(url)
        .ok()
        .and_then(|parsed| {
            parsed
                .path_segments()
                .and_then(|segments| segments.rev().find(|segment| !segment.is_empty()))
                .map(|segment| segment.to_string())
        })
        .unwrap_or_default();
    name = name
        .chars()
        .map(|ch| match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .take(60)
        .collect();
    if name.is_empty() {
        name = if side == "request" {
            "request-body".to_string()
        } else {
            "response-body".to_string()
        };
    }
    if !name.to_lowercase().ends_with(&format!(".{extension}")) {
        name = format!("{name}.{extension}");
    }
    name
}
