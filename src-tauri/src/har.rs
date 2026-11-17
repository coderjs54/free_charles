//! 将条目导出为 HAR 1.2，方便导入 Chrome DevTools 或其他工具。

use serde_json::{json, Map, Value};

use crate::models::{EntryDetail, HeaderPair, APP_NAME, APP_VERSION};

pub fn build_har(entries: &[EntryDetail]) -> Value {
    let har_entries: Vec<Value> = entries
        .iter()
        .filter(|entry| !entry.meta.tunnel && entry.meta.method != "CONNECT")
        .map(build_entry)
        .collect();

    let started = entries
        .first()
        .map(|entry| iso_time(entry.meta.start_time))
        .unwrap_or_else(|| iso_time(crate::models::now_ms()));

    json!({
        "log": {
            "version": "1.2",
            "creator": { "name": APP_NAME, "version": APP_VERSION },
            "browser": { "name": APP_NAME, "version": APP_VERSION },
            "pages": [{ "startedDateTime": started, "id": "page_1", "title": "FreeCharles Session" }],
            "entries": har_entries
        }
    })
}

fn build_entry(entry: &EntryDetail) -> Value {
    let query_string: Vec<Value> = url::Url::parse(&entry.meta.url)
        .map(|url| {
            url.query_pairs()
                .map(|(name, value)| json!({ "name": name, "value": value }))
                .collect()
        })
        .unwrap_or_default();

    let request_cookie_header = find_header(&entry.request_headers, "cookie");
    let response_cookies = parse_set_cookies(&entry.response_headers);
    let location = find_header(&entry.response_headers, "location");

    let post_data = if entry.request_body.kind == "text" {
        entry
            .request_body
            .text
            .as_ref()
            .map(|text| json!({ "mimeType": if entry.meta.request_mime.is_empty() { "text/plain" } else { entry.meta.request_mime.as_str() }, "text": text }))
    } else {
        None
    };

    let mut request = Map::new();
    request.insert("method".into(), json!(entry.meta.method));
    request.insert("url".into(), json!(entry.meta.url));
    request.insert("httpVersion".into(), json!(entry.http_version));
    request.insert(
        "cookies".into(),
        json!(parse_request_cookies(&request_cookie_header)),
    );
    request.insert(
        "headers".into(),
        json!(to_har_headers(&entry.request_headers)),
    );
    request.insert("queryString".into(), Value::Array(query_string));
    if let Some(post_data) = post_data {
        request.insert("postData".into(), post_data);
    }
    request.insert("headersSize".into(), json!(entry.request_header_size));
    request.insert("bodySize".into(), json!(entry.request_body.size));

    let mut response = Map::new();
    response.insert("status".into(), json!(entry.meta.status.unwrap_or(0)));
    response.insert(
        "statusText".into(),
        json!(entry.meta.status_text.clone().unwrap_or_default()),
    );
    response.insert("httpVersion".into(), json!(entry.http_version));
    response.insert("cookies".into(), json!(response_cookies));
    response.insert(
        "headers".into(),
        json!(to_har_headers(&entry.response_headers)),
    );
    response.insert(
        "content".into(),
        build_content(
            &entry.response_body.kind,
            entry.response_body.text.as_deref(),
            entry.response_body.base64.as_deref(),
            entry.response_body.decoded_size,
            entry.response_body.size,
            &entry.meta.response_mime,
        ),
    );
    response.insert("redirectURL".into(), json!(location));
    response.insert("headersSize".into(), json!(entry.response_header_size));
    response.insert("bodySize".into(), json!(entry.response_body.size));

    json!({
        "startedDateTime": iso_time(entry.meta.start_time),
        "time": entry.meta.duration.unwrap_or(0),
        "request": Value::Object(request),
        "response": Value::Object(response),
        "cache": {},
        "timings": {
            "send": 0,
            "wait": entry.timings.wait_ms.unwrap_or(0),
            "receive": entry.timings.receive_ms.unwrap_or(0)
        },
        "pageref": "page_1"
    })
}

fn build_content(
    kind: &str,
    text: Option<&str>,
    base64: Option<&str>,
    decoded_size: u64,
    size: u64,
    mime: &str,
) -> Value {
    let mut content = Map::new();
    content.insert(
        "size".into(),
        json!(if decoded_size > 0 { decoded_size } else { size }),
    );
    content.insert(
        "mimeType".into(),
        json!(if mime.is_empty() {
            "application/octet-stream"
        } else {
            mime
        }),
    );
    if kind == "text" {
        if let Some(text) = text {
            content.insert("text".into(), json!(text));
        }
    } else if kind == "binary" {
        if let Some(base64) = base64 {
            content.insert("text".into(), json!(base64));
            content.insert("encoding".into(), json!("base64"));
        }
    }
    Value::Object(content)
}

fn to_har_headers(headers: &[HeaderPair]) -> Vec<Value> {
    headers
        .iter()
        .map(|(name, value)| json!({ "name": name, "value": value }))
        .collect()
}

fn find_header(headers: &[HeaderPair], name: &str) -> String {
    let lower = name.to_lowercase();
    headers
        .iter()
        .find(|(key, _)| key.to_lowercase() == lower)
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

fn parse_request_cookies(cookie_header: &str) -> Vec<Value> {
    if cookie_header.is_empty() {
        return Vec::new();
    }
    cookie_header
        .split(';')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .map(|part| match part.find('=') {
            Some(index) if index > 0 => {
                json!({ "name": &part[..index], "value": &part[index + 1..] })
            }
            _ => json!({ "name": part, "value": "" }),
        })
        .collect()
}

fn parse_set_cookies(headers: &[HeaderPair]) -> Vec<Value> {
    let mut cookies = Vec::new();
    for (name, value) in headers {
        if !name.eq_ignore_ascii_case("set-cookie") {
            continue;
        }
        let mut segments: Vec<&str> = value.split(';').map(|part| part.trim()).collect();
        let first = segments.first().copied().unwrap_or("").to_string();
        if segments.is_empty() {
            continue;
        }
        segments.remove(0);
        let Some(index) = first.find('=') else {
            continue;
        };
        if index == 0 {
            continue;
        }
        let mut cookie = Map::new();
        cookie.insert("name".into(), json!(&first[..index]));
        cookie.insert("value".into(), json!(&first[index + 1..]));
        for segment in segments {
            let mut parts = segment.splitn(2, '=');
            let key = parts.next().unwrap_or("").trim().to_lowercase();
            let raw_value = parts.next().unwrap_or("").trim();
            match key.as_str() {
                "path" => {
                    cookie.insert("path".into(), json!(raw_value));
                }
                "domain" => {
                    cookie.insert("domain".into(), json!(raw_value));
                }
                "expires" => {
                    if let Ok(parsed) = chrono::DateTime::parse_from_rfc2822(raw_value) {
                        cookie.insert(
                            "expires".into(),
                            json!(parsed.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
                        );
                    }
                }
                "httponly" => {
                    cookie.insert("httpOnly".into(), json!(true));
                }
                "secure" => {
                    cookie.insert("secure".into(), json!(true));
                }
                _ => {}
            }
        }
        cookies.push(Value::Object(cookie));
    }
    cookies
}

fn iso_time(timestamp: u64) -> String {
    use chrono::{SecondsFormat, TimeZone, Utc};
    let seconds = (timestamp / 1000) as i64;
    let nanos = ((timestamp % 1000) * 1_000_000) as u32;
    Utc.timestamp_opt(seconds, nanos)
        .single()
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string())
}
