//! 上游 HTTP/HTTPS 代理支持。
//!
//! - 解析 `http://user:pass@host:port` 形式的上游代理地址
//! - 为 hyper 客户端提供自定义连接器：HTTP 目标走 absolute-form，HTTPS 目标先 CONNECT
//! - 供未解密 CONNECT 隧道复用同一套连接逻辑

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use base64::Engine;
use http::Uri;
use hyper_util::client::legacy::connect::{Connected, Connection};
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tower_service::Service;
use url::Url;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONNECT_HEADER: usize = 64 * 1024;

/// 上游代理配置；`authorization` 已包含 `Basic ` 前缀
#[derive(Clone, Debug, PartialEq)]
pub struct UpstreamProxyConfig {
    /// 规范化后的地址，仅供日志/诊断展示
    pub url: String,
    /// http 或 https
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub authorization: Option<String>,
}

/// 解析并校验上游代理地址；缺省协议按 http 处理
pub fn parse_upstream_proxy_url(raw: &str) -> Result<UpstreamProxyConfig, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err("请填写上游代理地址".to_string());
    }
    let with_protocol = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let url = Url::parse(&with_protocol).map_err(|_| "上游代理地址格式不正确".to_string())?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("上游代理仅支持 http / https 地址".to_string());
    }
    let host = url
        .host_str()
        .map(|value| {
            value
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string()
        })
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "上游代理地址缺少主机名".to_string())?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "上游代理端口不正确".to_string())?;
    let authorization = if url.username().is_empty() && url.password().is_none() {
        None
    } else {
        let username = percent_decode(url.username());
        let password = percent_decode(url.password().unwrap_or_default());
        let credentials = format!("{username}:{password}");
        Some(format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(credentials)
        ))
    };

    Ok(UpstreamProxyConfig {
        url: url.to_string(),
        scheme: url.scheme().to_string(),
        host,
        port,
        authorization,
    })
}

fn percent_decode(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .to_string()
}

/// hyper 客户端使用的上游连接器；未配置上游时退化为直连。
#[derive(Clone)]
pub struct UpstreamConnector {
    upstream: Option<UpstreamProxyConfig>,
    /// 与上游代理做 TLS 时使用，ALPN 固定为 http/1.1
    proxy_tls: Arc<ClientConfig>,
}

impl UpstreamConnector {
    pub fn new(upstream: Option<UpstreamProxyConfig>, proxy_tls: Arc<ClientConfig>) -> Self {
        Self {
            upstream,
            proxy_tls,
        }
    }
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

impl Service<Uri> for UpstreamConnector {
    type Response = TokioIo<UpstreamIo>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        let upstream = self.upstream.clone();
        let proxy_tls = self.proxy_tls.clone();
        Box::pin(async move {
            let host = dst
                .host()
                .map(|value| value.trim_start_matches('[').trim_end_matches(']'))
                .filter(|value| !value.is_empty())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "请求缺少主机名"))?
                .to_string();
            let is_https = dst.scheme_str() == Some("https");
            let port = dst.port_u16().unwrap_or(if is_https { 443 } else { 80 });

            let Some(config) = upstream else {
                let tcp = connect_tcp(&host, port).await?;
                return Ok(TokioIo::new(UpstreamIo::new(
                    UpstreamStream::Plain(tcp),
                    Vec::new(),
                    false,
                )));
            };

            let mut stream = open_proxy_stream(&config, &proxy_tls).await?;
            if is_https {
                // 先与上游代理建立 CONNECT 隧道，之后由 hyper-rustls 对目标做 TLS
                let prefix = establish_tunnel(
                    &mut stream,
                    &join_authority(&host, port),
                    config.authorization.as_deref(),
                )
                .await?;
                Ok(TokioIo::new(UpstreamIo::new(stream, prefix, false)))
            } else {
                // HTTP 目标：hyper 客户端会依据 connected().proxy(true) 使用 absolute-form
                Ok(TokioIo::new(UpstreamIo::new(stream, Vec::new(), true)))
            }
        })
    }
}

/// 为未解密的 CONNECT 隧道建立到目标的连接；配置上游时先经过上游代理。
pub async fn open_tunnel(
    upstream: Option<&UpstreamProxyConfig>,
    proxy_tls: &Arc<ClientConfig>,
    host: &str,
    port: u16,
) -> io::Result<UpstreamIo> {
    let Some(config) = upstream else {
        let tcp = connect_tcp(host, port).await?;
        return Ok(UpstreamIo::new(
            UpstreamStream::Plain(tcp),
            Vec::new(),
            false,
        ));
    };
    let mut stream = open_proxy_stream(config, proxy_tls).await?;
    let prefix = establish_tunnel(
        &mut stream,
        &join_authority(host, port),
        config.authorization.as_deref(),
    )
    .await?;
    Ok(UpstreamIo::new(stream, prefix, false))
}

async fn open_proxy_stream(
    config: &UpstreamProxyConfig,
    proxy_tls: &Arc<ClientConfig>,
) -> io::Result<UpstreamStream> {
    let tcp = connect_tcp(&config.host, config.port).await?;
    if config.scheme == "https" {
        let server_name = if let Ok(ip) = config.host.parse::<std::net::IpAddr>() {
            ServerName::IpAddress(ip.into())
        } else {
            ServerName::try_from(config.host.clone())
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?
        };
        let connector = TlsConnector::from(proxy_tls.clone());
        let stream = connector
            .connect(server_name, tcp)
            .await
            .map_err(io::Error::other)?;
        Ok(UpstreamStream::Tls(Box::new(stream)))
    } else {
        Ok(UpstreamStream::Plain(tcp))
    }
}

async fn connect_tcp(host: &str, port: u16) -> io::Result<TcpStream> {
    let connect = TcpStream::connect((host, port));
    match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Ok(Ok(stream)) => {
            let _ = stream.set_nodelay(true);
            Ok(stream)
        }
        Ok(Err(error)) => Err(error),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("连接上游代理超时（{host}:{port}）"),
        )),
    }
}

/// 向代理发送 CONNECT，读取 200 响应头；返回响应头之后可能属于目标的字节
async fn establish_tunnel(
    stream: &mut UpstreamStream,
    authority: &str,
    authorization: Option<&str>,
) -> io::Result<Vec<u8>> {
    let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some(value) = authorization {
        request.push_str(&format!("Proxy-Authorization: {value}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;

    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "上游代理在 CONNECT 握手期间关闭了连接",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_subslice(&buffer, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..index]);
            let status = head
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(0);
            if status != 200 {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    format!("上游代理拒绝了 CONNECT 请求（HTTP {status}）"),
                ));
            }
            return Ok(buffer[index + 4..].to_vec());
        }
        if buffer.len() > MAX_CONNECT_HEADER {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "上游代理响应头过长",
            ));
        }
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn join_authority(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

enum UpstreamStream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl AsyncRead for UpstreamStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for UpstreamStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_flush(cx),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
        }
    }
}

/// 支持「可能已经预读了一部分隧道数据」的连接包装
pub struct UpstreamIo {
    inner: UpstreamStream,
    prefix: Vec<u8>,
    prefix_pos: usize,
    /// 是否为 HTTP 代理直连（决定 hyper 是否使用 absolute-form）
    proxied: bool,
}

impl UpstreamIo {
    fn new(inner: UpstreamStream, prefix: Vec<u8>, proxied: bool) -> Self {
        Self {
            inner,
            prefix,
            prefix_pos: 0,
            proxied,
        }
    }
}

impl Connection for UpstreamIo {
    fn connected(&self) -> Connected {
        Connected::new().proxy(self.proxied)
    }
}

impl AsyncRead for UpstreamIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        if this.prefix_pos < this.prefix.len() {
            let remaining = &this.prefix[this.prefix_pos..];
            let count = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..count]);
            this.prefix_pos += count;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for UpstreamIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.as_mut().get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.as_mut().get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.as_mut().get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_proxy_with_default_port() {
        let config = parse_upstream_proxy_url("proxy.company.com").unwrap();
        assert_eq!(config.scheme, "http");
        assert_eq!(config.host, "proxy.company.com");
        assert_eq!(config.port, 80);
        assert!(config.authorization.is_none());
    }

    #[test]
    fn parses_https_proxy_with_credentials() {
        let config = parse_upstream_proxy_url("https://user:pass@proxy.company.com:8443").unwrap();
        assert_eq!(config.scheme, "https");
        assert_eq!(config.port, 8443);
        assert_eq!(config.authorization.as_deref(), Some("Basic dXNlcjpwYXNz"));
    }

    #[test]
    fn decodes_credentials() {
        let config = parse_upstream_proxy_url("http://u%40ser:p%3Ass@127.0.0.1:8080").unwrap();
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("u@ser:p:ss")
        );
        assert_eq!(config.authorization.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn rejects_invalid_proxy_urls() {
        assert!(parse_upstream_proxy_url("").is_err());
        assert!(parse_upstream_proxy_url("socks5://127.0.0.1:1080").is_err());
        assert!(parse_upstream_proxy_url("http://").is_err());
    }

    #[tokio::test]
    async fn opens_connect_tunnel_via_upstream() {
        // 目标 echo 服务
        let echo = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_addr = echo.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = echo.accept().await.unwrap();
            let mut buffer = [0u8; 1024];
            loop {
                let read = socket.read(&mut buffer).await.unwrap();
                if read == 0 {
                    break;
                }
                socket.write_all(&buffer[..read]).await.unwrap();
            }
        });

        // 最小上游代理：读取 CONNECT 头后返回 200，并与 echo 服务双向复制
        let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy.local_addr().unwrap();
        let (head_tx, head_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut client, _) = proxy.accept().await.unwrap();
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                let read = client.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
                if find_subslice(&buffer, b"\r\n\r\n").is_some() {
                    break;
                }
            }
            let _ = head_tx.send(String::from_utf8_lossy(&buffer).to_string());
            client
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
            let mut target = TcpStream::connect(echo_addr).await.unwrap();
            let _ = tokio::io::copy_bidirectional(&mut client, &mut target).await;
        });

        let config = UpstreamProxyConfig {
            url: format!("http://127.0.0.1:{}", proxy_addr.port()),
            scheme: "http".to_string(),
            host: "127.0.0.1".to_string(),
            port: proxy_addr.port(),
            authorization: Some("Basic dXNlcjpwYXNz".to_string()),
        };
        let tls = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth(),
        );
        let mut tunnel = open_tunnel(Some(&config), &tls, "127.0.0.1", echo_addr.port())
            .await
            .unwrap();
        tunnel.write_all(b"ping").await.unwrap();
        tunnel.flush().await.unwrap();
        let mut response = [0u8; 4];
        tunnel.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"ping");

        let head = head_rx.await.unwrap();
        assert!(head.starts_with(&format!("CONNECT 127.0.0.1:{} HTTP/1.1", echo_addr.port())));
        assert!(head.contains("Proxy-Authorization: Basic dXNlcjpwYXNz"));
    }
}
