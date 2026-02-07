export function toErrorMessage(err: unknown, fallback: string): string {
  if (typeof err === "string" && err.trim() !== "") {
    return err;
  }

  if (err instanceof Error && err.message.trim() !== "") {
    return err.message;
  }

  return `${fallback}: ${String(err)}`;
}
