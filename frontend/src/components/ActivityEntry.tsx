import { Link } from "react-router-dom";
import { ArrowUpRight, ChevronDown } from "lucide-react";
import { isRecord } from "../lib/api";
import { explorer, useChainId } from "../lib/evm";
import { formatAmount } from "../lib/amounts";
import { recordedCosts, type ActivityGroup } from "../lib/activity";

export function ActivityEntry({ group }: { group: ActivityGroup }) {
  const chainId = useChainId();
  const { latest, outcome, status, events, runId } = group;
  const costs = recordedCosts(events);
  const preview = isRecord(outcome.preview) ? outcome.preview : null;
  const summary = typeof preview?.summary === "string" ? preview.summary : outcome.message;
  const label = (value: string) => value.replaceAll("_", " ");
  return <details className="activity-entry" data-run-id={runId ?? undefined}>
    <summary className="readable-log-row" data-status={status} data-kind={outcome.kind}>
      <time dateTime={latest.timestamp} title={new Date(latest.timestamp).toLocaleString()}>{new Date(latest.timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</time>
      <div className="activity-agent"><strong>{latest.agent}</strong><small>{runId ? `agent run · ${events.length} events` : label(outcome.kind)}</small></div>
      <p className="activity-result">{summary.length > 145 ? `${summary.slice(0, 145)}…` : summary}</p>
      <span className="activity-cost">{costs.usdt !== null && <span>{formatAmount(costs.usdt)} USDT</span>}{costs.credits !== null && <span>{formatAmount(costs.credits)} USD credits</span>}{costs.usdt === null && costs.credits === null && <span>—</span>}</span>
      <span className="activity-state" data-status={status}>{label(status)}</span><ChevronDown size={14} />
    </summary>
    <div className="activity-details">
      {runId && <><p className="mono">run {runId}</p><Link className="text-link" to={`/agents/${latest.agent_id}/runs/${runId}`}>open run receipt <ArrowUpRight size={12} /></Link></>}
      {events.slice().reverse().map(event => <div className="activity-detail-event" key={event.id}><time dateTime={event.timestamp}>{new Date(event.timestamp).toLocaleTimeString()}</time><div><strong>{label(event.kind)} · {label(event.status)}</strong><p>{event.message}</p>{event.provider && <small>{event.provider}</small>}{event.amount && event.currency && <p>{event.amount} {event.currency}{event.currency === "USD" ? " in provider credits" : ""}</p>}</div>{event.tx_hash ? <a className="text-link" href={explorer(event.tx_hash, "tx", chainId)} target="_blank" rel="noreferrer" aria-label="View payment transaction">transaction <ArrowUpRight size={12} /></a> : <span />}</div>)}
      {summary !== outcome.message && <p>{summary}</p>}
    </div>
  </details>;
}
