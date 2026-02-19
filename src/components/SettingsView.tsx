import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  LayoutDashboard, 
  Cloud, 
  PieChart,
  Boxes,
  Bot,
  Settings as SettingsIcon, 
  FolderOpen,
  Power,
  Server,
  AlertCircle
} from "lucide-react";
import { useServerState } from "../hooks/useServerState";
import { useAuthAccounts } from "../hooks/useAuthAccounts";
import { useSettings } from "../hooks/useSettings";
import { useUsageDashboard } from "../hooks/useUsageDashboard";
import type { ServiceType } from "../types";
import { SERVICE_ORDER, PROVIDER_KEYS } from "../types";

import ServerStatus from "./ServerStatus";
import ServiceRow from "./ServiceRow";
import VercelGatewayControls from "./VercelGatewayControls";
import QwenEmailDialog from "./QwenEmailDialog";
import ZaiApiKeyDialog from "./ZaiApiKeyDialog";
import TitleBar from "./TitleBar";
import UsageDashboard from "./UsageDashboard";
import ModelsTab from "./ModelsTab";
import AgentsTab from "./AgentsTab";
import TabHeader from "./TabHeader";
import { useUpdater } from "../hooks/useUpdater";

import iconAntigravityLight from "../assets/icons/light/icon-antigravity.png";
import iconClaudeLight from "../assets/icons/light/icon-claude.png";
import iconCodexLight from "../assets/icons/light/icon-codex.png";
import iconGeminiLight from "../assets/icons/light/icon-gemini.png";
import iconCopilotLight from "../assets/icons/light/icon-copilot.png";
import iconQwenLight from "../assets/icons/light/icon-qwen.png";
import iconZaiLight from "../assets/icons/light/icon-zai.png";

import iconAntigravityDark from "../assets/icons/dark/icon-antigravity.png";
import iconClaudeDark from "../assets/icons/dark/icon-claude.png";
import iconCodexDark from "../assets/icons/dark/icon-codex.png";
import iconGeminiDark from "../assets/icons/dark/icon-gemini.png";
import iconCopilotDark from "../assets/icons/dark/icon-copilot.png";
import iconQwenDark from "../assets/icons/dark/icon-qwen.png";
import iconZaiDark from "../assets/icons/dark/icon-zai.png";

type ThemeMode = "light" | "dark";
type TabKey =
  | "dashboard"
  | "usage"
  | "services"
  | "models"
  | "agents"
  | "settings";

const TAB_ITEMS: Array<{
  key: TabKey;
  label: string;
  description: string;
  icon: typeof LayoutDashboard;
  group: "overview" | "configuration";
}> = [
  {
    key: "dashboard",
    label: "Dashboard",
    description: "Runtime health and account readiness at a glance.",
    icon: LayoutDashboard,
    group: "overview",
  },
  {
    key: "usage",
    label: "Usage",
    description: "Requests, token trends, and provider distribution.",
    icon: PieChart,
    group: "overview",
  },
  {
    key: "services",
    label: "Services",
    description: "Enable providers and manage connected identities.",
    icon: Cloud,
    group: "configuration",
  },
  {
    key: "models",
    label: "Models",
    description: "Browse model catalogs from your local runtime.",
    icon: Boxes,
    group: "configuration",
  },
  {
    key: "agents",
    label: "Custom Models",
    description: "Manage Factory custom models powered by CodeForwarder.",
    icon: Bot,
    group: "configuration",
  },
  {
    key: "settings",
    label: "Settings",
    description: "Desktop behavior and local file controls.",
    icon: SettingsIcon,
    group: "configuration",
  },
];

const SERVICE_ICON_MAP: Record<ThemeMode, Record<ServiceType, string>> = {
  light: {
    antigravity: iconAntigravityLight,
    claude: iconClaudeLight,
    codex: iconCodexLight,
    gemini: iconGeminiLight,
    "github-copilot": iconCopilotLight,
    qwen: iconQwenLight,
    zai: iconZaiLight,
  },
  dark: {
    antigravity: iconAntigravityDark,
    claude: iconClaudeDark,
    codex: iconCodexDark,
    gemini: iconGeminiDark,
    "github-copilot": iconCopilotDark,
    qwen: iconQwenDark,
    zai: iconZaiDark,
  },
};

function isTauriRuntime(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  return "__TAURI_INTERNALS__" in window;
}

function isMacOS(): boolean {
  if (typeof navigator === "undefined") {
    return false;
  }
  return /Macintosh|Mac OS X/.test(navigator.userAgent);
}

function getInitialThemeMode(): ThemeMode {
  if (typeof window === "undefined") {
    return "light";
  }

  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export default function SettingsView() {
  return useSettingsView();
}

function useSettingsView() {
  const {
    serverState,
    downloadProgress,
    startServer,
    stopServer,
    downloadBinary,
    lastError: serverError,
    clearLastError: clearServerError,
  } = useServerState();
  const {
    accounts,
    authenticatingService,
    authResult,
    runAuth,
    deleteAccount,
    saveZaiKey,
    lastError: accountsError,
    clearLastError: clearAccountsError,
  } = useAuthAccounts();
  const {
    settings,
    setProviderEnabled,
    setVercelConfig,
    setLaunchAtLogin,
    lastError: settingsError,
    clearLastError: clearSettingsError,
  } = useSettings();
  const {
    status: updateStatus,
    lastCheckedAt: updateLastCheckedAt,
    availableVersion,
    progressPercent,
    lastError: updateError,
    checkForUpdates,
  } = useUpdater();

  const [showQwenDialog, setShowQwenDialog] = useState(false);
  const [showZaiDialog, setShowZaiDialog] = useState(false);
  const [themeMode, setThemeMode] = useState<ThemeMode>(getInitialThemeMode);
  const [activeTab, setActiveTab] = useState<TabKey>("dashboard");
  const settingsScrollRef = useRef<HTMLElement | null>(null);
  const {
    range: usageRange,
    setRange: setUsageRange,
    dashboard: usageDashboard,
    isLoading: usageLoading,
    lastError: usageError,
    refresh: refreshUsage,
    clearLastError: clearUsageError,
  } = useUsageDashboard(activeTab === "usage");
  const operationalError = serverError ?? settingsError ?? accountsError;

  const updateStatusLabel = (() => {
    if (updateStatus === "checking") return "Checking...";
    if (updateStatus === "unavailable") return "Update server unavailable.";
    if (updateStatus === "downloading") {
      if (progressPercent !== null) return `Downloading (${progressPercent}%)...`;
      return "Downloading...";
    }
    if (updateStatus === "ready_to_restart") return "Update ready. Restart to apply.";
    if (updateStatus === "up_to_date") return "Up to date.";
    if (updateStatus === "error") return "Update failed.";
    return "Idle.";
  })();

  const updateCheckedAtLabel = (() => {
    if (!updateLastCheckedAt) return "Never checked.";
    try {
      return `Last checked: ${new Date(updateLastCheckedAt).toLocaleString()}`;
    } catch {
      return "Last checked: unknown";
    }
  })();

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const handleChange = (event: MediaQueryListEvent) => {
      setThemeMode(event.matches ? "dark" : "light");
    };

    setThemeMode(media.matches ? "dark" : "light");
    media.addEventListener("change", handleChange);

    return () => {
      media.removeEventListener("change", handleChange);
    };
  }, []);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", themeMode);
  }, [themeMode]);

  useEffect(() => {
    if (!settingsScrollRef.current) {
      return;
    }

    settingsScrollRef.current.scrollTop = 0;
  }, [activeTab]);

  if (!serverState || !settings) {
    return (
      <div className="settings-loading flex h-full w-full items-center justify-center gap-2 text-[color:var(--text-secondary)]">
        <span className="spinner" />
        <span>Loading settings...</span>
      </div>
    );
  }

  const handleStartStop = () => {
    if (serverState.is_running) {
      stopServer();
    } else {
      startServer();
    }
  };

  const handleConnect = (serviceType: ServiceType) => {
    if (serviceType === "qwen") {
      setShowQwenDialog(true);
    } else if (serviceType === "zai") {
      setShowZaiDialog(true);
    } else {
      runAuth({ type: serviceType });
    }
  };

  const handleQwenSubmit = (email: string) => {
    setShowQwenDialog(false);
    runAuth({ type: "qwen", data: { email } });
  };

  const handleZaiSubmit = (apiKey: string) => {
    setShowZaiDialog(false);
    saveZaiKey(apiKey);
  };

  const getCustomTitle = (serviceType: ServiceType): string | undefined => {
    if (
      serviceType === "claude" &&
      settings.vercel_gateway_enabled &&
      settings.vercel_api_key !== ""
    ) {
      return "Claude Code (via Vercel)";
    }
    return undefined;
  };

  const getAccounts = (serviceType: ServiceType) => {
    if (!accounts) return [];
    const sa = accounts[serviceType];
    return sa ? sa.accounts : [];
  };

  const isProviderEnabled = (serviceType: ServiceType) => {
    const key = PROVIDER_KEYS[serviceType];
    return settings.enabled_providers[key] ?? true;
  };

  const enabledServiceCount = SERVICE_ORDER.filter((serviceType) =>
    isProviderEnabled(serviceType)
  ).length;
  const totalAccounts = SERVICE_ORDER.reduce(
    (count, serviceType) => count + getAccounts(serviceType).length,
    0,
  );
  const expiredAccounts = SERVICE_ORDER.reduce(
    (count, serviceType) =>
      count + getAccounts(serviceType).filter((account) => account.is_expired).length,
    0,
  );
  const activeAccounts = totalAccounts - expiredAccounts;

  const overviewTabs = TAB_ITEMS.filter((item) => item.group === "overview");
  const configurationTabs = TAB_ITEMS.filter(
    (item) => item.group === "configuration",
  );

  const dismissOperationalError = () => {
    clearServerError();
    clearSettingsError();
    clearAccountsError();
  };

  const useNativeMacWindowChrome = isTauriRuntime() && isMacOS();

  return (
    <div className="settings-view grid h-full w-full overflow-hidden">
      {!useNativeMacWindowChrome ? <TitleBar /> : null}
      <aside className="sidebar flex min-w-0 flex-col border-r border-[color:var(--border)]">
        <div className="sidebar-header flex items-center gap-2" data-tauri-drag-region>
          <div>
            <p className="sidebar-eyebrow text-[10px] font-semibold tracking-[0.08em] text-[color:var(--text-muted)] uppercase">Control Center</p>
            <span className="sidebar-title">CodeForwarder</span>
          </div>
        </div>

        <nav className="sidebar-nav flex flex-col gap-0.5">
          <p className="sidebar-group-label">Overview</p>
          {overviewTabs.map((item) => {
            const Icon = item.icon;
            return (
              <button
                key={item.key}
                className={`sidebar-item inline-flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-sm font-medium transition ${activeTab === item.key ? "active" : ""}`}
                onClick={() => setActiveTab(item.key)}
              >
                <Icon className="sidebar-icon h-4 w-4 shrink-0" />
                {item.label}
              </button>
            );
          })}

          <p className="sidebar-group-label">Configure</p>
          {configurationTabs.map((item) => {
            const Icon = item.icon;
            return (
              <button
                key={item.key}
                className={`sidebar-item inline-flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-sm font-medium transition ${activeTab === item.key ? "active" : ""}`}
                onClick={() => setActiveTab(item.key)}
              >
                <Icon className="sidebar-icon h-4 w-4 shrink-0" />
                {item.label}
              </button>
            );
          })}
        </nav>

        <div className="sidebar-footer mt-auto flex flex-col gap-1.5">
          <div className={`status-pill inline-flex w-fit items-center gap-1.5 ${serverState.is_running ? "running" : "stopped"}`}>
            <Power className="status-icon h-3 w-3 shrink-0" size={12} />
            {serverState.is_running ? "Online" : "Offline"}
          </div>
          <p className="sidebar-runtime-meta text-xs text-[color:var(--text-muted)]">
            {enabledServiceCount} services · {activeAccounts} accounts
          </p>
        </div>
      </aside>

      <section className="app-shell min-w-0">
        {useNativeMacWindowChrome ? (
          <div className="macos-drag-strip" data-tauri-drag-region aria-hidden="true" />
        ) : null}
        <main className="settings-scroll" ref={settingsScrollRef}>
          {activeTab === "dashboard" && (
            <div className="tab-content animate-in flex flex-col gap-6 pb-6">
              <TabHeader
                title="Dashboard"
                subtitle="Runtime health and account readiness at a glance."
              />

              {operationalError ? (
                <div className="operation-error-banner flex items-center gap-3 rounded-md border border-[color:var(--danger)]/20" role="alert">
                  <AlertCircle size={16} className="error-icon h-4 w-4 shrink-0 text-[color:var(--danger)]" />
                  <p className="operation-error-message flex-1">{operationalError}</p>
                  <button
                    type="button"
                    className="btn btn-sm"
                    onClick={dismissOperationalError}
                  >
                    Dismiss
                  </button>
                </div>
              ) : null}

              <div className="stats-grid grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
                <div className="stat-item">
                  <span className="stat-label">Services</span>
                  <span className="stat-value">{enabledServiceCount}</span>
                </div>
                <div className="stat-item">
                  <span className="stat-label">Active</span>
                  <span className="stat-value">{activeAccounts}</span>
                </div>
                <div className="stat-item">
                  <span className="stat-label">Expired</span>
                  <span className="stat-value">{expiredAccounts}</span>
                </div>
              </div>

              <section className="settings-section">
                <div className="section-header">
                  <div className="section-title-row flex items-center gap-2 text-[color:var(--text-muted)]">
                    <Server size={14} />
                    <h2 className="section-title">Proxy Engine</h2>
                  </div>
                  <p className="section-description">
                    Control local proxy runtime and bundled binary readiness.
                  </p>
                </div>
                <ServerStatus
                  isRunning={serverState.is_running}
                  binaryAvailable={serverState.binary_available}
                  binaryDownloading={serverState.binary_downloading}
                  downloadProgress={downloadProgress?.progress ?? null}
                  onStartStop={handleStartStop}
                  onDownloadBinary={downloadBinary}
                />
              </section>
            </div>
          )}

        {activeTab === "usage" && (
          <div className="tab-content animate-in flex flex-col gap-6 pb-6">
            <UsageDashboard
              dashboard={usageDashboard}
              range={usageRange}
              onRangeChange={setUsageRange}
              onRefresh={refreshUsage}
              isLoading={usageLoading}
              error={usageError}
              onDismissError={clearUsageError}
            />
          </div>
        )}

        {activeTab === "services" && (
          <div className="tab-content animate-in flex flex-col gap-6 pb-6">
            <TabHeader
              title="Services"
              subtitle="Enable providers and manage connected accounts."
            />
            {authResult ? (
              <div
                className={`auth-result-banner rounded-md border ${authResult.success ? "success border-[color:var(--ok)]/25" : "error border-[color:var(--danger)]/20"}`}
                role="status"
                aria-live="polite"
              >
                <p className="auth-result-message">{authResult.message}</p>
              </div>
            ) : null}
            <section className="settings-section">
              <div className="service-list divide-y divide-[color:var(--border)]">
                {SERVICE_ORDER.map((serviceType) => (
                  <ServiceRow
                    key={serviceType}
                    serviceType={serviceType}
                    accounts={getAccounts(serviceType)}
                    isEnabled={isProviderEnabled(serviceType)}
                    isAuthenticating={authenticatingService === serviceType}
                    onConnect={() => handleConnect(serviceType)}
                    onDisconnect={(filePath) => deleteAccount(filePath)}
                    onToggleEnabled={(enabled) =>
                      setProviderEnabled(PROVIDER_KEYS[serviceType], enabled)
                    }
                    icon={SERVICE_ICON_MAP[themeMode][serviceType]}
                    customTitle={getCustomTitle(serviceType)}
                  >
                    {serviceType === "claude" ? (
                      <VercelGatewayControls
                        enabled={settings.vercel_gateway_enabled}
                        apiKey={settings.vercel_api_key}
                        onSave={setVercelConfig}
                      />
                    ) : undefined}
                  </ServiceRow>
                ))}
              </div>
            </section>
          </div>
        )}

        {activeTab === "models" && <ModelsTab />}

        {activeTab === "agents" && <AgentsTab />}

        {activeTab === "settings" && (
          <div className="tab-content animate-in flex flex-col gap-6 pb-6">
            <TabHeader
              title="Settings"
              subtitle="Desktop behavior and local file access."
            />
            <section className="settings-section">
              <div className="setting-row flex items-center justify-between gap-4">
                <div className="setting-label min-w-0 flex-1">
                  <span>App updates</span>
                  <small>
                    {updateStatusLabel}{" "}
                    {availableVersion ? `Available: ${availableVersion}.` : ""}
                    {updateError ? ` ${updateError}` : ""}
                  </small>
                  <small>{updateCheckedAtLabel}</small>
                </div>
                <div className="button-row flex items-center justify-end gap-2">
                  {updateStatus === "ready_to_restart" ? (
                    <button
                      className="btn btn-sm"
                      type="button"
                      onClick={() => relaunch()}
                    >
                      Restart to apply
                    </button>
                  ) : null}
                  <button
                    className="btn btn-sm"
                    type="button"
                    onClick={() => checkForUpdates({ manual: true })}
                    disabled={updateStatus === "checking" || updateStatus === "downloading"}
                  >
                    Check for updates
                  </button>
                </div>
              </div>
              <div className="setting-row flex items-center justify-between gap-4">
                <div className="setting-label min-w-0 flex-1">
                  <span>Launch at login</span>
                  <small>Start CodeForwarder automatically.</small>
                </div>
                <label className="toggle-switch" aria-label="Launch at login">
                  <input
                    type="checkbox"
                    checked={settings.launch_at_login}
                    onChange={(e) => setLaunchAtLogin(e.target.checked)}
                  />
                  <span className="toggle-slider" />
                </label>
              </div>
              <div className="setting-row flex items-center justify-between gap-4">
                <div className="setting-label min-w-0 flex-1">
                  <span>Auth files</span>
                  <small>Open the folder where account files are stored.</small>
                </div>
                <button
                  className="btn btn-sm"
                  type="button"
                  onClick={() => invoke("open_auth_folder")}
                >
                  <FolderOpen size={14} />
                  Open Folder
                </button>
              </div>
            </section>
          </div>
        )}
        </main>
      </section>

      <QwenEmailDialog
        isOpen={showQwenDialog}
        onClose={() => setShowQwenDialog(false)}
        onSubmit={handleQwenSubmit}
      />
      <ZaiApiKeyDialog
        isOpen={showZaiDialog}
        onClose={() => setShowZaiDialog(false)}
        onSubmit={handleZaiSubmit}
      />
    </div>
  );
}
