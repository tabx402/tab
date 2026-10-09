import { ArrowUpRight, Check, Clock3, ShieldCheck } from "lucide-react";
import { isRecord } from "../lib/api";
import type { AgentRun, RuntimeAgent } from "../lib/api";
import { amountFromUnits, formatAmount, usdtUnits } from "../lib/amounts";
import "./feature-flow.css";

const label = (key: string) => ({ chain: "chain data", research: "web research", openrouter: "model summary", x402: "paid tools", error: "run error" })[key] ?? key.replaceAll("_", " ");
const text = (value: unknown) => typeof value === "string" ? value : typeof value === "number" ? String(value) : "";
const statusClass = (value: string) => ["failed", "unavailable", "not_connected", "interrupted"].includes(value) ? "failure" : ["partial", "requires_authorization", "budget_reached"].includes(value) ? "attention" : ["completed", "confirmed", "settled"].includes(value) ? "success" : "neutral";

export function RunReceipt({ run, agent }: { run: AgentRun; agent?: RuntimeAgent }) {
  const output = isRecord(run.output) ? run.output : {};
  const entries = Object.entries(output);
  const model = isRecord(output.openrouter) ? output.openrouter : null;
  const chain = isRecord(output.chain) ? output.chain : null;
  const paid = isRecord(output.x402) ? output.x402 : null;
  const hash = text(paid?.tx_hash ?? paid?.transaction_hash);
  const transaction = /^0x[0-9a-fA-F]{64}$/.test(hash) ? hash : null;
  const providerCost = text(model?.cost_usd);
  const research = isRecord(output.research) ? output.research : null;
  const researchCost = text(research?.cost_usd);
  const providerCosts = [providerCost, researchCost].filter(cost => cost && usdtUnits(cost) !== null);
  const providerTotal = amountFromUnits(providerCosts.reduce((sum, cost) => sum + (usdtUnits(cost) ?? 0n), 0n));
  const outcome = run.status === "partial" ? "some work returned" : run.status === "completed" ? "run complete" : run.status === "failed" ? "run failed" : run.status.replaceAll("_", " ");
  return <article className="run-receipt" data-testid="run-receipt">
    <div className="run-receipt-top"><span className={`flow-badge ${statusClass(run.status)}`}>{run.status === "completed" ? <Check size={13} /> : <Clock3 size={13} />}{outcome}</span><time dateTime={run.started_at}>{new Date(run.started_at).toLocaleString()}</time></div>
    <div className="run-receipt-task"><span className="eyebrow">task</span><p>{agent?.purpose || "Run the agent’s saved task."}</p></div>
    <ol className="receipt-steps">
      <li><span className="receipt-step-number">1</span><div><h4>tools</h4><p>{entries.length ? entries.filter(([key]) => key !== "error").map(([key]) => label(key)).join(" · ") || "No tool returned a result." : "No tool returned a result."}</p></div></li>
      <li><span className="receipt-step-number">2</span><div><h4>cost</h4><p>{paid?.settled_usdt === true && typeof paid.amount === "string" && transaction ? `${formatAmount(paid.amount)} USDT settled` : "No USDT settlement recorded in this run."}</p>{providerCost && <p className="receipt-detail">{formatAmount(providerCost)} USD in model-provider credits · separate from your USDT budget</p>}{researchCost && <p className="receipt-detail">{formatAmount(researchCost)} USD in research-provider credits · separate from your USDT budget</p>}{providerCosts.length > 1 && <p className="receipt-detail">{formatAmount(providerTotal)} USD total provider-credit cost</p>}{chain && !model && !research && !paid && <p className="receipt-detail">Read-only chain call · 0 USDT</p>}</div></li>
      <li><span className="receipt-step-number">3</span><div><h4>result</h4>
        {chain && <div className="receipt-chain"><p><strong>{formatAmount(text(chain.usdt))} USDT</strong> · {formatAmount(text(chain.bnb))} BNB</p><p className="receipt-detail">BNB mainnet · block {text(chain.block ?? chain.block_number)}</p>{isAddressValue(chain.wallet) && <a className="text-link" href={`https://bscscan.com/address/${chain.wallet}`} target="_blank" rel="noreferrer">wallet on explorer <ArrowUpRight size={12} /></a>}</div>}
        {entries.filter(([key]) => key !== "chain").map(([key, value]) => {
          const record = isRecord(value) ? value : null;
          const status = text(record?.status);
          const sources = Array.isArray(record?.sources) ? record.sources.filter(isRecord) : [];
          return <section className="receipt-tool-result" key={key}><div><span>{label(key)}</span>{status && <span className={`flow-badge ${statusClass(status)}`}>{status.replaceAll("_", " ")}</span>}</div><p>{typeof value === "string" ? value : text(record?.summary ?? record?.message ?? record?.error) || (sources.length ? `${sources.length} sources returned.` : status ? status.replaceAll("_", " ") : "No readable output returned.")}</p>{sources.length > 0 && <ul>{sources.slice(0, 5).map((source, index) => { const url = text(source.url); return <li key={url || index}>{/^https?:\/\//.test(url) ? <a href={url} target="_blank" rel="noreferrer">{text(source.title) || url}<ArrowUpRight size={12} /></a> : text(source.title) || "source"}</li>; })}</ul>}</section>;
        })}
        {entries.length === 0 && <p>No result recorded yet.</p>}
      </div></li>
      <li><span className="receipt-step-number"><ShieldCheck size={15} /></span><div><h4>authorization</h4><p>{paid?.status === "requires_authorization" ? "Review the paid request below. Your wallet authorization is still required." : paid?.settled_usdt === true && transaction ? "A payment receipt is attached. Review the transaction for its settlement details." : "This run does not grant wallet payment permissions."}</p>{run.status === "partial" && <p className="receipt-detail">Review the unavailable or pending tool above before counting the task as finished.</p>}{transaction && <a className="text-link" href={`https://bscscan.com/tx/${transaction}`} target="_blank" rel="noreferrer">view transaction <ArrowUpRight size={13} /></a>}</div></li>
    </ol>
    <details className="receipt-raw"><summary>technical details</summary><pre className="run-output">{JSON.stringify(output, null, 2)}</pre></details>
  </article>;
}
function isAddressValue(value: unknown): value is string { return typeof value === "string" && /^0x[0-9a-fA-F]{40}$/.test(value); }
