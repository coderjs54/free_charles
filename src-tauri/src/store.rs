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
        self.meta.request_size = self.request_header_size + self.request_body_size;
    }

    pub fn finish_response_body(&mut self) {
        self.response_body = concat_chunks(&self.response_chunks);
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
    pub request_truncated: bool,
    pub request_encoding: Option<String>,
    pub response_body: Option<Bytes>,
    pub response_body_size: u64,
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
    sink: Option<EventSink>,
}

impl SessionStore {
    pub fn new(sink: Option<EventSink>, max_entries: usize) -> Self {
        Self {
            items: HashMap::new(),
            order: Vec::new(),
            seq: 0,
            max_entries,
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
        if let Some(tx) = self.items.get(id) {
            self.emit(StoreEvent::Update(tx.meta.clone()));
        }
    }

    pub fn finalize(&mut self, id: &str, state: &str, error: Option<String>) {
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
        let meta = tx.meta.clone();
        self.emit(StoreEvent::Update(meta));
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
        self.emit(StoreEvent::Remove(id.to_string()));
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
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

    fn trim(&mut self) {
        while self.order.len() > self.max_entries {
            let id = self.order.remove(0);
            self.items.remove(&id);
            self.emit(StoreEvent::Remove(id));
        }
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
            request_truncated: tx.request_truncated,
            request_encoding: tx.request_encoding.clone(),
            response_body: tx.captured_response(),
            response_body_size: tx.response_body_size,
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
            let request_header_size = meta
                .request_size
                .saturating_sub(entry.request_body_size);
            let response_header_size = meta
                .response_size
                .saturating_sub(entry.response_body_size);
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
            };
            if !self.items.contains_key(&meta.id) {
                self.items.insert(meta.id.clone(), tx);
                self.order.push(meta.id);
            }
        }

        // 会话导入由外层整体广播，无需逐条触发事件
        while self.order.len() > self.max_entries {
            let id = self.order.remove(0);
            self.items.remove(&id);
        }
        Ok(self.list())
    }
}
