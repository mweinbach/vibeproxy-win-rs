import { useState, useEffect } from "react";
import type { ServiceType, AuthAccount } from "../types";
import { SERVICE_DISPLAY_NAMES } from "../types";
import AccountRow from "./AccountRow";

interface ServiceRowProps {
  serviceType: ServiceType;
  accounts: AuthAccount[];
  isEnabled: boolean;
  isAuthenticating: boolean;
  onConnect: () => void;
  onDisconnect: (filePath: string) => void;
  onToggleEnabled: (enabled: boolean) => void;
  children?: React.ReactNode;
  icon: string;
  customTitle?: string;
}

export default function ServiceRow({
  serviceType,
  accounts,
  isEnabled,
  isAuthenticating,
  onConnect,
  onDisconnect,
  onToggleEnabled,
  children,
  icon,
  customTitle,
}: ServiceRowProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const displayName = customTitle ?? SERVICE_DISPLAY_NAMES[serviceType];
  const activeCount = accounts.filter((account) => !account.is_expired).length;
  const expiredCount = accounts.length - activeCount;

  // Auto-expand if any account is expired
  useEffect(() => {
    if (accounts.some((a) => a.is_expired)) {
      setIsExpanded(true);
    }
  }, [accounts]);

  return (
    <div className={`service-row ${isEnabled ? "" : "is-disabled"}`}>
      <div className="service-header">
        <label className="toggle-switch">
          <input
            type="checkbox"
            checked={isEnabled}
            onChange={(e) => onToggleEnabled(e.target.checked)}
          />
          <span className="toggle-slider" />
        </label>
        <img
          src={icon}
          alt={displayName}
          className="service-icon"
        />
        <span className="service-name">{displayName}</span>
        <div className="service-spacer" />
        {isAuthenticating ? (
          <span className="spinner" />
        ) : isEnabled ? (
          <button type="button" className="btn btn-sm" onClick={onConnect}>
            Add account
          </button>
        ) : (
          <span className="service-disabled-pill">Disabled</span>
        )}
      </div>

      {isEnabled && (
        <div className="service-accounts">
          {accounts.length > 0 ? (
            <>
              <button
                type="button"
                className="accounts-summary"
                onClick={() => setIsExpanded(!isExpanded)}
              >
                <span className="accounts-summary-main">
                  <span className="accounts-count">
                    {accounts.length} connected account
                    {accounts.length === 1 ? "" : "s"}
                  </span>
                  <span className="accounts-pills">
                    <span className="count-pill active">{activeCount} active</span>
                    {expiredCount > 0 && (
                      <span className="count-pill expired">
                        {expiredCount} expired
                      </span>
                    )}
                  </span>
                </span>
                {accounts.length > 1 && (
                  <span className="accounts-note">
                    Round-robin w/ auto-failover
                  </span>
                )}
                <span className={`chevron ${isExpanded ? "open" : ""}`}>&gt;</span>
              </button>

              {isExpanded && (
                <div className="accounts-list">
                  {accounts.map((account) => (
                    <AccountRow
                      key={account.id}
                      account={account}
                      serviceName={displayName}
                      onRemove={() => onDisconnect(account.file_path)}
                    />
                  ))}
                </div>
              )}
            </>
          ) : (
            <div className="no-accounts">No connected accounts yet.</div>
          )}

          {children ? <div className="service-extra">{children}</div> : null}
        </div>
      )}
    </div>
  );
}
