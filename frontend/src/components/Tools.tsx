import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { ArrowUpRight, ExternalLink } from "lucide-react";
import { request, type Provider } from "../lib/api";
import { formatAmount, usdtUnits } from "../lib/amounts";
import "./feature-flow.css";

export function Tools({ providers }: { providers: Provider[] }) {
  const [category, setCategory] = useState("all");
  const [credits, setCredits] = useState<{ currency: "USD"; daily_limit: string; used: string; remaining: string; blocked: boolean; day: string } | null>(null);
  const [creditError, setCreditError] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    request<{ operator_provider_credits: NonNullable<typeof credits> }>("/capabilities", { signal: controller.signal, cache: "no-store" }).then(data => {
      const value = data.operator_provider_credits;
      if (!value || value.currency !== "USD" || [value.daily_limit, value.used, value.remaining].some(amount => typeof amount !== "string" || usdtUnits(amount) === null)) throw Error("Provider credit status could not be verified.");
      setCredits(value);
    }).catch(() => { if (!controller.signal.aborted) setCreditError(true); });
    return () => controller.abort();
  }, []);
  const selected = providers.filter(provider => category === "all" || provider.category === category);
  const available = selected.filter(provider => provider.status === "live");
  const disconnected = selected.filter(provider => provider.status !== "live");
  function cards(items: Provider[], ready: boolean) {
    return <div className="provider-grid">{items.map((provider, index) => <article className="panel provider-card" data-provider-ready={ready} key={provider.id}>
      <div className="provider-top"><span className={`provider-letter shade-${index % 4}`}>{provider.name.slice(0, 1)}</span><span className="eyebrow">{provider.category}</span></div>
      <h2>{provider.name}</h2><p>{provider.description}</p>
      <div className={`flow-badge ${ready ? "success" : "attention"}`}>{ready ? "available now" : provider.status === "not_connected" ? "connection needed" : "being reviewed"}</div>
      {!ready && <p className="provider-next-step">{provider.category === "inference" ? "Model access and provider credit need to be configured before an agent can use this tool." : "A verified connection and its payment terms are needed before this tool can run."}</p>}
      <div className="provider-bottom">{ready ? <Link to="/account" className="text-link">use in an agent <ArrowUpRight size={14} /></Link> : <Link to="/docs#tools" className="text-link">read connection details <ArrowUpRight size={14} /></Link>}<a href={provider.website} target="_blank" rel="noreferrer" aria-label={`Visit ${provider.name}`}><ExternalLink size={16} /></a></div>
    </article>)}</div>;
  }
  return <>
    <section className="page-intro"><div><h1>tools for the task.</h1><p>Choose a connected tool, see what it returns, and review its cost before you grant access.</p></div><span className="status-chip">BNB mainnet</span></section>
    <aside className="provider-credit-banner"><div><h2>shared model and search budget</h2>{credits ? <p>{formatAmount(credits.daily_limit)} USD per day · {formatAmount(credits.remaining)} USD remaining today</p> : <p>{creditError ? "Provider credit status is unavailable." : "Checking provider credit availability…"}</p>}</div>{credits && <span className={`flow-badge ${credits.blocked || (usdtUnits(credits.remaining) ?? 0n) === 0n ? "attention" : "success"}`}>{credits.blocked || (usdtUnits(credits.remaining) ?? 0n) === 0n ? "daily budget limit reached" : "daily budget remaining"}</span>}<p>Operator credits pay connected model and search providers. Your wallet's USDT payments and BNB gas are accounted for separately.</p></aside>
    <div className="filter-row"><div className="segments" aria-label="Filter tools">{["all", "inference", "data"].map(value => <button key={value} className={category === value ? "selected" : ""} aria-pressed={category === value} onClick={() => setCategory(value)}>{value}</button>)}</div><span className="muted">{available.length} available · {disconnected.length} need a connection</span></div>
    <section className="tool-availability"><h2>available now</h2><p>These connections can be selected for a new agent. Paid calls still need the listed funding and authorization.</p>{available.length ? cards(available, true) : <div className="panel"><p>No connected tools match this filter.</p></div>}</section>
    {disconnected.length > 0 && <section className="tool-availability"><h2>next connections</h2><p>These providers appear in the catalog. Their connection status must change before agents can use them.</p>{cards(disconnected, false)}</section>}
  </>;
}
