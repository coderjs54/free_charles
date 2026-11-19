//! Tauri 命令：等价于 Electron 版本主进程注册的 IPC handler。

use tauri::{AppHandle, Manager};

use crate::body::{decode_buffer, render_body};
use crate::har::build_har;
use crate::models::{
    AppSettings, AppSettingsPatch, BodyView, ComposePayload, EntryDetail, EntryMeta, HttpsCaInfo,
    ProxyStatus, SaveBodyResult, SaveResult, SessionFileData, SystemProxyState, EVENT_LOADED,
    EVENT_SETTINGS,
};
use crate::proxy::ProxyOptionsPatch;
use crate::store::{DetailSnapshot, LAZY_BODY_THRESHOLD};
use crate::AppState;

/// 生成延迟加载占位；前端拿到 deferred=true 后会通过 get_entry_body 拉取完整内容
fn deferred_body_view(snapshot: &DetailSnapshot, side: &str) -> BodyView {
    let is_request = side == "request";
    BodyView {
        available: true,
        size: if is_request {
            snapshot.request_body_size
        } else {
            snapshot.response_body_size
        },
        captured_size: if is_request {
            snapshot.request_captured
        } else {
            snapshot.response_captured
        },
        decoded_size: 0,
        truncated: if is_request {
            snapshot.request_truncated
        } else {
            snapshot.response_truncated
        },
        text_truncated: false,
        kind: "empty".to_string(),
        mime: if is_request {
            snapshot.meta.request_mime.clone()
        } else {
            snapshot.meta.response_mime.clone()
        },
        encoding: if is_request {
            snapshot.request_encoding.clone()
        } else {
            snapshot.response_encoding.clone()
        },
        text: None,
        base64: None,
        decode_error: None,
        deferred: true,
    }
}

/// 取单侧 BodyView：大 Body 先返回占位，小 Body 走缓存/即时解码
fn build_body(
    state: &AppState,
    id: &str,
    snapshot: &DetailSnapshot,
    side: &str,
    lazy: bool,
) -> BodyView {
    let is_request = side == "request";
    let captured = if is_request {
        snapshot.request_captured
    } else {
        snapshot.response_captured
    };
    if lazy && snapshot.meta.state != "pending" && captured > LAZY_BODY_THRESHOLD {
        return deferred_body_view(snapshot, side);
    }
    if let Some(view) = state.store().cached_body_view(id, side) {
        return view;
    }
    let view = render_body(
        if is_request {
            snapshot.request_body.as_ref()
        } else {
            snapshot.response_body.as_ref()
        },
        if is_request {
            snapshot.request_body_size
        } else {
            snapshot.response_body_size
        },
        if is_request {
            snapshot.request_truncated
        } else {
            snapshot.response_truncated
        },
        if is_request {
            &snapshot.meta.request_mime
        } else {
            &snapshot.meta.response_mime
        },
        if is_request {
            snapshot.request_encoding.as_deref()
        } else {
            snapshot.response_encoding.as_deref()
        },
    );
    state.store().cache_body_view(id, side, view.clone());
    view
}

fn build_detail(state: &AppState, id: &str, lazy: bool) -> Option<EntryDetail> {
    let snapshot = state.store().snapshot(id)?;
    let request_body = build_body(state, id, &snapshot, "request", lazy);
    let response_body = build_body(state, id, &snapshot, "response", lazy);
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
pub fn get_entry(app: AppHandle, id: String, lazy_bodies: Option<bool>) -> Option<EntryDetail> {
    build_detail(&app.state::<AppState>(), &id, lazy_bodies.unwrap_or(false))
}

/// 按需拉取单侧完整 Body，供详情面板打开大 Body 时使用
#[tauri::command]
pub fn get_entry_body(app: AppHandle, id: String, side: String) -> Option<BodyView> {
    let state = app.state::<AppState>();
    let snapshot = state.store().snapshot(&id)?;
    let side = if side == "request" {
        "request"
    } else {
        "response"
    };
    Some(build_body(&state, &id, &snapshot, side, false))
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
    mut patch: AppSettingsPatch,
) -> Result<AppSettings, String> {
    let (previous, next) = {
        let state = app.state::<AppState>();
        let previous = state.settings().get();
        // 用户显式改动「自动系统代理」即视为已征求过同意
        if patch.auto_system_proxy.is_some()
            && patch.auto_system_proxy != Some(previous.auto_system_proxy)
        {
            patch.system_proxy_prompted = Some(true);
        }
        // 启用上游代理时必须先通过地址校验，避免保存一个无法使用的配置
        let next_upstream_enabled = patch
            .upstream_proxy_enabled
            .unwrap_or(previous.upstream_proxy_enabled);
        let next_upstream_url = patch
            .upstream_proxy_url
            .clone()
            .unwrap_or_else(|| previous.upstream_proxy_url.clone());
        if next_upstream_enabled {
            crate::upstream::parse_upstream_proxy_url(&next_upstream_url)
                .map_err(|message| format!("上游代理配置无效：{message}"))?;
        }
        let next = state.settings().update(patch);
        state.store().set_max_entries(next.max_entries);
        state.proxy.update_options(ProxyOptionsPatch {
            port: Some(next.proxy_port),
            allow_lan: Some(next.allow_lan),
            max_body_size: Some(next.max_body_size),
            record_tunnels: Some(next.record_tunnels),
            decrypt_https: Some(next.decrypt_https),
            allow_insecure_upstream: Some(next.allow_insecure_upstream),
            upstream_proxy_url: Some(if next.upstream_proxy_enabled {
                next.upstream_proxy_url.clone()
            } else {
                String::new()
            }),
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
    // 仅在影响系统代理的字段变化时重新应用/恢复，避免切换主题等操作触发注册表写入
    let system_proxy_changed = previous.auto_system_proxy != next.auto_system_proxy
        || previous.system_proxy_prompted != next.system_proxy_prompted
        || previous.proxy_port != next.proxy_port
        || previous.allow_lan != next.allow_lan;
    if system_proxy_changed {
        crate::sync_system_proxy(&app);
    }
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
        // HAR 导出必须使用完整 Body，不能走延迟加载
        if let Some(detail) = build_detail(&state, id, false) {
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
