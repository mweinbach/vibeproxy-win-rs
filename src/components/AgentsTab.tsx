import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Lock, Pencil, Plus, RefreshCw, Trash2 } from "lucide-react";
import type {
  AgentInstallResult,
  FactoryCustomModelsRemoveResult,
  FactoryCustomModelsState,
  FactoryCustomModelRow,
} from "../types";
import { toErrorMessage } from "../utils/error";
import AgentModelInstallDialog from "./AgentModelInstallDialog";
import CustomModelEditDialog from "./CustomModelEditDialog";

const FACTORY_NAMESPACE_KEY = "codeforwarder";

export default function AgentsTab() {
  const [state, setState] = useState<FactoryCustomModelsState | null>(null);
  const [search, setSearch] = useState("");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [lastAddResult, setLastAddResult] = useState<AgentInstallResult | null>(null);
  const [lastRemoveResult, setLastRemoveResult] = useState<FactoryCustomModelsRemoveResult | null>(
    null,
  );
  const [lastError, setLastError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isBusy, setIsBusy] = useState(false);
  const [showInstallDialog, setShowInstallDialog] = useState(false);
  const [editingModel, setEditingModel] = useState<FactoryCustomModelRow | null>(null);

  const models = state?.models ?? [];

  const refresh = async () => {
    setIsLoading(true);
    try {
      const next = await invoke<FactoryCustomModelsState>("list_factory_custom_models");
      setState(next);
      setLastError(null);
    } catch (err) {
      setState(null);
      setLastError(toErrorMessage(err, "Failed to load Factory custom models"));
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const canRemove = (m: FactoryCustomModelRow) => m.isProxy && !m.isSessionDefault;
  const canEdit = (m: FactoryCustomModelRow) => m.isProxy;

  const filteredModels = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (q === "") return models;
    return models.filter((m) => {
      const id = m.id.toLowerCase();
      const model = m.model.toLowerCase();
      const dn = m.displayName.toLowerCase();
      const provider = m.provider.toLowerCase();
      const baseUrl = m.baseUrl.toLowerCase();
      return (
        id.includes(q) ||
        model.includes(q) ||
        dn.includes(q) ||
        provider.includes(q) ||
        baseUrl.includes(q)
      );
    });
  }, [models, search]);

  const visibleRemovableIds = useMemo(
    () => filteredModels.filter((m) => canRemove(m)).map((m) => m.id),
    [filteredModels],
  );

  const allVisibleSelected =
    visibleRemovableIds.length > 0 && visibleRemovableIds.every((id) => selectedIds.has(id));

  const toggleRowSelected = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const setAllVisibleSelected = (checked: boolean) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      for (const id of visibleRemovableIds) {
        if (checked) next.add(id);
        else next.delete(id);
      }
      return next;
    });
  };

  const removeModels = async (ids: string[]) => {
    if (ids.length === 0) return;
    setIsBusy(true);
    setLastError(null);
    try {
      const result = await invoke<FactoryCustomModelsRemoveResult>(
        "remove_factory_custom_models",
        {
          ids,
        },
      );
      setLastRemoveResult(result);
      setLastAddResult(null);
      setSelectedIds(new Set());
      await refresh();
    } catch (err) {
      setLastError(toErrorMessage(err, "Failed to remove models"));
    } finally {
      setIsBusy(false);
    }
  };

  const handleRemoveSelected = async () => {
    await removeModels(Array.from(selectedIds));
  };

  const proxyCount = useMemo(() => models.filter((m) => m.isProxy).length, [models]);
  const externalCount = models.length - proxyCount;

  return (
    <div className="tab-content animate-in">
      <h1 className="page-title">Custom Models</h1>
      <p className="page-subtitle">
        Manage Factory custom models powered by CodeForwarder.
      </p>

      {lastAddResult ? (
        <div className="auth-result-banner success" role="status" aria-live="polite">
          <p className="auth-result-message">
            Added {lastAddResult.added} (skipped {lastAddResult.skipped_duplicates} duplicates)
          </p>
        </div>
      ) : null}

      {lastRemoveResult ? (
        <div className="auth-result-banner success" role="status" aria-live="polite">
          <p className="auth-result-message">
            Removed {lastRemoveResult.removed}.
            {lastRemoveResult.skippedNonProxy > 0
              ? ` Skipped ${lastRemoveResult.skippedNonProxy} non-proxy.`
              : ""}
            {lastRemoveResult.skippedNotFound > 0
              ? ` ${lastRemoveResult.skippedNotFound} not found.`
              : ""}
          </p>
        </div>
      ) : null}

      {lastError ? (
        <div className="auth-result-banner error" role="alert">
          <p className="auth-result-message">{lastError}</p>
        </div>
      ) : null}

      <section className="settings-section">
        <div className="stats-grid">
          <div className="stat-item">
            <span className="stat-label">Total</span>
            <span className="stat-value">{models.length}</span>
          </div>
          <div className="stat-item">
            <span className="stat-label">Proxy</span>
            <span className="stat-value">{proxyCount}</span>
          </div>
          <div className="stat-item">
            <span className="stat-label">External</span>
            <span className="stat-value">{externalCount}</span>
          </div>
        </div>

        <div className="agent-card">
          <div className="agent-card-body">
            <div className="agent-actions">
              <button
                type="button"
                className="btn btn-sm btn-primary"
                onClick={() => setShowInstallDialog(true)}
                disabled={isBusy}
              >
                <Plus size={14} />
                Add Models
              </button>
              <button
                type="button"
                className="btn btn-sm"
                onClick={handleRemoveSelected}
                disabled={isBusy || selectedIds.size === 0}
              >
                <Trash2 size={14} />
                Remove Selected
              </button>
              <button
                type="button"
                className="btn btn-sm"
                onClick={refresh}
                disabled={isBusy}
              >
                <RefreshCw size={14} className={isLoading ? "spin" : ""} />
                Refresh
              </button>
            </div>

            <label className="agent-model-field">
              <span className="agent-model-label">Search</span>
              <input
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Filter by name, model id, provider, base URL..."
              />
            </label>

            <div className="usage-table-wrap model-table-wrap">
              <table className="usage-table">
                <thead>
                  <tr>
                    <th style={{ width: 44 }}>
                      <input
                        type="checkbox"
                        checked={allVisibleSelected}
                        onChange={(e) => setAllVisibleSelected(e.target.checked)}
                        disabled={visibleRemovableIds.length === 0}
                      />
                    </th>
                    <th>Model</th>
                    <th style={{ width: 120 }}>Provider</th>
                    <th>Base URL</th>
                    <th style={{ width: 110 }}>Type</th>
                    <th style={{ width: 170 }}>Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredModels.map((m) => (
                    <tr key={m.id}>
                      <td>
                        <input
                          type="checkbox"
                          checked={selectedIds.has(m.id)}
                          onChange={() => toggleRowSelected(m.id)}
                          disabled={!canRemove(m)}
                        />
                      </td>
                      <td>
                        <div className="model-cell">
                          <div className="model-primary">
                            {m.displayName}
                            {m.isSessionDefault ? (
                              <span className="model-secondary"> (default)</span>
                            ) : null}
                          </div>
                          <div className="model-secondary">{m.model}</div>
                          <div className="model-secondary">{m.id}</div>
                        </div>
                      </td>
                      <td className="model-secondary">{m.provider}</td>
                      <td className="model-secondary">{m.baseUrl}</td>
                      <td className="model-secondary">
                        {m.isProxy ? "proxy" : "external"}
                      </td>
                      <td>
                        {m.isProxy ? (
                          <div className="models-actions">
                            <button
                              type="button"
                              className="btn btn-sm"
                              onClick={() => setEditingModel(m)}
                              disabled={!canEdit(m)}
                            >
                              <Pencil size={14} />
                              Edit
                            </button>
                            <button
                              type="button"
                              className="btn btn-sm"
                              onClick={() => removeModels([m.id])}
                              disabled={isBusy || !canRemove(m)}
                            >
                              <Trash2 size={14} />
                              Remove
                            </button>
                          </div>
                        ) : (
                          <div className="model-secondary" style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                            <Lock size={14} />
                            view-only
                          </div>
                        )}
                      </td>
                    </tr>
                  ))}
                  {filteredModels.length === 0 ? (
                    <tr>
                      <td colSpan={6} className="model-secondary">
                        {models.length === 0
                          ? "No custom models found."
                          : isLoading
                            ? "Loading..."
                            : "No matches."}
                      </td>
                    </tr>
                  ) : null}
                </tbody>
              </table>
            </div>

            <div className="agent-model-hint">
              {state?.factorySettingsPath ? `Factory: ${state.factorySettingsPath}` : ""}
            </div>
          </div>
        </div>
      </section>

      <AgentModelInstallDialog
        isOpen={showInstallDialog}
        agentKey={FACTORY_NAMESPACE_KEY}
        agentLabel="Custom Models"
        defaultDisplayPrefix=""
        onClose={() => setShowInstallDialog(false)}
        onInstalled={async (result) => {
          setLastAddResult(result);
          setLastRemoveResult(null);
          await refresh();
        }}
      />

      <CustomModelEditDialog
        isOpen={editingModel != null}
        model={editingModel}
        onClose={() => setEditingModel(null)}
        onSaved={async () => {
          await refresh();
        }}
      />
    </div>
  );
}
