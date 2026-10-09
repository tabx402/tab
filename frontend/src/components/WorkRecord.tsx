import { useEffect, useState } from "react";
import { request } from "../lib/api";
import type { WorkMetrics } from "../lib/api";

import { formatAmount } from "../lib/amounts";
const amount = (value: string | undefined) => formatAmount(value, 2);

export function WorkRecord() {
  const [metrics, setMetrics] = useState<WorkMetrics | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    const refresh = () => void request<WorkMetrics>("/metrics").then(value => {
      if (alive) { setMetrics(value); setFailed(false); }
    }).catch(() => { if (alive) setFailed(true); });
    refresh();
    const timer = setInterval(() => { if (!document.hidden) refresh(); }, 15000);
    return () => { alive = false; clearInterval(timer); };
  }, []);
  return <div className="panel home-work-record">
    <h2>work and payments</h2>
    <dl className="home-work-metrics" aria-label="Verified work metrics">
      <div><dt>earning bonded agents</dt><dd>{metrics?.bonded_agents_earning_payments ?? "—"}</dd></div>
      <div><dt>commitments</dt><dd>{metrics ? `${metrics.completed_commitments} / ${metrics.missed_commitments}` : "—"}<small>completed / missed</small></dd></div>
      <div><dt>customer payments</dt><dd>{amount(metrics?.customer_payments_usdt)}<small>USDT received</small></dd></div>
      <div><dt>actual buybacks</dt><dd>{amount(metrics?.buyback_usdt_spent)}<small>USDT spent · {metrics?.buyback_tokens_acquired ?? "—"} TAB acquired</small></dd></div>
    </dl>
    <p className="home-metrics-status" role="status">{failed ? "Metrics unavailable. Recorded values may be out of date." : !metrics ? "Loading verified work metrics…" : metrics.settlement_status === "not_deployed" ? "Settlement not deployed. Only verified receipts count." : `Recorded work · ${metrics.source}.`}</p>
  </div>;
}
