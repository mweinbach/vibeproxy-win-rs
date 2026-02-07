import { useState } from "react";

interface ZaiApiKeyDialogProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (apiKey: string) => void;
}

export default function ZaiApiKeyDialog({
  isOpen,
  onClose,
  onSubmit,
}: ZaiApiKeyDialogProps) {
  const [apiKey, setApiKey] = useState("");

  if (!isOpen) return null;

  const handleSubmit = () => {
    onSubmit(apiKey);
    setApiKey("");
  };

  const handleClose = () => {
    setApiKey("");
    onClose();
  };

  return (
    <div className="modal-overlay" onClick={handleClose}>
      <div className="modal-content" onClick={(e) => e.stopPropagation()}>
        <h3 className="modal-title">Z.AI API Key</h3>
        <p className="modal-subtitle">
          Enter your Z.AI API key from{" "}
          <a
            href="https://z.ai/manage-apikey/apikey-list"
            target="_blank"
            rel="noreferrer"
          >
            z.ai
          </a>
        </p>
        <input
          type="text"
          className="modal-input"
          placeholder="Paste your API key"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && apiKey.trim() !== "") handleSubmit();
          }}
          autoComplete="off"
          autoFocus
        />
        <div className="modal-buttons">
          <button type="button" className="btn btn-cancel" onClick={handleClose}>
            Cancel
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={apiKey.trim() === ""}
            onClick={handleSubmit}
          >
            Add Key
          </button>
        </div>
      </div>
    </div>
  );
}
