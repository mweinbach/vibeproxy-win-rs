import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ServerState, BinaryDownloadProgress } from "../types";
import { toErrorMessage } from "../utils/error";

const DEFAULT_SERVER_STATE: ServerState = {
  is_running: false,
  proxy_port: 8317,
  backend_port: 8318,
  binary_available: false,
  binary_downloading: false,
};

export function useServerState() {
  const [serverState, setServerState] = useState<ServerState | null>(null);
  const [downloadProgress, setDownloadProgress] =
    useState<BinaryDownloadProgress | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;

    const loadServerState = async () => {
      try {
        const state = await invoke<ServerState>("get_server_state");
        if (mounted) {
          setServerState(state);
          setLastError(null);
        }
      } catch (err) {
        console.error("Failed to get server state:", err);
        if (mounted) {
          setLastError(toErrorMessage(err, "Failed to load server state"));
        }
        try {
          const binaryAvailable = await invoke<boolean>("check_binary");
          if (mounted) {
            setServerState({
              ...DEFAULT_SERVER_STATE,
              binary_available: binaryAvailable,
            });
          }
        } catch (binaryErr) {
          console.error("Failed to check binary availability:", binaryErr);
          if (mounted) {
            setServerState(DEFAULT_SERVER_STATE);
          }
        }
      }
    };

    loadServerState();

    const unlistenStatus = listen<ServerState>(
      "server_status_changed",
      (event) => {
        setServerState(event.payload);
      },
    );

    const unlistenDownload = listen<BinaryDownloadProgress>(
      "binary_download_progress",
      (event) => {
        setDownloadProgress(event.payload);
      },
    );

    return () => {
      mounted = false;
      unlistenStatus.then((fn) => fn());
      unlistenDownload.then((fn) => fn());
    };
  }, []);

  const startServer = useCallback(async () => {
    try {
      await invoke("start_server");
      setLastError(null);
    } catch (err) {
      console.error("Failed to start server:", err);
      setLastError(toErrorMessage(err, "Failed to start server"));
    }
  }, []);

  const stopServer = useCallback(async () => {
    try {
      await invoke("stop_server");
      setLastError(null);
    } catch (err) {
      console.error("Failed to stop server:", err);
      setLastError(toErrorMessage(err, "Failed to stop server"));
    }
  }, []);

  const downloadBinary = useCallback(async () => {
    try {
      await invoke("download_binary");
      setLastError(null);
    } catch (err) {
      console.error("Failed to download binary:", err);
      setLastError(toErrorMessage(err, "Failed to download runtime"));
    }
  }, []);

  return {
    serverState,
    downloadProgress,
    startServer,
    stopServer,
    downloadBinary,
    lastError,
    clearLastError: () => setLastError(null),
  };
}
