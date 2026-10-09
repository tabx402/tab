import { useState } from "react";
import { Download, ChevronDown, ArrowUpRight } from "lucide-react";
import type { Receipt } from "../lib/api";
import { money } from "../lib/api";
export function Ledger({
  receipts,
  full = false,
  onReceipt,
}: {
  receipts: Receipt[];
  full?: boolean;
  onReceipt: (r: Receipt) => void;
}) {
  const [kind, setKind] = useState("all");
  const filtered = receipts.filter((r) => kind === "all" || r.kind === kind);
  function download() {
    const blob = new Blob([JSON.stringify(filtered, null, 2)], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "tab-example-receipts.json";
    a.click();
    URL.revokeObjectURL(url);
  }
  return (
    <section className={`panel terminal ${full ? "full-terminal" : ""}`}>
      <div className="panel-heading">
        <div>
          <span className="eyebrow">every call leaves a receipt</span>
          <h2>
            the open ledger <span className="tiny-dot" />
          </h2>
        </div>
        <div className="inline-controls">
          <button
            className="icon-button"
            aria-label="Download visible receipts"
            onClick={download}
          >
            <Download size={16} />
          </button>
          {full && <span className="eyebrow">snapshot</span>}
        </div>
      </div>
      <div className="terminal-toolbar">
        <div className="terminal-dots">
          <i />
          <i />
          <i />
          <span className="mono">receipts</span>
        </div>
        <label className="select-label">
          <select
            aria-label="Filter receipt type"
            value={kind}
            onChange={(e) => setKind(e.target.value)}
          >
            <option value="all">all events</option>
            <option value="spend">provider spend</option>
            <option value="repayment">repayments</option>
            <option value="limit">credit limits</option>
          </select>
          <ChevronDown size={12} />
        </label>
      </div>
      <div className="log-lines">
        {filtered.length ? (
          filtered.slice(0, full ? 100 : 5).map((r) => (
            <button
              className="log-line"
              key={r.id}
              onClick={() => onReceipt(r)}
            >
              <time>{new Date(r.timestamp).toISOString().slice(11, 19)}</time>
              <span className={`event-tag ${r.kind}`}>
                {r.kind === "spend"
                  ? "spent"
                  : r.kind === "limit"
                    ? "limit set"
                    : "repaid"}
              </span>
              <strong>{r.agent}</strong>
              <span className="log-description">
                {r.provider
                  ? `${r.provider} / ${r.description.split(" / ")[1]}`
                  : r.description}
              </span>
              <span className={r.kind === "repayment" ? "mint" : "blue"}>
                {r.kind === "repayment" ? "+" : r.kind === "spend" ? "−" : "="}
                {money(r.amount)} <small>USDT</small>
              </span>
              <ArrowUpRight size={13} />
            </button>
          ))
        ) : (
          <p className="empty">no receipts to display.</p>
        )}
      </div>
      <div className="terminal-foot">
        <span className="mono">
          {receipts.length
            ? "example dataset · no funds moved"
            : "public ledger · awaiting first settlement"}
        </span>
        <span className="mono">
          {filtered.length.toString().padStart(2, "0")} events
        </span>
      </div>
    </section>
  );
}
