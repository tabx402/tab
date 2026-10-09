import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Link } from "react-router-dom";
import { ArrowUpRight, Download, RefreshCw, Search } from "lucide-react";
import { request } from "../lib/api";
import { explorer, useChainId } from "../lib/evm";
import { usdtUnits, amountFromUnits, formatAmount } from "../lib/amounts";
import type { AgentEvent, RegistryData } from "../lib/api";
import type { components } from "../lib/api-schema";
const kindName = (kind: string) =>
  ({
    run_started: "run started",
    run_completed: "run finished",
    run_partial: "partial run",
    run_failed: "run failed",
    tool_result: "tool result",
    provider_payment: "payment",
    payment_required: "approval needed",
    registered: "registered",
    paused: "paused",
    resumed: "resumed",
    key_created: "access key",
    tool_unavailable: "tool unavailable",
  })[kind] ?? kind.replaceAll("_", " ");
type ActivitySource = "activity" | "registry" | "agents";
const unavailable = (event: AgentEvent) =>
  ["run_failed", "payment_failed", "tool_unavailable"].includes(event.kind) ||
  ["failed", "paid_delivery_failed", "unavailable", "not_connected", "budget_reached", "interrupted"].includes(event.status);
const partialRun = (event: AgentEvent) =>
  event.kind === "run_partial" || (event.kind === "run_completed" && event.status === "partial");
export function LiveActivity({ compact = false, controls = false, preview }: { compact?: boolean; controls?: boolean; preview?: ReactNode }) {
  const chainId = useChainId();
  const [liveAgents, setLiveAgents] = useState<
    components["schemas"]["PublicAgent"][]
  >([]);
  const [events, setEvents] = useState<AgentEvent[]>([]),
    [registry, setRegistry] = useState<RegistryData | null>(null),
    [query, setQuery] = useState(""),
    [filter, setFilter] = useState("all"),
    [updated, setUpdated] = useState<Date | null>(null);
  const [loaded, setLoaded] = useState<Record<ActivitySource, boolean>>({ activity: false, registry: false, agents: false });
  const [errors, setErrors] = useState<Record<ActivitySource, boolean>>({ activity: false, registry: false, agents: false });
  const [refreshing, setRefreshing] = useState(false);
  const mounted = useRef(false);
  const requests = useRef<Record<ActivitySource, AbortController | null>>({ activity: null, registry: null, agents: null });
  const refresh = useCallback(async () => {
    if (!mounted.current) return;
    async function load<T>(source: ActivitySource, path: string, accept: (value: T) => void) {
      if (requests.current[source]) return;
      const controller = new AbortController();
      requests.current[source] = controller;
      if (source === "activity") setRefreshing(true);
      const timeout = window.setTimeout(() => controller.abort(), 15000);
      const current = () => mounted.current && requests.current[source] === controller;
      try {
        const value = await request<T>(path, { signal: controller.signal, cache: "no-store" });
        if (!current()) return;
        accept(value);
        setLoaded(previous => ({ ...previous, [source]: true }));
        setErrors(previous => ({ ...previous, [source]: false }));
        if (source === "activity") setUpdated(new Date());
      } catch {
        if (current()) setErrors(previous => ({ ...previous, [source]: true }));
      } finally {
        clearTimeout(timeout);
        if (current()) {
          requests.current[source] = null;
          if (source === "activity") setRefreshing(false);
        }
      }
    }
    await Promise.allSettled([
      load<AgentEvent[]>("activity", "/activity?limit=500", setEvents),
      load<RegistryData>("registry", "/registry", value => {
        if (value.status === "unavailable") throw new Error("Registry unavailable.");
        setRegistry(value);
      }),
      load<components["schemas"]["PublicAgent"][]>("agents", "/agents/live", setLiveAgents),
    ]);
  }, []);
  useEffect(() => {
    mounted.current = true;
    void refresh();
    const timer = setInterval(() => {
      if (!document.hidden) void refresh();
    }, 5000);
    const onVisibility = () => { if (!document.hidden) void refresh(); };
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      mounted.current = false;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibility);
      for (const source of ["activity", "registry", "agents"] as const) {
        requests.current[source]?.abort();
        requests.current[source] = null;
      }
    };
  }, [refresh]);
  const filtered = events.filter(
    (e) =>
      (filter === "all" ||
        (filter === "payments"
          ? ["provider_payment", "payment_resolved"].includes(e.kind)
          : filter === "runs"
            ? e.kind.startsWith("run_")
            : filter === "attention" ? unavailable(e) || partialRun(e) || e.status === "awaiting_approval" : e.kind === "registered")) &&
      `${e.agent} ${e.message} ${e.provider ?? ""}`
        .toLowerCase()
        .includes(query.toLowerCase()),
  );
  const payments = events.filter(
    (e) =>
      ["provider_payment", "payment_resolved"].includes(e.kind) &&
      ["confirmed", "settled", "paid_delivery_failed"].includes(e.status) &&
      e.currency === "USDT",
  );
  const completed = events.filter(e => e.kind === "run_completed" && e.status === "completed");
  const partial = events.filter(partialRun);
  const failures = events.filter(unavailable);
  const approvals = events.filter(e => e.status === "awaiting_approval");
  const settledPayments = new Map(payments.map(event => [event.tx_hash ? `${event.agent_id}:${event.tx_hash.toLowerCase()}` : `event:${event.id}`, event]));
  const paid = [...settledPayments.values()].reduce((total, event) => total + (usdtUnits(event.amount ?? "") ?? 0n), 0n);
  const exportLog = () => {
    const url = URL.createObjectURL(
      new Blob(
        [
          JSON.stringify(
            {
              events: filtered,
              registration_transactions: registry?.transactions ?? [],
            },
            null,
            2,
          ),
        ],
        { type: "application/json" },
      ),
    );
    const a = document.createElement("a");
    a.href = url;
    a.download = "tab-activity.json";
    a.click();
    URL.revokeObjectURL(url);
  };
  return (
    <section className={`panel live-activity ${compact ? "compact" : ""} ${preview ? "with-preview" : ""}`}>
      <div className="panel-heading">
        <div>
          <h2>{compact ? "latest activity" : "log"}</h2>
        </div>
      </div>
      {!compact && (
        <>
          <div className="log-summary">
            <div>
              <strong>
                {
                  loaded.registry && loaded.agents ? new Set([
                    ...(registry?.agents.map((a) => `${registry.address}:${a.id}`) ?? []),
                    ...liveAgents.map((a) => `${a.registry_address ?? registry?.address ?? "registry"}:${a.registry_id}`),
                  ]).size : "—"
                }
              </strong>
              <span>registered agents</span>
            </div>
            <div>
              <strong>
                {
                  loaded.agents ? liveAgents.filter(
                    (a) => a.cadence !== "manual" && a.status === "ready",
                  ).length : "—"
                }
              </strong>
              <span>scheduled agents</span>
            </div>
            <div>
              <strong>
                {loaded.activity ? completed.length : "—"}
              </strong>
              <span>recent completed runs</span>
            </div>
            <div>
              <strong>
                {loaded.activity ? formatAmount(amountFromUnits(paid), 2) : "—"}{" "}
                <small>USDT</small>
              </strong>
              <span>settled payments in this log</span>
            </div>
          </div>
          <div className="activity-health" aria-label="Recent activity outcomes">
            <div data-tone="success"><strong>{loaded.activity ? completed.length : "—"}</strong><span>completed runs</span></div>
            <div data-tone="attention"><strong>{loaded.activity ? partial.length : "—"}</strong><span>partial runs</span></div>
            <div data-tone="attention"><strong>{loaded.activity ? approvals.length : "—"}</strong><span>approval requests</span></div>
            <div data-tone="failure"><strong>{loaded.activity ? failures.length : "—"}</strong><span>failed or unavailable</span></div>
            <p>Recorded events in the latest 500 entries. A run can include several tool events. Approval requests may have been resolved since they were recorded.</p>
          </div>
        </>
      )}
      <div className="activity-refresh">
        <div className="live-indicator" role="status">
          <i className={errors.activity || !loaded.activity ? "offline" : ""} />
          {errors.activity ? "connection interrupted" : loaded.activity ? "updates every 5s" : "connecting…"}
          <button
            className="icon-button"
            aria-label="Refresh activity"
            disabled={refreshing}
            onClick={() => void refresh()}
          >
            <RefreshCw size={14} />
          </button>
        </div>
      </div>
      {(!compact || controls) && <div className="log-toolbar">
            <label>
              <Search size={14} />
              <input
                aria-label="Search live activity"
                placeholder="agent, tool or event"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
            <select
              aria-label="Filter live activity"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            >
              <option value="all">all activity</option>
              <option value="runs">runs</option>
              <option value="payments">payments</option>
              <option value="registrations">registrations</option>
              <option value="attention">needs attention</option>
            </select>
            <button className="outline" disabled={!loaded.activity} onClick={exportLog}>
              <Download size={14} />
              export
            </button>
          </div>}
      {preview}
      {errors.activity && (
        <p role="alert" className="form-error">
          {loaded.activity ? "Activity could not be refreshed. Showing the last recorded log." : "Activity is temporarily unavailable. Try refreshing."}
        </p>
      )}
      {errors.registry && <p role="alert" className="form-error">Registration data is temporarily unavailable. {loaded.registry ? "Registered totals may be out of date." : "Registered totals are not available yet."}</p>}
      {errors.agents && <p role="alert" className="form-error">Agent data is temporarily unavailable. {loaded.agents ? "Agent totals may be out of date." : "Agent totals are not available yet."}</p>}
      <div className="readable-log">
        {filtered.slice(0, compact ? 3 : 500).map((e) => (
          <div className="readable-log-row" data-status={e.status} data-kind={e.kind} key={e.id}>
            <time title={e.timestamp}>
              {new Date(e.timestamp).toLocaleTimeString([], {
                hour: "2-digit",
                minute: "2-digit",
              })}
            </time>
            <span className={`log-kind ${e.kind}`} data-status={e.status}>{partialRun(e) ? "partial run" : kindName(e.kind)}</span>
            <div>
              <strong>{e.agent}</strong>
              <p>{e.message}</p>
            </div>
            <span className="event-outcome" data-status={e.status}>
              {e.amount && e.currency
                ? `${e.amount} ${e.currency}`
                : e.status.replaceAll("_", " ")}
            </span>
            {e.tx_hash ? (
              <a
                aria-label="View payment transaction"
                href={explorer(e.tx_hash, "tx", chainId)}
                target="_blank"
                rel="noreferrer"
              >
                <ArrowUpRight size={14} />
              </a>
            ) : (
              <span />
            )}
          </div>
        ))}
        {!filtered.length && (
          <div className="empty">
            {!loaded.activity
              ? errors.activity ? "Activity is unavailable right now." : "Loading public activity…"
              : events.length
                ? "No activity matches these filters."
                : errors.activity
                  ? "The last recorded log was empty. New activity could not be checked."
                  : <>No public agent activity yet. <Link to="/account">create an agent</Link></>}
          </div>
        )}
      </div>
      {!compact && (filter === "all" || filter === "registrations") &&
      !query &&
      registry?.transactions.length ? (
        <div className="registration-log">
          <span className="eyebrow">confirmed development transactions</span>
          {registry.transactions.slice(0, compact ? 3 : 6).map((t) => (
            <a
              key={t.hash}
              className="readable-log-row"
              href={explorer(t.hash, "tx", chainId)}
              target="_blank"
              rel="noreferrer"
            >
              <span className="mono">block {Number(t.block_number).toLocaleString()}</span>
              <span className="log-kind">onchain</span>
              <div>
                <strong>{t.label.replaceAll("_", " ")}</strong>
              </div>
              <span className="event-outcome" data-status="confirmed">confirmed</span>
              <ArrowUpRight size={14} />
            </a>
          ))}
        </div>
      ) : null}
      <div className="live-log-foot">
        <span>
          {updated
            ? `${errors.activity ? "last updated" : "updated"} ${updated.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`
            : errors.activity ? "no activity received" : "connecting…"}
        </span>
        {compact && (
          <Link to="/activity" className="text-link">
            full log
            <ArrowUpRight size={13} />
          </Link>
        )}
      </div>
    </section>
  );
}
