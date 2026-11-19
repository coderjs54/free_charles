//! 请求/响应体解码与展示数据构建（gzip / deflate / br / zstd）。

use std::io::Read;

use base64::Engine;
use bytes::Bytes;

use crate::models::BodyView;

const MAX_DISPLAY_TEXT: usize = 1_500_000;
const MAX_DISPLAY_BINARY: usize = 1_500_000;
/// 解压输出上限：防止压缩炸弹把内存耗尽（捕获上限只限制压缩后字节）
const MAX_DECODED_SIZE: usize = 128 * 1024 * 1024;

const BINARY_MIME_HINTS: &[&str] = &[
    "image/",
    "audio/",
    "video/",
    "font/",
    "application/zip",
    "application/gzip",
    "application/pdf",
    "application/octet-stream",
    "application/wasm",
    "application/x-protobuf",
];

const TEXT_MIME_HINTS: &[&str] = &[
    "text/",
    "json",
    "xml",
    "javascript",
    "ecmascript",
    "x-www-form-urlencoded",
    "graphql",
    "csv",
    "yaml",
    "xhtml",
    "svg",
];

pub struct DecodedBuffer {
    pub data: Vec<u8>,
    pub error: Option<String>,
}

/// 按 Content-Encoding 解码内容（gzip / deflate / br）
pub fn decode_buffer(buffer: &[u8], content_encoding: Option<&str>) -> DecodedBuffer {
    let encoding = content_encoding.unwrap_or("").trim().to_lowercase();
    if encoding.is_empty() || encoding == "identity" {
        return DecodedBuffer {
            data: buffer.to_vec(),
            error: None,
        };
    }

    let decoded = match encoding.as_str() {
        "gzip" | "x-gzip" => read_decoder(flate2::read::GzDecoder::new(buffer)),
        "br" => read_decoder(brotli::Decompressor::new(buffer, 4096)),
        "deflate" => match read_decoder(flate2::read::ZlibDecoder::new(buffer)) {
            Ok(data) => Ok(data),
            // 超过输出上限时直接失败，避免再尝试 raw deflate 造成二次解压
            Err(DecodeFailure::Limit) => Err(DecodeFailure::Limit),
            Err(DecodeFailure::Message(_)) => {
                read_decoder(flate2::read::DeflateDecoder::new(buffer))
            }
        },
        "zstd" => match zstd::stream::read::Decoder::new(buffer) {
            Ok(decoder) => read_decoder(decoder),
            Err(error) => Err(DecodeFailure::Message(error.to_string())),
        },
        _ => {
            return DecodedBuffer {
                data: buffer.to_vec(),
                error: None,
            }
        }
    };

    match decoded {
        Ok(data) => DecodedBuffer { data, error: None },
        Err(DecodeFailure::Limit) => DecodedBuffer {
            data: buffer.to_vec(),
            error: Some(format!(
                "内容解压后超过 {} MB 上限，已停止解码",
                MAX_DECODED_SIZE / 1024 / 1024
            )),
        },
        Err(DecodeFailure::Message(error)) => DecodedBuffer {
            data: buffer.to_vec(),
            error: Some(format!("内容解码失败：{error}")),
        },
    }
}

/// 解码失败原因：区分「超过输出上限」和普通解码错误，避免上限错误触发 deflate 回退
enum DecodeFailure {
    Limit,
    Message(String),
}

fn read_decoder<R: Read>(reader: R) -> Result<Vec<u8>, DecodeFailure> {
    let mut output = Vec::new();
    // 多读 1 字节用于判断是否超过上限
    let mut limited = reader.take(MAX_DECODED_SIZE as u64 + 1);
    limited
        .read_to_end(&mut output)
        .map_err(|error| DecodeFailure::Message(error.to_string()))?;
    if output.len() > MAX_DECODED_SIZE {
        return Err(DecodeFailure::Limit);
    }
    Ok(output)
}

/// 将捕获到的字节转换为前端可直接展示的 BodyView
pub fn render_body(
    buffer: Option<&Bytes>,
    transferred_size: u64,
    truncated: bool,
    mime: &str,
    content_encoding: Option<&str>,
) -> BodyView {
    let mut view = BodyView {
        available: false,
        size: transferred_size,
        captured_size: buffer.map(|value| value.len() as u64).unwrap_or(0),
        decoded_size: 0,
        truncated,
        text_truncated: false,
        kind: "empty".to_string(),
        mime: mime.to_string(),
        encoding: content_encoding
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string()),
        text: None,
        base64: None,
        decode_error: None,
        deferred: false,
    };

    let Some(buffer) = buffer.filter(|value| !value.is_empty()) else {
        if transferred_size > 0 {
            view.decode_error = Some("响应体未被捕获（可能被大小限制丢弃）".to_string());
        }
        return view;
    };

    let decoded = decode_buffer(buffer.as_ref(), content_encoding);
    let data = decoded.data;
    let text_like = is_text_mime(mime) || (!is_binary_mime(mime) && looks_like_text(&data));

    view.available = true;
    view.decoded_size = data.len() as u64;
    view.decode_error = decoded.error.clone();

    if !text_like || decoded.error.is_some() {
        let take = data.len().min(MAX_DISPLAY_BINARY);
        view.kind = "binary".to_string();
        view.base64 = Some(base64::engine::general_purpose::STANDARD.encode(&data[..take]));
        view.text_truncated = data.len() > take;
        return view;
    }

    let charset = parse_charset(mime);
    let mut text = decode_text(&data, &charset);
    if text.len() > MAX_DISPLAY_TEXT {
        // 按字符边界安全截断
        let mut end = MAX_DISPLAY_TEXT;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        view.text_truncated = true;
    }
    view.kind = "text".to_string();
    view.text = Some(text);
    view
}

pub fn is_text_mime(mime: &str) -> bool {
    let value = mime.to_lowercase();
    if value.is_empty() {
        return false;
    }
    if BINARY_MIME_HINTS.iter().any(|hint| value.contains(hint)) {
        return false;
    }
    TEXT_MIME_HINTS.iter().any(|hint| value.contains(hint))
}

pub fn is_binary_mime(mime: &str) -> bool {
    let value = mime.to_lowercase();
    BINARY_MIME_HINTS.iter().any(|hint| value.contains(hint))
}

fn looks_like_text(buffer: &[u8]) -> bool {
    let sample = &buffer[..buffer.len().min(2048)];
    if sample.is_empty() {
        return true;
    }
    let mut control = 0usize;
    for byte in sample {
        if *byte == 9 || *byte == 10 || *byte == 13 {
            continue;
        }
        if *byte < 32 || *byte == 127 {
            control += 1;
        }
    }
    (control as f64) / (sample.len() as f64) < 0.02
}

fn parse_charset(mime: &str) -> String {
    let lower = mime.to_lowercase();
    if let Some(index) = lower.find("charset") {
        let rest = &mime[index + "charset".len()..];
        let rest = rest.trim_start().trim_start_matches('=').trim_start();
        let rest = rest.trim_start_matches('"');
        let value: String = rest
            .chars()
            .take_while(|ch| !matches!(ch, '"' | ';' | ' ' | '\t'))
            .collect();
        if !value.is_empty() {
            return value;
        }
    }
    "utf-8".to_string()
}

fn decode_text(buffer: &[u8], charset: &str) -> String {
    if let Some(encoding) = encoding_rs::Encoding::for_label(charset.as_bytes()) {
        let (text, _, _) = encoding.decode(buffer);
        return text.into_owned();
    }
    String::from_utf8_lossy(buffer).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gzip(input: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(input).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn decodes_supported_encodings() {
        let text = br#"{"ok":true}"#;
        let compressed = gzip(text);
        assert_eq!(
            decode_buffer(&compressed, Some("gzip")).data.as_slice(),
            text.as_slice()
        );
        assert!(decode_buffer(&compressed, Some("gzip")).error.is_none());

        let zstd_data = zstd::encode_all(&text[..], 3).unwrap();
        assert_eq!(
            decode_buffer(&zstd_data, Some("zstd")).data.as_slice(),
            text.as_slice()
        );
        assert!(decode_buffer(&zstd_data, Some("zstd")).error.is_none());
    }

    #[test]
    fn keeps_original_bytes_on_decode_error() {
        let invalid = b"not-a-gzip-stream";
        let decoded = decode_buffer(invalid, Some("gzip"));
        assert_eq!(decoded.data.as_slice(), invalid.as_slice());
        assert!(decoded.error.is_some());
    }

    #[test]
    fn renders_text_body_with_deferred_default_false() {
        let view = render_body(
            Some(&Bytes::from_static(b"hello")),
            5,
            false,
            "text/plain",
            None,
        );
        assert_eq!(view.kind, "text");
        assert_eq!(view.text.as_deref(), Some("hello"));
        assert!(!view.deferred);
    }
}
