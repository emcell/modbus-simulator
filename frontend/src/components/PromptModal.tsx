import { useCallback, useEffect, useState } from "react";
import type { FormEvent } from "react";
import { Modal } from "./Modal";

/**
 * In-app replacement for `window.prompt`.
 *
 * Native dialogs are unreliable as a primary UI path: a browser that has
 * suppressed dialogs for the page (Chrome's "prevent this page from
 * creating additional dialogs", embedded/automated contexts) makes
 * `prompt()` return `null` immediately, so the click does nothing and the
 * user gets no feedback at all. This renders inside the document, and
 * surfaces mutation errors instead of dropping them on the floor.
 */
export function PromptModal({
  open,
  title,
  label,
  placeholder,
  submitLabel = "Create",
  onCancel,
  onSubmit,
}: {
  open: boolean;
  title: string;
  label: string;
  placeholder?: string;
  submitLabel?: string;
  onCancel: () => void;
  /** Rejections are caught and shown; the modal stays open on failure. */
  onSubmit: (value: string) => Promise<void>;
}) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // Reset + focus whenever the modal is opened.
  useEffect(() => {
    if (!open) return;
    setValue("");
    setErr(null);
    setBusy(false);
    const el = document.getElementById("prompt-modal-input");
    (el as HTMLInputElement | null)?.focus();
  }, [open]);

  const trimmed = value.trim();
  const canSubmit = !busy && trimmed.length > 0;

  const submit = useCallback(
    async (e?: FormEvent) => {
      e?.preventDefault();
      if (!canSubmit) return;
      setBusy(true);
      setErr(null);
      try {
        await onSubmit(trimmed);
      } catch (ex) {
        setErr((ex as Error).message);
      } finally {
        setBusy(false);
      }
    },
    [canSubmit, trimmed, onSubmit],
  );

  return (
    <Modal open={open} title={title} onClose={busy ? () => {} : onCancel}>
      <form onSubmit={submit} className="stack">
        <label>
          {label}
          <input
            id="prompt-modal-input"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder={placeholder}
          />
        </label>
        {err && <div className="error">{err}</div>}
        <div className="modal-footer">
          <button type="button" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button type="submit" className="primary" disabled={!canSubmit}>
            {busy ? "Working…" : submitLabel}
          </button>
        </div>
      </form>
    </Modal>
  );
}
