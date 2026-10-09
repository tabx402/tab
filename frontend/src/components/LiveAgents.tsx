import { shortId } from "../lib/evm";
import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ArrowUpRight } from "lucide-react";
import { request } from "../lib/api";
import type { components } from "../lib/api-schema";
import type { AgentEvent } from "../lib/api";
import { AgentPublicJobs } from "./Jobs";
import { AgentGlyph } from "./Drawings";
type PublicAgent = components["schemas"]["PublicAgent"];
export function LiveAgents({ onHover, onFocus }: { onHover?: (id: string | null) => void; onFocus?: (id: string) => void } = {}) {
  const [agents, setAgents] = useState<PublicAgent[]>([]),
    [error, setError] = useState("");
  useEffect(() => {
    void request<PublicAgent[]>("/agents/live")
      .then(setAgents)
      .catch((e) => setError(e.message));
  }, []);
  return (
    <section className="panel agents-panel">
      <div className="panel-heading">
        <h2>live agents</h2>
        <Link to="/account" className="text-link">
          create agent
          <ArrowUpRight size={14} />
        </Link>
      </div>
      <div className="agent-network-stats">
        <span><strong>{agents.length}</strong> public agents</span>
        <span data-tone="success"><strong>{agents.filter(a => a.last_run).length}</strong> have run</span>
        <span data-tone="attention"><strong>{agents.filter(a => a.status === "paused").length}</strong> paused</span>
      </div>
      {error && <p role="alert">{error}</p>}
      {agents.length ? (
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>agent</th>
                <th>tools</th>
                <th>daily cap</th>
                <th>schedule</th>
                <th>last run</th>
                <th>status</th>
              </tr>
            </thead>
            <tbody>
              {agents.map((a, i) => (
                <tr key={a.id} data-status={a.status} onMouseEnter={() => onHover?.(a.id)} onMouseLeave={() => onHover?.(null)} onClick={(e) => { if (!(e.target as HTMLElement).closest("a, button")) onFocus?.(a.id); }}>
                  <td>
                    <Link className="agent-name" to={`/agents/${a.id}`}>
                      <AgentGlyph index={i} />
                      <span>
                        <strong>{a.name}</strong>
                        <small title={a.registry_id||undefined}>#{shortId(a.registry_id)}</small>
                      </span>
                    </Link>
                  </td>
                  <td>{a.tools.join(" · ")}</td>
                  <td>{a.daily_cap} USDT</td>
                  <td>{a.cadence}</td>
                  <td className="agent-last-run">{a.last_run ? <time dateTime={a.last_run}>{new Date(a.last_run).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</time> : "not run yet"}</td>
                  <td><span className="agent-status" data-status={a.status}>{a.status}</span></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <p className="empty">no public agents registered yet.</p>
      )}
    </section>
  );
}
export function LiveAgentProfile() {
  const { id } = useParams();
  const [data, setData] = useState<{
      agent: PublicAgent;
      events: AgentEvent[];
      runs?: {id:string;status:string;started_at:string}[];
    } | null>(null),
    [error, setError] = useState("");
  useEffect(() => {
    void request<{ agent: PublicAgent; events: AgentEvent[] }>(
      `/agents/live/${id}`,
    )
      .then(setData)
      .catch((e) => setError(e.message));
  }, [id]);
  if (error) return <div className="empty">{error}</div>;
  if (!data) return <div className="empty">reading agent…</div>;
  return (
    <>
      <section className="page-intro">
        <div>
          <span className="eyebrow" title={data.agent.registry_id||undefined}>agent #{shortId(data.agent.registry_id)}</span>
          <h1>{data.agent.name}</h1>
          <p>
            {data.agent.cadence} · {data.agent.daily_cap} USDT daily spending
            cap · {data.agent.status}
          </p>
        </div>
        <AgentGlyph />
      </section>
      <AgentPublicJobs id={data.agent.id} />
      <section className="panel"><h2>run receipts</h2>{data.runs?.length ? data.runs.map(run=><p key={run.id}><Link className="text-link" to={`/agents/${data.agent.id}/runs/${run.id}`}>{run.status} · {new Date(run.started_at).toLocaleString()} <ArrowUpRight size={14}/></Link></p>) : <p className="muted">No public runs recorded yet.</p>}</section>
      <section className="panel agent-controls">
        <h2>activity</h2>
        <div className="personal-events">
          {data.events.map((e) => (
            <div key={e.id} data-status={e.status}>
              <time>{new Date(e.timestamp).toLocaleTimeString()}</time>
              <span>{e.message}</span>
              <small data-status={e.status}>{e.status.replaceAll("_", " ")}</small>
            </div>
          ))}
        </div>
        <Link className="text-link" to="/activity">
          full log
          <ArrowUpRight size={13} />
        </Link>
      </section>
    </>
  );
}
