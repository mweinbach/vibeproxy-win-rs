import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { check, type DownloadEvent } from "@tauri-apps/plugin-updater";

const LAST_UPDATE_CHECK_STORAGE_KEY = "codeforwarder.lastUpdateCheckAt";
const AUTO_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

type UpdaterStatus =
  | "idle"
  | "checking"
  | "up_to_date"
  | "downloading"
  | "ready_to_restart"
  | "error";

type CheckForUpdatesOptions = {
  manual?: boolean;
};

function isTauriRuntime(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  return "__TAURI_INTERNALS__" in window;
}

function readLastUpdateCheckAt(): number | null {
  if (typeof window === "undefined") {
    return null;
  }
  const rawValue = window.localStorage.getItem(LAST_UPDATE_CHECK_STORAGE_KEY);
  if (!rawValue) {
    return null;
  }
  const parsed = Number(rawValue);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return null;
  }
  return parsed;
}

function persistLastUpdateCheckAt(value: number): void {
  if (typeof window === "undefined") {
    return;
  }
  window.localStorage.setItem(LAST_UPDATE_CHECK_STORAGE_KEY, String(value));
}

export function useUpdater() {
  const [status, setStatus] = useState<UpdaterStatus>("idle");
  const [lastCheckedAt, setLastCheckedAt] = useState<number | null>(
    readLastUpdateCheckAt
  );
  const [availableVersion, setAvailableVersion] = useState<string | null>(null);
  const [downloadedBytes, setDownloadedBytes] = useState(0);
  const [contentLength, setContentLength] = useState<number | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);
  const isCheckingRef = useRef(false);

  const checkForUpdates = useCallback(
    async ({ manual = false }: CheckForUpdatesOptions = {}) => {
      if (!isTauriRuntime()) {
        return;
      }
      if (isCheckingRef.current) {
        return;
      }

      const now = Date.now();
      if (
        !manual &&
        lastCheckedAt !== null &&
        now - lastCheckedAt < AUTO_CHECK_INTERVAL_MS
      ) {
        return;
      }

      isCheckingRef.current = true;
      setStatus("checking");
      setLastError(null);
      setAvailableVersion(null);
      setDownloadedBytes(0);
      setContentLength(null);

      try {
        const update = await check();
        const checkedAt = Date.now();
        persistLastUpdateCheckAt(checkedAt);
        setLastCheckedAt(checkedAt);

        if (!update) {
          setStatus("up_to_date");
          return;
        }

        setAvailableVersion(update.version);
        setStatus("downloading");

        await update.downloadAndInstall((event: DownloadEvent) => {
          if (event.event === "Started") {
            setDownloadedBytes(0);
            setContentLength(event.data.contentLength ?? null);
            return;
          }

          if (event.event === "Progress") {
            setDownloadedBytes((previous) => previous + event.data.chunkLength);
          }
        });

        setStatus("ready_to_restart");
        await update.close();
      } catch (error) {
        setStatus("error");
        setLastError(
          error instanceof Error ? error.message : "Failed to update app"
        );
      } finally {
        isCheckingRef.current = false;
      }
    },
    [lastCheckedAt]
  );

  useEffect(() => {
    void checkForUpdates();
  }, [checkForUpdates]);

  const progressPercent = useMemo(() => {
    if (
      status !== "downloading" ||
      contentLength === null ||
      contentLength <= 0
    ) {
      return null;
    }

    return Math.max(0, Math.min(100, Math.round((downloadedBytes / contentLength) * 100)));
  }, [contentLength, downloadedBytes, status]);

  return {
    status,
    lastCheckedAt,
    availableVersion,
    progressPercent,
    lastError,
    checkForUpdates,
  };
}
