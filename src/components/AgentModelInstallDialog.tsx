import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  type AgentInstallResult,
  type FactoryCustomModelInput,
  type ProviderModelDefinitionsResponse,
  type ProviderModelInfo,
} from "../types";
import { toErrorMessage } from "../utils/error";

type FactoryProvider = "anthropic" | "openai";

const PROVIDER_CHANNELS: Array<{ key: string; label: string }> = [
  { key: "claude", label: "Claude" },
  { key: "codex", label: "Codex" },
  { key: "gemini", label: "Gemini" },
  { key: "qwen", label: "Qwen" },
  { key: "github-copilot", label: "GitHub Copilot" },
  { key: "antigravity", label: "Antigravity" },
];

function channelDefaults(channel: string): { provider: FactoryProvider; baseUrl: string } {
  if (channel === "claude") {
    return { provider: "anthropic", baseUrl: "http://localhost:8317" };
  }
  return { provider: "openai", baseUrl: "http://localhost:8317/v1" };
}

function isProxyBaseUrl(raw: string): boolean {
  const trimmed = raw.trim();
  if (trimmed === "") return false;
  const lower = trimmed.toLowerCase();
  if (
    lower.startsWith("http://localhost:8317") ||
    lower.startsWith("https://localhost:8317") ||
    lower.startsWith("http://127.0.0.1:8317") ||
    lower.startsWith("https://127.0.0.1:8317") ||
    lower.startsWith("http://0.0.0.0:8317") ||
    lower.startsWith("https://0.0.0.0:8317") ||
    lower.startsWith("http://[::1]:8317") ||
    lower.startsWith("https://[::1]:8317")
  ) {
    return true;
  }
  try {
    const url = new URL(trimmed);
    const port = url.port ? Number(url.port) : url.protocol === "https:" ? 443 : 80;
    if (port !== 8317) return false;
    const host = url.hostname.toLowerCase();
    return host === "localhost" || host === "127.0.0.1" || host === "0.0.0.0" || host === "::1";
  } catch {
    return false;
  }
}

function hasThinkingLevels(model: ProviderModelInfo): string[] {
  const levels = model.thinking?.levels;
  return Array.isArray(levels) ? levels.filter((v) => typeof v === "string" && v.trim() !== "") : [];
}

function canUseThinkingBudgets(model: ProviderModelInfo): boolean {
  if (hasThinkingLevels(model).length > 0) return false;
  const thinking = model.thinking;
  if (!thinking) return false;
  return thinking.min != null || thinking.max != null || thinking.zero_allowed != null || thinking.dynamic_allowed != null;
}

function parseBudgets(raw: string): number[] {
  const parts = raw
    .split(/[\s,]+/)
    .map((p) => p.trim())
    .filter((p) => p !== "");
  const out: number[] = [];
  for (const p of parts) {
    const n = Number(p);
    if (!Number.isFinite(n)) continue;
    const i = Math.floor(n);
    if (i <= 0) continue;
    out.push(i);
  }
  return Array.from(new Set(out)).sort((a, b) => a - b);
}

function makeBudgetVariant(modelId: string, budget: number): string {
  const trimmed = modelId.trim();
  if (trimmed === "") return trimmed;
  if (trimmed.includes("-thinking-")) {
    return trimmed;
  }
  if (trimmed.endsWith("-thinking")) {
    return `${trimmed}-${budget}`;
  }
  return `${trimmed}-thinking-${budget}`;
}

function makeLevelVariant(modelId: string, level: string): string {
  const trimmed = modelId.trim();
  if (trimmed === "") return trimmed;
  if (/\([^)]+\)$/.test(trimmed)) {
    return trimmed;
  }
  return `${trimmed}(${level})`;
}

function titleCase(raw: string): string {
  const trimmed = raw.trim();
  if (trimmed === "") return trimmed;
  if (trimmed.length === 1) return trimmed.toUpperCase();
  return trimmed[0].toUpperCase() + trimmed.slice(1);
}

function defaultDisplayNameForModel(model: ProviderModelInfo): string {
  return model.display_name?.trim() || model.id;
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

function buildBudgetLabels(budgets: number[]): Map<number, string> {
  const labels = ["Low", "Medium", "High", "XHigh"];
  const out = new Map<number, string>();
  if (budgets.length >= 2 && budgets.length <= labels.length) {
    budgets.forEach((b, idx) => {
      out.set(b, labels[idx]);
    });
    return out;
  }
  budgets.forEach((b) => out.set(b, `Thinking ${b}`));
  return out;
}

interface AgentModelInstallDialogProps {
  isOpen: boolean;
  agentKey: string;
  agentLabel: string;
  initialChannel?: string;
  defaultDisplayPrefix?: string;
  onClose: () => void;
  onInstalled?: (result: AgentInstallResult) => void;
}

export default function AgentModelInstallDialog({
  isOpen,
  agentKey,
  agentLabel,
  initialChannel,
  defaultDisplayPrefix,
  onClose,
  onInstalled,
}: AgentModelInstallDialogProps) {
  const [channel, setChannel] = useState(initialChannel ?? "claude");
  const [modelsResponse, setModelsResponse] = useState<ProviderModelDefinitionsResponse | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [lastError, setLastError] = useState<string | null>(null);

  const [search, setSearch] = useState("");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());

  const defaults = useMemo(() => channelDefaults(channel), [channel]);
  const [factoryProvider, setFactoryProvider] = useState<FactoryProvider>(defaults.provider);
  const [baseUrl, setBaseUrl] = useState(defaults.baseUrl);
  const [displayPrefix, setDisplayPrefix] = useState(
    defaultDisplayPrefix ?? `${agentLabel}: `,
  );
  const [noImageSupport, setNoImageSupport] = useState(false);

  const [includeBase, setIncludeBase] = useState(true);
  const [selectedLevels, setSelectedLevels] = useState<Set<string>>(new Set(["high"]));
  const [budgetCsv, setBudgetCsv] = useState("4000, 10000, 32000");

  useEffect(() => {
    if (!isOpen) return;
    setChannel(initialChannel ?? "claude");
    setSearch("");
    setSelectedIds(new Set());
    setIncludeBase(true);
    setSelectedLevels(new Set(["high"]));
    setBudgetCsv("4000, 10000, 32000");
    setNoImageSupport(false);
    setDisplayPrefix(defaultDisplayPrefix ?? `${agentLabel}: `);
    setLastError(null);
  }, [agentLabel, defaultDisplayPrefix, initialChannel, isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    const next = channelDefaults(channel);
    setFactoryProvider(next.provider);
    setBaseUrl(next.baseUrl);
  }, [channel, isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    setIsLoading(true);
    setLastError(null);
    invoke<ProviderModelDefinitionsResponse>("get_provider_model_definitions", { channel })
      .then((resp) => {
        setModelsResponse(resp);
      })
      .catch((err) => {
        setModelsResponse(null);
        setLastError(
          toErrorMessage(
            err,
            "Failed to load models (make sure Proxy Engine is running)",
          ),
        );
      })
      .finally(() => {
        setIsLoading(false);
      });
  }, [channel, isOpen]);

  const allModels = modelsResponse?.models ?? [];

  const filteredModels = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (q === "") return allModels;
    return allModels.filter((m) => {
      const id = m.id.toLowerCase();
      const dn = (m.display_name ?? "").toLowerCase();
      return id.includes(q) || dn.includes(q);
    });
  }, [allModels, search]);

  const selectedModels = useMemo(() => {
    if (selectedIds.size === 0) return [];
    const byId = new Map(allModels.map((m) => [m.id, m] as const));
    const out: ProviderModelInfo[] = [];
    for (const id of selectedIds) {
      const m = byId.get(id);
      if (m) out.push(m);
    }
    out.sort((a, b) => a.id.localeCompare(b.id));
    return out;
  }, [allModels, selectedIds]);

  const unionLevels = useMemo(() => {
    const set = new Set<string>();
    for (const m of selectedModels) {
      for (const level of hasThinkingLevels(m)) {
        set.add(level);
      }
    }
    return Array.from(set).sort();
  }, [selectedModels]);

  const budgets = useMemo(() => parseBudgets(budgetCsv), [budgetCsv]);
  const budgetLabels = useMemo(() => buildBudgetLabels(budgets), [budgets]);

  const previewCount = useMemo(() => {
    if (selectedModels.length === 0) return 0;
    let count = 0;
    for (const m of selectedModels) {
      if (includeBase) count += 1;
      const levels = hasThinkingLevels(m);
      if (levels.length > 0) {
        for (const l of selectedLevels) {
          if (levels.includes(l)) count += 1;
        }
      } else if (canUseThinkingBudgets(m)) {
        count += budgets.length;
      }
    }
    return count;
  }, [budgets.length, includeBase, selectedLevels, selectedModels]);

  if (!isOpen) return null;

  const toggleSelected = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const setAllVisible = (checked: boolean) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      for (const m of filteredModels) {
        if (checked) next.add(m.id);
        else next.delete(m.id);
      }
      return next;
    });
  };

  const handleInstall = async () => {
    setLastError(null);
    const prefix = displayPrefix;
    const inputs: FactoryCustomModelInput[] = [];
    for (const model of selectedModels) {
      const baseId = model.id;
      const baseName = defaultDisplayNameForModel(model);

      const variants: Array<{ modelId: string; labelSuffix: string }> = [];
      if (includeBase) {
        variants.push({ modelId: baseId, labelSuffix: "" });
      }

      const levels = hasThinkingLevels(model);
      if (levels.length > 0) {
        for (const level of selectedLevels) {
          if (!levels.includes(level)) continue;
          const variantId = makeLevelVariant(baseId, level);
          if (variantId === baseId) continue;
          variants.push({ modelId: variantId, labelSuffix: ` (${titleCase(level)})` });
        }
      } else if (canUseThinkingBudgets(model)) {
        for (const budget of budgets) {
          const variantId = makeBudgetVariant(baseId, budget);
          if (variantId === baseId) continue;
          const label = budgetLabels.get(budget) ?? `Thinking ${budget}`;
          variants.push({ modelId: variantId, labelSuffix: ` (${label})` });
        }
      }

      for (const v of variants) {
        inputs.push({
          model: v.modelId,
          baseUrl,
          apiKey: "dummy-not-used",
          displayName: `${prefix}${baseName}${v.labelSuffix}`.trim(),
          noImageSupport,
          provider: factoryProvider,
        });
      }
    }

    try {
      const result = await invoke<AgentInstallResult>("install_agent_models", {
        agentKey,
        models: inputs,
      });
      onInstalled?.(result);
      onClose();
    } catch (err) {
      setLastError(toErrorMessage(err, "Failed to install models"));
    }
  };

  const levelsDisabled = unionLevels.length === 0;
  const budgetsDisabled = selectedModels.every((m) => !canUseThinkingBudgets(m));
  const canInstall = selectedModels.length > 0 && isProxyBaseUrl(baseUrl);

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-content modal-content-wide" onClick={(e) => e.stopPropagation()}>
        <h3 className="modal-title">Add Models to {agentLabel}</h3>
        <p className="modal-subtitle">
          Pick a provider catalog, select models, and choose reasoning variants to install into
          Factory custom models.
        </p>

        {lastError ? (
          <div className="auth-result-banner error" role="alert">
            <p className="auth-result-message">{lastError}</p>
          </div>
        ) : null}

        <div className="agent-model-controls">
          <label className="agent-model-field">
            <span className="agent-model-label">Provider</span>
            <select
              value={channel}
              onChange={(e) => setChannel(e.target.value)}
              className="agent-model-select"
            >
              {PROVIDER_CHANNELS.map((opt) => (
                <option key={opt.key} value={opt.key}>
                  {opt.label}
                </option>
              ))}
            </select>
          </label>
          <label className="agent-model-field">
            <span className="agent-model-label">Search</span>
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Filter models..."
            />
          </label>
        </div>

        <div className="agent-model-grid">
          <section className="agent-model-pane">
            <div className="agent-model-pane-head">
              <div className="agent-model-pane-title">
                <strong>Models</strong>
                <span className="agent-model-pane-meta">
                  {isLoading ? "Loading..." : `${filteredModels.length} shown / ${allModels.length} total`}
                </span>
              </div>
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={
                    filteredModels.length > 0 &&
                    filteredModels.every((m) => selectedIds.has(m.id))
                  }
                  onChange={(e) => setAllVisible(e.target.checked)}
                />
                Select visible
              </label>
            </div>

            <div className="usage-table-wrap model-table-wrap">
              <table className="usage-table">
                <thead>
                  <tr>
                    <th style={{ width: 44 }}>Add</th>
                    <th>Model</th>
                    <th>Thinking</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredModels.map((m) => (
                    <tr key={m.id}>
                      <td>
                        <input
                          type="checkbox"
                          checked={selectedIds.has(m.id)}
                          onChange={() => toggleSelected(m.id)}
                        />
                      </td>
                      <td>
                        <div className="model-cell">
                          <div className="model-primary">{m.id}</div>
                          {m.display_name ? (
                            <div className="model-secondary">{m.display_name}</div>
                          ) : null}
                        </div>
                      </td>
                      <td className="model-secondary">{formatThinkingSummary(m)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>

          <section className="agent-model-pane">
            <div className="agent-model-pane-head">
              <div className="agent-model-pane-title">
                <strong>Install Options</strong>
                <span className="agent-model-pane-meta">{previewCount} variants</span>
              </div>
            </div>

            <div className="agent-model-options">
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={includeBase}
                  onChange={(e) => setIncludeBase(e.target.checked)}
                />
                Include base model
              </label>

              <div className="agent-model-section">
                <div className="agent-model-section-title">Reasoning levels</div>
                <div className={`agent-model-chiprow ${levelsDisabled ? "is-disabled" : ""}`}>
                  {unionLevels.length === 0 ? (
                    <span className="empty-note">Select a model with level-based reasoning.</span>
                  ) : (
                    unionLevels.map((level) => (
                      <label key={level} className="chip-check">
                        <input
                          type="checkbox"
                          checked={selectedLevels.has(level)}
                          onChange={(e) => {
                            const checked = e.target.checked;
                            setSelectedLevels((prev) => {
                              const next = new Set(prev);
                              if (checked) next.add(level);
                              else next.delete(level);
                              return next;
                            });
                          }}
                        />
                        <span>{level}</span>
                      </label>
                    ))
                  )}
                </div>
              </div>

              <div className="agent-model-section">
                <div className="agent-model-section-title">Thinking budgets</div>
                <input
                  type="text"
                  value={budgetCsv}
                  onChange={(e) => setBudgetCsv(e.target.value)}
                  disabled={budgetsDisabled}
                  placeholder="e.g. 4000, 10000, 32000"
                />
                <div className="agent-model-hint">
                  Budgets above ~32000 will be clamped by CodeForwarder.
                </div>
              </div>

              <div className="agent-model-section">
                <div className="agent-model-section-title">Factory mapping</div>
                <label className="agent-model-inline">
                  <span>Provider</span>
                  <select
                    value={factoryProvider}
                    onChange={(e) => setFactoryProvider(e.target.value as FactoryProvider)}
                  >
                    <option value="anthropic">anthropic</option>
                    <option value="openai">openai</option>
                  </select>
                </label>
                <label className="agent-model-inline">
                  <span>Base URL</span>
                  <input type="text" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} />
                </label>
                {!isProxyBaseUrl(baseUrl) ? (
                  <div className="agent-model-hint">
                    Only proxy URLs on localhost:8317 are supported.
                  </div>
                ) : null}
                <label className="agent-model-inline">
                  <span>Name prefix</span>
                  <input
                    type="text"
                    value={displayPrefix}
                    onChange={(e) => setDisplayPrefix(e.target.value)}
                    placeholder={`${agentLabel}: `}
                  />
                </label>
                <label className="checkbox-row">
                  <input
                    type="checkbox"
                    checked={noImageSupport}
                    onChange={(e) => setNoImageSupport(e.target.checked)}
                  />
                  Mark as no image support
                </label>
              </div>
            </div>
          </section>
        </div>

        <div className="modal-buttons">
          <button type="button" className="btn btn-cancel" onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={!canInstall || isLoading}
            onClick={handleInstall}
          >
            Install
          </button>
        </div>
      </div>
    </div>
  );
}
