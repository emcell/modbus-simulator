import { useState } from "react";

export function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);

  const onClick = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard API can be blocked (non-https, permissions). Fall back.
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand("copy");
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1500);
      } catch {
        /* give up silently */
      }
      ta.remove();
    }
  };

  return (
    <button type="button" onClick={onClick} title="Copy to clipboard">
      {copied ? "✓ Copied" : "Copy"}
    </button>
  );
}
