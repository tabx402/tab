import { useEffect, useRef, useState } from "react";
import { ArrowUpRight, Check, Copy } from "lucide-react";
import "./official-token.css";

const TAB_ADDRESS = "0xf07449517ae4b48808098c573a5347e67c714444";

export function OfficialToken() {
  const [status, setStatus] = useState<"idle" | "copying" | "copied" | "manual">("idle");
  const reset = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(reset.current), []);

  async function copyAddress() {
    clearTimeout(reset.current);
    setStatus("copying");
    try {
      await navigator.clipboard.writeText(TAB_ADDRESS);
      setStatus("copied");
      reset.current = setTimeout(() => setStatus("idle"), 2200);
    } catch {
      setStatus("manual");
    }
  }

  return (
    <div className="official-token" role="group" aria-label="Official TAB token contract on BNB Smart Chain">
      <div className="official-token-row">
        <span className="official-token-label">official <strong>$TAB</strong></span>
        <code className="official-token-address" title={TAB_ADDRESS}>
          <span className="official-token-full" aria-hidden="true">{TAB_ADDRESS}</span>
          <span className="official-token-short" aria-hidden="true">{TAB_ADDRESS.slice(0, 8)}…{TAB_ADDRESS.slice(-6)}</span>
          <span className="official-token-sr">{TAB_ADDRESS}</span>
        </code>
        <button type="button" className="official-token-action" onClick={() => void copyAddress()} disabled={status === "copying"} aria-label="Copy TAB contract address" title="Copy contract address" data-copied={status === "copied"}>
          {status === "copied" ? <Check size={14} aria-hidden="true" /> : <Copy size={14} aria-hidden="true" />}
        </button>
        <a className="official-token-action" href={`https://bscscan.com/token/${TAB_ADDRESS}`} target="_blank" rel="noopener noreferrer" aria-label="View TAB on BscScan (opens in a new tab)" title="View on BscScan">
          <ArrowUpRight size={15} aria-hidden="true" />
        </a>
      </div>
      <span className="official-token-feedback" role="status" aria-live="polite">
        {status === "copied" ? "address copied" : status === "manual" ? "copy unavailable. select the address below." : ""}
      </span>
      {status === "manual" && <input className="official-token-manual" aria-label="Full TAB contract address" value={TAB_ADDRESS} readOnly autoFocus onFocus={event => event.currentTarget.select()} />}
    </div>
  );
}
