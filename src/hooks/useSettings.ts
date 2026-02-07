import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppSettings } from "../types";
import { toErrorMessage } from "../utils/error";

const DEFAULT_SETTINGS: AppSettings = {
  enabled_providers: {},
  vercel_gateway_enabled: false,
  vercel_api_key: "",
  launch_at_login: false,
};

export function useSettings() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);

  useEffect(() => {
    invoke<AppSettings>("get_settings")
      .then((value) => {
        setSettings(value);
        setLastError(null);
      })
      .catch((err) => {
        console.error("Failed to get settings:", err);
        setSettings(DEFAULT_SETTINGS);
        setLastError(toErrorMessage(err, "Failed to load settings"));
      });
  }, []);

  const setProviderEnabled = useCallback(
    async (provider: string, enabled: boolean) => {
      setSettings((prev) => {
        if (!prev) return prev;
        return {
          ...prev,
          enabled_providers: { ...prev.enabled_providers, [provider]: enabled },
        };
      });
      try {
        await invoke("set_provider_enabled", { provider, enabled });
        setLastError(null);
      } catch (err) {
        console.error("Failed to set provider enabled:", err);
        setLastError(toErrorMessage(err, "Failed to update provider state"));
        invoke<AppSettings>("get_settings")
          .then(setSettings)
          .catch((e) => console.error("Failed to refetch settings:", e));
      }
    },
    [],
  );

  const setVercelConfig = useCallback(
    async (enabled: boolean, apiKey: string) => {
      setSettings((prev) => {
        if (!prev) return prev;
        return {
          ...prev,
          vercel_gateway_enabled: enabled,
          vercel_api_key: apiKey,
        };
      });
      try {
        await invoke("set_vercel_config", { enabled, api_key: apiKey });
        setLastError(null);
      } catch (err) {
        console.error("Failed to set Vercel config:", err);
        setLastError(toErrorMessage(err, "Failed to update Vercel configuration"));
        invoke<AppSettings>("get_settings")
          .then(setSettings)
          .catch((e) => console.error("Failed to refetch settings:", e));
      }
    },
    [],
  );

  const setLaunchAtLogin = useCallback(async (enabled: boolean) => {
    setSettings((prev) => {
      if (!prev) return prev;
      return { ...prev, launch_at_login: enabled };
    });
    try {
      await invoke("set_launch_at_login", { enabled });
      setLastError(null);
    } catch (err) {
      console.error("Failed to set launch at login:", err);
      setLastError(toErrorMessage(err, "Failed to update launch at login"));
      invoke<AppSettings>("get_settings")
        .then(setSettings)
        .catch((e) => console.error("Failed to refetch settings:", e));
    }
  }, []);

  return {
    settings,
    setProviderEnabled,
    setVercelConfig,
    setLaunchAtLogin,
    lastError,
    clearLastError: () => setLastError(null),
  };
}
