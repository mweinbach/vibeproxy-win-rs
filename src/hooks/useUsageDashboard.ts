import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { UsageDashboardPayload, UsageRange } from "../types";
import { toErrorMessage } from "../utils/error";

const DEFAULT_RANGE: UsageRange = "7d";

const EMPTY_DASHBOARD: UsageDashboardPayload = {
  dashboard: {
    range: DEFAULT_RANGE,
    summary: {
      total_requests: 0,
      total_tokens: 0,
      input_tokens: 0,
      output_tokens: 0,
      cached_tokens: 0,
      reasoning_tokens: 0,
      error_count: 0,
      error_rate: 0,
    },
    timeseries: [],
    breakdown: [],
  },
};

export function useUsageDashboard(isActive: boolean) {
  const [range, setRange] = useState<UsageRange>(DEFAULT_RANGE);
  const [dashboard, setDashboard] = useState<UsageDashboardPayload>(EMPTY_DASHBOARD);
  const [isLoading, setIsLoading] = useState(true);
  const [lastError, setLastError] = useState<string | null>(null);

  const fetchDashboard = useCallback(async () => {
    try {
      const result = await invoke<UsageDashboardPayload>("get_usage_dashboard", {
        range,
      });
      setDashboard(result);
      setLastError(null);
    } catch (err) {
      console.error("Failed to load usage dashboard:", err);
      setLastError(toErrorMessage(err, "Failed to load usage dashboard"));
    } finally {
      setIsLoading(false);
    }
  }, [range]);

  useEffect(() => {
    setIsLoading(true);
    fetchDashboard();
  }, [fetchDashboard]);

  useEffect(() => {
    if (!isActive) return;

    const id = window.setInterval(() => {
      fetchDashboard();
    }, 10_000);

    return () => {
      window.clearInterval(id);
    };
  }, [fetchDashboard, isActive]);

  return useMemo(
    () => ({
      range,
      setRange,
      dashboard,
      isLoading,
      lastError,
      refresh: fetchDashboard,
      clearLastError: () => setLastError(null),
    }),
    [dashboard, fetchDashboard, isLoading, lastError, range],
  );
}
