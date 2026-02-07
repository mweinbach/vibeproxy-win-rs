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
