import { BarChart3, RefreshCw } from "lucide-react";
import type {
  UsageDashboardPayload,
  UsageRange,
  UsageBreakdownRow,
} from "../types";

interface UsageDashboardProps {
  dashboard: UsageDashboardPayload;
  range: UsageRange;
  onRangeChange: (range: UsageRange) => void;
  onRefresh: () => void;
  isLoading: boolean;
  error: string | null;
  onDismissError: () => void;
}

const RANGE_OPTIONS: Array<{ label: string; value: UsageRange }> = [
  { label: "24h", value: "24h" },
  { label: "7d", value: "7d" },
  { label: "30d", value: "30d" },
  { label: "All", value: "all" },
];

function formatNumber(value: number): string {
  return new Intl.NumberFormat("en-US").format(Math.max(0, Math.round(value)));
}

function formatPercent(value: number): string {
  return `${value.toFixed(1)}%`;
}

function getProviderBreakdown(rows: UsageBreakdownRow[]) {
  const byProvider = new Map<string, { requests: number; tokens: number }>();
  rows.forEach((row) => {
    const current = byProvider.get(row.provider) ?? { requests: 0, tokens: 0 };
    current.requests += row.requests;
    current.tokens += row.total_tokens;
    byProvider.set(row.provider, current);
  });
  return [...byProvider.entries()]
    .map(([provider, value]) => ({ provider, ...value }))
    .sort((a, b) => b.tokens - a.tokens);
}

export default function UsageDashboard({
  dashboard,
  range,
  onRangeChange,
  onRefresh,
  isLoading,
  error,
  onDismissError,
}: UsageDashboardProps) {
  const vibe = dashboard.vibe;
  const native = dashboard.native;
  const providerBreakdown = getProviderBreakdown(vibe.breakdown);
  const maxPointTokens = Math.max(
    1,
    ...vibe.timeseries.map((point) => point.total_tokens),
  );
  const maxProviderTokens = Math.max(
    1,
    ...providerBreakdown.map((row) => row.tokens),
  );

  return (
    <div className="usage-dashboard">
      <section className="settings-section usage-controls">
        <div className="section-header">
          <div className="section-title-row">
            <BarChart3 size={14} />
            <h2 className="section-title">Usage Analytics</h2>
          </div>
          <p className="section-description">
            Track requests and token usage by provider, model, and account.
          </p>
        </div>
        <div className="usage-controls-row">
          <div className="usage-range-picker">
            {RANGE_OPTIONS.map((option) => (
              <button
                type="button"
                key={option.value}
                className={`range-pill ${range === option.value ? "active" : ""}`}
                onClick={() => onRangeChange(option.value)}
              >
                {option.label}
              </button>
            ))}
          </div>
          <button type="button" className="btn btn-sm" onClick={onRefresh}>
            <RefreshCw size={14} className={isLoading ? "spin" : ""} />
            Refresh
          </button>
        </div>
        {error ? (
          <div className="operation-error-banner" role="alert">
            <p className="operation-error-message">{error}</p>
            <button type="button" className="btn btn-sm" onClick={onDismissError}>
              Dismiss
            </button>
          </div>
        ) : null}
      </section>

      <div className="usage-kpi-grid">
        <div className="stat-card">
          <span className="stat-label">Total Requests</span>
          <span className="stat-value">
            {formatNumber(vibe.summary.total_requests)}
          </span>
        </div>
        <div className="stat-card">
          <span className="stat-label">Total Tokens</span>
          <span className="stat-value">
            {formatNumber(vibe.summary.total_tokens)}
          </span>
        </div>
        <div className="stat-card">
          <span className="stat-label">Input Tokens</span>
          <span className="stat-value">
            {formatNumber(vibe.summary.input_tokens)}
          </span>
        </div>
        <div className="stat-card">
          <span className="stat-label">Output Tokens</span>
          <span className="stat-value">
            {formatNumber(vibe.summary.output_tokens)}
          </span>
        </div>
        <div className="stat-card">
          <span className="stat-label">Cached Tokens</span>
          <span className="stat-value">
            {formatNumber(vibe.summary.cached_tokens)}
          </span>
        </div>
        <div className="stat-card">
          <span className="stat-label">Reasoning Tokens</span>
          <span className="stat-value">
            {formatNumber(vibe.summary.reasoning_tokens)}
          </span>
        </div>
        <div className="stat-card">
          <span className="stat-label">Error Rate</span>
          <span className="stat-value">{formatPercent(vibe.summary.error_rate)}</span>
        </div>
      </div>

      <div className="usage-grid-two">
        <section className="settings-section">
          <div className="section-header">
            <h2 className="section-title">Token Trend</h2>
            <p className="section-description">Total tokens per time bucket.</p>
          </div>
          {vibe.timeseries.length === 0 ? (
            <p className="empty-note">No usage events yet for this range.</p>
          ) : (
            <div className="token-chart">
              {vibe.timeseries.map((point) => (
                <div className="token-bar" key={`${point.bucket}-${point.total_tokens}`}>
                  <div
                    className="token-bar-fill"
                    style={{
                      height: `${Math.max(
                        6,
                        Math.round((point.total_tokens / maxPointTokens) * 100),
                      )}%`,
                    }}
                    title={`${point.bucket}: ${formatNumber(point.total_tokens)} tokens`}
                  />
                  <span className="token-bar-label">{point.bucket}</span>
                </div>
              ))}
            </div>
          )}
        </section>

        <section className="settings-section">
          <div className="section-header">
            <h2 className="section-title">Provider Share</h2>
            <p className="section-description">Token distribution by provider.</p>
          </div>
          {providerBreakdown.length === 0 ? (
            <p className="empty-note">No provider usage yet.</p>
          ) : (
            <div className="provider-share-list">
              {providerBreakdown.map((row) => (
                <div className="provider-share-row" key={row.provider}>
                  <div className="provider-share-label">
                    <span>{row.provider}</span>
                    <span>{formatNumber(row.tokens)} tokens</span>
                  </div>
                  <div className="provider-share-track">
                    <div
                      className="provider-share-fill"
                      style={{
                        width: `${Math.max(
                          2,
                          Math.round((row.tokens / maxProviderTokens) * 100),
                        )}%`,
                      }}
                    />
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>

      <div className="usage-grid-two">
        <section className="settings-section">
          <div className="section-header">
            <h2 className="section-title">Detailed Breakdown</h2>
            <p className="section-description">
              Provider, model, and account-level request/token usage.
            </p>
          </div>
          {vibe.breakdown.length === 0 ? (
            <p className="empty-note">No detailed usage data available yet.</p>
          ) : (
            <div className="usage-table-wrap">
              <table className="usage-table">
                <thead>
                  <tr>
                    <th>Provider</th>
                    <th>Model</th>
                    <th>Account</th>
                    <th>Requests</th>
                    <th>Tokens</th>
                    <th>Cached</th>
                    <th>Reasoning</th>
                    <th>Last Seen</th>
                  </tr>
                </thead>
                <tbody>
                  {vibe.breakdown.map((row) => (
                    <tr key={`${row.provider}-${row.model}-${row.account_key}`}>
                      <td>{row.provider}</td>
                      <td>{row.model}</td>
                      <td>{row.account_label || row.account_key}</td>
                      <td>{formatNumber(row.requests)}</td>
                      <td>{formatNumber(row.total_tokens)}</td>
                      <td>{formatNumber(row.cached_tokens)}</td>
                      <td>{formatNumber(row.reasoning_tokens)}</td>
                      <td>{row.last_seen ? new Date(row.last_seen).toLocaleString() : "-"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </section>

        <section className="settings-section">
          <div className="section-header">
            <h2 className="section-title">Native Comparison (Temporary)</h2>
            <p className="section-description">
              CLIProxy native usage status: <strong>{native.status}</strong>
            </p>
          </div>
          <div className="native-summary-grid">
            <div className="stat-card">
              <span className="stat-label">Effective Range</span>
              <span className="stat-value">{native.effective_range}</span>
            </div>
            <div className="stat-card">
              <span className="stat-label">Requests</span>
              <span className="stat-value">
                {formatNumber(native.summary?.total_requests ?? 0)}
              </span>
            </div>
            <div className="stat-card">
              <span className="stat-label">Tokens</span>
              <span className="stat-value">
                {formatNumber(native.summary?.total_tokens ?? 0)}
              </span>
            </div>
          </div>
          {native.message ? <p className="native-note">{native.message}</p> : null}
          <p className="native-note">
            Last synced: {native.last_synced_at ? new Date(native.last_synced_at).toLocaleString() : "Never"}
          </p>
          {native.rows.length > 0 ? (
            <div className="usage-table-wrap native-table-wrap">
              <table className="usage-table">
                <thead>
                  <tr>
                    <th>Source</th>
                    <th>Model</th>
                    <th>Auth Index</th>
                    <th>Requests</th>
                    <th>Tokens</th>
                  </tr>
                </thead>
                <tbody>
                  {native.rows.map((row, index) => (
                    <tr key={`${row.source}-${row.model}-${index}`}>
                      <td>{row.source}</td>
                      <td>{row.model}</td>
                      <td>{row.auth_index ?? "-"}</td>
                      <td>{formatNumber(row.requests)}</td>
                      <td>{formatNumber(row.tokens)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <p className="empty-note">No native row-level data available.</p>
          )}
        </section>
      </div>
    </div>
  );
}
