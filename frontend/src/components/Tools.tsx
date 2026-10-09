import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { ArrowUpRight, ExternalLink, Telescope, Network, Sprout, Wallet } from "lucide-react";
import { request, type Provider } from "../lib/api";
import { formatAmount, usdtUnits } from "../lib/amounts";
import "./feature-flow.css";

const purposes = ["all", "research", "chain reads", "models", "paid tools"];
const purposeFor = (provider: Provider) => provider.id === "x402" ? "paid tools" : ["bnb-rpc", "dune"].includes(provider.id) ? "chain reads" : provider.category === "inference" ? "models" : "research";
const returns: Record<string, string> = {
  "bnb-rpc": "BNB and USDT balances, with a confirmed block number.",
  "openrouter": "A model response with provider-credit usage.",
  "anthropic": "A Claude response through the configured model route.",
  "tavily": "Search results with source links and excerpts.",
  "web-search": "A research summary with its source links.",
  "dune": "Rows from an onchain dataset once connected.",
  "x402": "Market data with a separate USDT payment receipt.",
};
const purposeIcon = { research: Telescope, "chain reads": Network, models: Sprout, "paid tools": Wallet };

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
  const selected = providers.filter(provider => category === "all" || purposeFor(provider) === category);
  const available = selected.filter(provider => provider.status === "live");
  const disconnected = selected.filter(provider => provider.status !== "live");
  function cards(items: Provider[], ready: boolean) {
    return <div className="provider-grid">{items.map((provider) => { const Icon = purposeIcon[purposeFor(provider)]; return <article className="panel provider-card" data-provider-ready={ready} key={provider.id}>
      <div className="provider-top"><span className="provider-letter tool-drawing"><Icon size={29} strokeWidth={1.2} /></span><span className="eyebrow">{purposeFor(provider)}</span></div>
      <h2>{provider.name}</h2><p>{provider.description}</p>
      <div className="tool-return"><small>{ready ? "what it returns" : "when connected"}</small><span>{returns[provider.id] ?? provider.description}</span></div>
      <div className={`flow-badge ${ready ? "success" : "attention"}`}>{ready ? "available now" : provider.status === "not_connected" ? "connection needed" : "being reviewed"}</div>
      {!ready && <p className="provider-next-step">{provider.category === "inference" ? "Model access and provider credit need to be configured before an agent can use this tool." : "A verified connection and its payment terms are needed before this tool can run."}</p>}
      <div className="provider-bottom">{ready ? <Link to="/account" className="text-link">use in an agent <ArrowUpRight size={14} /></Link> : <Link to="/docs#tools" className="text-link">read connection details <ArrowUpRight size={14} /></Link>}<a href={provider.website} target="_blank" rel="noreferrer" aria-label={`Visit ${provider.name}`}><ExternalLink size={16} /></a></div>
    </article>; })}</div>;
  }
  return <>
    <section className="page-intro"><div><h1>tools for the task.</h1><p>Choose a connected tool, see what it returns, and review its cost before you grant access.</p></div><span className="status-chip">BNB mainnet</span></section>
    <aside className="provider-credit-banner"><div><h2>shared model and search budget</h2>{credits ? <p>{formatAmount(credits.daily_limit)} USD per day · {formatAmount(credits.remaining)} USD remaining today</p> : <p>{creditError ? "Provider credit status is unavailable." : "Checking provider credit availability…"}</p>}</div>{credits && <span className={`flow-badge ${credits.blocked || (usdtUnits(credits.remaining) ?? 0n) === 0n ? "attention" : "success"}`}>{credits.blocked || (usdtUnits(credits.remaining) ?? 0n) === 0n ? "daily budget limit reached" : "daily budget remaining"}</span>}<p>Operator credits pay connected model and search providers. Your wallet's USDT payments and BNB gas are accounted for separately.</p></aside>
    <div className="filter-row"><div className="tool-purpose-filters" aria-label="Filter tools">{purposes.map(value => <button key={value} className={category === value ? "selected" : ""} aria-pressed={category === value} onClick={() => setCategory(value)}>{value}</button>)}</div><span className="muted">{available.length} available · {disconnected.length} need a connection</span></div>
    <section className="tool-availability"><h2>available now</h2><p>These connections can be selected for a new agent. Paid calls still need the listed funding and authorization.</p>{available.length ? cards(available, true) : <div className="panel"><p>No connected tools match this filter.</p></div>}</section>
    {disconnected.length > 0 && <section className="tool-availability"><h2>next connections</h2><p>These providers appear in the catalog. Their connection status must change before agents can use them.</p>{cards(disconnected, false)}</section>}
  </>;
}
