import { useEffect, useState } from "react";
import { ArrowUpRight, RefreshCw } from "lucide-react";
import { request } from "../lib/api";
import type { RegistryData } from "../lib/api";
import { explorer, shortId, useChainId } from "../lib/evm";
export function Registry({ onHover, onFocus }: { onHover?: (id: string | null) => void; onFocus?: (id: string) => void } = {}) {
  const chainId = useChainId();
  const [data, setData] = useState<RegistryData | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const refresh = async () => {
    setBusy(true);
    try {
      setData(await request<RegistryData>("/registry"));
      setError("");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => {
    void refresh();
  }, []);
  if (!data && !error)
    return (
      <section className="panel registry-panel">
        <span className="eyebrow">onchain registry</span>
        <p className="muted">reading agent registrations…</p>
      </section>
    );
  if (data?.status === "not_deployed") return null;
  return (
    <section className="panel registry-panel">
      <div className="panel-heading">
        <div>
          <span className="eyebrow">BNB Smart Chain · {chainId}</span>
          <h2>registered agents</h2>
        </div>
        <button
          className="icon-button"
          disabled={busy}
          onClick={refresh}
          aria-label="Refresh onchain agents"
        >
          <RefreshCw size={15} />
        </button>
      </div>
      <p className="muted registry-note">
        These registrations come from the verified BNB Smart Chain contract.
        Backing and paid-service settlement have their own funding requirements.
      </p>
      {data?.discovery && data.status !== "live" && <p className="field-help" role="status">{data.status === "catching_up" ? `Checking confirmed history · ${data.discovery.remaining_blocks ?? "…"} blocks remaining.` : data.status === "stale" ? "Showing the last verified registrations while the connection recovers." : "Checking confirmed registrations…"}</p>}
      {error || (data?.error && !data?.block_number) ? (
        <p role="alert" className="form-error">
          {error || data?.error}
        </p>
      ) : (
        <>
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>agent</th>
                  <th>policy cap / day</th>
                  <th>version</th>
                  <th>registry status</th>
                  <th>credit</th>
                </tr>
              </thead>
              <tbody>
                {data?.agents.map((a) => (
                  <tr key={a.id} onMouseEnter={() => onHover?.(`reg:${data.address ?? ""}:${a.id}`)} onMouseLeave={() => onHover?.(null)} onClick={() => onFocus?.(`reg:${data.address ?? ""}:${a.id}`)}>
                    <td>
                      <strong>{a.name}</strong>
                      <small>
                        #{shortId(a.id)} · {a.purpose}
                      </small>
                    </td>
                    <td>
                      {a.daily_cap} USDT
                      <small>
                        {a.policy_matches
                          ? a.providers.join(", ")
                          : "policy changed · document unavailable"}
                      </small>
                    </td>
                    <td>{a.version}</td>
                    <td>{a.paused ? "paused" : "registered"}</td>
                    <td>separate agreement</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="registry-transactions">
            <span className="eyebrow">verified transactions</span>
            {data?.transactions.map((t) => (
              <a
                key={t.hash}
                href={explorer(t.hash, "tx", chainId)}
                target="_blank"
                rel="noreferrer"
              >
                <span>{t.label}</span>
                <span className="mint">{t.status}</span>
                <ArrowUpRight size={13} />
              </a>
            ))}
          </div>
          <div className="registry-footer">
            {data?.address && <a
              className="text-link"
              href={explorer(data?.address ?? "", "address", chainId)}
              target="_blank"
              rel="noreferrer"
            >
              registry contract <ArrowUpRight size={13} />
            </a>}
            <span className="mono">
              block {data?.block_number ?? "unavailable"}
            </span>
          </div>
        </>
      )}
    </section>
  );
}
