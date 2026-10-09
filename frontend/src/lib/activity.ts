import type { AgentEvent } from "./api";
import { amountFromUnits, usdtUnits } from "./amounts";

export const paymentKinds = ["payment", "provider_payment", "payment_resolved"];
export const isSettledPayment = (event: AgentEvent) => paymentKinds.includes(event.kind) && ["confirmed", "settled", "paid_delivery_failed"].includes(event.status) && event.currency === "USDT";
export type ActivityGroup = { key: string; runId: string | null; events: AgentEvent[]; latest: AgentEvent; outcome: AgentEvent; status: string };

export function groupActivity(events: AgentEvent[]): ActivityGroup[] {
  const groups = new Map<string, AgentEvent[]>();
  for (const event of events) {
    const key = event.run_id ? `run:${event.agent_id}:${event.run_id}` : `event:${event.id}`;
    const group = groups.get(key) ?? [];
    if (!group.some(item => item.id === event.id)) group.push(event);
    groups.set(key, group);
  }
  return [...groups].map(([key, items]) => {
    items.sort((a, b) => b.id - a.id);
    const latest = items[0];
    const completed = items.find(item => ["run_completed", "run_partial", "run_failed"].includes(item.kind));
    const outcome = completed ?? latest;
    return { key, runId: latest.run_id ?? null, events: items, latest, outcome, status: completed?.status ?? (latest.run_id ? "running" : latest.status) };
  }).sort((a, b) => b.latest.id - a.latest.id);
}

export function recordedCosts(events: AgentEvent[]) {
  const settled = new Map<string, AgentEvent>();
  let credits = 0n, hasCredits = false;
  for (const event of events) {
    if (isSettledPayment(event)) settled.set(event.tx_hash ? `${event.agent_id}:${event.tx_hash.toLowerCase()}` : `event:${event.id}`, event);
    if (event.currency === "USD" && event.amount && usdtUnits(event.amount) !== null) { credits += usdtUnits(event.amount)!; hasCredits = true; }
  }
  return { usdt: settled.size ? amountFromUnits([...settled.values()].reduce((sum, event) => sum + (usdtUnits(event.amount ?? "") ?? 0n), 0n)) : null, credits: hasCredits ? amountFromUnits(credits) : null };
}
