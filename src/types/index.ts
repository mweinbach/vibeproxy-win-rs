export type ServiceType =
  | "claude"
  | "codex"
  | "github-copilot"
  | "gemini"
  | "qwen"
  | "antigravity"
  | "zai";

export interface AuthAccount {
  id: string;
  email: string | null;
  login: string | null;
  service_type: ServiceType;
  expired: string | null;
  is_expired: boolean;
  file_path: string;
  display_name: string;
}

export interface ServiceAccounts {
  service_type: ServiceType;
  accounts: AuthAccount[];
  active_count: number;
  expired_count: number;
}

export interface ServerState {
  is_running: boolean;
  proxy_port: number;
  backend_port: number;
  binary_available: boolean;
  binary_downloading: boolean;
}

export interface AppSettings {
  enabled_providers: Record<string, boolean>;
  vercel_gateway_enabled: boolean;
  vercel_api_key: string;
  launch_at_login: boolean;
}

export interface AuthCommand {
  type: string;
  data?: { email: string };
}

export interface BinaryDownloadProgress {
  progress: number;
  bytes_downloaded: number;
  total_bytes: number;
}

export type UsageRange = "24h" | "7d" | "30d" | "all";

export interface UsageSummary {
  total_requests: number;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cached_tokens: number;
  reasoning_tokens: number;
  error_count: number;
  error_rate: number;
}

export interface UsageTimeseriesPoint {
  bucket: string;
  requests: number;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cached_tokens: number;
  reasoning_tokens: number;
  error_count: number;
}

export interface UsageBreakdownRow {
  provider: string;
  model: string;
  account_key: string;
  account_label: string;
  requests: number;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cached_tokens: number;
  reasoning_tokens: number;
  error_count: number;
  last_seen: string | null;
}

export interface UsageDashboard {
  range: UsageRange;
  summary: UsageSummary;
  timeseries: UsageTimeseriesPoint[];
  breakdown: UsageBreakdownRow[];
}

export interface UsageDashboardPayload {
  dashboard: UsageDashboard;
}

export const SERVICE_DISPLAY_NAMES: Record<ServiceType, string> = {
  claude: "Claude Code",
  codex: "Codex",
  "github-copilot": "GitHub Copilot",
  gemini: "Gemini",
  qwen: "Qwen",
  antigravity: "Antigravity",
  zai: "Z.AI GLM",
};

export const SERVICE_ORDER: ServiceType[] = [
  "antigravity",
  "claude",
  "codex",
  "gemini",
  "github-copilot",
  "qwen",
  "zai",
];

export const PROVIDER_KEYS: Record<ServiceType, string> = {
  claude: "claude",
  codex: "codex",
  "github-copilot": "github-copilot",
  gemini: "gemini",
  qwen: "qwen",
  antigravity: "antigravity",
  zai: "zai",
};

export const SERVICE_ICONS: Record<ServiceType, string> = {
  claude: "icon-claude.png",
  codex: "icon-codex.png",
  "github-copilot": "icon-copilot.png",
  gemini: "icon-gemini.png",
  qwen: "icon-qwen.png",
  antigravity: "icon-antigravity.png",
  zai: "icon-zai.png",
};

// ---------------------------------------------------------------------------
// Models / Agents
// ---------------------------------------------------------------------------

export interface ThinkingSupport {
  min?: number;
  max?: number;
  zero_allowed?: boolean;
  dynamic_allowed?: boolean;
  levels?: string[];
}

export interface ProviderModelInfo {
  id: string;
  object?: string;
  created?: number;
  owned_by?: string;
  model_type?: string;
  display_name?: string;
  version?: string;
  description?: string;
  context_length?: number;
  max_completion_tokens?: number;
  supported_parameters?: string[];
  supported_endpoints?: string[];
  thinking?: ThinkingSupport;
}

export interface ProviderModelDefinitionsResponse {
  channel: string;
  models: ProviderModelInfo[];
}

export interface FactoryCustomModelInput {
  model: string;
  baseUrl: string;
  apiKey: string;
  displayName: string;
  noImageSupport: boolean;
  provider: string;
}

export interface FactoryCustomModelRow {
  id: string;
  index?: number;
  model: string;
  baseUrl: string;
  displayName: string;
  noImageSupport: boolean;
  provider: string;
  isProxy: boolean;
  isSessionDefault: boolean;
}

export interface FactoryCustomModelsState {
  factorySettingsPath: string;
  sessionDefaultModel?: string;
  models: FactoryCustomModelRow[];
}

export interface FactoryCustomModelsRemoveResult {
  removed: number;
  skippedNonProxy: number;
  skippedNotFound: number;
  factorySettingsPath: string;
}

export interface AgentInstallResult {
  agent_key: string;
  total_requested: number;
  added: number;
  skipped_duplicates: number;
  skipped_invalid: number;
  factory_settings_path: string;
}
