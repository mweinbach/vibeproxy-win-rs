import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useServerState } from "../hooks/useServerState";
import { useAuthAccounts } from "../hooks/useAuthAccounts";
import { useSettings } from "../hooks/useSettings";
import type { ServiceType } from "../types";
import { SERVICE_ORDER, PROVIDER_KEYS } from "../types";

import ServerStatus from "./ServerStatus";
import ServiceRow from "./ServiceRow";
import VercelGatewayControls from "./VercelGatewayControls";
import QwenEmailDialog from "./QwenEmailDialog";
import ZaiApiKeyDialog from "./ZaiApiKeyDialog";
import Footer from "./Footer";

import glyphLight from "../assets/icons/light/glyph.png";
import iconAntigravityLight from "../assets/icons/light/icon-antigravity.png";
import iconClaudeLight from "../assets/icons/light/icon-claude.png";
import iconCodexLight from "../assets/icons/light/icon-codex.png";
import iconGeminiLight from "../assets/icons/light/icon-gemini.png";
import iconCopilotLight from "../assets/icons/light/icon-copilot.png";
import iconQwenLight from "../assets/icons/light/icon-qwen.png";
import iconZaiLight from "../assets/icons/light/icon-zai.png";

import glyphDark from "../assets/icons/dark/glyph.png";
import iconAntigravityDark from "../assets/icons/dark/icon-antigravity.png";
import iconClaudeDark from "../assets/icons/dark/icon-claude.png";
import iconCodexDark from "../assets/icons/dark/icon-codex.png";
import iconGeminiDark from "../assets/icons/dark/icon-gemini.png";
import iconCopilotDark from "../assets/icons/dark/icon-copilot.png";
import iconQwenDark from "../assets/icons/dark/icon-qwen.png";
import iconZaiDark from "../assets/icons/dark/icon-zai.png";

type ThemeMode = "light" | "dark";

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

const GLYPH_MAP: Record<ThemeMode, string> = {
  light: glyphLight,
  dark: glyphDark,
};

function getInitialThemeMode(): ThemeMode {
  if (typeof window === "undefined") {
    return "light";
  }

  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export default function SettingsView() {
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

  const [showQwenDialog, setShowQwenDialog] = useState(false);
  const [showZaiDialog, setShowZaiDialog] = useState(false);
  const [themeMode, setThemeMode] = useState<ThemeMode>(getInitialThemeMode);
  const operationalError = serverError ?? settingsError ?? accountsError;

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

  if (!serverState || !settings) {
    return (
      <div className="settings-loading">
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
  const dismissOperationalError = () => {
    clearServerError();
    clearSettingsError();
    clearAccountsError();
  };

  return (
    <div className="settings-view">
      <div className="settings-scroll">
        <header className="app-hero">
          <img src={GLYPH_MAP[themeMode]} alt="VibeProxy" className="app-hero-icon" />
          <div className="app-hero-copy">
            <h1 className="app-hero-title">VibeProxy</h1>
            <p className="app-hero-subtitle">
              Manage proxy uptime and account routing from one clean dashboard.
            </p>
          </div>
          <div className={`hero-server-pill ${serverState.is_running ? "running" : "stopped"}`}>
            {serverState.is_running ? "Server online" : "Server offline"}
          </div>
        </header>
        {operationalError ? (
          <div className="operation-error-banner" role="alert">
            <p className="operation-error-message">{operationalError}</p>
            <button
              type="button"
              className="btn btn-sm"
              onClick={dismissOperationalError}
            >
              Dismiss
            </button>
          </div>
        ) : null}

        <div className="stats-grid">
          <div className="stat-card">
            <span className="stat-label">Enabled services</span>
            <span className="stat-value">{enabledServiceCount}</span>
          </div>
          <div className="stat-card">
            <span className="stat-label">Active accounts</span>
            <span className="stat-value">{activeAccounts}</span>
          </div>
          <div className="stat-card">
            <span className="stat-label">Expired accounts</span>
            <span className="stat-value">{expiredAccounts}</span>
          </div>
        </div>

        <section className="settings-section">
          <div className="section-header">
            <h2 className="section-title">Server</h2>
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

        <section className="settings-section">
          <div className="section-header">
            <h2 className="section-title">General</h2>
            <p className="section-description">Desktop behavior and local file access.</p>
          </div>
          <div className="setting-row">
            <div className="setting-label">
              <span>Launch at login</span>
              <small>Start VibeProxy with Windows.</small>
            </div>
            <label className="toggle-switch">
              <input
                type="checkbox"
                checked={settings.launch_at_login}
                onChange={(e) => setLaunchAtLogin(e.target.checked)}
              />
              <span className="toggle-slider" />
            </label>
          </div>
          <div className="setting-row">
            <div className="setting-label">
              <span>Auth files</span>
              <small>Open the folder where account files are stored.</small>
            </div>
            <button
              className="btn btn-sm"
              type="button"
              onClick={() => invoke("open_auth_folder")}
            >
              Open Folder
            </button>
          </div>
        </section>

        <section className="settings-section">
          <div className="section-header">
            <h2 className="section-title">Services</h2>
            <p className="section-description">Enable providers and manage connected accounts.</p>
          </div>
          {authResult ? (
            <div
              className={`auth-result-banner ${authResult.success ? "success" : "error"}`}
              role="status"
              aria-live="polite"
            >
              <p className="auth-result-message">{authResult.message}</p>
            </div>
          ) : null}
          <div className="service-list">
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

      <Footer />

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
