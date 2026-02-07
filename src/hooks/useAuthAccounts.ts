import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { SERVICE_ORDER } from "../types";
import type { ServiceAccounts, ServiceType } from "../types";
import { toErrorMessage } from "../utils/error";

interface AuthResult {
  success: boolean;
  message: string;
}

function getEmptyAccounts(): Record<ServiceType, ServiceAccounts> {
  return SERVICE_ORDER.reduce((acc, serviceType) => {
    acc[serviceType] = {
      service_type: serviceType,
      accounts: [],
      active_count: 0,
      expired_count: 0,
    };
    return acc;
  }, {} as Record<ServiceType, ServiceAccounts>);
}

export function useAuthAccounts() {
  const [accounts, setAccounts] = useState<Record<
    ServiceType,
    ServiceAccounts
  > | null>(null);
  const [authenticatingService, setAuthenticatingService] =
    useState<ServiceType | null>(null);
  const [authResult, setAuthResult] = useState<AuthResult | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);

  const fetchAccounts = useCallback(async () => {
    try {
      const data =
        await invoke<Record<string, ServiceAccounts>>("get_auth_accounts");
      setAccounts(data as Record<ServiceType, ServiceAccounts>);
      setLastError(null);
    } catch (err) {
      console.error("Failed to get auth accounts:", err);
      setAccounts(getEmptyAccounts());
      setLastError(toErrorMessage(err, "Failed to load auth accounts"));
    }
  }, []);

  useEffect(() => {
    fetchAccounts();

    const unlisten = listen("auth_accounts_changed", () => {
      fetchAccounts();
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [fetchAccounts]);

  const runAuth = useCallback(async (command: { type: string; data?: { email: string } }) => {
    const serviceType = command.type as ServiceType;
    setAuthenticatingService(serviceType);
    setAuthResult(null);
    try {
      const [success, message] = await invoke<[boolean, string]>("run_auth", {
        command,
      });
      setAuthResult({ success, message });
      setLastError(null);
    } catch (err) {
      console.error("Failed to run auth:", err);
      const message = toErrorMessage(err, "Authentication failed");
      setAuthResult({ success: false, message });
      setLastError(message);
    } finally {
      setAuthenticatingService(null);
    }
  }, []);

  const deleteAccount = useCallback(
    async (filePath: string) => {
      try {
        await invoke("delete_auth_account", { file_path: filePath });
        setLastError(null);
      } catch (err) {
        console.error("Failed to delete account:", err);
        setLastError(toErrorMessage(err, "Failed to delete account"));
      }
    },
    [],
  );

  const saveZaiKey = useCallback(
    async (apiKey: string) => {
      try {
        await invoke("save_zai_api_key", { api_key: apiKey });
        setAuthResult({ success: true, message: "Z.AI API key saved." });
        setLastError(null);
      } catch (err) {
        console.error("Failed to save ZAI key:", err);
        setLastError(toErrorMessage(err, "Failed to save Z.AI API key"));
      }
    },
    [],
  );

  return {
    accounts,
    authenticatingService,
    authResult,
    runAuth,
    deleteAccount,
    saveZaiKey,
    lastError,
    clearLastError: () => setLastError(null),
  };
}
