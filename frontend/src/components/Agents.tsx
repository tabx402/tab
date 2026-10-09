import { Link } from "react-router-dom";
import { ArrowUpRight } from "lucide-react";
import type { Agent } from "../lib/api";
import { money } from "../lib/api";
import { AgentGlyph } from "./Drawings";
export function AgentsTable({
  agents,
  compact = false,
}: {
  agents: Agent[];
  compact?: boolean;
}) {
  return (
    <section className="panel agents-panel">
      <div className="panel-heading">
        <div>
          <h2>agents</h2>
        </div>
        {compact && (
          <Link className="text-link" to="/agents">
            all agents <ArrowUpRight size={14} />
          </Link>
        )}
      </div>
      {agents.length ? (
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>agent</th>
                <th>credit limit</th>
                <th>in use</th>
                <th>repaid</th>
                <th>backer</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {agents.map((a, i) => (
                <tr key={a.id}>
                  <td>
                    <Link className="agent-name" to={`/agents/${a.id}`}>
                      <span className={`glyph-box shade-${i % 4}`}>
                        <AgentGlyph index={i} />
                      </span>
                      <span>
                        <strong>{a.name}</strong>
                        <small>{a.purpose}</small>
                      </span>
                    </Link>
                  </td>
                  <td>
                    {money(a.limit)} <small>USDT</small>
                  </td>
                  <td>
                    <span>{money(a.outstanding)}</span>
                    <div className="mini-bar">
                      <i
                        style={{ width: `${(a.outstanding / a.limit) * 100}%` }}
                      />
                    </div>
                  </td>
                  <td className="mint">
                    {money(a.repaid)}
                    <small>{a.repayments} repayments</small>
                  </td>
                  <td>{a.backer}</td>
                  <td>
                    <Link to={`/agents/${a.id}`} aria-label={`View ${a.name}`}>
                      <ArrowUpRight size={16} />
                    </Link>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div className="empty">
          no backed agents yet.{" "}
          <Link to="/account">
            create your first agent plan <ArrowUpRight size={14} />
          </Link>
        </div>
      )}
    </section>
  );
}
