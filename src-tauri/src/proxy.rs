use crate::db::Database;
use crate::models::{OptimizePromptInput, ProxySettings, SaveProxySettingsInput};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("proxy is not configured")]
    NotConfigured,
    #[error("invalid proxy URL: {0}")]
    InvalidUrl(String),
    #[error("authentication failed ({0})")]
    Authentication(u16),
    #[error("model is unavailable ({0})")]
    ModelUnavailable(u16),
    #[error("proxy is rate limited ({0})")]
    RateLimited(u16),
    #[error("proxy is overloaded ({0})")]
    Overloaded(u16),
    #[error("proxy protocol error ({0})")]
    Protocol(String),
    #[error("proxy request failed: {0}")]
    Network(String),
    #[error("proxy request timed out")]
    Timeout,
    #[error("proxy request cancelled")]
    Cancelled,
    #[error("keychain error: {0}")]
    Keychain(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyProtocol {
    Responses,
    ChatCompletions,
}

impl ProxyProtocol {
    pub fn parse(value: &str) -> Result<Self, ProxyError> {
        match value {
            "responses" => Ok(Self::Responses),
            "chat_completions" | "chat-completions" => Ok(Self::ChatCompletions),
            other => Err(ProxyError::Protocol(format!("unsupported protocol `{other}`"))),
        }
    }
}

pub fn normalize_endpoint(base_url: &str, protocol: ProxyProtocol) -> Result<String, ProxyError> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() { return Err(ProxyError::NotConfigured); }
    let mut url = url::Url::parse(trimmed).map_err(|e| ProxyError::InvalidUrl(e.to_string()))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(ProxyError::InvalidUrl("URL must use http or https".into()));
    }
    let endpoint = match protocol { ProxyProtocol::Responses => "responses", ProxyProtocol::ChatCompletions => "chat/completions" };
    let path = url.path().trim_end_matches('/');
    let path_lower = path.to_ascii_lowercase();
    let endpoint_lower = format!("/{endpoint}");
    let final_path = if path_lower.ends_with(&endpoint_lower) {
        path.to_string()
    } else if path_lower.ends_with("/v1") {
        format!("{path}/{endpoint}")
    } else if path_lower.is_empty() || path_lower == "/" {
        format!("/v1/{endpoint}")
    } else if path_lower.contains("/v1") {
        format!("{path}/{endpoint}")
    } else {
        format!("{path}/v1/{endpoint}")
    };
    url.set_path(&final_path);
    Ok(url.to_string())
}

pub fn keychain_ref() -> String { format!("proxy-key-{}", Uuid::new_v4()) }

pub fn store_api_key(reference: &str, value: &str) -> Result<(), ProxyError> {
    if value.trim().is_empty() { return Ok(()); }
    let entry = keyring::Entry::new("dev.vibeworking.desktop", reference).map_err(|e| ProxyError::Keychain(e.to_string()))?;
    entry.set_password(value).map_err(|e| ProxyError::Keychain(e.to_string()))
}

fn load_api_key(reference: Option<&str>) -> Result<Option<String>, ProxyError> {
    let Some(reference) = reference else { return Ok(None); };
    let entry = keyring::Entry::new("dev.vibeworking.desktop", reference).map_err(|e| ProxyError::Keychain(e.to_string()))?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(ProxyError::Keychain(error.to_string())),
    }
}

pub fn save_settings(db: &Database, input: SaveProxySettingsInput) -> Result<ProxySettings, String> {
    let protocol = ProxyProtocol::parse(&input.protocol).map_err(|e| e.to_string())?;
    let _ = normalize_endpoint(&input.base_url, protocol).map_err(|e| e.to_string())?;
    if input.model.trim().is_empty() { return Err("model name cannot be empty".into()); }
    let reference = if input.api_key.as_deref().is_some_and(|v| !v.trim().is_empty()) {
        let reference = keychain_ref();
        store_api_key(&reference, input.api_key.as_deref().unwrap_or("")).map_err(|e| e.to_string())?;
        Some(reference)
    } else {
        db.proxy_settings().map_err(|e| e.to_string())?.api_key_ref
    };
    let settings = ProxySettings { base_url: input.base_url.trim().trim_end_matches('/').to_string(), protocol: input.protocol, model: input.model.trim().to_string(), timeout_seconds: input.timeout_seconds.clamp(5, 600), api_key_ref: reference, has_api_key: true };
    db.save_proxy_settings(&settings).map_err(|e| e.to_string())?;
    Ok(settings)
}

pub async fn optimize_prompt(db: &Database, input: OptimizePromptInput) -> Result<String, String> {
    let task = db.get_task(&input.task_id).map_err(|e| e.to_string())?;
    let project = db.get_project(&task.project_id).map_err(|e| e.to_string())?;
    let settings = db.proxy_settings().map_err(|e| e.to_string())?;
    if settings.base_url.trim().is_empty() || settings.model.trim().is_empty() { return Err(ProxyError::NotConfigured.to_string()); }
    let protocol = ProxyProtocol::parse(&settings.protocol).map_err(|e| e.to_string())?;
    let endpoint = normalize_endpoint(&settings.base_url, protocol).map_err(|e| e.to_string())?;
    let api_key = load_api_key(settings.api_key_ref.as_deref()).map_err(|e| e.to_string())?;
    let prompt = build_optimizer_prompt(&task.title, &task.original_request, &project.context, &project.constraints, input.include_context);
    let client = reqwest::Client::builder().timeout(Duration::from_secs(settings.timeout_seconds.clamp(5, 600))).build().map_err(|e| e.to_string())?;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Some(key) = api_key { headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {key}")).map_err(|e| e.to_string())?); }
    let body = match protocol {
        ProxyProtocol::Responses => json!({ "model": settings.model, "input": prompt, "stream": true }),
        ProxyProtocol::ChatCompletions => json!({ "model": settings.model, "messages": [{"role":"user","content": prompt}], "stream": true }),
    };
    let response = client.post(endpoint).headers(headers).json(&body).send().await.map_err(|e| if e.is_timeout() { ProxyError::Timeout.to_string() } else { ProxyError::Network(e.to_string()).to_string() })?;
    let status = response.status();
    if !status.is_success() { return Err(classify_http_error(status.as_u16(), response.text().await.unwrap_or_default()).to_string()); }
    parse_stream(response, protocol).await.map_err(|e| e.to_string())
}

pub async fn test_connection(db: &Database) -> Result<String, String> {
    let settings = db.proxy_settings().map_err(|e| e.to_string())?;
    if settings.base_url.trim().is_empty() {
        return Err(ProxyError::NotConfigured.to_string());
    }
    let protocol = ProxyProtocol::parse(&settings.protocol).map_err(|e| e.to_string())?;
    let endpoint = normalize_endpoint(&settings.base_url, protocol).map_err(|e| e.to_string())?;
    let api_key = load_api_key(settings.api_key_ref.as_deref()).map_err(|e| e.to_string())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(settings.timeout_seconds.clamp(5, 600)))
        .build()
        .map_err(|e| e.to_string())?;
    let mut request = client.head(endpoint);
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }
    let response = request.send().await.map_err(|e| {
        if e.is_timeout() { ProxyError::Timeout.to_string() } else { ProxyError::Network(e.to_string()).to_string() }
    })?;
    let status = response.status();
    if status.is_success() || status.as_u16() == 405 {
        Ok(format!("Endpoint reachable (HTTP {})", status.as_u16()))
    } else {
        Err(classify_http_error(status.as_u16(), response.text().await.unwrap_or_default()).to_string())
    }
}

fn build_optimizer_prompt(title: &str, request: &str, context: &str, constraints: &str, include_context: bool) -> String {
    let mut result = format!("将下面的开发任务整理成一个可直接交给 coding agent 执行的简洁 Prompt。保留用户意图，不编造仓库文件、接口或依赖。输出目标、必要上下文、范围、约束和验收标准；信息不足时明确假设。\n\n任务标题：{title}\n原始需求：\n{request}");
    if include_context && (!context.trim().is_empty() || !constraints.trim().is_empty()) {
        result.push_str("\n\n项目上下文：\n");
        if !context.trim().is_empty() { result.push_str(context.trim()); }
        if !constraints.trim().is_empty() { result.push_str("\n\n执行约束：\n"); result.push_str(constraints.trim()); }
    }
    result
}

async fn parse_stream(response: reqwest::Response, protocol: ProxyProtocol) -> Result<String, ProxyError> {
    let mut stream = response.bytes_stream();
    let mut parser = SseParser::default();
    let mut output = String::new();
    let mut completed = false;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| ProxyError::Network(e.to_string()))?;
        for event in parser.push(&bytes)? {
            if event == "[DONE]" { completed = true; continue; }
            let value: Value = serde_json::from_str(&event).map_err(|e| ProxyError::Protocol(format!("invalid SSE JSON: {e}")))?;
            if let Some(error) = value.get("error") { return Err(ProxyError::Protocol(error.to_string())); }
            if value.get("type").and_then(Value::as_str).is_some_and(|kind| matches!(kind,"response.completed"|"response.failed"|"response.incomplete")) {
                completed = value.get("type").and_then(Value::as_str) == Some("response.completed");
            }
            let delta = match protocol {
                ProxyProtocol::Responses => value.get("delta").and_then(Value::as_str).or_else(|| value.pointer("/response/output_text").and_then(Value::as_str)),
                ProxyProtocol::ChatCompletions => value.pointer("/choices/0/delta/content").and_then(Value::as_str),
            };
            if let Some(delta) = delta { output.push_str(delta); }
        }
    }
    for event in parser.finish()? {
        if event == "[DONE]" { completed = true; break; }
        let value: Value = serde_json::from_str(&event).map_err(|e| ProxyError::Protocol(format!("invalid final SSE JSON: {e}")))?;
        if let Some(delta) = match protocol { ProxyProtocol::Responses => value.get("delta").and_then(Value::as_str), ProxyProtocol::ChatCompletions => value.pointer("/choices/0/delta/content").and_then(Value::as_str) } { output.push_str(delta); }
    }
    if !completed { return Err(ProxyError::Protocol("stream ended before a completion event".into())); }
    if output.trim().is_empty() { return Err(ProxyError::Protocol("stream completed without text".into())); }
    Ok(output)
}

fn classify_http_error(status: u16, body: String) -> ProxyError {
    match status {
        401 | 403 => ProxyError::Authentication(status),
        404 | 400 => ProxyError::ModelUnavailable(status),
        408 | 429 => ProxyError::RateLimited(status),
        500 | 502 | 503 | 504 => ProxyError::Overloaded(status),
        _ => ProxyError::Protocol(format!("HTTP {status}: {}", body.chars().take(500).collect::<String>())),
    }
}

#[derive(Default)]
struct SseParser { buffer: Vec<u8>, data: Vec<String> }

impl SseParser {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, ProxyError> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(pos) = self.buffer.iter().position(|b| *b == b'\n') {
            let line = self.buffer.drain(..=pos).collect::<Vec<_>>();
            let line = String::from_utf8(line[..line.len() - 1].to_vec()).map_err(|_| ProxyError::Protocol("SSE contained invalid UTF-8".into()))?;
            let line = line.strip_suffix('\r').unwrap_or(&line);
            if line.is_empty() {
                if !self.data.is_empty() { events.push(self.data.join("\n")); self.data.clear(); }
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data.push(data.strip_prefix(' ').unwrap_or(data).to_string());
            }
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<String>, ProxyError> {
        if !self.buffer.is_empty() { return Err(ProxyError::Protocol("stream ended mid-SSE line".into())); }
        let mut events = Vec::new();
        if !self.data.is_empty() { events.push(self.data.join("\n")); self.data.clear(); }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_base_url_without_duplicate_v1() {
        assert_eq!(normalize_endpoint("https://proxy.example", ProxyProtocol::Responses).unwrap(), "https://proxy.example/v1/responses");
        assert_eq!(normalize_endpoint("https://proxy.example/", ProxyProtocol::Responses).unwrap(), "https://proxy.example/v1/responses");
        assert_eq!(normalize_endpoint("https://proxy.example/v1", ProxyProtocol::Responses).unwrap(), "https://proxy.example/v1/responses");
        assert_eq!(normalize_endpoint("https://proxy.example/v1/", ProxyProtocol::ChatCompletions).unwrap(), "https://proxy.example/v1/chat/completions");
        assert_eq!(normalize_endpoint("https://proxy.example/custom/v1", ProxyProtocol::Responses).unwrap(), "https://proxy.example/custom/v1/responses");
    }

    #[test]
    fn parses_split_utf8_sse() {
        let mut parser = SseParser::default();
        let text = "data: {\"delta\":\"你好\"}\n\n".as_bytes();
        let split = text.iter().position(|b| *b >= 0x80).unwrap() + 1;
        assert!(parser.push(&text[..split]).unwrap().is_empty());
        let events = parser.push(&text[split..]).unwrap();
        assert_eq!(events, vec![r#"{"delta":"你好"}"#.to_string()]);
    }
}
