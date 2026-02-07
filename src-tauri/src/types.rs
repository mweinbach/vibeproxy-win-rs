use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceType {
    Claude,
    Codex,
    #[serde(rename = "github-copilot")]
    Copilot,
    Gemini,
    Qwen,
    Antigravity,
    Zai,
}

impl ServiceType {
    pub fn provider_key(&self) -> &'static str {
        match self {
            ServiceType::Claude => "claude",
            ServiceType::Codex => "codex",
            ServiceType::Copilot => "github-copilot",
            ServiceType::Gemini => "gemini",
            ServiceType::Antigravity => "antigravity",
            ServiceType::Qwen => "qwen",
            ServiceType::Zai => "zai",
        }
    }

    pub fn all() -> &'static [ServiceType] {
        &[
            ServiceType::Claude,
            ServiceType::Codex,
            ServiceType::Copilot,
            ServiceType::Gemini,
            ServiceType::Qwen,
            ServiceType::Antigravity,
            ServiceType::Zai,
        ]
    }

    pub fn from_str_loose(s: &str) -> Option<ServiceType> {
        match s.to_lowercase().as_str() {
            "claude" => Some(ServiceType::Claude),
            "codex" => Some(ServiceType::Codex),
            "github-copilot" | "copilot" => Some(ServiceType::Copilot),
            "gemini" => Some(ServiceType::Gemini),
            "qwen" => Some(ServiceType::Qwen),
            "antigravity" => Some(ServiceType::Antigravity),
            "zai" => Some(ServiceType::Zai),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthAccount {
    pub id: String,
    pub email: Option<String>,
    pub login: Option<String>,
    pub service_type: ServiceType,
    pub expired: Option<String>,
    pub is_expired: bool,
    pub file_path: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccounts {
    pub service_type: ServiceType,
    pub accounts: Vec<AuthAccount>,
    pub active_count: usize,
    pub expired_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerState {
    pub is_running: bool,
    pub proxy_port: u16,
    pub backend_port: u16,
    pub binary_available: bool,
    pub binary_downloading: bool,
}

impl Default for ServerState {
    fn default() -> Self {
        Self {
            is_running: false,
            proxy_port: 8317,
            backend_port: 8318,
            binary_available: false,
            binary_downloading: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub enabled_providers: HashMap<String, bool>,
    pub vercel_gateway_enabled: bool,
    pub vercel_api_key: String,
    pub launch_at_login: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            enabled_providers: HashMap::new(),
            vercel_gateway_enabled: false,
            vercel_api_key: String::new(),
            launch_at_login: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AuthCommand {
    #[serde(rename = "claude")]
    ClaudeLogin,
    #[serde(rename = "codex")]
    CodexLogin,
    #[serde(rename = "github-copilot")]
    CopilotLogin,
    #[serde(rename = "gemini")]
    GeminiLogin,
    #[serde(rename = "qwen")]
    QwenLogin { email: String },
    #[serde(rename = "antigravity")]
    AntigravityLogin,
}

#[derive(Debug, Clone)]
pub struct VercelGatewayConfig {
    pub enabled: bool,
    pub api_key: String,
}

impl VercelGatewayConfig {
    pub fn is_active(&self) -> bool {
        self.enabled && !self.api_key.is_empty()
    }
}

impl Default for VercelGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryDownloadProgress {
    pub progress: f64,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageSummary {
    pub total_requests: i64,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub error_count: i64,
    pub error_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageTimeseriesPoint {
    pub bucket: String,
    pub requests: i64,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub error_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageBreakdownRow {
    pub provider: String,
    pub model: String,
    pub account_key: String,
    pub account_label: String,
    pub requests: i64,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub error_count: i64,
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VibeUsageDashboard {
    pub range: String,
    pub summary: UsageSummary,
    pub timeseries: Vec<UsageTimeseriesPoint>,
    pub breakdown: Vec<UsageBreakdownRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeUsageSummary {
    pub total_requests: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeUsageRow {
    pub source: String,
    pub model: String,
    pub auth_index: Option<String>,
    pub requests: i64,
    pub tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeUsagePanel {
    pub status: String,
    pub effective_range: String,
    pub message: Option<String>,
    pub summary: Option<NativeUsageSummary>,
    pub rows: Vec<NativeUsageRow>,
    pub last_synced_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageDashboardPayload {
    pub vibe: VibeUsageDashboard,
    pub native: NativeUsagePanel,
}
