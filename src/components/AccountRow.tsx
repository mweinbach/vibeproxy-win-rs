import type { AuthAccount } from "../types";

interface AccountRowProps {
  account: AuthAccount;
  serviceName: string;
  onRemove: () => void;
}

export default function AccountRow({
  account,
  serviceName,
  onRemove,
}: AccountRowProps) {
  const handleRemove = () => {
    if (
      window.confirm(
        `Are you sure you want to remove ${account.display_name} from ${serviceName}?`
      )
    ) {
      onRemove();
    }
  };

  return (
    <div className={`account-row ${account.is_expired ? "is-expired" : ""}`}>
      <span
        className={`account-dot ${account.is_expired ? "expired" : "active"}`}
      />
      <span className="account-name" title={account.display_name}>
        {account.display_name}
      </span>
      {account.is_expired && (
        <span className="expired-badge">(expired)</span>
      )}
      <button type="button" className="btn-remove" onClick={handleRemove}>
        Remove
      </button>
    </div>
  );
}
