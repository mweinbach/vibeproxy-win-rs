use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use crate::types::VercelGatewayConfig;

const HARD_TOKEN_CAP: i64 = 32000;
const MINIMUM_HEADROOM: i64 = 1024;
const HEADROOM_RATIO: f64 = 0.1;
const VERCEL_GATEWAY_HOST: &str = "ai-gateway.vercel.sh";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const INTERLEAVED_THINKING_BETA: &str = "interleaved-thinking-2025-05-14";
const HTTP_CONNECT_TIMEOUT_SECS: u64 = 5;
const HTTP_READ_TIMEOUT_SECS: u64 = 90;

pub struct ThinkingProxy {
    pub proxy_port: u16,
    pub target_port: u16,
    pub vercel_config: Arc<RwLock<VercelGatewayConfig>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pub is_running: bool,
}

impl ThinkingProxy {
    pub fn new(vercel_config: Arc<RwLock<VercelGatewayConfig>>) -> Self {
        Self {
            proxy_port: 8317,
            target_port: 8318,
            vercel_config,
            shutdown_tx: None,
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
        let target_port = self.target_port;

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _addr)) => {
                                let io = TokioIo::new(stream);
                                let vc = vercel_config.clone();
                                tokio::spawn(async move {
                                    let svc = service_fn(move |req| {
                                        let vc = vc.clone();
                                        async move {
                                            handle_request(req, vc, target_port).await
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

        Ok(())
    }

    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
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
) -> Result<Response<Full<Bytes>>, hyper::Error> {
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

    // 5. Vercel gateway routing
    let vc = vercel_config.read().await;
    if vc.is_active() && method == hyper::Method::POST && is_claude_model_request(&modified_body) {
        let api_key = vc.api_key.clone();
        drop(vc);
        log::info!("[ThinkingProxy] Routing Claude request via Vercel AI Gateway");
        return Ok(forward_to_vercel(
            &method,
            "/v1/messages",
            &headers,
            &modified_body,
            thinking_enabled,
            &api_key,
        )
        .await
        .unwrap_or_else(|e| {
            log::error!("[ThinkingProxy] Vercel forward error: {}", e);
            make_response(
                StatusCode::BAD_GATEWAY,
                "Bad Gateway - Could not connect to Vercel AI Gateway",
            )
        }));
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
        Ok(resp) => {
            // If 404 and path doesn't start with /api/ or /v1/, retry with /api/ prefix
            if resp.status() == StatusCode::NOT_FOUND
                && !path.starts_with("/api/")
                && !path.starts_with("/v1/")
            {
                let new_path = format!("/api{}", path);
                log::info!(
                    "[ThinkingProxy] Got 404 for {}, retrying with {}",
                    path,
                    new_path
                );
                return Ok(forward_to_backend(
                    &method,
                    &new_path,
                    &headers,
                    &modified_body,
                    thinking_enabled,
                    target_port,
                )
                .await
                .unwrap_or_else(|e| {
                    log::error!("[ThinkingProxy] Backend retry error: {}", e);
                    make_response(StatusCode::BAD_GATEWAY, "Bad Gateway")
                }));
            }
            Ok(resp)
        }
        Err(e) => {
            log::error!("[ThinkingProxy] Backend forward error: {}", e);
            Ok(make_response(StatusCode::BAD_GATEWAY, "Bad Gateway"))
        }
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
) -> Result<Response<Full<Bytes>>, Box<dyn std::error::Error + Send + Sync>> {
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

    Ok(build_proxy_response(status, &resp_headers, resp_body))
}

/// Forward a request to the local backend (CLIProxyAPI) on the target port.
async fn forward_to_backend(
    method: &hyper::Method,
    path: &str,
    headers: &hyper::HeaderMap,
    body: &str,
    thinking_enabled: bool,
    target_port: u16,
) -> Result<Response<Full<Bytes>>, Box<dyn std::error::Error + Send + Sync>> {
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

    Ok(build_proxy_response(status, &resp_headers, resp_body))
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
}
