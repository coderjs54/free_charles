//! 抓包会话存储。
//!
//! 请求/响应体以 `Bytes` 形式保存在后端内存中，前端只在需要时拉取详情，
//! 避免大量请求时事件数据量过大。

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use bytes::Bytes;
use uuid::Uuid;

use crate::models::{
    now_ms, BodyView, EntryDetail, EntryMeta, HeaderPair, SessionEntryData, SessionFileData,
    TimingInfo,
};

/// 存储层向外抛出的变更事件，由外层转换成 Tauri 事件广播给前端
pub enum StoreEvent {
    Add(EntryMeta),
    Update(EntryMeta),
    Remove(String),
    Clear,
}

pub type EventSink = Arc<dyn Fn(StoreEvent) + Send + Sync>;

/// 默认会话内容内存预算：256 MB；长会话或大 Body 下自动淘汰最旧记录
const DEFAULT_MAX_SESSION_BYTES: u64 = 256 * 1024 * 1024;

/// Body 展示缓存上限：超过该大小的条目不做缓存，避免和原始 Buffer 重复占用内存
const MAX_CACHED_BODY_BYTES: u64 = 512 * 1024;

/// 详情接口中超过该大小的 Body 先返回占位，由前端按需拉取
pub const LAZY_BODY_THRESHOLD: u64 = 512 * 1024;

/// 已完成条目的 Body 展示缓存，避免详情面板重复解压
#[derive(Clone, Default)]
pub struct BodyViewCache {
    pub request: Option<BodyView>,
    pub response: Option<BodyView>,
}

pub struct TransactionInit {
    pub method: String,
    pub url: String,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub protocol: String,
    pub client_ip: String,
    pub tunnel: bool,
    pub decrypted: bool,
    pub upgrade: bool,
    pub note: Option<String>,
    pub repeat_of: Option<String>,
    pub http_version: Option<String>,
}

#[derive(Clone)]
pub struct StoredTransaction {
    pub meta: EntryMeta,
    pub request_headers: Vec<HeaderPair>,
    pub response_headers: Vec<HeaderPair>,
    pub request_header_size: u64,
    pub response_header_size: u64,
    pub request_chunks: Vec<Bytes>,
    pub response_chunks: Vec<Bytes>,
    pub request_body: Option<Bytes>,
    pub response_body: Option<Bytes>,
    pub request_body_size: u64,
    pub response_body_size: u64,
    pub request_captured: usize,
    pub response_captured: usize,
    pub request_truncated: bool,
    pub response_truncated: bool,
    pub request_encoding: Option<String>,
    pub response_encoding: Option<String>,
    pub timings: TimingInfo,
    pub http_version: String,
    /// 已完成条目的 Body 展示缓存（仅缓存小 Body）
    pub body_view_cache: Option<BodyViewCache>,
}

impl StoredTransaction {
    /// 请求体分片写入内存，超过上限的部分只计数不保存
    pub fn push_request_chunk(&mut self, chunk: &Bytes, limit: usize) {
        self.request_body_size += chunk.len() as u64;
        let room = limit.saturating_sub(self.request_captured);
        if room > 0 {
            let slice = if chunk.len() <= room {
                chunk.clone()
            } else {
                chunk.slice(0..room)
            };
            self.request_captured += slice.len();
            self.request_chunks.push(slice);
        }
        if chunk.len() > room {
            self.request_truncated = true;
        }
        self.meta.request_size = self.request_header_size + self.request_body_size;
    }

    /// 响应体分片写入内存，超过上限的部分只计数不保存
    pub fn push_response_chunk(&mut self, chunk: &Bytes, limit: usize) {
        self.response_body_size += chunk.len() as u64;
        let room = limit.saturating_sub(self.response_captured);
        if room > 0 {
            let slice = if chunk.len() <= room {
                chunk.clone()
            } else {
                chunk.slice(0..room)
            };
            self.response_captured += slice.len();
            self.response_chunks.push(slice);
        }
        if chunk.len() > room {
            self.response_truncated = true;
        }
        self.meta.response_size = self.response_header_size + self.response_body_size;
    }

    pub fn finish_request_body(&mut self) {
        self.request_body = concat_chunks(&self.request_chunks);
        // 合并后立即释放分块，避免与合并结果同时占用内存
        self.request_chunks = Vec::new();
        self.meta.request_size = self.request_header_size + self.request_body_size;
    }

    pub fn finish_response_body(&mut self) {
        self.response_body = concat_chunks(&self.response_chunks);
        self.response_chunks = Vec::new();
        self.meta.response_size = self.response_header_size + self.response_body_size;
    }

    pub fn captured_request(&self) -> Option<Bytes> {
        self.request_body
            .clone()
            .or_else(|| concat_chunks(&self.request_chunks))
    }

    pub fn captured_response(&self) -> Option<Bytes> {
        self.response_body
            .clone()
            .or_else(|| concat_chunks(&self.response_chunks))
    }
}

/// BodyView 的内存估算：字符串约为 2 字节/字符，base64 为 ASCII
fn body_view_bytes(view: Option<&BodyView>) -> u64 {
    let Some(view) = view else {
        return 0;
    };
    let text = view
        .text
        .as_ref()
        .map(|value| value.len() as u64 * 2)
        .unwrap_or(0);
    let base64 = view
        .base64
        .as_ref()
        .map(|value| value.len() as u64)
        .unwrap_or(0);
    text + base64
}

fn concat_chunks(chunks: &[Bytes]) -> Option<Bytes> {
    if chunks.is_empty() {
        return None;
    }
    let mut buffer = Vec::with_capacity(chunks.iter().map(|chunk| chunk.len()).sum());
    for chunk in chunks {
        buffer.extend_from_slice(chunk);
    }
    Some(Bytes::from(buffer))
}

/// 详情渲染所需的快照，渲染在锁外执行避免长时间持锁
pub struct DetailSnapshot {
    pub meta: EntryMeta,
    pub request_headers: Vec<HeaderPair>,
    pub response_headers: Vec<HeaderPair>,
    pub request_header_size: u64,
    pub response_header_size: u64,
    pub request_body: Option<Bytes>,
    pub request_body_size: u64,
    pub request_captured: u64,
    pub request_truncated: bool,
    pub request_encoding: Option<String>,
    pub response_body: Option<Bytes>,
    pub response_body_size: u64,
    pub response_captured: u64,
    pub response_truncated: bool,
    pub response_encoding: Option<String>,
    pub timings: TimingInfo,
    pub http_version: String,
}

impl DetailSnapshot {
    pub fn into_detail(self, request_body: BodyView, response_body: BodyView) -> EntryDetail {
        EntryDetail {
            meta: self.meta,
            request_headers: self.request_headers,
            response_headers: self.response_headers,
            request_header_size: self.request_header_size,
            response_header_size: self.response_header_size,
            request_body,
            response_body,
            timings: self.timings,
            http_version: self.http_version,
        }
    }
}

pub struct SessionStore {
    items: HashMap<String, StoredTransaction>,
    order: Vec<String>,
    seq: u64,
    max_entries: usize,
    /// 会话内容总内存预算，超出后按最旧优先淘汰
    max_session_bytes: u64,
    total_bytes: u64,
    /// 每个条目已计入内存预算的字节数，用于增量更新
    accounted_bytes: HashMap<String, u64>,
    sink: Option<EventSink>,
}

impl SessionStore {
    pub fn new(sink: Option<EventSink>, max_entries: usize) -> Self {
        Self {
            items: HashMap::new(),
            order: Vec::new(),
            seq: 0,
            max_entries,
            max_session_bytes: DEFAULT_MAX_SESSION_BYTES,
            total_bytes: 0,
            accounted_bytes: HashMap::new(),
            sink,
        }
    }

    fn emit(&self, event: StoreEvent) {
        if let Some(sink) = &self.sink {
            sink(event);
        }
    }

    /// 构造一条新记录（尚未加入会话列表）
    pub fn create(&mut self, init: TransactionInit) -> StoredTransaction {
        self.seq += 1;
        let start_time = now_ms();
        let meta = EntryMeta {
            id: Uuid::new_v4().to_string(),
            seq: self.seq,
            start_time,
            method: init.method.to_uppercase(),
            url: init.url,
            host: init.host,
            port: init.port,
            path: init.path,
            protocol: init.protocol,
            client_ip: init.client_ip,
            status: None,
            status_text: None,
            state: "pending".to_string(),
            error: None,
            request_size: 0,
            response_size: 0,
            duration: None,
            request_mime: String::new(),
            response_mime: String::new(),
            tunnel: init.tunnel,
            decrypted: init.decrypted,
            upgrade: init.upgrade,
            note: init.note,
            repeat_of: init.repeat_of,
        };
        StoredTransaction {
            timings: TimingInfo {
                started_at: start_time,
                ..Default::default()
            },
            http_version: init.http_version.unwrap_or_else(|| "HTTP/1.1".to_string()),
            meta,
            request_headers: Vec::new(),
            response_headers: Vec::new(),
            request_header_size: 0,
            response_header_size: 0,
            request_chunks: Vec::new(),
            response_chunks: Vec::new(),
            request_body: None,
            response_body: None,
            request_body_size: 0,
            response_body_size: 0,
            request_captured: 0,
            response_captured: 0,
            request_truncated: false,
            response_truncated: false,
            request_encoding: None,
            response_encoding: None,
            body_view_cache: None,
        }
    }

    /// 请求信息准备完成后加入会话并通知列表
    pub fn commit(&mut self, tx: StoredTransaction) {
        if self.items.contains_key(&tx.meta.id) {
            return;
        }
        let meta = tx.meta.clone();
        self.items.insert(tx.meta.id.clone(), tx);
        self.order.push(meta.id.clone());
        self.account(&meta.id);
        self.emit(StoreEvent::Add(meta));
        self.trim();
    }

    pub fn tx(&self, id: &str) -> Option<&StoredTransaction> {
        self.items.get(id)
    }

    pub fn tx_mut(&mut self, id: &str) -> Option<&mut StoredTransaction> {
        self.items.get_mut(id)
    }

    pub fn touch(&mut self, id: &str) {
        let Some(meta) = self.items.get(id).map(|tx| tx.meta.clone()) else {
            return;
        };
        self.reaccount(id);
        self.emit(StoreEvent::Update(meta));
        self.trim();
    }

    pub fn finalize(&mut self, id: &str, state: &str, error: Option<String>) {
        {
            let Some(tx) = self.items.get_mut(id) else {
                return;
            };
            // 隧道等长连接可能多次触发结束事件，只保留首次结果
            if tx.timings.total_ms.is_some() {
                return;
            }
            let completed_at = now_ms();
            tx.timings.completed_at = Some(completed_at);
            tx.timings.total_ms = Some(completed_at.saturating_sub(tx.timings.started_at));
            tx.finish_request_body();
            tx.finish_response_body();
            tx.meta.state = state.to_string();
            tx.meta.duration = tx.timings.total_ms;
            tx.meta.error = error.clone();
            if state == "error" && tx.meta.status.is_none() {
                if let Some(message) = error {
                    if tx.meta.note.is_none() {
                        tx.meta.note = Some(message);
                    }
                }
            }
        }
        self.touch(id);
    }

    pub fn list(&self) -> Vec<EntryMeta> {
        self.order
            .iter()
            .filter_map(|id| self.items.get(id).map(|tx| tx.meta.clone()))
            .collect()
    }

    pub fn remove(&mut self, id: &str) {
        if self.items.remove(id).is_none() {
            return;
        }
        self.order.retain(|item| item != id);
        self.forget(id);
        self.emit(StoreEvent::Remove(id.to_string()));
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.total_bytes = 0;
        self.accounted_bytes.clear();
        self.emit(StoreEvent::Clear);
    }

    /// 代理停止时把尚未完成的请求标记为失败，避免列表长期显示 pending
    pub fn fail_pending(&mut self, reason: &str) {
        let ids: Vec<String> = self
            .items
            .values()
            .filter(|tx| tx.meta.state == "pending")
            .map(|tx| tx.meta.id.clone())
            .collect();
        for id in ids {
            self.finalize(&id, "error", Some(reason.to_string()));
        }
    }

    pub fn set_max_entries(&mut self, value: usize) {
        self.max_entries = value.max(1);
        self.trim();
    }

    /// 仅测试使用：把内存预算调小以验证淘汰逻辑
    #[cfg(test)]
    pub fn set_max_session_bytes(&mut self, value: u64) {
        self.max_session_bytes = value;
        self.trim();
    }

    fn trim(&mut self) {
        self.enforce_limits(true);
    }

    /// 同时按条目数和总内存预算淘汰最旧记录；导入会话时传 false，避免逐条发事件
    fn enforce_limits(&mut self, emit_events: bool) {
        while self.order.len() > self.max_entries {
            self.evict_oldest(emit_events);
        }
        // 至少保留最新一条，避免单条超大内容刚捕获就被立即淘汰
        while self.total_bytes > self.max_session_bytes && self.order.len() > 1 {
            self.evict_oldest(emit_events);
        }
    }

    fn evict_oldest(&mut self, emit_events: bool) {
        if self.order.is_empty() {
            return;
        }
        let id = self.order.remove(0);
        self.items.remove(&id);
        self.forget(&id);
        if emit_events {
            self.emit(StoreEvent::Remove(id));
        }
    }

    /// 条目当前占用的内存估算：头字节 + 实际捕获的 Body 字节 + Body 展示缓存
    fn bytes_of(&self, tx: &StoredTransaction) -> u64 {
        tx.request_header_size
            + tx.response_header_size
            + tx.request_captured as u64
            + tx.response_captured as u64
            + tx.body_view_cache
                .as_ref()
                .map(|cache| {
                    body_view_bytes(cache.request.as_ref())
                        + body_view_bytes(cache.response.as_ref())
                })
                .unwrap_or(0)
    }

    fn account(&mut self, id: &str) {
        let Some(bytes) = self.items.get(id).map(|tx| self.bytes_of(tx)) else {
            return;
        };
        self.accounted_bytes.insert(id.to_string(), bytes);
        self.total_bytes = self.total_bytes.saturating_add(bytes);
    }

    fn reaccount(&mut self, id: &str) {
        let Some(previous) = self.accounted_bytes.get(id).copied() else {
            return;
        };
        let Some(next) = self.items.get(id).map(|tx| self.bytes_of(tx)) else {
            return;
        };
        self.accounted_bytes.insert(id.to_string(), next);
        if next >= previous {
            self.total_bytes = self.total_bytes.saturating_add(next - previous);
        } else {
            self.total_bytes = self.total_bytes.saturating_sub(previous - next);
        }
    }

    fn forget(&mut self, id: &str) {
        if let Some(bytes) = self.accounted_bytes.remove(id) {
            self.total_bytes = self.total_bytes.saturating_sub(bytes);
        }
    }

    /// 读取已缓存的 BodyView（仅小 Body 会缓存）
    pub fn cached_body_view(&self, id: &str, side: &str) -> Option<BodyView> {
        let tx = self.items.get(id)?;
        let cache = tx.body_view_cache.as_ref()?;
        if side == "request" {
            cache.request.clone()
        } else {
            cache.response.clone()
        }
    }

    /// 缓存小 Body 的展示结果，并重新计入内存预算
    pub fn cache_body_view(&mut self, id: &str, side: &str, view: BodyView) {
        let cacheable = self
            .items
            .get(id)
            .map(|tx| {
                tx.meta.state != "pending"
                    && (tx.request_captured + tx.response_captured) as u64 <= MAX_CACHED_BODY_BYTES
            })
            .unwrap_or(false);
        if !cacheable {
            return;
        }
        if let Some(tx) = self.items.get_mut(id) {
            let cache = tx
                .body_view_cache
                .get_or_insert_with(BodyViewCache::default);
            if side == "request" {
                cache.request = Some(view);
            } else {
                cache.response = Some(view);
            }
        }
        self.reaccount(id);
        self.trim();
    }

    pub fn snapshot(&self, id: &str) -> Option<DetailSnapshot> {
        let tx = self.items.get(id)?;
        Some(DetailSnapshot {
            meta: tx.meta.clone(),
            request_headers: tx.request_headers.clone(),
            response_headers: tx.response_headers.clone(),
            request_header_size: tx.request_header_size,
            response_header_size: tx.response_header_size,
            request_body: tx.captured_request(),
            request_body_size: tx.request_body_size,
            request_captured: tx.request_captured as u64,
            request_truncated: tx.request_truncated,
            request_encoding: tx.request_encoding.clone(),
            response_body: tx.captured_response(),
            response_body_size: tx.response_body_size,
            response_captured: tx.response_captured as u64,
            response_truncated: tx.response_truncated,
            response_encoding: tx.response_encoding.clone(),
            timings: tx.timings.clone(),
            http_version: tx.http_version.clone(),
        })
    }

    pub fn export_session(&self) -> SessionFileData {
        let encoder = base64::engine::general_purpose::STANDARD;
        let entries = self
            .order
            .iter()
            .filter_map(|id| self.items.get(id))
            .map(|tx| SessionEntryData {
                meta: tx.meta.clone(),
                request_headers: tx.request_headers.clone(),
                response_headers: tx.response_headers.clone(),
                request_body_base64: tx
                    .captured_request()
                    .map(|buffer| encoder.encode(buffer.as_ref())),
                response_body_base64: tx
                    .captured_response()
                    .map(|buffer| encoder.encode(buffer.as_ref())),
                request_body_size: tx.request_body_size,
                response_body_size: tx.response_body_size,
                request_truncated: tx.request_truncated,
                response_truncated: tx.response_truncated,
                request_encoding: tx.request_encoding.clone(),
                response_encoding: tx.response_encoding.clone(),
                timings: tx.timings.clone(),
                http_version: tx.http_version.clone(),
            })
            .collect();

        SessionFileData {
            app: "FreeCharles".to_string(),
            version: 1,
            saved_at: now_ms(),
            entries,
        }
    }

    pub fn import_session(&mut self, data: SessionFileData) -> Result<Vec<EntryMeta>, String> {
        // 兼容改名前的会话文件
        if data.app != "FreeCharles" && data.app != "FakeCharles" {
            return Err("无法识别的会话文件".to_string());
        }
        let decoder = base64::engine::general_purpose::STANDARD;
        self.items.clear();
        self.order.clear();
        self.seq = 0;
        self.total_bytes = 0;
        self.accounted_bytes.clear();

        for entry in data.entries {
            let mut meta = entry.meta;
            if meta.id.is_empty() || meta.url.is_empty() {
                continue;
            }
            if meta.state == "pending" {
                meta.state = "complete".to_string();
                if meta.note.is_none() {
                    meta.note = Some("会话恢复时请求尚未完成".to_string());
                }
            }
            self.seq = self.seq.max(meta.seq);
            let request_body = entry
                .request_body_base64
                .as_deref()
                .and_then(|value| decoder.decode(value).ok())
                .map(Bytes::from);
            let response_body = entry
                .response_body_base64
                .as_deref()
                .and_then(|value| decoder.decode(value).ok())
                .map(Bytes::from);
            let request_header_size = meta.request_size.saturating_sub(entry.request_body_size);
            let response_header_size = meta.response_size.saturating_sub(entry.response_body_size);
            let tx = StoredTransaction {
                meta: meta.clone(),
                request_headers: entry.request_headers,
                response_headers: entry.response_headers,
                request_header_size,
                response_header_size,
                request_chunks: Vec::new(),
                response_chunks: Vec::new(),
                request_captured: request_body.as_ref().map(|b| b.len()).unwrap_or(0),
                response_captured: response_body.as_ref().map(|b| b.len()).unwrap_or(0),
                request_body,
                response_body,
                request_body_size: entry.request_body_size,
                response_body_size: entry.response_body_size,
                request_truncated: entry.request_truncated,
                response_truncated: entry.response_truncated,
                request_encoding: entry.request_encoding,
                response_encoding: entry.response_encoding,
                timings: entry.timings,
                http_version: if entry.http_version.is_empty() {
                    "HTTP/1.1".to_string()
                } else {
                    entry.http_version
                },
                body_view_cache: None,
            };
            if !self.items.contains_key(&meta.id) {
                self.items.insert(meta.id.clone(), tx);
                self.order.push(meta.id.clone());
                self.account(&meta.id);
            }
        }

        // 会话导入由外层整体广播，无需逐条触发事件
        self.enforce_limits(false);
        Ok(self.list())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init(path: &str) -> TransactionInit {
        TransactionInit {
            method: "GET".to_string(),
            url: format!("http://example.com{path}"),
            host: "example.com".to_string(),
            port: 80,
            path: path.to_string(),
            protocol: "http".to_string(),
            client_ip: "127.0.0.1".to_string(),
            tunnel: false,
            decrypted: false,
            upgrade: false,
            note: None,
            repeat_of: None,
            http_version: Some("HTTP/1.1".to_string()),
        }
    }

    fn commit_with_body(store: &mut SessionStore, path: &str, body: &[u8]) {
        let mut tx = store.create(init(path));
        tx.push_request_chunk(&Bytes::copy_from_slice(body), 1024 * 1024);
        tx.finish_request_body();
        tx.meta.state = "complete".to_string();
        store.commit(tx);
    }

    #[test]
    fn evicts_oldest_by_entry_limit() {
        let mut store = SessionStore::new(None, 10);
        store.set_max_entries(2);
        for index in 0..3 {
            commit_with_body(&mut store, &format!("/{index}"), b"ok");
        }
        let list = store.list();
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|meta| meta.path != "/0"));
    }

    #[test]
    fn evicts_oldest_by_memory_budget() {
        let mut store = SessionStore::new(None, 10);
        store.set_max_session_bytes(64);
        commit_with_body(&mut store, "/first", &[b'a'; 128]);
        commit_with_body(&mut store, "/second", &[b'b'; 128]);
        let list = store.list();
        // 至少保留最新一条，避免单条超大内容刚捕获就被淘汰
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, "/second");
    }

    #[test]
    fn caches_small_completed_body_view() {
        let mut store = SessionStore::new(None, 10);
        let mut tx = store.create(init("/cache"));
        tx.push_response_chunk(&Bytes::from_static(b"hello"), 1024);
        tx.meta.response_mime = "text/plain".to_string();
        tx.finish_response_body();
        tx.meta.state = "complete".to_string();
        let id = tx.meta.id.clone();
        store.commit(tx);

        assert!(store.cached_body_view(&id, "response").is_none());
        let view = crate::body::render_body(
            Some(&Bytes::from_static(b"hello")),
            5,
            false,
            "text/plain",
            None,
        );
        store.cache_body_view(&id, "response", view);
        assert_eq!(
            store
                .cached_body_view(&id, "response")
                .and_then(|view| view.text),
            Some("hello".to_string())
        );
    }

    #[test]
    fn keeps_upgrade_flag_from_init() {
        let mut store = SessionStore::new(None, 10);
        let mut init = init("/ws");
        init.upgrade = true;
        let tx = store.create(init);
        assert!(tx.meta.upgrade);
    }
}
