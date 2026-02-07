import { useEffect, useRef, useState } from "react";

interface VercelGatewayControlsProps {
  enabled: boolean;
  apiKey: string;
  onSave: (enabled: boolean, apiKey: string) => void;
}

export default function VercelGatewayControls({
  enabled,
  apiKey,
  onSave,
}: VercelGatewayControlsProps) {
  const [localEnabled, setLocalEnabled] = useState(enabled);
  const [localKey, setLocalKey] = useState(apiKey);
  const [showingSaved, setShowingSaved] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => {
    setLocalEnabled(enabled);
  }, [enabled]);

  useEffect(() => {
    setLocalKey(apiKey);
  }, [apiKey]);

  useEffect(() => {
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    };
  }, []);

  const handleToggle = (checked: boolean) => {
    setLocalEnabled(checked);
    if (!checked) {
      onSave(false, localKey);
    }
  };

  const handleSave = () => {
    onSave(localEnabled, localKey);
    setShowingSaved(true);
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => setShowingSaved(false), 1500);
  };

  return (
    <div className="vercel-controls">
      <label className="checkbox-row">
        <input
          type="checkbox"
          checked={localEnabled}
          onChange={(e) => handleToggle(e.target.checked)}
        />
        <span>Use Vercel AI Gateway</span>
      </label>
      {localEnabled && (
        <div className="vercel-key-row">
          <span className="vercel-key-label">Vercel API key</span>
          <input
            type="password"
            className="vercel-key-input"
            placeholder="vercel_ai_xxxxx"
            value={localKey}
            onChange={(e) => setLocalKey(e.target.value)}
          />
          {showingSaved ? (
            <span className="saved-text">Saved</span>
          ) : (
            <button
              className="btn btn-sm"
              type="button"
              disabled={localKey.trim() === ""}
              onClick={handleSave}
            >
              Save
            </button>
          )}
        </div>
      )}
    </div>
  );
}
