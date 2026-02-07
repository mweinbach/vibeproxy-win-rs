use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use crate::types::VercelGatewayConfig;
use crate::usage_tracker::{UsageEvent, UsageTracker};
use chrono::Utc;
use uuid::Uuid;

const HARD_TOKEN_CAP: i64 = 32000;
const MINIMUM_HEADROOM: i64 = 1024;
const HEADROOM_RATIO: f64 = 0.1;
const VERCEL_GATEWAY_HOST: &str = "ai-gateway.vercel.sh";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const INTERLEAVED_THINKING_BETA: &str = "interleaved-thinking-2025-05-14";
const HTTP_CONNECT_TIMEOUT_SECS: u64 = 5;
const HTTP_READ_TIMEOUT_SECS: u64 = 90;

struct ForwardOutcome {
    response: Response<Full<Bytes>>,
    status_code: u16,
    body: Bytes,
}

#[derive(Default)]
struct TokenUsage {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    usage_json: Option<String>,
    account_hint: Option<String>,
}

#[derive(Clone)]
struct TrackingSeed {
    request_id: String,
    started_at: Instant,
    method: String,
    path: String,
    provider: String,
    model: String,
    account_key: String,
    account_label: String,
    request_bytes: i64,
}

pub struct ThinkingProxy {
    pub proxy_port: u16,
    pub target_port: u16,
    pub vercel_config: Arc<RwLock<VercelGatewayConfig>>,
    pub usage_tracker: Arc<UsageTracker>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    serve_task: Option<tokio::task::JoinHandle<()>>,
    pub is_running: bool,
}

impl ThinkingProxy {
    pub fn new(vercel_config: Arc<RwLock<VercelGatewayConfig>>, usage_tracker: Arc<UsageTracker>) -> Self {
        Self {
            proxy_port: 8317,
            target_port: 8318,
            vercel_config,
            usage_tracker,
            shutdown_tx: None,
            serve_task: None,
            is_running: false,
        }
    }

    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.is_running {
            log::info!("[ThinkingProxy] Already running");
            return Ok(());
        }

        let addr = format!("127.0.0.1:{}", self.proxy_port);
        let listener = TcpListener::bind(&addr).await?;
        log::info!("[ThinkingProxy] Listening on port {}", self.proxy_port);

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        self.shutdown_tx = Some(shutdown_tx);
        self.is_running = true;

        let vercel_config = self.vercel_config.clone();
        let usage_tracker = self.usage_tracker.clone();
        let target_port = self.target_port;

        let serve_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _addr)) => {
                                let io = TokioIo::new(stream);
                                let vc = vercel_config.clone();
                                let tracker = usage_tracker.clone();
                                tokio::spawn(async move {
                                    let svc = service_fn(move |req| {
                                        let vc = vc.clone();
                                        let tracker = tracker.clone();
                                        async move {
                                            handle_request(req, vc, target_port, tracker).await
                                        }
                                    });
                                    if let Err(e) = http1::Builder::new()
                                        .serve_connection(io, svc)
                                        .await
                                    {
                                        log::error!("[ThinkingProxy] Connection error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                log::error!("[ThinkingProxy] Accept error: {}", e);
                            }
                        }
                    }
                    _ = &mut shutdown_rx => {
                        log::info!("[ThinkingProxy] Shutdown signal received");
                        break;
                    }
                }
            }
        });
        self.serve_task = Some(serve_task);

        Ok(())
    }

    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.serve_task.take() {
            match tokio::time::timeout(Duration::from_secs(2), handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    log::warn!("[ThinkingProxy] Proxy task join error: {}", e);
                }
                Err(_) => {
                    log::warn!("[ThinkingProxy] Timed out waiting for proxy task to stop");
                }
            }
        }
        self.is_running = false;
        log::info!("[ThinkingProxy] Stopped");
    }

    pub fn is_running(&self) -> bool {
        self.is_running
    }
}

fn make_response(status: StatusCode, body: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain")
        .header("Connection", "close")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}

fn make_redirect(location: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", location)
        .header("Content-Length", "0")
        .header("Connection", "close")
        .body(Full::new(Bytes::new()))
        .unwrap()
}

fn shared_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(HTTP_CONNECT_TIMEOUT_SECS))
            .read_timeout(Duration::from_secs(HTTP_READ_TIMEOUT_SECS))
            .pool_idle_timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(16)
            .tcp_nodelay(true)
            .build()
            .expect("Failed to build proxy HTTP client")
    })
}

async fn handle_request(
    req: Request<hyper::body::Incoming>,
    vercel_config: Arc<RwLock<VercelGatewayConfig>>,
    target_port: u16,
    usage_tracker: Arc<UsageTracker>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let request_started_at = Instant::now();
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();
    let headers = req.headers().clone();

    log::info!("[ThinkingProxy] Incoming request: {} {}", method, path);

    // Collect request body
    use http_body_util::BodyExt;
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            log::error!("[ThinkingProxy] Error reading request body: {}", e);
            return Ok(make_response(
                StatusCode::BAD_REQUEST,
                "Invalid request body",
            ));
        }
    };
    let body_string = String::from_utf8_lossy(&body_bytes).to_string();

    // 1. Amp CLI login redirects
    if path.starts_with("/auth/cli-login") || path.starts_with("/api/auth/cli-login") {
        let login_path = if path.starts_with("/api/") {
            &path[4..]
        } else {
            &path
        };
        let redirect_url = format!("https://ampcode.com{}", login_path);
        log::info!(
            "[ThinkingProxy] Redirecting Amp CLI login to: {}",
            redirect_url
        );
        return Ok(make_redirect(&redirect_url));
    }

    // 2. Amp provider path rewriting
    let rewritten_path = if path.starts_with("/provider/") {
        log::info!(
            "[ThinkingProxy] Rewriting Amp provider path: {} -> /api{}",
            path,
            path
        );
        format!("/api{}", path)
    } else {
        path.clone()
    };

    // 3. Amp management requests: anything not targeting provider or /v1
    let is_provider_path = rewritten_path.starts_with("/api/provider/");
    let is_cli_proxy_path =
        rewritten_path.starts_with("/v1/") || rewritten_path.starts_with("/api/v1/");
    let is_inference_request = is_provider_path || is_cli_proxy_path;
    if !is_provider_path && !is_cli_proxy_path {
        log::info!(
            "[ThinkingProxy] Amp management request, forwarding to ampcode.com: {}",
            rewritten_path
        );
        return Ok(
            forward_to_amp(&method, &rewritten_path, &headers, &body_string)
                .await
                .unwrap_or_else(|e| {
                    log::error!("[ThinkingProxy] Amp forward error: {}", e);
                    make_response(
                        StatusCode::BAD_GATEWAY,
                        "Bad Gateway - Could not connect to ampcode.com",
                    )
                }),
        );
    }

    // 4. Process thinking parameter for POST requests
    let mut modified_body = body_string.clone();
    let mut thinking_enabled = false;

    if method == hyper::Method::POST && !body_string.is_empty() {
        let (new_body, is_thinking) = process_thinking_parameter(&body_string);
        modified_body = new_body;
        thinking_enabled = is_thinking;
    }

    let tracking_seed = if is_inference_request {
        Some(build_tracking_seed(
            &method,
            &rewritten_path,
            &headers,
            &modified_body,
            body_bytes.len() as i64,
            request_started_at,
        ))
    } else {
        None
    };

    // 5. Vercel gateway routing
    let vc = vercel_config.read().await;
    if vc.is_active() && method == hyper::Method::POST && is_claude_model_request(&modified_body) {
        let api_key = vc.api_key.clone();
        drop(vc);
        log::info!("[ThinkingProxy] Routing Claude request via Vercel AI Gateway");
        let result = forward_to_vercel(
            &method,
            "/v1/messages",
            &headers,
            &modified_body,
            thinking_enabled,
            &api_key,
        )
        .await;

        return Ok(match result {
            Ok(outcome) => {
                record_usage_if_needed(
                    usage_tracker.clone(),
                    tracking_seed,
                    outcome.status_code,
                    outcome.body,
                );
                outcome.response
            }
            Err(e) => {
                log::error!("[ThinkingProxy] Vercel forward error: {}", e);
                record_usage_if_needed(usage_tracker.clone(), tracking_seed, 502, Bytes::new());
                make_response(
                    StatusCode::BAD_GATEWAY,
                    "Bad Gateway - Could not connect to Vercel AI Gateway",
                )
            }
        });
    }
    drop(vc);

    // 6. Default: forward to local backend on target_port
    let result = forward_to_backend(
        &method,
        &rewritten_path,
        &headers,
        &modified_body,
        thinking_enabled,
        target_port,
    )
    .await;

    match result {
        Ok(outcome) => {
            // If 404 and path doesn't start with /api/ or /v1/, retry with /api/ prefix
            if outcome.status_code == StatusCode::NOT_FOUND.as_u16()
                && !path.starts_with("/api/")
                && !path.starts_with("/v1/")
            {
                let new_path = format!("/api{}", path);
                log::info!(
                    "[ThinkingProxy] Got 404 for {}, retrying with {}",
                    path,
                    new_path
                );
                let retry_result = forward_to_backend(
                    &method,
                    &new_path,
                    &headers,
                    &modified_body,
                    thinking_enabled,
                    target_port,
                )
                .await;
                return Ok(match retry_result {
                    Ok(retry_outcome) => {
                        record_usage_if_needed(
                            usage_tracker.clone(),
                            tracking_seed,
                            retry_outcome.status_code,
                            retry_outcome.body,
                        );
                        retry_outcome.response
                    }
                    Err(e) => {
                        log::error!("[ThinkingProxy] Backend retry error: {}", e);
                        record_usage_if_needed(usage_tracker.clone(), tracking_seed, 502, Bytes::new());
                        make_response(StatusCode::BAD_GATEWAY, "Bad Gateway")
                    }
                });
            }
            record_usage_if_needed(
                usage_tracker.clone(),
                tracking_seed,
                outcome.status_code,
                outcome.body,
            );
            Ok(outcome.response)
        }
        Err(e) => {
            log::error!("[ThinkingProxy] Backend forward error: {}", e);
            record_usage_if_needed(usage_tracker, tracking_seed, 502, Bytes::new());
            Ok(make_response(StatusCode::BAD_GATEWAY, "Bad Gateway"))
        }
    }
}

fn build_tracking_seed(
    method: &hyper::Method,
    rewritten_path: &str,
    headers: &hyper::HeaderMap,
    body: &str,
    request_bytes: i64,
    started_at: Instant,
) -> TrackingSeed {
    let model = extract_model_from_body(body).unwrap_or_else(|| "unknown".to_string());
    let provider = infer_provider_from_path_and_model(rewritten_path, &model);
    let account_hint = extract_account_hint(headers, body);
    let account_key = account_hint.unwrap_or_else(|| "unknown".to_string());

    TrackingSeed {
        request_id: Uuid::new_v4().to_string(),
        started_at,
        method: method.to_string(),
        path: rewritten_path.to_string(),
        provider,
        model,
        account_key: account_key.clone(),
        account_label: account_key,
        request_bytes,
    }
}

fn record_usage_if_needed(
    usage_tracker: Arc<UsageTracker>,
    seed: Option<TrackingSeed>,
    status_code: u16,
    response_body: Bytes,
) {
    let Some(mut seed) = seed else {
        return;
    };

    let mut usage = extract_token_usage(&response_body);
    if seed.account_key == "unknown" {
        if let Some(account_hint) = usage.account_hint.take() {
            if !account_hint.trim().is_empty() {
                seed.account_key = account_hint.clone();
                seed.account_label = account_hint;
            }
        }
    }

    let event = UsageEvent {
        request_id: seed.request_id,
        timestamp_utc: Utc::now().timestamp(),
        method: seed.method,
        path: seed.path,
        provider: seed.provider,
        model: seed.model,
        account_key: seed.account_key,
        account_label: seed.account_label,
        status_code: status_code as i64,
        duration_ms: seed.started_at.elapsed().as_millis() as i64,
        request_bytes: seed.request_bytes,
        response_bytes: response_body.len() as i64,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        total_tokens: usage.total_tokens,
        cached_tokens: usage.cached_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        usage_json: usage.usage_json,
    };

    tokio::spawn(async move {
        if let Err(e) = usage_tracker.record_event(event).await {
            log::warn!("[ThinkingProxy] Failed to persist usage event: {}", e);
        }
    });
}

fn extract_model_from_body(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    json.get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn infer_provider_from_path_and_model(path: &str, model: &str) -> String {
    let path_parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if path_parts.len() >= 3 && path_parts[0] == "api" && path_parts[1] == "provider" {
        return path_parts[2].to_string();
    }

    let model_lower = model.to_ascii_lowercase();
    if model_lower.starts_with("claude-") {
        return "claude".to_string();
    }
    if model_lower.starts_with("gemini-") {
        return "gemini".to_string();
    }
    if model_lower.starts_with("qwen-") {
        return "qwen".to_string();
    }
    if model_lower.starts_with("glm-") || model_lower.starts_with("zai-") {
        return "zai".to_string();
    }
    if model_lower.starts_with("gpt-")
        || model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("o4")
        || model_lower.starts_with("o5")
    {
        return "codex".to_string();
    }
    if model_lower.contains("copilot") {
        return "github-copilot".to_string();
    }
    if model_lower.contains("antigravity") {
        return "antigravity".to_string();
    }
    "unknown".to_string()
}

fn extract_account_hint(headers: &hyper::HeaderMap, body: &str) -> Option<String> {
    let header_keys = [
        "x-vibeproxy-account",
        "x-vibeproxy-account-id",
        "x-auth-account",
        "x-auth-index",
        "x-account-id",
        "x-account-key",
    ];
    for header in header_keys {
        if let Some(value) = headers.get(header).and_then(|v| v.to_str().ok()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    for key in ["auth_index", "account_id", "account", "account_key"] {
        if let Some(value) = json.get(key) {
            if let Some(s) = value.as_str() {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            } else if value.is_number() {
                return Some(value.to_string());
            }
        }
    }

    None
}

fn extract_token_usage(response_body: &[u8]) -> TokenUsage {
    if response_body.is_empty() {
        return TokenUsage::default();
    }

    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(response_body) {
        if let Some(usage) = extract_usage_from_json_value(&json) {
            return usage;
        }
    }

    let text = String::from_utf8_lossy(response_body);
    let mut aggregate = TokenUsage::default();
    let mut saw_usage = false;
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("data:") {
            continue;
        }
        let payload = line.trim_start_matches("data:").trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
            if let Some(parsed) = extract_usage_from_json_value(&json) {
                saw_usage = true;
                merge_usage(&mut aggregate, parsed);
            }
        }
    }

    if saw_usage {
        aggregate
    } else {
        TokenUsage::default()
    }
}

fn merge_usage(target: &mut TokenUsage, source: TokenUsage) {
    target.input_tokens = sum_optional_i64(target.input_tokens, source.input_tokens);
    target.output_tokens = sum_optional_i64(target.output_tokens, source.output_tokens);
    target.cached_tokens = sum_optional_i64(target.cached_tokens, source.cached_tokens);
    target.reasoning_tokens = sum_optional_i64(target.reasoning_tokens, source.reasoning_tokens);
    target.total_tokens = sum_optional_i64(target.total_tokens, source.total_tokens);
    if target.usage_json.is_none() {
        target.usage_json = source.usage_json;
    }
    if target.account_hint.is_none() {
        target.account_hint = source.account_hint;
    }
}

fn sum_optional_i64(current: Option<i64>, incoming: Option<i64>) -> Option<i64> {
    match (current, incoming) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn extract_usage_from_json_value(value: &serde_json::Value) -> Option<TokenUsage> {
    if let Some(obj) = value.as_object() {
        if let Some(usage_value) = obj.get("usage") {
            if let Some(parsed) = parse_usage_object(usage_value) {
                return Some(parsed);
            }
        }

        if let Some(parsed) = parse_usage_object(value) {
            return Some(parsed);
        }

        for nested in obj.values() {
            if let Some(parsed) = extract_usage_from_json_value(nested) {
                return Some(parsed);
            }
        }
    } else if let Some(arr) = value.as_array() {
        for nested in arr {
            if let Some(parsed) = extract_usage_from_json_value(nested) {
                return Some(parsed);
            }
        }
    }
    None
}

fn parse_usage_object(value: &serde_json::Value) -> Option<TokenUsage> {
    let obj = value.as_object()?;

    let input_tokens = find_number_in_object(
        obj,
        &[
            "input_tokens",
            "prompt_tokens",
            "promptTokenCount",
            "inputTokenCount",
        ],
    );
    let output_tokens = find_number_in_object(
        obj,
        &[
            "output_tokens",
            "completion_tokens",
            "outputTokenCount",
            "candidatesTokenCount",
        ],
    );
    let total_tokens =
        find_number_in_object(obj, &["total_tokens", "totalTokenCount", "tokens"])
            .or_else(|| find_number_in_object_deep(value, &["total_tokens", "totalTokenCount", "tokens"]));
    let cached_tokens = find_number_in_object(
        obj,
        &[
            "cached_tokens",
            "cached_input_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ],
    )
    .or_else(|| {
        find_number_in_object_deep(
            value,
            &[
                "cached_tokens",
                "cached_input_tokens",
                "cache_read_input_tokens",
                "cache_creation_input_tokens",
            ],
        )
    });
    let reasoning_tokens = find_number_in_object(
        obj,
        &["reasoning_tokens", "thinking_tokens", "reasoningTokenCount"],
    )
    .or_else(|| {
        find_number_in_object_deep(
            value,
            &["reasoning_tokens", "thinking_tokens", "reasoningTokenCount"],
        )
    });
    let account_hint = find_string_or_number_in_object(
        obj,
        &["auth_index", "account_index", "account_id", "account"],
    );

    if input_tokens.is_none()
        && output_tokens.is_none()
        && total_tokens.is_none()
        && cached_tokens.is_none()
        && reasoning_tokens.is_none()
        && account_hint.is_none()
    {
        return None;
    }

    let total_tokens = total_tokens.or_else(|| match (input_tokens, output_tokens) {
        (Some(input), Some(output)) => Some(input + output),
        _ => None,
    });

    Some(TokenUsage {
        input_tokens,
        output_tokens,
        total_tokens,
        cached_tokens,
        reasoning_tokens,
        usage_json: serde_json::to_string(value).ok(),
        account_hint,
    })
}

fn find_number_in_object(obj: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<i64> {
    for key in keys {
        if let Some(value) = obj.get(*key) {
            if let Some(parsed) = value.as_i64() {
                return Some(parsed);
            }
            if let Some(parsed) = value.as_u64() {
                return Some(parsed as i64);
            }
            if let Some(parsed) = value.as_f64() {
                return Some(parsed.round() as i64);
            }
            if let Some(parsed) = value.as_str().and_then(|v| v.parse::<i64>().ok()) {
                return Some(parsed);
            }
        }
    }
    None
}

fn find_string_or_number_in_object(
    obj: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    for key in keys {
        if let Some(value) = obj.get(*key) {
            if let Some(parsed) = value.as_str() {
                let trimmed = parsed.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            } else if value.is_number() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn find_number_in_object_deep(value: &serde_json::Value, keys: &[&str]) -> Option<i64> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(v) = map.get(*key) {
                    if let Some(parsed) = v.as_i64() {
                        return Some(parsed);
                    }
                    if let Some(parsed) = v.as_u64() {
                        return Some(parsed as i64);
                    }
                    if let Some(parsed) = v.as_f64() {
                        return Some(parsed.round() as i64);
                    }
                    if let Some(parsed) = v.as_str().and_then(|s| s.parse::<i64>().ok()) {
                        return Some(parsed);
                    }
                }
            }
            for nested in map.values() {
                if let Some(found) = find_number_in_object_deep(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(arr) => {
            for nested in arr {
                if let Some(found) = find_number_in_object_deep(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn is_claude_model_request(body: &str) -> bool {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let Some(model) = json.get("model").and_then(|m| m.as_str()) else {
        return false;
    };
    model.starts_with("claude-") || model.starts_with("gemini-claude-")
}

/// Processes the JSON body to add thinking parameter if model name has a thinking suffix.
/// Returns (modified_body, thinking_enabled).
fn process_thinking_parameter(body: &str) -> (String, bool) {
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(body) else {
        return (body.to_string(), false);
    };

    let Some(model) = json
        .get("model")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
    else {
        return (body.to_string(), false);
    };

    // Only process Claude models (including gemini-claude variants)
    if !model.starts_with("claude-") && !model.starts_with("gemini-claude-") {
        return (body.to_string(), false);
    }

    // Check for thinking suffix pattern: -thinking-NUMBER
    let thinking_prefix = "-thinking-";
    if let Some(thinking_pos) = model.rfind(thinking_prefix) {
        let after_prefix = &model[thinking_pos + thinking_prefix.len()..];

        // Try to parse the number after -thinking-
        if let Ok(budget) = after_prefix.parse::<i64>() {
            if budget > 0 {
                // Determine clean model name
                let clean_model = if model.starts_with("gemini-claude-") {
                    // For gemini-claude-* models, preserve "-thinking" and only strip the number
                    // e.g. gemini-claude-opus-4-5-thinking-10000 -> gemini-claude-opus-4-5-thinking
                    model[..thinking_pos + thinking_prefix.len() - 1].to_string()
                } else {
                    // For claude-* models, strip the entire suffix
                    // e.g. claude-opus-4-5-20251101-thinking-10000 -> claude-opus-4-5-20251101
                    model[..thinking_pos].to_string()
                };

                let effective_budget = budget.min(HARD_TOKEN_CAP - 1);
                if effective_budget != budget {
                    log::info!(
                        "[ThinkingProxy] Adjusted thinking budget from {} to {} to stay within limits",
                        budget,
                        effective_budget
                    );
                }

                json["model"] = serde_json::Value::String(clean_model.clone());

                // Add thinking parameter
                json["thinking"] = serde_json::json!({
                    "type": "enabled",
                    "budget_tokens": effective_budget
                });

                // Ensure max token limits are greater than the thinking budget
                let token_headroom =
                    MINIMUM_HEADROOM.max((effective_budget as f64 * HEADROOM_RATIO) as i64);
                let desired_max_tokens = effective_budget + token_headroom;
                let mut required_max_tokens = desired_max_tokens.min(HARD_TOKEN_CAP);
                if required_max_tokens <= effective_budget {
                    required_max_tokens = (effective_budget + 1).min(HARD_TOKEN_CAP);
                }

                let has_max_output_tokens = json.get("max_output_tokens").is_some();
                let mut adjusted = false;

                if let Some(current) = json.get("max_tokens").and_then(|v| v.as_i64()) {
                    if current <= effective_budget {
                        json["max_tokens"] = serde_json::Value::Number(required_max_tokens.into());
                    }
                    adjusted = true;
                }

                if let Some(current) = json.get("max_output_tokens").and_then(|v| v.as_i64()) {
                    if current <= effective_budget {
                        json["max_output_tokens"] =
                            serde_json::Value::Number(required_max_tokens.into());
                    }
                    adjusted = true;
                }

                if !adjusted {
                    if has_max_output_tokens {
                        json["max_output_tokens"] =
                            serde_json::Value::Number(required_max_tokens.into());
                    } else {
                        json["max_tokens"] = serde_json::Value::Number(required_max_tokens.into());
                    }
                }

                log::info!(
                    "[ThinkingProxy] Transformed model '{}' -> '{}' with thinking budget {}",
                    model,
                    clean_model,
                    effective_budget
                );

                if let Ok(modified) = serde_json::to_string(&json) {
                    return (modified, true);
                }
            } else {
                // Invalid budget (non-positive) - strip suffix, no thinking
                let clean_model = if model.starts_with("gemini-claude-") {
                    model[..thinking_pos + thinking_prefix.len() - 1].to_string()
                } else {
                    model[..thinking_pos].to_string()
                };
                json["model"] = serde_json::Value::String(clean_model.clone());
                log::info!(
                    "[ThinkingProxy] Stripped invalid thinking suffix from '{}' -> '{}' (no thinking)",
                    model,
                    clean_model
                );
                if let Ok(modified) = serde_json::to_string(&json) {
                    return (modified, true);
                }
            }
        } else {
            // Not a valid number after -thinking- ; strip suffix, no thinking
            let clean_model = if model.starts_with("gemini-claude-") {
                model[..thinking_pos + thinking_prefix.len() - 1].to_string()
            } else {
                model[..thinking_pos].to_string()
            };
            json["model"] = serde_json::Value::String(clean_model.clone());
            log::info!(
                "[ThinkingProxy] Stripped invalid thinking suffix from '{}' -> '{}' (no thinking)",
                model,
                clean_model
            );
            if let Ok(modified) = serde_json::to_string(&json) {
                return (modified, true);
            }
        }
    } else if model.ends_with("-thinking") || model.contains("-thinking(") {
        // Model ends with -thinking or uses -thinking(budget) syntax
        // Enable beta header but don't modify body - let backend handle thinking budget
        log::info!(
            "[ThinkingProxy] Detected thinking model '{}' - enabling beta header, passing through to backend",
            model
        );
        return (body.to_string(), true);
    }

    (body.to_string(), false)
}

/// Build a reqwest header map from hyper headers, excluding hop-by-hop headers.
fn build_forwarding_headers(
    headers: &hyper::HeaderMap,
    excluded: &[&str],
) -> reqwest::header::HeaderMap {
    let mut out = reqwest::header::HeaderMap::new();
    for (name, value) in headers.iter() {
        let name_lower = name.as_str().to_lowercase();
        if excluded.iter().any(|&ex| name_lower == ex) {
            continue;
        }
        if let Ok(n) = reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()) {
            if let Ok(v) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                out.append(n, v);
            }
        }
    }
    out
}

/// Build a hyper Response from a reqwest response (status, headers, body).
fn build_proxy_response(
    status: reqwest::StatusCode,
    resp_headers: &reqwest::header::HeaderMap,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let mut builder = Response::builder().status(status.as_u16());
    for (name, value) in resp_headers.iter() {
        // Skip hop-by-hop headers
        let name_lower = name.as_str().to_lowercase();
        if name_lower == "transfer-encoding" || name_lower == "connection" {
            continue;
        }
        builder = builder.header(name.as_str(), value.as_bytes());
    }
    builder.body(Full::new(body)).unwrap()
}

/// Forward a request to ampcode.com and rewrite Location headers / cookie domains in the response.
async fn forward_to_amp(
    method: &hyper::Method,
    path: &str,
    headers: &hyper::HeaderMap,
    body: &str,
) -> Result<Response<Full<Bytes>>, Box<dyn std::error::Error + Send + Sync>> {
    let client = shared_http_client();
    let url = format!("https://ampcode.com{}", path);

    let excluded = ["host", "content-length", "connection", "transfer-encoding"];
    let mut fwd_headers = build_forwarding_headers(headers, &excluded);
    fwd_headers.insert(
        reqwest::header::HOST,
        reqwest::header::HeaderValue::from_static("ampcode.com"),
    );

    let reqwest_method = reqwest::Method::from_bytes(method.as_str().as_bytes())?;
    let resp = client
        .request(reqwest_method, &url)
        .headers(fwd_headers)
        .body(body.to_string())
        .send()
        .await?;

    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let resp_body = resp.bytes().await?;

    // Rewrite response: Location headers and cookie domains
    // We need to rewrite both headers and body for Set-Cookie
    let mut builder = Response::builder().status(status.as_u16());

    for (name, value) in resp_headers.iter() {
        let name_lower = name.as_str().to_lowercase();
        if name_lower == "transfer-encoding" || name_lower == "connection" {
            continue;
        }

        let value_str = String::from_utf8_lossy(value.as_bytes()).to_string();
        let rewritten_value = if name_lower == "location" {
            rewrite_amp_location(&value_str)
        } else if name_lower == "set-cookie" {
            rewrite_amp_cookie(&value_str)
        } else {
            value_str
        };

        if let Ok(v) = reqwest::header::HeaderValue::from_str(&rewritten_value) {
            builder = builder.header(name.as_str(), v);
        }
    }

    Ok(builder.body(Full::new(resp_body)).unwrap())
}

/// Rewrite Location header values from ampcode.com responses.
fn rewrite_amp_location(value: &str) -> String {
    // Rewrite absolute ampcode.com URLs to /api/ local prefix
    if value.starts_with("https://ampcode.com/") || value.starts_with("http://ampcode.com/") {
        let after_host = if value.starts_with("https://") {
            &value["https://ampcode.com/".len()..]
        } else {
            &value["http://ampcode.com/".len()..]
        };
        return format!("/api/{}", after_host);
    }
    // Rewrite relative locations to prepend /api/
    if value.starts_with('/') {
        return format!("/api{}", value);
    }
    value.to_string()
}

/// Rewrite Set-Cookie domain from ampcode.com to localhost.
fn rewrite_amp_cookie(value: &str) -> String {
    value
        .replace("Domain=.ampcode.com", "Domain=localhost")
        .replace("Domain=ampcode.com", "Domain=localhost")
}

/// Forward a request to the Vercel AI Gateway.
async fn forward_to_vercel(
    method: &hyper::Method,
    path: &str,
    headers: &hyper::HeaderMap,
    body: &str,
    thinking_enabled: bool,
    api_key: &str,
) -> Result<ForwardOutcome, Box<dyn std::error::Error + Send + Sync>> {
    let client = shared_http_client();
    let url = format!("https://{}{}", VERCEL_GATEWAY_HOST, path);

    let excluded = [
        "host",
        "content-length",
        "connection",
        "transfer-encoding",
        "authorization",
        "x-api-key",
        "anthropic-beta",
    ];

    // Capture existing anthropic-beta header before filtering
    let existing_beta = headers
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let mut fwd_headers = build_forwarding_headers(headers, &excluded);

    // Vercel auth and required headers
    fwd_headers.insert(
        reqwest::header::HeaderName::from_static("x-api-key"),
        reqwest::header::HeaderValue::from_str(api_key)?,
    );
    fwd_headers.insert(
        reqwest::header::HeaderName::from_static("anthropic-version"),
        reqwest::header::HeaderValue::from_static(ANTHROPIC_VERSION),
    );
    fwd_headers.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_static("application/json"),
    );
    fwd_headers.insert(
        reqwest::header::HOST,
        reqwest::header::HeaderValue::from_static(VERCEL_GATEWAY_HOST),
    );

    // Thinking beta header
    if thinking_enabled {
        let beta_value = match &existing_beta {
            Some(existing) if !existing.contains(INTERLEAVED_THINKING_BETA) => {
                format!("{},{}", existing, INTERLEAVED_THINKING_BETA)
            }
            Some(existing) => existing.clone(),
            None => INTERLEAVED_THINKING_BETA.to_string(),
        };
        fwd_headers.insert(
            reqwest::header::HeaderName::from_static("anthropic-beta"),
            reqwest::header::HeaderValue::from_str(&beta_value)?,
        );
    } else if let Some(existing) = &existing_beta {
        fwd_headers.insert(
            reqwest::header::HeaderName::from_static("anthropic-beta"),
            reqwest::header::HeaderValue::from_str(existing)?,
        );
    }

    let reqwest_method = reqwest::Method::from_bytes(method.as_str().as_bytes())?;
    let resp = client
        .request(reqwest_method, &url)
        .headers(fwd_headers)
        .body(body.to_string())
        .send()
        .await?;

    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let resp_body = resp.bytes().await?;

    Ok(ForwardOutcome {
        response: build_proxy_response(status, &resp_headers, resp_body.clone()),
        status_code: status.as_u16(),
        body: resp_body,
    })
}

/// Forward a request to the local backend (CLIProxyAPI) on the target port.
async fn forward_to_backend(
    method: &hyper::Method,
    path: &str,
    headers: &hyper::HeaderMap,
    body: &str,
    thinking_enabled: bool,
    target_port: u16,
) -> Result<ForwardOutcome, Box<dyn std::error::Error + Send + Sync>> {
    let client = shared_http_client();
    let url = format!("http://127.0.0.1:{}{}", target_port, path);

    let excluded = [
        "host",
        "content-length",
        "connection",
        "transfer-encoding",
        "anthropic-beta",
    ];

    // Capture existing anthropic-beta header before filtering
    let existing_beta = headers
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let mut fwd_headers = build_forwarding_headers(headers, &excluded);

    fwd_headers.insert(
        reqwest::header::HOST,
        reqwest::header::HeaderValue::from_str(&format!("127.0.0.1:{}", target_port))?,
    );

    // Add/merge anthropic-beta header when thinking is enabled
    if thinking_enabled {
        let beta_value = match &existing_beta {
            Some(existing) if !existing.contains(INTERLEAVED_THINKING_BETA) => {
                format!("{},{}", existing, INTERLEAVED_THINKING_BETA)
            }
            Some(existing) => existing.clone(),
            None => INTERLEAVED_THINKING_BETA.to_string(),
        };
        fwd_headers.insert(
            reqwest::header::HeaderName::from_static("anthropic-beta"),
            reqwest::header::HeaderValue::from_str(&beta_value)?,
        );
        log::info!("[ThinkingProxy] Added interleaved thinking beta header");
    } else if let Some(existing) = &existing_beta {
        fwd_headers.insert(
            reqwest::header::HeaderName::from_static("anthropic-beta"),
            reqwest::header::HeaderValue::from_str(existing)?,
        );
    }

    let reqwest_method = reqwest::Method::from_bytes(method.as_str().as_bytes())?;
    let resp = client
        .request(reqwest_method, &url)
        .headers(fwd_headers)
        .body(body.to_string())
        .send()
        .await?;

    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let resp_body = resp.bytes().await?;

    Ok(ForwardOutcome {
        response: build_proxy_response(status, &resp_headers, resp_body.clone()),
        status_code: status.as_u16(),
        body: resp_body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_thinking_parameter_claude_with_budget() {
        let body = r#"{"model":"claude-opus-4-5-20251101-thinking-5000","max_tokens":1024}"#;
        let (result, enabled) = process_thinking_parameter(body);
        assert!(enabled);
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(json["model"], "claude-opus-4-5-20251101");
        assert_eq!(json["thinking"]["type"], "enabled");
        assert_eq!(json["thinking"]["budget_tokens"], 5000);
    }

    #[test]
    fn test_process_thinking_parameter_gemini_claude_with_budget() {
        let body = r#"{"model":"gemini-claude-opus-4-5-thinking-10000","max_tokens":1024}"#;
        let (result, enabled) = process_thinking_parameter(body);
        assert!(enabled);
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(json["model"], "gemini-claude-opus-4-5-thinking");
        assert_eq!(json["thinking"]["type"], "enabled");
        assert_eq!(json["thinking"]["budget_tokens"], 10000);
    }

    #[test]
    fn test_process_thinking_parameter_no_suffix() {
        let body = r#"{"model":"claude-opus-4-5-20251101","max_tokens":1024}"#;
        let (result, enabled) = process_thinking_parameter(body);
        assert!(!enabled);
        assert_eq!(result, body);
    }

    #[test]
    fn test_process_thinking_parameter_thinking_only_suffix() {
        let body = r#"{"model":"gemini-claude-opus-4-5-thinking","max_tokens":1024}"#;
        let (result, enabled) = process_thinking_parameter(body);
        assert!(enabled);
        // Body should be unchanged, just beta header enabled
        assert_eq!(result, body);
    }

    #[test]
    fn test_process_thinking_parameter_non_claude_model() {
        let body = r#"{"model":"gpt-4","max_tokens":1024}"#;
        let (result, enabled) = process_thinking_parameter(body);
        assert!(!enabled);
        assert_eq!(result, body);
    }

    #[test]
    fn test_process_thinking_parameter_hard_cap() {
        let body = r#"{"model":"claude-opus-4-5-20251101-thinking-99999","max_tokens":1024}"#;
        let (result, enabled) = process_thinking_parameter(body);
        assert!(enabled);
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(json["thinking"]["budget_tokens"], HARD_TOKEN_CAP - 1);
    }

    #[test]
    fn test_process_thinking_parameter_adjusts_max_tokens() {
        let body = r#"{"model":"claude-sonnet-4-5-20250929-thinking-5000","max_tokens":100}"#;
        let (result, enabled) = process_thinking_parameter(body);
        assert!(enabled);
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        // max_tokens should be bumped since 100 <= 5000
        let max_tokens = json["max_tokens"].as_i64().unwrap();
        assert!(max_tokens > 5000);
    }

    #[test]
    fn test_rewrite_amp_location() {
        assert_eq!(rewrite_amp_location("/foo"), "/api/foo");
        assert_eq!(rewrite_amp_location("https://ampcode.com/bar"), "/api/bar");
        assert_eq!(rewrite_amp_location("http://ampcode.com/baz"), "/api/baz");
        assert_eq!(
            rewrite_amp_location("https://other.com/x"),
            "https://other.com/x"
        );
    }

    #[test]
    fn test_rewrite_amp_cookie() {
        assert_eq!(
            rewrite_amp_cookie("session=abc; Domain=.ampcode.com; Path=/"),
            "session=abc; Domain=localhost; Path=/"
        );
        assert_eq!(
            rewrite_amp_cookie("session=abc; Domain=ampcode.com; Path=/"),
            "session=abc; Domain=localhost; Path=/"
        );
    }

    #[test]
    fn test_is_claude_model_request() {
        assert!(is_claude_model_request(r#"{"model":"claude-opus-4-5"}"#));
        assert!(is_claude_model_request(
            r#"{"model":"gemini-claude-opus-4-5-thinking"}"#
        ));
        assert!(!is_claude_model_request(r#"{"model":"gpt-4"}"#));
        assert!(!is_claude_model_request(r#"{"invalid":"json"}"#));
    }

    #[test]
    fn test_extract_usage_nested_cached_and_reasoning_tokens() {
        let payload = serde_json::json!({
            "usage": {
                "input_tokens": 100,
                "input_tokens_details": {
                    "cached_tokens": 42
                },
                "output_tokens": 50,
                "output_tokens_details": {
                    "reasoning_tokens": 31
                },
                "total_tokens": 150
            }
        });

        let usage = extract_usage_from_json_value(&payload).expect("expected usage");
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.cached_tokens, Some(42));
        assert_eq!(usage.output_tokens, Some(50));
        assert_eq!(usage.reasoning_tokens, Some(31));
        assert_eq!(usage.total_tokens, Some(150));
    }
}
