//! HTTP/HTTPS 抓包代理。
//!
//! - 支持标准 HTTP 代理请求（absolute-form URL）
//! - HTTPS 默认通过 CONNECT 隧道透传；开启解密后用本地 CA 做 TLS 中间人
//! - 请求/响应体按配置上限截断捕获，供详情面板查看

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::header::{
    HeaderMap, HeaderName, HeaderValue, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HOST,
    UPGRADE,
};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::{Body, Frame, Incoming};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, watch};
use tokio_rustls::TlsAcceptor;
use url::Url;

use crate::cert::HttpsCa;
use crate::models::{
    now_ms, ComposePayload, EntryMeta, HeaderPair, ProxyStatus, APP_NAME, EVENT_STATUS,
};
use crate::store::{SessionStore, StoreEvent, TransactionInit};

pub type RespBody = UnsyncBoxBody<Bytes, hyper::Error>;
pub type ReqBody = UnsyncBoxBody<Bytes, hyper::Error>;

/// 这些头只对单跳连接有效，转发时需要剥离
const HOP_BY_HOP: [&str; 8] = [
    "proxy-connection",
    "proxy-authorization",
    "connection",
    "keep-alive",
    "upgrade",
    "te",
    "trailer",
    "transfer-encoding",
];

/// 回写给客户端的响应需要剥离的连接相关头
const RESPONSE_DROP_HEADERS: [&str; 6] = [
    "connection",
    "keep-alive",
    "proxy-connection",
    "transfer-encoding",
    "upgrade",
    "trailer",
];

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const SELF_NOTE: &str =
    "HTTPS 流量以 CONNECT 隧道透传，未解密。可在设置中开启 HTTPS 解密后查看内容。";

pub type ProxyClient = Client<HttpsConnector<HttpConnector>, ReqBody>;

#[derive(Clone)]
pub struct ProxyOptions {
    pub port: u16,
    pub allow_lan: bool,
    pub max_body_size: u64,
    pub record_tunnels: bool,
    pub recording: bool,
    /// 是否对 HTTPS CONNECT 连接做 TLS 中间人解密
    pub decrypt_https: bool,
    /// 是否忽略代理到上游服务器的 TLS 证书错误
    pub allow_insecure_upstream: bool,
}

#[derive(Default)]
pub struct ProxyOptionsPatch {
    pub port: Option<u16>,
    pub allow_lan: Option<bool>,
    pub max_body_size: Option<u64>,
    pub record_tunnels: Option<bool>,
    pub decrypt_https: Option<bool>,
    pub allow_insecure_upstream: Option<bool>,
}

pub struct ServerHandle {
    shutdown: Option<oneshot::Sender<()>>,
    live: watch::Sender<bool>,
    task: tauri::async_runtime::JoinHandle<()>,
}

pub struct ProxyInner {
    pub store: Arc<Mutex<SessionStore>>,
    pub app: tauri::AppHandle,
    pub options: Mutex<ProxyOptions>,
    pub status: Mutex<ProxyStatus>,
    pub server: Mutex<Option<ServerHandle>>,
    pub client: Mutex<ProxyClient>,
    pub ca: Arc<HttpsCa>,
}

pub struct CaptureProxy {
    inner: Arc<ProxyInner>,
}

impl Clone for CaptureProxy {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

fn lock_store(store: &Arc<Mutex<SessionStore>>) -> MutexGuard<'_, SessionStore> {
    store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn never_to_hyper(value: Infallible) -> hyper::Error {
    match value {}
}

fn empty_body() -> RespBody {
    Empty::<Bytes>::new().map_err(never_to_hyper).boxed_unsync()
}

fn full_body(bytes: Bytes) -> RespBody {
    Full::new(bytes).map_err(never_to_hyper).boxed_unsync()
}

/// body 结束的原因。
///
/// hyper 在写满 Content-Length 后会直接丢弃 body 而不再 poll 到 `None`，
/// 因此需要用 `Dropped` 区分「正常写完被丢弃」和「传输中断」。
pub enum BodyEnd {
    /// 正常读取到流结束
    Finished,
    /// 传输过程中出错
    Error(String),
    /// body 被丢弃（可能是写完，也可能是客户端提前断开）
    Dropped,
}

type DataCallback = Box<dyn FnMut(&Bytes) + Send>;
type EndCallback = Box<dyn FnMut(BodyEnd) + Send>;

/// 透传 body 的同时把内容分片交给抓包回调
pub struct CaptureBody<B> {
    inner: B,
    on_data: Option<DataCallback>,
    on_end: Option<EndCallback>,
}

impl<B> CaptureBody<B> {
    pub fn new(inner: B, on_data: DataCallback, on_end: EndCallback) -> Self {
        Self {
            inner,
            on_data: Some(on_data),
            on_end: Some(on_end),
        }
    }
}

impl<B> Drop for CaptureBody<B> {
    fn drop(&mut self) {
        if let Some(mut callback) = self.on_end.take() {
            callback(BodyEnd::Dropped);
        }
    }
}

impl<B> Body for CaptureBody<B>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: std::fmt::Display,
{
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Bytes>, B::Error>>> {
        let this = self.as_mut().get_mut();
        let polled = std::pin::Pin::new(&mut this.inner).poll_frame(cx);
        match &polled {
            std::task::Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    if let Some(callback) = this.on_data.as_mut() {
                        callback(data);
                    }
                }
            }
            std::task::Poll::Ready(Some(Err(error))) => {
                let message = error.to_string();
                if let Some(mut callback) = this.on_end.take() {
                    callback(BodyEnd::Error(message));
                }
            }
            std::task::Poll::Ready(None) => {
                if let Some(mut callback) = this.on_end.take() {
                    callback(BodyEnd::Finished);
                }
            }
            std::task::Poll::Pending => {}
        }
        polled
    }
}

impl ProxyInner {
    pub fn emit_status(&self) {
        let status = self.status_snapshot();
        let _ = tauri::Emitter::emit(&self.app, EVENT_STATUS, status);
    }

    pub fn status_snapshot(&self) -> ProxyStatus {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn options_snapshot(&self) -> ProxyOptions {
        self.options
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn recording(&self) -> bool {
        self.options_snapshot().recording
    }

    fn max_body_size(&self) -> u64 {
        self.options_snapshot().max_body_size
    }

    fn record_tunnels(&self) -> bool {
        self.options_snapshot().record_tunnels
    }

    fn decrypt_https(&self) -> bool {
        self.options_snapshot().decrypt_https
    }

    /// 复用同一个连接池的客户端（支持 http / https）
    fn client(&self) -> ProxyClient {
        self.client
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 判断 origin-form 请求是否是在直接访问代理自身，避免代理把请求转发给自己造成死循环
    fn is_self_request(&self, req: &Request<Incoming>) -> bool {
        if req.uri().scheme().is_some() {
            return false;
        }
        let Some(host) = req.headers().get(HOST).and_then(|value| value.to_str().ok()) else {
            return false;
        };
        let (target_host, target_port) = split_host_port(host);
        if target_port != self.status_snapshot().port {
            return false;
        }
        let mut local_names = vec![
            "127.0.0.1".to_string(),
            "localhost".to_string(),
            "::1".to_string(),
            "0.0.0.0".to_string(),
        ];
        local_names.push(lan_address().to_lowercase());
        local_names.iter().any(|name| name == &target_host.to_lowercase())
    }
}

impl CaptureProxy {
    pub fn new(
        store: Arc<Mutex<SessionStore>>,
        app: tauri::AppHandle,
        options: ProxyOptions,
        ca: Arc<HttpsCa>,
    ) -> Self {
        let status = ProxyStatus {
            running: false,
            host: "127.0.0.1".to_string(),
            port: options.port,
            recording: options.recording,
            error: None,
            started_at: None,
        };
        let client = build_client(options.allow_insecure_upstream);
        Self {
            inner: Arc::new(ProxyInner {
                store,
                app,
                options: Mutex::new(options),
                status: Mutex::new(status),
                server: Mutex::new(None),
                client: Mutex::new(client),
                ca,
            }),
        }
    }

    pub fn status(&self) -> ProxyStatus {
        self.inner.status_snapshot()
    }

    pub fn update_options(&self, patch: ProxyOptionsPatch) {
        let mut rebuild_client = false;
        {
            let mut options = self
                .inner
                .options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(value) = patch.port {
                options.port = value;
            }
            if let Some(value) = patch.allow_lan {
                options.allow_lan = value;
            }
            if let Some(value) = patch.max_body_size {
                options.max_body_size = value;
            }
            if let Some(value) = patch.record_tunnels {
                options.record_tunnels = value;
            }
            if let Some(value) = patch.decrypt_https {
                options.decrypt_https = value;
            }
            if let Some(value) = patch.allow_insecure_upstream {
                // 证书校验策略变化后需要重建上游连接池
                rebuild_client = options.allow_insecure_upstream != value;
                options.allow_insecure_upstream = value;
            }
        }
        if rebuild_client {
            let insecure = self.inner.options_snapshot().allow_insecure_upstream;
            *self
                .inner
                .client
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = build_client(insecure);
        }
        let options = self.inner.options_snapshot();
        let mut status = self.inner.status_snapshot();
        if status.port != options.port || status.recording != options.recording {
            status.port = options.port;
            status.recording = options.recording;
            *self
                .inner
                .status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = status;
            self.inner.emit_status();
        }
    }

    pub fn set_recording(&self, recording: bool) -> ProxyStatus {
        {
            let mut options = self
                .inner
                .options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            options.recording = recording;
        }
        {
            let mut status = self
                .inner
                .status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            status.recording = recording;
        }
        self.inner.emit_status();
        self.status()
    }

    /// 启动代理监听；端口被占用等错误会写入状态并返回错误信息
    pub async fn start(&self) -> Result<ProxyStatus, String> {
        self.stop(true).await;
        let options = self.inner.options_snapshot();
        let bind_host = if options.allow_lan { "0.0.0.0" } else { "127.0.0.1" };

        let listener = match TcpListener::bind((bind_host, options.port)).await {
            Ok(listener) => listener,
            Err(error) => {
                let message = describe_listen_error(&error, options.port);
                let host = if options.allow_lan {
                    lan_address()
                } else {
                    "127.0.0.1".to_string()
                };
                *self
                    .inner
                    .status
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = ProxyStatus {
                    running: false,
                    host,
                    port: options.port,
                    recording: options.recording,
                    error: Some(message.clone()),
                    started_at: None,
                };
                self.inner.emit_status();
                crate::sync_system_proxy(&self.inner.app);
                return Err(message);
            }
        };

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let (live_tx, live_rx) = watch::channel(true);
        let inner = self.inner.clone();
        let task = tauri::async_runtime::spawn(async move {
            accept_loop(inner, listener, shutdown_rx, live_rx).await;
        });

        *self
            .inner
            .server
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ServerHandle {
            shutdown: Some(shutdown_tx),
            live: live_tx,
            task,
        });

        let host = if options.allow_lan {
            lan_address()
        } else {
            "127.0.0.1".to_string()
        };
        *self
            .inner
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = ProxyStatus {
            running: true,
            host,
            port: options.port,
            recording: options.recording,
            error: None,
            started_at: Some(now_ms()),
        };
        self.inner.emit_status();
        crate::sync_system_proxy(&self.inner.app);
        Ok(self.status())
    }

    /// 通知监听任务退出，并交回任务句柄
    fn shutdown_server(&self) -> Option<tauri::async_runtime::JoinHandle<()>> {
        let mut guard = self
            .inner
            .server
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut handle = guard.take()?;
        if let Some(shutdown) = handle.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = handle.live.send(false);
        Some(handle.task)
    }

    /// 停止代理并等待监听端口真正释放（重新绑定同一端口时必需）
    pub async fn stop(&self, silent: bool) {
        if let Some(task) = self.shutdown_server() {
            let _ = task.await;
        }
        self.finish_stop(silent);
    }

    /// 退出应用时使用：不等待任务结束，尽快释放资源
    pub fn stop_now(&self, silent: bool) {
        if let Some(task) = self.shutdown_server() {
            task.abort();
        }
        self.finish_stop(silent);
    }

    fn finish_stop(&self, silent: bool) {
        lock_store(&self.inner.store).fail_pending("代理已停止，请求未完成");

        {
            let mut status = self
                .inner
                .status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            status.running = false;
            status.started_at = None;
        }
        if !silent {
            self.inner.emit_status();
            crate::sync_system_proxy(&self.inner.app);
        }
    }

    /// 重放指定请求（仅 HTTP）
    pub fn resend(&self, id: &str) -> Result<EntryMeta, String> {
        let (source_meta, request_headers, body, body_truncated) = {
            let store = lock_store(&self.inner.store);
            let tx = store.tx(id).ok_or_else(|| "原请求已不存在".to_string())?;
            (
                tx.meta.clone(),
                tx.request_headers.clone(),
                tx.captured_request(),
                tx.request_truncated,
            )
        };
        if source_meta.state == "pending" {
            return Err("请求尚未完成，暂时无法重放".to_string());
        }
        if source_meta.tunnel || source_meta.method == "CONNECT" {
            return Err("HTTPS 隧道请求无法重放，请先开启 HTTPS 解密并选择解密后的请求".to_string());
        }
        let target = Url::parse(&source_meta.url).map_err(|_| "原请求 URL 无法解析".to_string())?;
        if target.scheme() != "http" && target.scheme() != "https" {
            return Err("当前请求协议不支持重放".to_string());
        }
        let is_https = target.scheme() == "https";
        let authority = authority_of(&target);
        let headers = build_outgoing_headers_from_pairs(&request_headers, &authority);

        let tx_id = self.create_transaction(TransactionInit {
            method: source_meta.method.clone(),
            url: source_meta.url.clone(),
            host: source_meta.host.clone(),
            port: source_meta.port,
            path: source_meta.path.clone(),
            protocol: if is_https { "https" } else { "http" }.to_string(),
            client_ip: "127.0.0.1".to_string(),
            tunnel: false,
            decrypted: is_https,
            note: Some(if body_truncated {
                "重放请求（原请求体已被截断）".to_string()
            } else {
                "重放请求".to_string()
            }),
            repeat_of: Some(source_meta.id.clone()),
            http_version: Some("HTTP/1.1".to_string()),
        });
        {
            let limit = self.inner.max_body_size() as usize;
            let mut store = lock_store(&self.inner.store);
            if let Some(tx) = store.tx_mut(&tx_id) {
                tx.request_headers = request_headers;
                tx.request_header_size = headers_size(&tx.request_headers)
                    + request_line_size(&source_meta.method, &source_meta.path);
                tx.request_encoding = find_pair(&tx.request_headers, "content-encoding");
                tx.meta.request_mime = find_pair(&tx.request_headers, "content-type").unwrap_or_default();
                if let Some(body) = &body {
                    tx.push_request_chunk(body, limit);
                }
                tx.meta.request_size = tx.request_header_size + tx.request_body_size;
            }
            store.touch(&tx_id);
        }

        let meta = self
            .meta_of(&tx_id)
            .ok_or_else(|| "重放请求创建失败".to_string())?;
        self.spawn_simple_request(tx_id, source_meta.method, target, headers, body);
        Ok(meta)
    }

    /// 手动构造并发送请求
    pub fn compose(&self, payload: ComposePayload) -> Result<EntryMeta, String> {
        let raw_url = payload.url.trim();
        if raw_url.is_empty() {
            return Err("请填写请求 URL".to_string());
        }
        let with_protocol = if raw_url.contains("://") {
            raw_url.to_string()
        } else {
            format!("http://{raw_url}")
        };
        let target = Url::parse(&with_protocol).map_err(|_| "URL 格式不正确".to_string())?;
        if target.scheme() != "http" {
            return Err("FreeCharles 仅支持发送 HTTP 请求（不支持 HTTPS）".to_string());
        }
        let method = if payload.method.trim().is_empty() {
            "GET".to_string()
        } else {
            payload.method.trim().to_uppercase()
        };
        let pairs = parse_header_text(&payload.headers);
        let authority = authority_of(&target);
        let mut headers = build_outgoing_headers_from_pairs(&pairs, &authority);
        let body = if payload.body.is_empty() {
            None
        } else {
            Some(Bytes::from(payload.body.clone()))
        };
        if let Some(body) = &body {
            if !headers.contains_key(CONTENT_LENGTH) {
                if let Ok(value) = HeaderValue::from_str(&body.len().to_string()) {
                    headers.insert(CONTENT_LENGTH, value);
                }
            }
        }
        if !headers.contains_key("user-agent") {
            headers.insert("user-agent", HeaderValue::from_static("FreeCharles/1.0"));
        }
        let path = format!(
            "{}{}",
            target.path(),
            target
                .query()
                .map(|query| format!("?{query}"))
                .unwrap_or_default()
        );
        let header_pairs = pairs_from_header_map(&headers, &authority);
        let header_size = headers_size(&header_pairs) + request_line_size(&method, &path);

        let tx_id = self.create_transaction(TransactionInit {
            method: method.clone(),
            url: target.to_string(),
            host: target.host_str().unwrap_or_default().to_string(),
            port: target.port_or_known_default().unwrap_or(80),
            path,
            protocol: "http".to_string(),
            client_ip: "127.0.0.1".to_string(),
            tunnel: false,
            decrypted: false,
            note: Some("Compose 手动发送".to_string()),
            repeat_of: None,
            http_version: Some("HTTP/1.1".to_string()),
        });
        {
            let limit = self.inner.max_body_size() as usize;
            let mut store = lock_store(&self.inner.store);
            if let Some(tx) = store.tx_mut(&tx_id) {
                tx.request_headers = header_pairs;
                tx.request_header_size = header_size;
                tx.meta.request_mime = find_pair(&tx.request_headers, "content-type").unwrap_or_default();
                tx.request_encoding = find_pair(&tx.request_headers, "content-encoding");
                if let Some(body) = &body {
                    tx.push_request_chunk(body, limit);
                }
                tx.meta.request_size = header_size + tx.request_body_size;
            }
            store.touch(&tx_id);
        }

        let meta = self
            .meta_of(&tx_id)
            .ok_or_else(|| "请求创建失败".to_string())?;
        self.spawn_simple_request(tx_id, method, target, headers, body);
        Ok(meta)
    }

    fn meta_of(&self, id: &str) -> Option<EntryMeta> {
        let store = lock_store(&self.inner.store);
        store.tx(id).map(|tx| tx.meta.clone())
    }

    fn create_transaction(&self, init: TransactionInit) -> String {
        let mut store = lock_store(&self.inner.store);
        let tx = store.create(init);
        let id = tx.meta.id.clone();
        store.commit(tx);
        id
    }

    /// Compose / Repeat 的发送流程：不面向客户端，只记录响应
    fn spawn_simple_request(
        &self,
        tx_id: String,
        method: String,
        target: Url,
        headers: HeaderMap,
        body: Option<Bytes>,
    ) {
        let inner = self.inner.clone();
        tauri::async_runtime::spawn(async move {
            let perf_start = Instant::now();
            let uri_text = format!(
                "{}://{}{}{}",
                target.scheme(),
                authority_of(&target),
                if target.path().is_empty() {
                    "/"
                } else {
                    target.path()
                },
                target
                    .query()
                    .map(|query| format!("?{query}"))
                    .unwrap_or_default()
            );
            let uri = match uri_text.parse::<http::Uri>() {
                Ok(uri) => uri,
                Err(_) => {
                    lock_store(&inner.store).finalize(
                        &tx_id,
                        "error",
                        Some("请求地址无法解析".to_string()),
                    );
                    return;
                }
            };
            let request_body: ReqBody = match body {
                Some(bytes) => Full::new(bytes).map_err(never_to_hyper).boxed_unsync(),
                None => Empty::<Bytes>::new().map_err(never_to_hyper).boxed_unsync(),
            };
            let request = match Request::builder()
                .method(method.as_str())
                .uri(uri)
                .body(request_body)
            {
                Ok(mut request) => {
                    *request.headers_mut() = headers;
                    request
                }
                Err(error) => {
                    lock_store(&inner.store).finalize(
                        &tx_id,
                        "error",
                        Some(format!("请求构造失败：{error}")),
                    );
                    return;
                }
            };

            match tokio::time::timeout(REQUEST_TIMEOUT, inner.client().request(request)).await {
                Err(_) => {
                    lock_store(&inner.store).finalize(
                        &tx_id,
                        "error",
                        Some("请求超时（5 分钟）".to_string()),
                    );
                }
                Ok(Err(error)) => {
                    let message = friendly_client_error(&error);
                    lock_store(&inner.store).finalize(&tx_id, "error", Some(message));
                }
                Ok(Ok(response)) => {
                    record_response_head(&inner, &Some(tx_id.clone()), &response, perf_start);
                    let expected = if method == "HEAD" {
                        None
                    } else {
                        expected_response_length(response.headers(), response.status())
                    };
                    let mut body =
                        capture_response_body(&inner, &Some(tx_id.clone()), response.into_body(), expected);
                    while let Some(frame) = body.frame().await {
                        if frame.is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }
}

async fn accept_loop(
    inner: Arc<ProxyInner>,
    listener: TcpListener,
    mut shutdown: oneshot::Receiver<()>,
    live: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, peer) = match accepted {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                let connection_ctx = inner.clone();
                let mut live_rx = live.clone();
                tauri::async_runtime::spawn(async move {
                    let io = TokioIo::new(stream);
                    let service_ctx = connection_ctx.clone();
                    let service = service_fn(move |request: Request<Incoming>| {
                        let ctx = service_ctx.clone();
                        async move { handle(ctx, request, peer, None).await }
                    });
                    let connection = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service)
                        .with_upgrades();
                    tokio::select! {
                        _ = connection => {}
                        _ = live_rx.changed() => {}
                    }
                });
            }
        }
    }
}

/// HTTPS 解密后的连接上下文（CONNECT 目标地址）
#[derive(Clone)]
struct MitmTarget {
    host: String,
    port: u16,
}

async fn handle(
    ctx: Arc<ProxyInner>,
    request: Request<Incoming>,
    peer: SocketAddr,
    mitm: Option<MitmTarget>,
) -> Result<Response<RespBody>, Infallible> {
    match route(ctx, request, peer, mitm).await {
        Ok(response) => Ok(response),
        Err(message) => Ok(message_response(
            StatusCode::BAD_GATEWAY,
            "Bad Gateway",
            &message,
        )),
    }
}

async fn route(
    ctx: Arc<ProxyInner>,
    request: Request<Incoming>,
    peer: SocketAddr,
    mitm: Option<MitmTarget>,
) -> Result<Response<RespBody>, String> {
    if request.method() == Method::CONNECT {
        return Ok(handle_connect(ctx, request, peer).await);
    }

    let decrypted = mitm.is_some();
    // 浏览器直连代理端口时是 origin-form 请求，返回使用说明；解密后的流量不做该判断
    if !decrypted && ctx.is_self_request(&request) {
        return Ok(message_response(
            StatusCode::OK,
            "FreeCharles HTTP 代理已运行",
            "当前访问的是代理端口本身。请在浏览器或系统中配置 HTTP 代理后访问目标站点。",
        ));
    }

    let target = match &mitm {
        Some(info) => Target::from_mitm(&request, &info.host, info.port)?,
        None => Target::from_request(&request)?,
    };
    if !decrypted && target.scheme != "http" {
        return Ok(message_response(
            StatusCode::NOT_IMPLEMENTED,
            "Not Implemented",
            "FreeCharles 只抓取 HTTP 请求。HTTPS 会以 CONNECT 隧道形式透传，不进行解密。",
        ));
    }
    if request.headers().contains_key(UPGRADE) {
        return handle_upgrade(ctx, request, peer, target, decrypted).await;
    }

    let method = request.method().clone();
    let request_headers = request.headers().clone();
    let raw_target = request.uri().to_string();

    // 记录请求头（recordTunnels / recording 打开时才记录）
    let tx_id = if ctx.recording() {
        Some(record_request(
            &ctx,
            TransactionInit {
                method: method.to_string(),
                url: target.url.clone(),
                host: target.host.clone(),
                port: target.port,
                path: target.path.clone(),
                protocol: target.scheme.clone(),
                client_ip: peer.ip().to_string(),
                tunnel: false,
                decrypted,
                note: None,
                repeat_of: None,
                http_version: Some(version_text(request.version())),
            },
            &request_headers,
            &raw_target,
        ))
    } else {
        None
    };

    let authority = target.authority();
    let outgoing_headers = build_outgoing_headers(&request_headers, &authority);
    let uri = target
        .upstream_uri()
        .parse::<http::Uri>()
        .map_err(|_| "请求地址无法解析".to_string())?;
    let request_body = capture_request_body(&ctx, &tx_id, request.into_body());
    let mut outgoing = Request::builder()
        .method(method.clone())
        .uri(uri)
        .body(request_body.boxed_unsync())
        .map_err(|error| format!("请求构造失败：{error}"))?;
    *outgoing.headers_mut() = outgoing_headers;

    let perf_start = Instant::now();
    match tokio::time::timeout(REQUEST_TIMEOUT, ctx.client().request(outgoing)).await {
        Err(_) => {
            if let Some(id) = &tx_id {
                lock_store(&ctx.store).finalize(id, "error", Some("请求超时（5 分钟）".to_string()));
            }
            Ok(message_response(
                StatusCode::GATEWAY_TIMEOUT,
                "Gateway Timeout",
                "请求超时（5 分钟）",
            ))
        }
        Ok(Err(error)) => {
            let message = friendly_client_error(&error);
            if let Some(id) = &tx_id {
                lock_store(&ctx.store).finalize(id, "error", Some(message.clone()));
            }
            Ok(message_response(
                StatusCode::BAD_GATEWAY,
                "Bad Gateway",
                &message,
            ))
        }
        Ok(Ok(response)) => {
            record_response_head(&ctx, &tx_id, &response, perf_start);
            Ok(build_client_response(
                &ctx,
                &tx_id,
                response,
                true,
                method == Method::HEAD,
            ))
        }
    }
}

/// CONNECT 隧道：建立上游 TCP 连接后接管客户端连接做双向透传
async fn handle_connect(
    ctx: Arc<ProxyInner>,
    request: Request<Incoming>,
    peer: SocketAddr,
) -> Response<RespBody> {
    let authority = request
        .uri()
        .authority()
        .map(|value| value.as_str().to_string())
        .or_else(|| {
            request
                .headers()
                .get(HOST)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string())
        })
        .unwrap_or_default();
    let (host, port) = split_host_port(&authority);
    if host.is_empty() {
        return message_response(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "无法解析 CONNECT 目标地址",
        );
    }

    // 开启 HTTPS 解密时优先走 TLS 中间人；证书准备失败则退回普通隧道
    if ctx.decrypt_https() && ctx.ca.ready() {
        match ctx.ca.server_config(&host) {
            Ok(config) => {
                return start_mitm(ctx, request, peer, host, port, config);
            }
            Err(message) => {
                eprintln!("[FreeCharles] HTTPS 解密证书准备失败：{message}");
            }
        }
    }

    let record = ctx.recording() && ctx.record_tunnels();
    let request_headers = request.headers().clone();
    let tx_id = if record {
        let url_host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.clone()
        };
        let id = record_request(
            &ctx,
            TransactionInit {
                method: "CONNECT".to_string(),
                url: format!("https://{url_host}:{port}"),
                host: host.clone(),
                port,
                path: "/".to_string(),
                protocol: "https".to_string(),
                client_ip: peer.ip().to_string(),
                tunnel: true,
                decrypted: false,
                note: Some(SELF_NOTE.to_string()),
                repeat_of: None,
                http_version: Some("HTTP/1.1".to_string()),
            },
            &request_headers,
            &authority,
        );
        Some(id)
    } else {
        None
    };

    match TcpStream::connect((host.as_str(), port)).await {
        Ok(upstream) => {
            if let Some(id) = &tx_id {
                let mut store = lock_store(&ctx.store);
                if let Some(tx) = store.tx_mut(id) {
                    tx.meta.status = Some(200);
                    tx.meta.status_text = Some("Connection Established".to_string());
                    tx.response_headers = vec![("proxy-agent".to_string(), APP_NAME.to_string())];
                    tx.response_header_size = 0;
                    tx.meta.response_size = 0;
                }
                store.finalize(id, "complete", None);
            }
            tauri::async_runtime::spawn(async move {
                if let Ok(upgraded) = hyper::upgrade::on(request).await {
                    let mut client_io = TokioIo::new(upgraded);
                    let mut upstream_io = upstream;
                    let _ = tokio::io::copy_bidirectional(&mut client_io, &mut upstream_io).await;
                }
            });
            let mut response = Response::new(empty_body());
            *response.status_mut() = StatusCode::OK;
            response
                .headers_mut()
                .insert("proxy-agent", HeaderValue::from_static("FreeCharles"));
            response
        }
        Err(error) => {
            let message = friendly_io_error(&error);
            if let Some(id) = &tx_id {
                lock_store(&ctx.store).finalize(id, "error", Some(message.clone()));
            }
            message_response(
                StatusCode::BAD_GATEWAY,
                "Bad Gateway",
                &format!("HTTPS 隧道建立失败：{message}"),
            )
        }
    }
}

/// 对 CONNECT 执行 TLS 中间人：先回 200，再与客户端完成 TLS 握手，
/// 之后把解密出来的 HTTP 请求交给统一的处理流程，并标记为已解密。
fn start_mitm(
    ctx: Arc<ProxyInner>,
    request: Request<Incoming>,
    peer: SocketAddr,
    host: String,
    port: u16,
    config: Arc<rustls::ServerConfig>,
) -> Response<RespBody> {
    tauri::async_runtime::spawn(async move {
        let upgraded = match hyper::upgrade::on(request).await {
            Ok(upgraded) => upgraded,
            Err(_) => return,
        };
        let acceptor = TlsAcceptor::from(config);
        let tls = match acceptor.accept(TokioIo::new(upgraded)).await {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("[FreeCharles] HTTPS 解密握手失败：{error}");
                return;
            }
        };

        let service_ctx = ctx.clone();
        let target = MitmTarget { host, port };
        let service = service_fn(move |request: Request<Incoming>| {
            let ctx = service_ctx.clone();
            let target = target.clone();
            async move { handle(ctx, request, peer, Some(target)).await }
        });
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(tls), service)
            .with_upgrades()
            .await;
    });

    let mut response = Response::new(empty_body());
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert("proxy-agent", HeaderValue::from_static("FreeCharles"));
    response
}

/// WebSocket 等 HTTP Upgrade 请求：与上游同时升级后透传
async fn handle_upgrade(
    ctx: Arc<ProxyInner>,
    mut request: Request<Incoming>,
    peer: SocketAddr,
    target: Target,
    decrypted: bool,
) -> Result<Response<RespBody>, String> {
    let method = request.method().clone();
    let request_headers = request.headers().clone();
    let raw_target = request.uri().to_string();
    let tx_id = if ctx.recording() {
        Some(record_request(
            &ctx,
            TransactionInit {
                method: method.to_string(),
                url: target.url.clone(),
                host: target.host.clone(),
                port: target.port,
                path: target.path.clone(),
                protocol: target.scheme.clone(),
                client_ip: peer.ip().to_string(),
                tunnel: false,
                decrypted,
                note: Some("WebSocket / Upgrade 连接（仅透传，不解析内容）".to_string()),
                repeat_of: None,
                http_version: Some(version_text(request.version())),
            },
            &request_headers,
            &raw_target,
        ))
    } else {
        None
    };

    let authority = target.authority();
    let uri = target
        .upstream_uri()
        .parse::<http::Uri>()
        .map_err(|_| "请求地址无法解析".to_string())?;
    let outgoing_headers = build_upgrade_headers(&request_headers, &authority);

    // 客户端升级句柄必须在响应发出之前取到
    let client_upgrade = hyper::upgrade::on(&mut request);

    let body: ReqBody = if tx_id.is_some() {
        capture_request_body(&ctx, &tx_id, request.into_body()).boxed_unsync()
    } else {
        request.into_body().boxed_unsync()
    };
    let mut outgoing = Request::builder()
        .method(method)
        .uri(uri)
        .body(body)
        .map_err(|error| format!("请求构造失败：{error}"))?;
    *outgoing.headers_mut() = outgoing_headers;

    let perf_start = Instant::now();
    let mut upstream_response = match tokio::time::timeout(REQUEST_TIMEOUT, ctx.client().request(outgoing)).await {
        Err(_) => {
            if let Some(id) = &tx_id {
                lock_store(&ctx.store).finalize(id, "error", Some("请求超时（5 分钟）".to_string()));
            }
            return Ok(message_response(
                StatusCode::GATEWAY_TIMEOUT,
                "Gateway Timeout",
                "请求超时（5 分钟）",
            ));
        }
        Ok(Err(error)) => {
            let message = friendly_client_error(&error);
            if let Some(id) = &tx_id {
                lock_store(&ctx.store).finalize(id, "error", Some(message.clone()));
            }
            return Ok(message_response(
                StatusCode::BAD_GATEWAY,
                "Bad Gateway",
                &message,
            ));
        }
        Ok(Ok(response)) => response,
    };

    record_response_head(&ctx, &tx_id, &upstream_response, perf_start);

    if upstream_response.status() == StatusCode::SWITCHING_PROTOCOLS {
        let upstream_upgrade = hyper::upgrade::on(&mut upstream_response);
        let store = ctx.store.clone();
        let finalize_id = tx_id.clone();
        tauri::async_runtime::spawn(async move {
            if let (Ok(client_io), Ok(server_io)) = (client_upgrade.await, upstream_upgrade.await) {
                let mut client_io = TokioIo::new(client_io);
                let mut server_io = TokioIo::new(server_io);
                let _ = tokio::io::copy_bidirectional(&mut client_io, &mut server_io).await;
            }
            if let Some(id) = finalize_id {
                lock_store(&store).finalize(&id, "complete", None);
            }
        });
        let mut response = Response::new(empty_body());
        *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
        *response.headers_mut() = filter_upgrade_response_headers(upstream_response.headers());
        return Ok(response);
    }

    Ok(build_client_response(
        &ctx,
        &tx_id,
        upstream_response,
        true,
        false,
    ))
}

/// 记录请求头并加入会话列表
fn record_request(
    ctx: &Arc<ProxyInner>,
    init: TransactionInit,
    headers: &HeaderMap,
    raw_target: &str,
) -> String {
    let mut store = lock_store(&ctx.store);
    let mut tx = store.create(init);
    tx.request_headers = pairs_from_header_map(headers, "");
    tx.request_header_size =
        headers_size_raw(&tx.request_headers) + request_line_size(&tx.meta.method, raw_target);
    tx.request_encoding = find_pair(&tx.request_headers, "content-encoding");
    tx.meta.request_mime = find_pair(&tx.request_headers, "content-type").unwrap_or_default();
    tx.meta.request_size = tx.request_header_size;
    let id = tx.meta.id.clone();
    store.commit(tx);
    id
}

fn record_response_head(
    ctx: &Arc<ProxyInner>,
    tx_id: &Option<String>,
    response: &Response<Incoming>,
    perf_start: Instant,
) {
    let Some(id) = tx_id else {
        return;
    };
    let headers = response.headers();
    let mut store = lock_store(&ctx.store);
    if let Some(tx) = store.tx_mut(id) {
        tx.meta.status = Some(response.status().as_u16());
        tx.meta.status_text = Some(
            response
                .status()
                .canonical_reason()
                .unwrap_or("")
                .to_string(),
        );
        tx.meta.response_mime = find_pair_map(headers, CONTENT_TYPE).unwrap_or_default();
        tx.response_encoding = find_pair_map(headers, CONTENT_ENCODING);
        tx.response_headers = pairs_from_header_map(headers, "");
        tx.response_header_size = headers_size_raw(&tx.response_headers)
            + status_line_size(response.status());
        tx.meta.response_size = tx.response_header_size;
        tx.http_version = version_text(response.version());
        tx.timings.first_byte_at = Some(now_ms());
        tx.timings.wait_ms = Some(perf_start.elapsed().as_millis() as u64);
    }
    store.touch(id);
}

fn capture_request_body(
    ctx: &Arc<ProxyInner>,
    tx_id: &Option<String>,
    body: Incoming,
) -> CaptureBody<Incoming> {
    let Some(id) = tx_id.clone() else {
        return CaptureBody::new(body, Box::new(|_: &Bytes| {}), Box::new(|_: BodyEnd| {}));
    };
    let store = ctx.store.clone();
    let limit = ctx.max_body_size() as usize;
    let data_id = id.clone();
    let data_store = store.clone();
    let end_store = store;
    CaptureBody::new(
        body,
        Box::new(move |data: &Bytes| {
            let mut store = lock_store(&data_store);
            if let Some(tx) = store.tx_mut(&data_id) {
                tx.push_request_chunk(data, limit);
            }
        }),
        Box::new(move |end: BodyEnd| {
            let mut store = lock_store(&end_store);
            if let Some(tx) = store.tx_mut(&id) {
                tx.finish_request_body();
            }
            match end {
                BodyEnd::Error(message) => store.finalize(&id, "error", Some(message)),
                _ => store.touch(&id),
            }
        }),
    )
}

/// `expected` 为上游声明的 Content-Length；用于判断 body 被丢弃时是否已完整读取
fn capture_response_body(
    ctx: &Arc<ProxyInner>,
    tx_id: &Option<String>,
    body: Incoming,
    expected: Option<u64>,
) -> CaptureBody<Incoming> {
    let Some(id) = tx_id.clone() else {
        return CaptureBody::new(body, Box::new(|_: &Bytes| {}), Box::new(|_: BodyEnd| {}));
    };
    let store = ctx.store.clone();
    let limit = ctx.max_body_size() as usize;
    let data_id = id.clone();
    let data_store = store.clone();
    let end_store = store;
    CaptureBody::new(
        body,
        Box::new(move |data: &Bytes| {
            let mut store = lock_store(&data_store);
            if let Some(tx) = store.tx_mut(&data_id) {
                tx.push_response_chunk(data, limit);
            }
        }),
        Box::new(move |end: BodyEnd| {
            let mut store = lock_store(&end_store);
            let received = store
                .tx(&id)
                .map(|tx| tx.response_body_size)
                .unwrap_or(0);
            if let Some(tx) = store.tx_mut(&id) {
                let ended_at = now_ms();
                tx.timings.receive_ms = Some(
                    ended_at.saturating_sub(tx.timings.first_byte_at.unwrap_or(ended_at)),
                );
            }
            let failure = match end {
                BodyEnd::Finished => None,
                BodyEnd::Error(message) => Some(message),
                BodyEnd::Dropped => match expected {
                    Some(length) if received < length => Some(format!(
                        "客户端提前断开连接（已接收 {received}/{length} 字节）"
                    )),
                    Some(_) => None,
                    // 分块传输或无需 body 的响应，丢弃即视为结束
                    None => None,
                },
            };
            match failure {
                Some(message) => store.finalize(&id, "error", Some(message)),
                None => store.finalize(&id, "complete", None),
            }
        }),
    )
}

fn build_client_response(
    ctx: &Arc<ProxyInner>,
    tx_id: &Option<String>,
    upstream: Response<Incoming>,
    drop_connection_headers: bool,
    head_request: bool,
) -> Response<RespBody> {
    let status = upstream.status();
    let version = upstream.version();
    let headers = upstream.headers().clone();
    let expected = if head_request {
        None
    } else {
        expected_response_length(&headers, status)
    };
    let body = capture_response_body(ctx, tx_id, upstream.into_body(), expected).boxed_unsync();
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.version_mut() = version;
    *response.headers_mut() = if drop_connection_headers {
        filter_response_headers(&headers)
    } else {
        headers
    };
    response
}

/// 解析绝对形式 / 常规形式的请求目标
struct Target {
    scheme: String,
    host: String,
    port: u16,
    path: String,
    url: String,
}

impl Target {
    fn from_request(request: &Request<Incoming>) -> Result<Self, String> {
        let raw = request.uri().to_string();
        let url = if request.uri().scheme().is_some() {
            Url::parse(&raw).map_err(|_| "无法解析请求地址".to_string())?
        } else {
            let host = request
                .headers()
                .get(HOST)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| "缺少 Host 请求头".to_string())?;
            let path = if raw.starts_with('/') {
                raw.clone()
            } else {
                format!("/{raw}")
            };
            Url::parse(&format!("http://{host}{path}"))
                .map_err(|_| "无法解析请求地址".to_string())?
        };
        Ok(Self {
            scheme: url.scheme().to_string(),
            host: url.host_str().unwrap_or_default().to_string(),
            port: url.port_or_known_default().unwrap_or(80),
            path: format!(
                "{}{}",
                url.path(),
                url.query()
                    .map(|query| format!("?{query}"))
                    .unwrap_or_default()
            ),
            url: url.to_string(),
        })
    }

    /// 解密后的 HTTPS 请求：目标地址来自 CONNECT，请求行只有 path
    fn from_mitm(request: &Request<Incoming>, host: &str, port: u16) -> Result<Self, String> {
        let path = request
            .uri()
            .path_and_query()
            .map(|value| value.as_str().to_string())
            .unwrap_or_else(|| "/".to_string());
        let url_host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        let port_part = if port == 443 {
            String::new()
        } else {
            format!(":{port}")
        };
        let url = format!("https://{url_host}{port_part}{path}");
        Ok(Self {
            scheme: "https".to_string(),
            host: host.to_string(),
            port,
            path,
            url,
        })
    }

    fn authority(&self) -> String {
        let default_port = if self.scheme == "https" { 443 } else { 80 };
        if self.port == default_port {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    fn upstream_uri(&self) -> String {
        format!(
            "{}://{}{}",
            self.scheme,
            self.authority(),
            if self.path.is_empty() {
                "/"
            } else {
                self.path.as_str()
            }
        )
    }
}

fn authority_of(url: &Url) -> String {
    let port = url.port_or_known_default().unwrap_or(80);
    let host = url.host_str().unwrap_or_default();
    let default_port = if url.scheme() == "https" { 443 } else { 80 };
    if port == default_port {
        host.to_string()
    } else {
        format!("{host}:{port}")
    }
}

fn build_outgoing_headers(headers: &HeaderMap, authority: &str) -> HeaderMap {
    let mut outgoing = HeaderMap::new();
    for (name, value) in headers.iter() {
        let lower = name.as_str();
        if HOP_BY_HOP.contains(&lower) || lower == "host" || lower == "expect" {
            continue;
        }
        outgoing.append(name.clone(), value.clone());
    }
    if let Ok(value) = HeaderValue::from_str(authority) {
        outgoing.insert(HOST, value);
    }
    outgoing
}

fn build_upgrade_headers(headers: &HeaderMap, authority: &str) -> HeaderMap {
    let mut outgoing = HeaderMap::new();
    for (name, value) in headers.iter() {
        let lower = name.as_str();
        if lower == "host" || lower == "expect" || lower == "proxy-connection" {
            continue;
        }
        outgoing.append(name.clone(), value.clone());
    }
    if let Ok(value) = HeaderValue::from_str(authority) {
        outgoing.insert(HOST, value);
    }
    outgoing
}

fn build_outgoing_headers_from_pairs(pairs: &[HeaderPair], authority: &str) -> HeaderMap {
    let mut outgoing = HeaderMap::new();
    for (name, value) in pairs {
        let lower = name.to_lowercase();
        if HOP_BY_HOP.contains(&lower.as_str()) || lower == "host" || lower == "expect" {
            continue;
        }
        let Ok(header_name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let Ok(header_value) = HeaderValue::from_str(value) else {
            continue;
        };
        outgoing.append(header_name, header_value);
    }
    if let Ok(value) = HeaderValue::from_str(authority) {
        outgoing.insert(HOST, value);
    }
    outgoing
}

fn filter_response_headers(headers: &HeaderMap) -> HeaderMap {
    let mut filtered = HeaderMap::new();
    for (name, value) in headers.iter() {
        if RESPONSE_DROP_HEADERS.contains(&name.as_str()) {
            continue;
        }
        filtered.append(name.clone(), value.clone());
    }
    filtered
}

fn filter_upgrade_response_headers(headers: &HeaderMap) -> HeaderMap {
    let mut filtered = HeaderMap::new();
    for (name, value) in headers.iter() {
        if matches!(
            name.as_str(),
            "keep-alive" | "proxy-connection" | "transfer-encoding" | "trailer"
        ) {
            continue;
        }
        filtered.append(name.clone(), value.clone());
    }
    filtered
}

fn pairs_from_header_map(headers: &HeaderMap, _authority: &str) -> Vec<HeaderPair> {
    headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).to_string(),
            )
        })
        .collect()
}

fn find_pair_map(headers: &HeaderMap, name: HeaderName) -> Option<String> {
    headers
        .get(&name)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).to_string())
}

fn find_pair(pairs: &[HeaderPair], name: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn parse_header_text(text: &str) -> Vec<HeaderPair> {
    let mut pairs = Vec::new();
    for raw_line in text.split(['\r', '\n']) {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(index) = line.find(':') {
            if index > 0 {
                pairs.push((
                    line[..index].trim().to_string(),
                    line[index + 1..].trim().to_string(),
                ));
            }
        }
    }
    pairs
}

/// 上游声明的响应体长度；204 / 304 / 101 一定没有响应体
fn expected_response_length(headers: &HeaderMap, status: StatusCode) -> Option<u64> {
    if matches!(
        status,
        StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED | StatusCode::SWITCHING_PROTOCOLS
    ) {
        return None;
    }
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn headers_size(pairs: &[HeaderPair]) -> u64 {
    let mut size = 2u64;
    for (name, value) in pairs {
        size += name.len() as u64 + value.len() as u64 + 4;
    }
    size
}

fn headers_size_raw(pairs: &[HeaderPair]) -> u64 {
    headers_size(pairs)
}

fn request_line_size(method: &str, target: &str) -> u64 {
    method.len() as u64 + target.len() as u64 + 8 + 4
}

fn status_line_size(status: StatusCode) -> u64 {
    let reason = status.canonical_reason().unwrap_or("");
    8 + 1 + 3 + 1 + reason.len() as u64 + 2
}

fn version_text(version: http::Version) -> String {
    match version {
        http::Version::HTTP_09 => "HTTP/0.9".to_string(),
        http::Version::HTTP_10 => "HTTP/1.0".to_string(),
        http::Version::HTTP_11 => "HTTP/1.1".to_string(),
        http::Version::HTTP_2 => "HTTP/2.0".to_string(),
        http::Version::HTTP_3 => "HTTP/3.0".to_string(),
        other => format!("{other:?}"),
    }
}

fn split_host_port(value: &str) -> (String, u16) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return (String::new(), 443);
    }
    if let Some(rest) = trimmed.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let host = rest[..end].to_string();
            let tail = &rest[end + 1..];
            let port = tail
                .strip_prefix(':')
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(443);
            return (host, port);
        }
    }
    match trimmed.rfind(':') {
        Some(index) => {
            let host = trimmed[..index].to_string();
            let port = trimmed[index + 1..].parse::<u16>().unwrap_or(443);
            (host, port)
        }
        None => (trimmed.to_string(), 443),
    }
}

/// 取本机的局域网 IPv4 地址，用于「允许局域网连接」时展示代理地址
pub fn lan_address() -> String {
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(address) = socket.local_addr() {
                return address.ip().to_string();
            }
        }
    }
    "0.0.0.0".to_string()
}

fn message_response(status: StatusCode, title: &str, message: &str) -> Response<RespBody> {
    let html = message_html(title, message);
    let mut response = Response::new(full_body(Bytes::from(html)));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

fn message_html(title: &str, message: &str) -> String {
    format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><title>{title} · FreeCharles</title>
<style>body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:#f4f6f9;font-family:\"Segoe UI\",\"Microsoft YaHei\",sans-serif;color:#1f2937}}
.card{{background:#fff;border:1px solid #e5e8ee;border-radius:12px;padding:32px 36px;max-width:560px;box-shadow:0 12px 32px rgba(15,23,42,.08)}}
h1{{margin:0 0 8px;font-size:18px}}code{{background:#f1f5f9;padding:2px 6px;border-radius:4px;font-family:Consolas,monospace}}
p{{line-height:1.7;font-size:13px;color:#4b5563}}</style></head><body>
<div class=\"card\"><h1>{title}</h1><p>{message}</p>
<p>这是 <strong>FreeCharles</strong> 的 HTTP 代理端口。请把浏览器或系统的 <strong>HTTP 代理</strong> 指向当前地址后访问页面。</p></div></body></html>",
        title = escape_html(title),
        message = escape_html(message)
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn friendly_io_error(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::ConnectionRefused => "连接被拒绝：目标服务器未监听该端口".to_string(),
        std::io::ErrorKind::NotFound => "域名解析失败，无法找到目标服务器".to_string(),
        std::io::ErrorKind::TimedOut => "连接目标服务器超时".to_string(),
        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe => {
            "连接被目标服务器重置".to_string()
        }
        std::io::ErrorKind::HostUnreachable | std::io::ErrorKind::NetworkUnreachable => {
            "目标主机不可达".to_string()
        }
        _ => error.to_string(),
    }
}

/// 构建支持 http / https 的上游客户端；`allow_insecure` 时跳过证书校验
fn build_client(allow_insecure: bool) -> ProxyClient {
    let config = if allow_insecure {
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(InsecureVerifier))
            .with_no_client_auth()
    } else {
        let mut roots = RootCertStore::empty();
        // 使用系统根证书，兼容企业内部 CA
        let native = rustls_native_certs::load_native_certs();
        for cert in native.certs {
            let _ = roots.add(cert);
        }
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth()
    };

    let connector = HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http()
        .enable_http1()
        .build();
    Client::builder(TokioExecutor::new()).build(connector)
}

/// 仅在用户显式开启「忽略上游证书错误」时使用
#[derive(Debug)]
struct InsecureVerifier;

impl ServerCertVerifier for InsecureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

fn friendly_client_error(error: &hyper_util::client::legacy::Error) -> String {
    let mut source = std::error::Error::source(error);
    while let Some(current) = source {
        if let Some(io_error) = current.downcast_ref::<std::io::Error>() {
            return friendly_io_error(io_error);
        }
        source = current.source();
    }
    error.to_string()
}

fn describe_listen_error(error: &std::io::Error, port: u16) -> String {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => format!(
            "端口 {port} 已被占用（可能正在运行 Charles 或其他代理工具），请关闭占用程序或在设置中更换端口"
        ),
        std::io::ErrorKind::PermissionDenied => {
            format!("没有权限监听端口 {port}，请更换为 1024 以上的端口")
        }
        _ => format!("代理启动失败：{error}"),
    }
}

/// 供命令层复用的事件回调构造
pub fn make_event_sink(app: tauri::AppHandle) -> crate::store::EventSink {
    use tauri::Emitter;
    Arc::new(move |event: StoreEvent| match event {
        StoreEvent::Add(meta) => {
            let _ = app.emit(crate::models::EVENT_ENTRY, meta);
        }
        StoreEvent::Update(meta) => {
            let _ = app.emit(crate::models::EVENT_ENTRY_UPDATE, meta);
        }
        StoreEvent::Remove(id) => {
            let _ = app.emit(crate::models::EVENT_ENTRY_REMOVE, id);
        }
        StoreEvent::Clear => {
            let _ = app.emit(crate::models::EVENT_CLEAR, ());
        }
    })
}
