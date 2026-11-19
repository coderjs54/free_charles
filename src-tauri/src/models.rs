//! 前后端共享数据结构，字段命名与前端 TypeScript 类型保持一致（camelCase）。

use serde::{Deserialize, Serialize};

/// 请求/响应头键值对（保留重复头，如 Set-Cookie）
pub type HeaderPair = (String, String);

fn is_none<T>(value: &Option<T>) -> bool {
    value.is_none()
}

/// 序列化时省略值为 false 的布尔字段，保持与前端旧数据结构兼容
fn is_false(value: &bool) -> bool {
    !*value
}

/// 列表行使用的轻量元数据，主进程会通过事件增量推送给前端
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct EntryMeta {
    pub id: String,
    /// 会话内自增序号
    pub seq: u64,
    /// 请求开始时间（毫秒时间戳）
    pub start_time: u64,
    pub method: String,
    /// 完整 URL
    pub url: String,
    /// 主机名（不含端口）
    pub host: String,
    pub port: u16,
    /// pathname + search
    pub path: String,
    pub protocol: String,
    pub client_ip: String,
    #[serde(skip_serializing_if = "is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "is_none")]
    pub status_text: Option<String>,
    pub state: String,
    #[serde(skip_serializing_if = "is_none")]
    pub error: Option<String>,
    /// 请求头 + 请求体字节数
    pub request_size: u64,
    /// 响应头 + 响应体字节数
    pub response_size: u64,
    #[serde(skip_serializing_if = "is_none")]
    pub duration: Option<u64>,
    pub request_mime: String,
    pub response_mime: String,
    /// 是否为 HTTPS CONNECT 隧道（未解密）
    pub tunnel: bool,
    /// 是否为通过本地 CA 解密后的 HTTPS 请求
    pub decrypted: bool,
    /// 是否为 WebSocket / Upgrade 连接（仅透传，不解析帧）
    #[serde(default, skip_serializing_if = "is_false")]
    pub upgrade: bool,
    #[serde(skip_serializing_if = "is_none")]
    pub note: Option<String>,
    /// 若为重放请求，指向原请求 id
    #[serde(skip_serializing_if = "is_none")]
    pub repeat_of: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BodyView {
    pub available: bool,
    /// 网络传输的字节数
    pub size: u64,
    /// 实际捕获到的字节数
    pub captured_size: u64,
    /// 解码后字节数
    pub decoded_size: u64,
    /// 捕获是否被大小限制截断
    pub truncated: bool,
    /// 文本展示是否因过长被截断
    pub text_truncated: bool,
    /// empty | text | binary
    pub kind: String,
    pub mime: String,
    #[serde(skip_serializing_if = "is_none")]
    pub encoding: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub base64: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub decode_error: Option<String>,
    /// 大 Body 被延迟加载，前端需通过 `get_entry_body` 单独拉取
    #[serde(default, skip_serializing_if = "is_false")]
    pub deferred: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TimingInfo {
    pub started_at: u64,
    #[serde(skip_serializing_if = "is_none")]
    pub first_byte_at: Option<u64>,
    #[serde(skip_serializing_if = "is_none")]
    pub completed_at: Option<u64>,
    /// 收到首字节前的等待耗时
    #[serde(skip_serializing_if = "is_none")]
    pub wait_ms: Option<u64>,
    /// 接收响应体耗时
    #[serde(skip_serializing_if = "is_none")]
    pub receive_ms: Option<u64>,
    #[serde(skip_serializing_if = "is_none")]
    pub total_ms: Option<u64>,
}

/// 详情面板数据（按需请求，避免列表事件数据过大）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDetail {
    #[serde(flatten)]
    pub meta: EntryMeta,
    pub request_headers: Vec<HeaderPair>,
    pub response_headers: Vec<HeaderPair>,
    pub request_header_size: u64,
    pub response_header_size: u64,
    pub request_body: BodyView,
    pub response_body: BodyView,
    pub timings: TimingInfo,
    pub http_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub running: bool,
    /// 供用户配置的代理地址（本机或局域网 IP）
    pub host: String,
    pub port: u16,
    pub recording: bool,
    #[serde(skip_serializing_if = "is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub started_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub proxy_port: u16,
    pub auto_start_proxy: bool,
    /// 允许局域网设备连接（绑定 0.0.0.0）
    pub allow_lan: bool,
    /// 启动代理时自动接管 Windows 系统代理，退出时恢复
    pub auto_system_proxy: bool,
    /// 是否已就「自动接管系统代理」征求过用户同意
    pub system_proxy_prompted: bool,
    /// 是否启用本地 CA，对 HTTPS 做 TLS 中间人解密
    pub decrypt_https: bool,
    /// 是否忽略代理到上游服务器的 TLS 证书错误（仅调试自签名/内部站点）
    pub allow_insecure_upstream: bool,
    /// 是否通过上游 HTTP/HTTPS 代理转发
    pub upstream_proxy_enabled: bool,
    /// 上游代理地址，例如 http://user:pass@proxy.company.com:8080
    pub upstream_proxy_url: String,
    /// 单个请求/响应体最大捕获字节数
    pub max_body_size: u64,
    /// 会话最大保留条目数
    pub max_entries: usize,
    /// 是否记录 HTTPS CONNECT 隧道（只显示提示，不抓内容）
    pub record_tunnels: bool,
    /// 界面主题：light / dark
    pub theme: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            proxy_port: 8888,
            auto_start_proxy: true,
            allow_lan: false,
            auto_system_proxy: true,
            // 首次接管系统代理前需要用户确认
            system_proxy_prompted: false,
            // HTTPS 解密需要用户主动安装根证书，默认关闭
            decrypt_https: false,
            allow_insecure_upstream: false,
            upstream_proxy_enabled: false,
            upstream_proxy_url: String::new(),
            max_body_size: 5 * 1024 * 1024,
            max_entries: 3000,
            record_tunnels: true,
            theme: "light".to_string(),
        }
    }
}

/// 前端提交的部分更新字段
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettingsPatch {
    pub proxy_port: Option<u16>,
    pub auto_start_proxy: Option<bool>,
    pub allow_lan: Option<bool>,
    pub auto_system_proxy: Option<bool>,
    pub system_proxy_prompted: Option<bool>,
    pub decrypt_https: Option<bool>,
    pub allow_insecure_upstream: Option<bool>,
    pub upstream_proxy_enabled: Option<bool>,
    pub upstream_proxy_url: Option<String>,
    pub max_body_size: Option<u64>,
    pub max_entries: Option<usize>,
    pub record_tunnels: Option<bool>,
    pub theme: Option<String>,
}

/// 本地 HTTPS 解密根证书状态
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HttpsCaInfo {
    /// 当前平台是否支持生成解密证书
    pub supported: bool,
    /// 是否支持一键安装到系统信任存储
    pub installable: bool,
    pub generated: bool,
    #[serde(skip_serializing_if = "is_none")]
    pub cert_path: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub cert_pem: Option<String>,
    pub platform: String,
    #[serde(skip_serializing_if = "is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub error: Option<String>,
}

/// Windows 系统代理自动配置状态
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemProxyState {
    pub supported: bool,
    pub active: bool,
    /// 当前接管的地址，例如 127.0.0.1:8888
    pub server: String,
    #[serde(skip_serializing_if = "is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposePayload {
    pub method: String,
    pub url: String,
    /// 多行 "Name: value" 文本
    #[serde(default)]
    pub headers: String,
    #[serde(default)]
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub path: String,
    pub count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBodyResult {
    pub path: String,
    pub size: usize,
}

pub const APP_NAME: &str = "FreeCharles";
/// 构建时由 Cargo 注入，避免版本号在多处硬编码
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const MIN_BODY_LIMIT: u64 = 64 * 1024;
pub const MAX_BODY_LIMIT: u64 = 50 * 1024 * 1024;
pub const MIN_ENTRIES: usize = 100;
pub const MAX_ENTRIES: usize = 20000;

/// 会话文件结构
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFileData {
    pub app: String,
    pub version: u32,
    pub saved_at: u64,
    pub entries: Vec<SessionEntryData>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntryData {
    pub meta: EntryMeta,
    #[serde(default)]
    pub request_headers: Vec<HeaderPair>,
    #[serde(default)]
    pub response_headers: Vec<HeaderPair>,
    #[serde(default)]
    pub request_body_base64: Option<String>,
    #[serde(default)]
    pub response_body_base64: Option<String>,
    #[serde(default)]
    pub request_body_size: u64,
    #[serde(default)]
    pub response_body_size: u64,
    #[serde(default)]
    pub request_truncated: bool,
    #[serde(default)]
    pub response_truncated: bool,
    #[serde(default)]
    pub request_encoding: Option<String>,
    #[serde(default)]
    pub response_encoding: Option<String>,
    #[serde(default)]
    pub timings: TimingInfo,
    #[serde(default = "default_http_version")]
    pub http_version: String,
}

fn default_http_version() -> String {
    "HTTP/1.1".to_string()
}

/// 事件通道名，需与前端 `lib/api.ts` 中的订阅名保持一致
pub const EVENT_ENTRY: &str = "event:entry";
pub const EVENT_ENTRY_UPDATE: &str = "event:entry-update";
pub const EVENT_ENTRY_REMOVE: &str = "event:entry-remove";
pub const EVENT_CLEAR: &str = "event:clear";
pub const EVENT_LOADED: &str = "event:loaded";
pub const EVENT_STATUS: &str = "event:status";
pub const EVENT_SETTINGS: &str = "event:settings";
pub const EVENT_SYSTEM_PROXY: &str = "event:system-proxy";
pub const EVENT_COMMAND: &str = "event:command";
pub const EVENT_ERROR: &str = "event:error";

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}
