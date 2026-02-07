import { useState } from "react";

interface QwenEmailDialogProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (email: string) => void;
}

export default function QwenEmailDialog({
  isOpen,
  onClose,
  onSubmit,
}: QwenEmailDialogProps) {
  const [email, setEmail] = useState("");

  if (!isOpen) return null;

  const handleSubmit = () => {
    onSubmit(email);
    setEmail("");
  };

  const handleClose = () => {
    setEmail("");
    onClose();
  };

  return (
    <div className="modal-overlay" onClick={handleClose}>
      <div className="modal-content" onClick={(e) => e.stopPropagation()}>
        <h3 className="modal-title">Qwen Account Email</h3>
        <p className="modal-subtitle">
          Enter your Qwen account email address
        </p>
        <input
          type="email"
          className="modal-input"
          placeholder="your.email@example.com"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && email.trim() !== "") handleSubmit();
          }}
          autoComplete="email"
          autoFocus
        />
        <div className="modal-buttons">
          <button type="button" className="btn btn-cancel" onClick={handleClose}>
            Cancel
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={email.trim() === ""}
            onClick={handleSubmit}
          >
            Continue
          </button>
        </div>
      </div>
    </div>
  );
}
