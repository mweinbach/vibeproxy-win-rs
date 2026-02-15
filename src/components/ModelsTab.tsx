import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bot, RefreshCw } from "lucide-react";
import type { ProviderModelDefinitionsResponse, ProviderModelInfo, AgentInstallResult } from "../types";
import { toErrorMessage } from "../utils/error";
import AgentModelInstallDialog from "./AgentModelInstallDialog";

const PROVIDER_CHANNELS: Array<{ key: string; label: string }> = [
  { key: "claude", label: "Claude" },
  { key: "codex", label: "Codex" },
  { key: "gemini", label: "Gemini" },
  { key: "qwen", label: "Qwen" },
  { key: "github-copilot", label: "GitHub Copilot" },
  { key: "antigravity", label: "Antigravity" },
];

function hasThinkingLevels(model: ProviderModelInfo): string[] {
  const levels = model.thinking?.levels;
  return Array.isArray(levels) ? levels.filter((v) => typeof v === "string" && v.trim() !== "") : [];
}

function formatThinkingSummary(model: ProviderModelInfo): string {
  const levels = hasThinkingLevels(model);
  if (levels.length > 0) {
    return `Levels: ${levels.join(", ")}`;
  }
  if (model.thinking?.min != null || model.thinking?.max != null) {
    const min = model.thinking?.min != null ? String(model.thinking.min) : "?";
    const max = model.thinking?.max != null ? String(model.thinking.max) : "?";
    return `Budget: ${min}-${max}`;
  }
  return "-";
}

function supportsThinking(model: ProviderModelInfo): boolean {
  if (hasThinkingLevels(model).length > 0) return true;
  return model.thinking?.min != null || model.thinking?.max != null;
}

export default function ModelsTab() {
  const [channel, setChannel] = useState("claude");
  const [search, setSearch] = useState("");
  const [modelsResponse, setModelsResponse] = useState<ProviderModelDefinitionsResponse | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [lastError, setLastError] = useState<string | null>(null);

  const [showInstallDialog, setShowInstallDialog] = useState(false);
  const [lastInstallResult, setLastInstallResult] = useState<AgentInstallResult | null>(null);

  const refresh = async () => {
    setIsLoading(true);
    setLastError(null);
    try {
      const resp = await invoke<ProviderModelDefinitionsResponse>("get_provider_model_definitions", {
        channel,
      });
      setModelsResponse(resp);
    } catch (err) {
      setModelsResponse(null);
      setLastError(toErrorMessage(err, "Failed to load models (make sure Proxy Engine is running)"));
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [channel]);

  const allModels = modelsResponse?.models ?? [];
  const thinkingReadyCount = useMemo(
    () => allModels.filter((model) => supportsThinking(model)).length,
    [allModels],
  );

  const filteredModels = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (q === "") return allModels;
    return allModels.filter((m) => {
      const id = m.id.toLowerCase();
      const dn = (m.display_name ?? "").toLowerCase();
      return id.includes(q) || dn.includes(q);
    });
  }, [allModels, search]);

  return (
    <div className="tab-content animate-in">
      <h1 className="page-title">Models</h1>
      <p className="page-subtitle">
        Browse runtime model catalogs and install selections into Custom Models.
      </p>

      {lastInstallResult ? (
        <div className="auth-result-banner success" role="status" aria-live="polite">
          <p className="auth-result-message">
            Installed for {lastInstallResult.agent_key}: added {lastInstallResult.added}, skipped {lastInstallResult.skipped_duplicates} duplicates.
          </p>
        </div>
      ) : null}

      {lastError ? (
        <div className="auth-result-banner error" role="alert">
          <p className="auth-result-message">{lastError}</p>
        </div>
      ) : null}

      <section className="settings-section">
        <div className="models-toolbar">
          <label className="models-field">
            <span className="models-label">Provider</span>
            <select value={channel} onChange={(e) => setChannel(e.target.value)}>
              {PROVIDER_CHANNELS.map((opt) => (
                <option key={opt.key} value={opt.key}>
                  {opt.label}
                </option>
              ))}
            </select>
          </label>

          <label className="models-field">
            <span className="models-label">Search</span>
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Filter models..."
            />
          </label>

          <div className="models-actions">
            <button
              type="button"
              className="btn btn-sm"
              onClick={() => setShowInstallDialog(true)}
              disabled={isLoading}
            >
              <Bot size={14} />
              Add to Custom Models
            </button>
            <button type="button" className="btn btn-sm" onClick={refresh} disabled={isLoading}>
              <RefreshCw size={14} className={isLoading ? "spin" : ""} />
              Refresh
            </button>
          </div>
        </div>

        <div className="stats-grid">
          <div className="stat-item">
            <span className="stat-label">Available</span>
            <span className="stat-value">{allModels.length}</span>
          </div>
          <div className="stat-item">
            <span className="stat-label">Visible</span>
            <span className="stat-value">{filteredModels.length}</span>
          </div>
          <div className="stat-item">
            <span className="stat-label">Reasoning-ready</span>
            <span className="stat-value">{thinkingReadyCount}</span>
          </div>
        </div>

        <div className="usage-table-wrap model-table-wrap">
          <table className="usage-table">
            <thead>
              <tr>
                <th>Model</th>
                <th>Thinking</th>
              </tr>
            </thead>
            <tbody>
              {filteredModels.map((m) => (
                <tr key={m.id}>
                  <td>
                    <div className="model-cell">
                      <div className="model-primary">{m.id}</div>
                      {m.display_name ? <div className="model-secondary">{m.display_name}</div> : null}
                    </div>
                  </td>
                  <td className="model-secondary">{formatThinkingSummary(m)}</td>
                </tr>
              ))}
              {filteredModels.length === 0 ? (
                <tr>
                  <td colSpan={2} className="model-secondary">
                    {isLoading ? "Loading..." : "No models found."}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>
      </section>

      <AgentModelInstallDialog
        isOpen={showInstallDialog}
        agentKey="codeforwarder"
        agentLabel="Custom Models"
        defaultDisplayPrefix=""
        initialChannel={channel}
        onClose={() => setShowInstallDialog(false)}
        onInstalled={(result) => {
          setLastInstallResult(result);
          // Refresh list after install so user can keep browsing.
          refresh();
        }}
      />
    </div>
  );
}
