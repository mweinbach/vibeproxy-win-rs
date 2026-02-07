interface ServerStatusProps {
  isRunning: boolean;
  binaryAvailable: boolean;
  binaryDownloading: boolean;
  downloadProgress: number | null;
  onStartStop: () => void;
  onDownloadBinary: () => void;
}

export default function ServerStatus({
  isRunning,
  binaryAvailable,
  binaryDownloading,
  downloadProgress,
  onStartStop,
  onDownloadBinary,
}: ServerStatusProps) {
  const readyCaption = isRunning
    ? "Local runtime is active and currently handling traffic."
    : "Runtime is ready. First start may take a moment while bundled files are staged.";

  if (!binaryAvailable) {
    return (
      <div className="server-status">
        <div className="status-copy">
          <span className="status-label">Proxy engine</span>
          <p className="status-caption">
            No runtime detected yet. Download the latest CLIProxyAPIPlus binary.
          </p>
          <p className="status-subhint">
            If this build includes a bundled runtime, it will be detected automatically.
          </p>
        </div>
        <div className="status-right">
          {binaryDownloading ? (
            <div className="download-progress">
              <progress
                className="download-progress-bar"
                value={downloadProgress ?? undefined}
                max={100}
              />
              <span className="progress-text">
                {downloadProgress != null
                  ? `${Math.round(downloadProgress)}% complete`
                  : "Downloading..."}
              </span>
            </div>
          ) : (
            <button className="btn btn-primary" onClick={onDownloadBinary}>
              Download Runtime
            </button>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="server-status">
      <div className="status-copy">
        <span className="status-label">Proxy engine</span>
        <p className="status-caption">{readyCaption}</p>
        <p className="status-subhint">
          Built-in runtime support is enabled for packaged builds.
        </p>
      </div>
      <div className="status-right">
        <button
          className={`btn btn-status ${isRunning ? "is-running" : "is-stopped"}`}
          onClick={onStartStop}
        >
          <span className={`status-dot ${isRunning ? "running" : "stopped"}`} />
          <span>{isRunning ? "Running" : "Stopped"}</span>
        </button>
      </div>
    </div>
  );
}
