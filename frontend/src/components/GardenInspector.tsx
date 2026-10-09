import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { ArrowUpRight, X } from "lucide-react";
import { isRecord, request } from "../lib/api";
import { formatAmount } from "../lib/amounts";

export type GardenSelection = { key: string; name: string; meta: string; href?: string; runs: number; paid: string | null; lastEvent: number };

export function GardenInspector({ selection, close }: { selection: GardenSelection; close: () => void }) {
  const [result, setResult] = useState<{ key: string; text: string; status?: string; href?: string } | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    if (!selection.href) return;
    void (async () => {
      const profile = await request<{ runs?: { id: string }[] }>(`/agents/live/${encodeURIComponent(selection.key)}`, { signal: controller.signal });
      const run = profile.runs?.[0];
      if (!run) { setResult({ key: selection.key, text: "No public run recorded yet." }); return; }
      const receipt = await request<{ status: string; output: unknown }>(`/agents/live/${encodeURIComponent(selection.key)}/runs/${encodeURIComponent(run.id)}`, { signal: controller.signal });
      const output = isRecord(receipt.output) ? receipt.output : {};
      const chain = isRecord(output.chain) ? output.chain : null;
      const text = typeof output.summary === "string" ? output.summary : chain ? `Chain check returned${typeof chain.block === "number" ? ` at block ${chain.block.toLocaleString()}` : ""}.` : "Open the run to review its recorded outcome.";
      setResult({ key: selection.key, text, status: receipt.status, href: `/agents/${selection.key}/runs/${run.id}` });
    })().catch(() => { if (!controller.signal.aborted) setResult({ key: selection.key, text: "The latest public result is temporarily unavailable." }); });
    return () => controller.abort();
  }, [selection.key, selection.href, selection.lastEvent]);
  const current = result?.key === selection.key ? result : null;
  return <aside className="garden-inspector" aria-label={`Selected agent: ${selection.name}`}>
    <button className="icon-button" aria-label="Close agent details" onClick={close}><X size={17} /></button>
    <h3>{selection.name}</h3><p>{selection.meta}</p>
    {selection.href ? <><dl><div><dt>completed runs</dt><dd>{selection.runs}</dd></div><div><dt>settled payments</dt><dd>{selection.paid === null ? "unavailable" : `${formatAmount(selection.paid)} USDT`}</dd></div></dl><div className="selected-result" aria-live="polite"><span>latest public result{current?.status ? ` · ${current.status.replaceAll("_", " ")}` : ""}</span><p>{current ? current.text.length > 210 ? `${current.text.slice(0, 210)}…` : current.text : "Loading the latest public run…"}</p>{current?.href && <Link className="text-link" to={current.href}>read the run <ArrowUpRight size={13} /></Link>}</div><Link className="text-link" to={selection.href}>open agent <ArrowUpRight size={14} /></Link></> : <p>{selection.key === "registry" ? "Each branch represents a registered agent. Select an agent to inspect its shared work." : "This registration has no public agent profile yet."}</p>}
  </aside>;
}
