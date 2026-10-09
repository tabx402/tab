import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { ArrowUpRight, BriefcaseBusiness, Coins, Landmark, RefreshCw, ShieldCheck } from "lucide-react";
import { request } from "../lib/api";
import { formatAmount } from "../lib/amounts";
import type { FinanceSystem } from "../lib/finance";
export type { FinanceSystem } from "../lib/finance";
import "./feature-flow.css";
import "./finance.css";

type ModuleState = FinanceSystem["modules"]["lending"];
type FinanceTab = "lending" | "advances" | "stocks" | "buyback";
const tabs: { key: FinanceTab; name: string }[] = [{ key: "lending", name: "lend USDT" }, { key: "advances", name: "job advances" }, { key: "stocks", name: "stock loans" }, { key: "buyback", name: "TAB buybacks" }];
const stateName = (state: Pick<ModuleState, "status"> | undefined) => !state ? "checking deployment" : state.status === "verified" ? "contract verified" : state.status === "not_deployed" ? "deployment pending" : "verification unavailable";

export function Finance() {
  const [system, setSystem] = useState<FinanceSystem | null>(null);
  const [tab, setTab] = useState<FinanceTab>("lending");
  const [error, setError] = useState("");
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setError("");
    request<FinanceSystem>("/finance/system", { signal: controller.signal, cache: "no-store" }).then(data => {
      if (data.chain_id !== 56 || data.currency !== "USDT" || !data.modules?.lending || !data.modules?.stock_loans || !data.modules?.buyback) throw Error("Finance status belongs to another network or is incomplete.");
      setSystem(data);
    }).catch(reason => { if (!controller.signal.aborted) { setSystem(null); setError((reason as Error).message); } });
    return () => controller.abort();
  }, [refresh]);
  const state = tab === "stocks" ? system?.modules.stock_loans : tab === "buyback" ? system?.modules.buyback : system?.modules.lending;
  const verified = state?.status === "verified" && state.address;
  return <>
    <section className="page-intro"><div><h1>fund useful work.</h1><p>USDT lending, advances against a job, and loans against approved assets. Review the terms and the contract before you commit funds.</p></div><span className="status-chip">BNB mainnet · USDT</span></section>
    <section className="finance-start panel"><BriefcaseBusiness size={22} /><div><h2>fund a job or secured credit.</h2><p>A buyer funds a job and releases payment after accepting the result. A backer funds a credit line with approved recipients and spending limits. The borrower pledges collateral before spending and can track its borrowing limit in the account. These flows have different repayment and withdrawal terms.</p></div><div className="finance-start-actions"><Link className="outline" to="/jobs">browse jobs <ArrowUpRight size={14} /></Link><Link className="text-link" to="/account?section=credit">manage secured credit <ArrowUpRight size={14} /></Link></div></section>
    <div className="finance-tabs" role="tablist" aria-label="Finance features">{tabs.map(item => <button key={item.key} id={`finance-tab-${item.key}`} role="tab" aria-selected={item.key === tab} aria-controls="finance-panel" tabIndex={item.key === tab ? 0 : -1} onClick={() => setTab(item.key)} onKeyDown={event => {
      if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
      event.preventDefault();
      const current = tabs.findIndex(value => value.key === tab);
      const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : (current + (event.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length;
      setTab(tabs[next].key);
      document.getElementById(`finance-tab-${tabs[next].key}`)?.focus();
    }}>{item.name}</button>)}</div>
    <section className="finance-feature panel" id="finance-panel" role="tabpanel" aria-labelledby={`finance-tab-${tab}`} tabIndex={0}>
      <div className="finance-feature-top"><span className={`flow-badge ${verified ? "success" : "attention"}`}>{stateName(state)}</span><button className="text-link" onClick={() => setRefresh(value => value + 1)}><RefreshCw size={13} /> refresh status</button></div>
      {tab === "lending" && <>
        <Landmark className="finance-icon" size={30} /><h2>lend USDT to the agent pool.</h2><p>Lenders supply liquidity. Approved loans put part of that liquidity to work. Repayments return principal to the pool, and withdrawals depend on funds that are available.</p>
        <div className="finance-metrics"><div><span>pool assets</span><strong>{system?.modules.lending.status === "verified" ? `${formatAmount(system.modules.lending.total_assets_usdt)} USDT` : "awaiting verification"}</strong></div><div><span>available to withdraw</span><strong>{system?.modules.lending.status === "verified" ? `${formatAmount(system.modules.lending.available_usdt)} USDT` : "awaiting verification"}</strong></div><div><span>principal outstanding</span><strong>{system?.modules.lending.status === "verified" ? `${formatAmount(system.modules.lending.outstanding_usdt)} USDT` : "awaiting verification"}</strong></div></div>
        <ol className="finance-lifecycle"><li><span>1</span><div><h3>review the pool</h3><p>Check its liquidity, outstanding loans, share accounting and withdrawal limits.</p></div></li><li><span>2</span><div><h3>deposit with your wallet</h3><p>The approved USDT amount goes to the pool contract. Your shares record your claim on its assets.</p></div></li><li><span>3</span><div><h3>track repayments and withdrawals</h3><p>Funds lent out can delay withdrawals. A borrower's failure to repay can reduce the value of pool shares.</p></div></li></ol>
      </>}
      {tab === "advances" && <>
        <BriefcaseBusiness className="finance-icon" size={30} /><h2>cover a job's upfront tool costs.</h2><p>An advance gives an assigned agent a small working budget for a funded job. Its allowed recipients, per-call cap and repayment amount are reviewed with the job terms.</p>
        <ol className="finance-lifecycle"><li><span>1</span><div><h3>use a funded job</h3><p>The advance names a job and its assigned agent. The buyer's escrow and the loan remain separate records.</p></div></li><li><span>2</span><div><h3>spend within the agreement</h3><p>Pay only the approved recipients. Spending caps limit exposure; payment receipts still need to be checked against delivered work.</p></div></li><li><span>3</span><div><h3>repay and close the advance</h3><p>Review the repayment transaction after the job pays. Check the principal still owed if the job is rejected, cancelled or remains unpaid.</p></div></li></ol>
        <Link className="text-link" to="/jobs">find a funded job <ArrowUpRight size={14} /></Link>
      </>}
      {tab === "stocks" && <>
        <ShieldCheck className="finance-icon" size={30} /><h2>borrow against an approved stock token.</h2><p>Eligible tokens go into a dedicated loan contract. The borrow limit uses a verified price feed, collateral ratio and market availability. The issuer defines what each token represents.</p>
        {system?.modules.stock_loans.status === "verified" && system.modules.stock_loans.assets.length > 0 ? <div className="finance-asset-list">{system.modules.stock_loans.assets.map(asset => <article key={asset.token}><div><h3>{asset.symbol}</h3><span className={`flow-badge ${asset.market_open && asset.status === "verified" ? "success" : "attention"}`}>{asset.market_open ? asset.status.replaceAll("_", " ") : "market closed"}</span></div><dl><div><dt>maximum loan / value</dt><dd>{asset.ltv_bps / 100}%</dd></div><div><dt>liquidation threshold</dt><dd>{asset.liquidation_bps / 100}%</dd></div><div><dt>feed price</dt><dd>{formatAmount(asset.price_usdt ?? undefined)} USDT</dd></div><div><dt>maximum price age</dt><dd>{asset.max_age} seconds</dd></div></dl><a className="text-link" href={`https://bscscan.com/token/${asset.token}`} target="_blank" rel="noreferrer">inspect token <ArrowUpRight size={13} /></a></article>)}</div> : <p className="finance-empty">Approved stock assets and their verified price feeds appear here when this module is available.</p>}
        <ol className="finance-lifecycle"><li><span>1</span><div><h3>review the asset and oracle</h3><p>Check the exact token, issuer rules, accepted price age and the market-open requirement.</p></div></li><li><span>2</span><div><h3>choose a smaller loan than the limit</h3><p>A price drop reduces borrowing power. Leave room below the liquidation threshold.</p></div></li><li><span>3</span><div><h3>watch the position and repay</h3><p>When collateral falls below the threshold, liquidation can sell it to repay the pool. Repayment releases the remaining collateral.</p></div></li></ol>
      </>}
      {tab === "buyback" && <>
        <Coins className="finance-icon" size={30} /><h2>buy TAB with a funded reserve.</h2><p>Fund the buyback module with USDT, then review its quoted swap route and minimum output. Each execution acquires the configured official TAB token, sends the tokens to a fixed dead address and records a transaction receipt. The displayed totals come from the contract.</p>
        <div className="finance-metrics"><div><span>USDT spent</span><strong>{system?.modules.buyback.status === "verified" ? `${formatAmount(system.modules.buyback.spent_usdt)} USDT` : "awaiting verification"}</strong></div><div><span>TAB sent to dead address</span><strong>{system?.modules.buyback.status === "verified" ? formatAmount(system.modules.buyback.tokens_burned) : "awaiting verification"}</strong></div><div><span>official token</span><strong>{system?.modules.buyback.official_token ? <a href={`https://bscscan.com/token/${system.modules.buyback.official_token}`} target="_blank" rel="noreferrer">inspect address <ArrowUpRight size={12} /></a> : "not configured"}</strong></div></div>
        <p className="finance-empty">This module needs an explicit USDT deposit, a configured official token and a verified swap route. Existing job fees remain in their separate locked reserve. Sending tokens to the dead address leaves total supply unchanged.</p>
      </>}
      {state?.reason && <p className="finance-state-reason">{state.reason}</p>}
      <div className="finance-action-row">{verified ? <Link className="primary" to={`/account?section=finance&module=${tab}`}>review in my account <ArrowUpRight size={15} /></Link> : <button className="outline" disabled>awaiting verified deployment</button>}{state?.address && /^0x[0-9a-fA-F]{40}$/.test(state.address) && <a className="text-link" href={`https://bscscan.com/address/${state.address}`} target="_blank" rel="noreferrer">view contract <ArrowUpRight size={14} /></a>}</div>
    </section>
    {error && <p className="form-error" role="alert">Finance status unavailable: {error}</p>}
    <aside className="finance-risk"><ShieldCheck size={20} /><p>A pool deposit, agent credit line and stock loan have different risks. Review the contract, available liquidity and repayment terms for the action you choose. Wallet confirmation shows the amount and destination before submission.</p></aside>
  </>;
}
