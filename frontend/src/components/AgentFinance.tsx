import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { ArrowUpRight, RefreshCw } from "lucide-react";
import { isAddress } from "viem";
import type { Job, JobActionRecord, RuntimeAgent, WalletActionIntent } from "../lib/api";
import type { AccountAPI } from "../lib/jobs";
import type { PaymentSystem } from "../lib/payments";
import { formatAmount } from "../lib/amounts";
import { BNB_CHAIN_ID, explorer, sameAddress, waitForTransaction, type EvmTransaction } from "../lib/evm";
import { recordSubmitted } from "../lib/transactions";
import { financeAction, financeModuleKey, financeUnits, isFinanceIntent, validateFinanceIntent, type FinanceAction, type FinanceData, type FinanceInput, type FinanceQuote } from "../lib/finance";
import { walletPromptRejected } from "./AgentWalletActions";
import "./finance.css";
import "./feature-flow.css";

type PendingFinance = { intent: WalletActionIntent; expected: FinanceInput; hash: string; uncertain: boolean; quote?: FinanceQuote };
type Quoted = { quote: FinanceQuote; input: FinanceInput; at: number };
type FinanceSection = "lending" | "advances" | "stocks" | "buyback";
const groups: { key: FinanceSection; title: string; actions: { value: FinanceAction; title: string }[] }[] = [
  { key: "lending", title: "USDT pools", actions: [{ value: "pool_deposit", title: "deposit in the working-capital pool" }, { value: "pool_redeem", title: "redeem working-capital pool shares" }, { value: "stock_deposit", title: "deposit in the stock-loan pool" }, { value: "stock_redeem", title: "redeem stock-loan pool shares" }] },
  { key: "advances", title: "job advances", actions: [{ value: "advance_approve", title: "underwrite a job advance" }, { value: "advance_accept", title: "accept a job advance" }, { value: "advance_spend", title: "pay an approved recipient" }, { value: "advance_repay", title: "repay a job advance" }, { value: "advance_close", title: "close future advance spending" }] },
  { key: "stocks", title: "stock loans", actions: [{ value: "stock_borrow", title: "borrow against an approved token" }, { value: "stock_add_collateral", title: "add collateral to a position" }, { value: "stock_withdraw", title: "withdraw position collateral" }, { value: "stock_repay", title: "repay stock-loan principal" }, { value: "stock_liquidate", title: "liquidate an eligible position" }] },
  { key: "buyback", title: "TAB buybacks", actions: [{ value: "buyback_fund", title: "fund the buyback reserve" }, { value: "buyback_execute", title: "execute the quoted buyback" }] },
];
const remember = (key: string, value: PendingFinance | null) => { try { value ? localStorage.setItem(key, JSON.stringify(value)) : localStorage.removeItem(key); } catch { /* Component state retains the reviewed action. */ } };
const saved = (key: string): PendingFinance | null => { try { return JSON.parse(localStorage.getItem(key) || "null") as PendingFinance | null; } catch { return null; } };
function walletHasUncertainAction(wallet: string | null | undefined) {
  try {
    return Object.keys(localStorage).filter(key => /^tab:(?:finance|wallet|credit):pending:|^tab-job-action:/.test(key)).some(key => {
      const value = JSON.parse(localStorage.getItem(key) || "null") as { intent?: { sender?: string; tx_hash?: string }; uncertain?: boolean; hash?: string; txHash?: string } | null;
      return Boolean(value?.intent && sameAddress(value.intent.sender, wallet) && (value.uncertain || value.hash || value.txHash || value.intent.tx_hash));
    });
  } catch { return true; }
}
const validHash = (value: string) => /^0x[0-9a-fA-F]{64}$/.test(value) && !/^0x0{64}$/.test(value);
const changedEvent = () => window.dispatchEvent(new Event("tab:wallet-actions-changed"));
const positive = (value: string, decimals = 18) => { const units = financeUnits(value, decimals); if (units <= 0n) throw Error("Enter a positive amount."); return units; };

export function AgentFinance({ agent, api, send, changed }: { agent: RuntimeAgent; api: AccountAPI; send: (sender: string, txs: EvmTransaction[]) => Promise<string>; changed: () => Promise<void> }) {
  const [searchParams] = useSearchParams();
  const initialSection = groups.find(group => group.key === searchParams.get("module"))?.key || "lending";
  const [data, setData] = useState<FinanceData | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [payments, setPayments] = useState<PaymentSystem | null>(null);
  const [ready, setReady] = useState(false);
  const [pending, setPending] = useState(false);
  const [globalPending, setGlobalPending] = useState(false);
  const [foreignIntent, setForeignIntent] = useState<WalletActionIntent | null>(null);
  const [pendingJob, setPendingJob] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [section, setSection] = useState<FinanceSection>(initialSection);
  const [action, setAction] = useState<FinanceAction>(groups.find(group => group.key === initialSection)!.actions[0].value);
  const [amount, setAmount] = useState("1");
  const [loanId, setLoanId] = useState("");
  const [requestId, setRequestId] = useState("");
  const [token, setToken] = useState("");
  const [collateral, setCollateral] = useState("");
  const [recipient, setRecipient] = useState("");
  const [tool, setTool] = useState("");
  const [requestHash, setRequestHash] = useState("");
  const [receiptHash, setReceiptHash] = useState("");
  const [minimumOut, setMinimumOut] = useState("");
  const [deadline, setDeadline] = useState(0);
  const [quoted, setQuoted] = useState<Quoted | null>(null);
  const [intent, setIntent] = useState<WalletActionIntent | null>(null);
  const [expected, setExpected] = useState<FinanceInput | null>(null);
  const [hash, setHash] = useState("");
  const [uncertain, setUncertain] = useState(false);
  const [reviewedQuote, setReviewedQuote] = useState<FinanceQuote | undefined>();
  const storageKey = `tab:finance:pending:${agent.id}`;
  const mounted = useRef(false);
  const module = data?.system.modules[financeModuleKey(action)];
  const moduleReady = module?.status === "verified" && !!module.address;
  const isRecovery = ["pool_redeem", "stock_redeem", "advance_repay", "advance_close", "stock_add_collateral", "stock_withdraw", "stock_repay", "stock_liquidate"].includes(action);
  const allowed = ready && moduleReady && !!agent.registry_id && !!agent.wallet && !pending && !globalPending && !intent && (agent.status === "ready" || isRecovery);

  const refresh = useCallback(async () => {
    const result = await api<FinanceData>(`/account/runtime/${agent.id}/finance`);
    const jobActions = result.pending_job_intents ?? await api<JobActionRecord[]>("/account/job-actions");
    if (result.system.chain_id !== BNB_CHAIN_ID || result.system.currency !== "USDT" || !sameAddress(result.wallet, agent.wallet)) throw Error("Finance positions belong to another wallet or network.");
    if (!mounted.current) return;
    setData(result);
    setReady(true);
    let local = saved(storageKey);
    if (local && !local.hash && !local.uncertain && Date.parse(local.intent.expires_at) <= Date.now() && !result.pending_intents.some(item => item.id === local!.intent.id)) { remember(storageKey, null); local = null; }
    const next = result.pending_intents.find(item => isFinanceIntent(item) && item.agent_id === agent.id) || (local?.intent.agent_id === agent.id ? local.intent : null);
    const foreign = result.pending_intents.find(item => item.agent_id !== agent.id && sameAddress(item.sender, agent.wallet)) || null;
    const jobPending = jobActions.some(item => item.confirmed === 0 && sameAddress(item.intent.sender, agent.wallet));
    setForeignIntent(foreign); setPendingJob(jobPending);
    const foreignLocal = saved(`tab:wallet:pending:${agent.id}`) || saved(`tab:credit:pending:${agent.id}`);
    setGlobalPending(Boolean(result.pending_intents.length || next || foreignLocal || jobPending || walletHasUncertainAction(agent.wallet)));
    setIntent(next);
    if (next) {
      const match = local?.intent.id === next.id;
      setExpected(match ? local!.expected : next.details as FinanceInput);
      setHash(next.tx_hash || (match ? local!.hash : ""));
      setUncertain(Boolean(next.tx_hash || (match && local!.uncertain)));
      setReviewedQuote(match ? local!.quote : undefined);
    } else { setExpected(null); setHash(""); setUncertain(false); setReviewedQuote(undefined); }
    await Promise.allSettled([
      api<Job[]>("/account/jobs").then(owned => { if (mounted.current) setJobs(owned); }),
      api<PaymentSystem>("/x402/system").then(value => { if (mounted.current) setPayments(value); }),
    ]);
  }, [api, agent.id, agent.wallet, storageKey]);
  useEffect(() => {
    mounted.current = true;
    if (!agent.registry_id || !agent.wallet) return () => { mounted.current = false; };
    const update = () => { void refresh().catch(reason => { if (mounted.current) { setReady(false); setError((reason as Error).message); } }); };
    update();
    window.addEventListener("tab:wallet-actions-changed", update);
    window.addEventListener("tab:jobs-changed", update);
    return () => { mounted.current = false; window.removeEventListener("tab:wallet-actions-changed", update); window.removeEventListener("tab:jobs-changed", update); };
  }, [api, refresh, agent.registry_id, agent.wallet]);

  async function act(operation: () => Promise<void>) {
    setPending(true); setError(""); setNotice("");
    try { await operation(); } catch (reason) { setError((reason as Error).message); } finally { if (mounted.current) setPending(false); }
  }
  function selectAction(next: FinanceAction) { setAction(next); setLoanId(""); setRequestId(""); setQuoted(null); setMinimumOut(""); setDeadline(0); setError(""); }
  const advance = data?.lending.loans.find(loan => loan.id === loanId);
  const stock = data?.stock_loans.loans.find(loan => loan.id === loanId);
  const asset = data?.system.modules.stock_loans.assets.find(item => sameAddress(item.token, token));
  const currentRequest = data?.requests.find(request => request.id === requestId);
  const eligibleJobs = jobs.filter(job => job.funding === "funded" && job.executor_id === agent.id && job.state === "open" && !job.paused && Date.parse(job.deadline) > Date.now());
  function input(forQuote = false): FinanceInput {
    const result: FinanceInput = { action, amount: "0" };
    if (!["advance_approve", "advance_accept", "advance_close", "stock_add_collateral", "stock_withdraw"].includes(action)) { positive(amount); result.amount = amount; }
    if (["pool_redeem", "stock_redeem"].includes(action)) {
      const position = action === "pool_redeem" ? data?.lending : data?.stock_loans;
      if (!position || positive(amount) > financeUnits(position.max_redeem)) throw Error("The share redemption exceeds this wallet's currently available withdrawal.");
    }
    if (action === "advance_approve") { if (!data?.roles.underwriter || !currentRequest || currentRequest.status !== "requested") throw Error("Choose a pending request with the verified underwriter wallet."); Object.assign(result, { request_id: requestId, amount: currentRequest.amount, loan_id: currentRequest.loan_id, job_id: currentRequest.job_id, target_agent_id: currentRequest.agent_id, per_call: currentRequest.per_call, daily_cap: currentRequest.daily_cap, expires_at: currentRequest.expires_at, signer: currentRequest.signer, tools: currentRequest.tools, recipients: currentRequest.recipients }); }
    if (action.startsWith("advance_") && action !== "advance_approve") {
      const key = action.replace("advance_", "") as "accept" | "spend" | "repay" | "close";
      if (!advance || !advance.actions[key]) throw Error("Choose an advance eligible for this action.");
      result.loan_id = advance.id;
      if (action === "advance_repay" && positive(amount) > financeUnits(advance.debt)) throw Error("Repayment exceeds the outstanding advance principal.");
      if (action === "advance_spend") {
        if (!advance.recipients.some(address => sameAddress(address, recipient)) || !advance.tools.includes(tool) || !validHash(requestHash) || !validHash(receiptHash) || positive(amount) > financeUnits(advance.available) || positive(amount) > financeUnits(advance.per_call)) throw Error("Choose an allowed recipient/tool, exact commitments, and an amount within the advance's available funds and per-call cap.");
        Object.assign(result, { recipient, tool, request_hash: requestHash, receipt_hash: receiptHash });
      }
    }
    if (action === "stock_borrow") {
      if (!asset || asset.status !== "verified" || !asset.market_open) throw Error("Choose an approved collateral token with a fresh price and an open market.");
      positive(collateral, asset.decimals); result.token_address = asset.token; result.collateral_amount = collateral;
    }
    if (["stock_add_collateral", "stock_withdraw", "stock_repay", "stock_liquidate"].includes(action)) {
      const key = action.replace("stock_", "") as "add_collateral" | "withdraw" | "repay" | "liquidate";
      if (stock) { if (!stock.actions[key]) throw Error("This position is not eligible for the selected action."); result.loan_id = stock.id; }
      else if (action === "stock_liquidate" && validHash(loanId)) result.loan_id = loanId;
      else throw Error("Choose an eligible stock-loan position.");
      if (action === "stock_add_collateral" || action === "stock_withdraw") { positive(collateral, stock!.decimals); result.collateral_amount = collateral; result.token_address = stock!.token; if (action === "stock_withdraw" && positive(collateral, stock!.decimals) > financeUnits(stock!.collateral, stock!.decimals)) throw Error("Withdrawal exceeds the recorded collateral."); }
      if (action === "stock_repay" && positive(amount) > financeUnits(stock!.debt)) throw Error("Repayment exceeds the stock loan's debt.");
      if (action === "stock_liquidate" && !forQuote) { positive(minimumOut, stock?.decimals ?? quoted?.quote.decimals ?? 18); result.minimum_out = minimumOut; if (quoted?.quote.token) result.token_address = quoted.quote.token; }
    }
    if (action === "buyback_execute") {
      if (!data?.roles.buyback_operator) throw Error("Only the verified operator can execute this buyback.");
      if (!forQuote) { positive(minimumOut, data.system.modules.buyback.token_decimals ?? 18); if (!Number.isInteger(deadline) || deadline <= Date.now() / 1000 || deadline > Date.now() / 1000 + 600) throw Error("The quoted buyback deadline expired. Request another quote."); Object.assign(result, { minimum_out: minimumOut, deadline }); }
    }
    return result;
  }
  const needsQuote = ["stock_borrow", "stock_liquidate", "buyback_execute"].includes(action);
  function checkedQuote() {
    const base = input(true);
    if (!quoted || Date.now() - quoted.at > 60000 || JSON.stringify(base) !== JSON.stringify(quoted.input) || !["verified", "available", "ready"].includes(quoted.quote.status)) throw Error("Request a fresh quote for the exact amounts before preparing this action.");
    if (action === "stock_borrow" && positive(amount) > financeUnits(quoted.quote.maximum_borrow_usdt || "0")) throw Error("The principal exceeds the quoted collateral borrowing limit.");
    if (action === "stock_liquidate" && positive(amount) > financeUnits(quoted.quote.maximum_repay_usdt || "0")) throw Error("The principal exceeds the quoted liquidation amount.");
    if (action === "stock_liquidate" && positive(minimumOut, stock?.decimals ?? quoted.quote.decimals ?? 18) !== financeUnits(quoted.quote.collateral_out || "0", stock?.decimals ?? quoted.quote.decimals ?? 18)) throw Error("Keep the minimum collateral output at the exact verified liquidation quote.");
    if (action === "buyback_execute" && positive(minimumOut, data?.system.modules.buyback.token_decimals ?? 18) < financeUnits(quoted.quote.minimum_tokens || "0", data?.system.modules.buyback.token_decimals ?? 18)) throw Error("Keep the minimum output at or above the contract's quoted slippage floor.");
  }
  async function quote() {
    if (!allowed) throw Error("Resolve pending actions and verify this module before requesting a quote.");
    const body = input(true);
    const result = await api<FinanceQuote>("/finance/quote", { method: "POST", body: JSON.stringify(body) });
    if (result.chain_id !== BNB_CHAIN_ID || result.currency !== "USDT" || result.action !== action) throw Error("The finance quote belongs to another network or action.");
    setQuoted({ quote: result, input: body, at: Date.now() });
    if (action === "buyback_execute") { setMinimumOut(result.minimum_tokens || ""); setDeadline(result.deadline || Math.floor(Date.now() / 1000) + 300); }
    if (action === "stock_liquidate") setMinimumOut(result.collateral_out || "");
    if (!["verified", "available", "ready"].includes(result.status)) throw Error(result.reason || "The price, market or route is unavailable for this action.");
  }
  async function prepare() {
    if (!allowed || !data) throw Error("Verify finance data and finish pending wallet actions first.");
    const body = input();
    if (needsQuote) checkedQuote();
    const next = await api<WalletActionIntent>(`/account/runtime/${agent.id}/finance/prepare`, { method: "POST", body: JSON.stringify(body) });
    changedEvent();
    validateFinanceIntent(next, agent, data, body, needsQuote ? quoted?.quote : undefined);
    setIntent(next); setExpected(body); setGlobalPending(true); setHash(""); setUncertain(false);
    setReviewedQuote(needsQuote ? quoted?.quote : undefined);
    remember(storageKey, { intent: next, expected: body, hash: "", uncertain: false, quote: needsQuote ? quoted?.quote : undefined });
  }
  async function confirm(value: string) {
    if (!intent) return;
    await recordSubmitted(api, `/account/wallet-actions/${intent.id}/submitted`, value);
    const result = await api<{ status: string }>(`/account/wallet-actions/${intent.id}/confirm`, { method: "POST", body: JSON.stringify({ tx_hash: value }) });
    if (result.status !== "confirmed") throw Error("The finance transaction is not confirmed yet. Keep its hash and verify again.");
    remember(storageKey, null); setIntent(null); setExpected(null); setHash(""); setUncertain(false); setNotice("Finance transaction confirmed on BNB mainnet.");
    changedEvent(); await Promise.all([refresh(), changed()]);
  }
  async function sign() {
    if (!intent || !expected || !data || uncertain || hash) throw Error("This action may already be submitted. Verify its transaction before signing another.");
    let safeQuote = reviewedQuote;
    if (financeAction(intent.action) === "stock_liquidate" && !data.stock_loans.loans.some(loan => loan.id === intent.details.loan_id)) safeQuote = await api<FinanceQuote>("/finance/quote", { method: "POST", body: JSON.stringify(expected) });
    validateFinanceIntent(intent, agent, data, expected, safeQuote);
    remember(storageKey, { intent, expected, hash: "", uncertain: true, quote: safeQuote }); setUncertain(true);
    let transactionHash: string;
    try { transactionHash = await send(intent.sender, intent.transactions); }
    catch (reason) { if (walletPromptRejected(reason)) { remember(storageKey, { intent, expected, hash: "", uncertain: false, quote: safeQuote }); setUncertain(false); } throw reason; }
    setHash(transactionHash); remember(storageKey, { intent, expected, hash: transactionHash, uncertain: false, quote: safeQuote });
    await recordSubmitted(api, `/account/wallet-actions/${intent.id}/submitted`, transactionHash);
    await waitForTransaction(transactionHash); await confirm(transactionHash);
  }
  const advanceOptions = data?.lending.loans.filter(loan => loan.actions[action.replace("advance_", "") as keyof typeof loan.actions]) || [];
  const stockOptions = data?.stock_loans.loans.filter(loan => loan.actions[action.replace("stock_", "") as keyof typeof loan.actions]) || [];
  const activeInput = intent?.details;

  return <section className="paid-tool agent-finance" data-testid="agent-finance"><h3>lending and working capital</h3><p>Review pool shares, job advances, stock collateral and TAB buybacks. Every transfer uses the verified module's contract and this agent's wallet.</p>
    {!agent.wallet || !agent.registry_id ? <p className="muted">Register this agent on BNB mainnet before preparing finance actions.</p> : <>
      <div className="finance-account-tabs" aria-label="Choose a finance section">{groups.map(group => <button key={group.key} className={section === group.key ? "selected" : ""} aria-pressed={section === group.key} disabled={pending || !!intent} onClick={() => { setSection(group.key); selectAction(group.actions[0].value); }}>{group.title}</button>)}</div>
      {data && <div className="finance-wallet-summary">
        {section === "lending" ? <>{([ ["working-capital pool", data.lending, data.system.modules.lending], ["stock-loan pool", data.stock_loans, data.system.modules.stock_loans] ] as const).map(([name, position, state]) => <article key={name}><h4>{name}</h4><span className={`flow-badge ${state.status === "verified" ? "success" : "attention"}`}>{state.status.replaceAll("_", " ")}</span><dl><div><dt>your shares</dt><dd>{state.status === "verified" ? formatAmount(position.shares) : "awaiting verification"}</dd></div><div><dt>share value</dt><dd>{state.status === "verified" ? `${formatAmount(position.share_value_usdt)} USDT` : "awaiting verification"}</dd></div><div><dt>shares you can redeem</dt><dd>{state.status === "verified" ? formatAmount(position.max_redeem) : "awaiting verification"}</dd></div></dl><p>Zero-interest lending. Unpaid principal can reduce share value, and lent funds can delay withdrawal.</p></article>)}</> : section === "advances" ? <>
          {data.lending.loans.length === 0 && <p className="muted">No job advances are recorded for this wallet.</p>}{data.lending.loans.map(loan => <article key={loan.id}><h4>{loan.closed ? "closed advance" : loan.accepted ? "accepted advance" : "awaiting borrower acceptance"}</h4><p className="wallet-address">{loan.id}</p><dl><div><dt>available</dt><dd>{formatAmount(loan.available)} USDT</dd></div><div><dt>owed</dt><dd>{formatAmount(loan.debt)} USDT</dd></div><div><dt>repaid</dt><dd>{formatAmount(loan.repaid)} USDT</dd></div></dl><details><summary>advance terms</summary><p>Zero interest · unsecured · expires {new Date(loan.expires_at * 1000).toLocaleString()}</p><p>{formatAmount(loan.per_call)} USDT per call · {formatAmount(loan.daily_cap)} USDT per day</p><p className="wallet-address">borrower {loan.borrower}</p><p className="wallet-address">signer {loan.signer}</p><p>tools: {loan.tools.join(", ")}</p>{loan.recipients.map(address => <p key={address} className="wallet-address">recipient {address}</p>)}</details></article>)}
        </> : section === "stocks" ? <>{data.stock_loans.loans.length === 0 && <p className="muted">No stock-loan positions are recorded for this wallet.</p>}{data.stock_loans.loans.map(loan => <article key={loan.id}><h4>{loan.symbol} collateral</h4><p className="wallet-address">{loan.id}</p><span className={`flow-badge ${loan.liquidatable ? "failure" : loan.oracle_status === "verified" ? "success" : "attention"}`}>{loan.liquidatable ? "liquidation eligible" : loan.oracle_status.replaceAll("_", " ")}</span><dl><div><dt>collateral</dt><dd>{formatAmount(loan.collateral)} {loan.symbol}</dd></div><div><dt>principal owed</dt><dd>{formatAmount(loan.debt)} USDT</dd></div><div><dt>maximum borrowing</dt><dd>{loan.maximum_borrow_usdt === null ? "price unavailable" : `${formatAmount(loan.maximum_borrow_usdt)} USDT`}</dd></div><div><dt>liquidation debt threshold</dt><dd>{loan.liquidation_debt_usdt === null ? "price unavailable" : `${formatAmount(loan.liquidation_debt_usdt)} USDT`}</dd></div></dl></article>)}</> : <article><h4>explicitly funded buyback reserve</h4><dl><div><dt>USDT available</dt><dd>{data.system.modules.buyback.status === "verified" ? `${formatAmount(data.system.modules.buyback.available_usdt)} USDT` : "awaiting verification"}</dd></div><div><dt>USDT spent</dt><dd>{data.system.modules.buyback.status === "verified" ? `${formatAmount(data.system.modules.buyback.spent_usdt)} USDT` : "awaiting verification"}</dd></div><div><dt>TAB sent to dead address</dt><dd>{data.system.modules.buyback.status === "verified" ? formatAmount(data.system.modules.buyback.tokens_burned) : "awaiting verification"}</dd></div></dl><p>Funding is irreversible. Only the configured operator can execute purchases. Existing job fees remain in their separate locked reserve. Tokens sent to the dead address remain in total supply.</p></article>}
      </div>}
      {section === "advances" && <AdvanceRequestForm agent={agent} api={api} jobs={eligibleJobs} payments={payments} disabled={pending || globalPending || !ready} run={act} refreshed={async () => { await refresh(); setNotice("Advance request saved. No funds borrowed. The underwriter reviews its terms before approving a loan."); }} />}
      <form className="policy-fields finance-action-form" onSubmit={event => { event.preventDefault(); void act(prepare); }}>
        <label>finance action<select aria-label="finance action" value={action} disabled={pending || !!intent} onChange={event => selectAction(event.target.value as FinanceAction)}>{groups.find(group => group.key === section)!.actions.map(item => <option key={item.value} value={item.value} disabled={(item.value === "advance_approve" && !data?.roles.underwriter) || (item.value === "buyback_execute" && !data?.roles.buyback_operator)}>{item.title}</option>)}</select></label>
        {!moduleReady && <p className="field-help">{module?.reason || "Checking this module's BNB mainnet deployment."} Wallet actions stay disabled until the contract is verified.</p>}
        {module?.paused && !isRecovery && <p className="field-help">This module is paused. Repayment and eligible withdrawals remain available; new credit is stopped.</p>}
        {globalPending && !intent && <p className="field-help">Finish the pending wallet or credit action in its panel before preparing finance actions.</p>}
        {foreignIntent && <Link className="text-link" to={`/account?agent=${encodeURIComponent(foreignIntent.agent_id)}&section=${isFinanceIntent(foreignIntent) ? "finance" : foreignIntent.action.endsWith("_credit") ? "credit" : "backing"}`}>open the agent with the pending action <ArrowUpRight size={13} /></Link>}
        {pendingJob && <Link className="text-link" to="/account?section=jobs">finish the pending job transaction <ArrowUpRight size={13} /></Link>}
        {agent.status === "paused" && !isRecovery && <p className="field-help">Resume this agent before creating a new financial commitment.</p>}
        {action === "advance_approve" && <><label>advance request<select aria-label="advance request" value={requestId} onChange={event => setRequestId(event.target.value)}><option value="">choose a pending request</option>{data?.requests.filter(item => item.status === "requested").map(item => <option key={item.id} value={item.id}>{item.agent_name} · {item.job_title} · {item.amount} USDT</option>)}</select></label>{currentRequest && <div className="finance-request-terms"><p>{currentRequest.amount} USDT principal · {currentRequest.per_call} per call · {currentRequest.daily_cap} per day</p><p>Expires {new Date(currentRequest.expires_at * 1000).toLocaleString()} · zero interest · unsecured</p><p className="wallet-address">borrower {currentRequest.borrower}</p><p className="wallet-address">signer {currentRequest.signer}</p><p>tools: {currentRequest.tools.join(", ")}</p>{currentRequest.recipients.map(address => <p className="wallet-address" key={address}>recipient {address}</p>)}</div>}</>}
        {action.startsWith("advance_") && action !== "advance_approve" && <label>job advance<select aria-label="job advance" value={loanId} onChange={event => { setLoanId(event.target.value); setRecipient(""); setTool(""); }}><option value="">choose an eligible advance</option>{advanceOptions.map(loan => <option key={loan.id} value={loan.id}>{loan.id.slice(0, 12)} · {loan.debt} USDT owed</option>)}</select></label>}
        {["stock_add_collateral", "stock_withdraw", "stock_repay", "stock_liquidate"].includes(action) && <label>stock-loan position{action === "stock_liquidate" ? <input aria-label="stock-loan identifier" value={loanId} onChange={event => { setLoanId(event.target.value); setQuoted(null); }} placeholder="0x… loan identifier" spellCheck={false} /> : <select aria-label="stock-loan position" value={loanId} onChange={event => { setLoanId(event.target.value); setQuoted(null); }}><option value="">choose an eligible position</option>{stockOptions.map(loan => <option key={loan.id} value={loan.id}>{loan.symbol} · {loan.id.slice(0, 12)} · {loan.debt} USDT owed</option>)}</select>}</label>}
        {action === "stock_borrow" && <label>approved stock token<select aria-label="approved stock token" value={token} onChange={event => { setToken(event.target.value); setQuoted(null); }}><option value="">choose collateral</option>{data?.system.modules.stock_loans.assets.map(item => <option key={item.token} value={item.token} disabled={item.status !== "verified" || !item.market_open}>{item.symbol} · {item.market_open ? item.status.replaceAll("_", " ") : "market closed"}</option>)}</select></label>}
        {["stock_borrow", "stock_add_collateral", "stock_withdraw"].includes(action) && <label>collateral amount · {asset?.symbol || stock?.symbol || "token units"}<input aria-label="collateral amount" inputMode="decimal" value={collateral} onChange={event => { setCollateral(event.target.value); setQuoted(null); }} /></label>}
        {!["advance_approve", "advance_accept", "advance_close", "stock_add_collateral", "stock_withdraw"].includes(action) && <label>{action.endsWith("redeem") ? "shares to redeem" : action === "stock_borrow" ? "USDT principal to borrow" : action.includes("repay") ? "USDT principal to repay" : "amount in USDT"}<input aria-label="finance amount" inputMode="decimal" value={amount} onChange={event => { setAmount(event.target.value); setQuoted(null); }} /></label>}
        {action === "advance_spend" && <><label>approved recipient<select aria-label="advance recipient" value={recipient} onChange={event => setRecipient(event.target.value)}><option value="">choose a recipient</option>{advance?.recipients.map(address => <option key={address} value={address}>{address}</option>)}</select></label><label>approved tool<select aria-label="advance tool" value={tool} onChange={event => setTool(event.target.value)}><option value="">choose a tool</option>{advance?.tools.map(value => <option key={value} value={value}>{value}</option>)}</select></label><label>request commitment<input value={requestHash} onChange={event => setRequestHash(event.target.value)} placeholder="0x… 32-byte request hash" spellCheck={false} /></label><label>receipt commitment<input value={receiptHash} onChange={event => setReceiptHash(event.target.value)} placeholder="0x… 32-byte receipt hash" spellCheck={false} /></label><p className="field-help">This contract payment records the commitments and pays the recipient. Review the delivered result separately. It does not fetch an x402 response.</p></>}
        {needsQuote && <><button type="button" className="outline" disabled={!allowed} onClick={() => void act(quote)}>get {action === "buyback_execute" ? "buyback" : action === "stock_liquidate" ? "liquidation" : "borrowing"} quote</button>{quoted && <div className="finance-quote"><span className={`flow-badge ${["verified", "available", "ready"].includes(quoted.quote.status) ? "success" : "attention"}`}>{quoted.quote.status.replaceAll("_", " ")}</span>{quoted.quote.maximum_borrow_usdt && <p>Maximum principal: {formatAmount(quoted.quote.maximum_borrow_usdt)} USDT</p>}{quoted.quote.collateral_value_usdt && <p>Collateral value: {formatAmount(quoted.quote.collateral_value_usdt)} USDT</p>}{quoted.quote.maximum_repay_usdt && <p>Maximum liquidation repayment: {formatAmount(quoted.quote.maximum_repay_usdt)} USDT</p>}{quoted.quote.collateral_out && <p>Quoted collateral received: {formatAmount(quoted.quote.collateral_out)}</p>}{quoted.quote.quoted_tokens && <p>Quoted TAB: {formatAmount(quoted.quote.quoted_tokens)}</p>}{quoted.quote.minimum_tokens && <p>Contract minimum TAB: {formatAmount(quoted.quote.minimum_tokens)}</p>}<p>Quotes expire after one minute; market and liquidity checks run again onchain.</p></div>}</>}
        {["stock_liquidate", "buyback_execute"].includes(action) && <label>minimum {action === "stock_liquidate" ? "collateral received" : "TAB received"}<input aria-label="minimum output" inputMode="decimal" value={minimumOut} onChange={event => setMinimumOut(event.target.value)} /></label>}
        {action === "buyback_execute" && deadline > 0 && <p className="field-help">Swap deadline: {new Date(deadline * 1000).toLocaleString()} · tokens go to the burn address.</p>}
        {action === "advance_accept" && <p className="field-help">Accepting enables the agreed spending permission. Only the principal actually spent becomes owed. Job payment does not automatically repay this loan.</p>}
        {action === "advance_close" && <p className="field-help">Closing releases unused liquidity and stops future spending. Principal already spent remains owed.</p>}
        {action === "stock_borrow" && <p className="field-help">Borrowing transfers your collateral into this module and sends the principal to your wallet. A falling collateral price can make the position eligible for liquidation.</p>}
        {action.endsWith("redeem") && <p className="field-help">Redeem shares for the pool's current USDT value. Available liquidity limits withdrawals; share value can fall when loans default.</p>}
        {action === "buyback_fund" && <p className="field-help">This deposit funds purchases and cannot be withdrawn. It does not execute a swap or connect existing protocol fee reserves.</p>}
        <button className="outline" disabled={!allowed || (module?.paused === true && !isRecovery)}>{pending ? "checking…" : "review finance transaction"}</button>
      </form>
      {intent && <div className="payment-quote finance-review"><h4>review {financeAction(intent.action).replaceAll("_", " ")}</h4><p>BNB mainnet · {intent.transactions.length} wallet transaction{intent.transactions.length === 1 ? "" : "s"}</p><p className="wallet-address">wallet {intent.sender}</p><p className="wallet-address">contract {intent.to}</p>{activeInput && <dl className="finance-review-terms">{Object.entries(activeInput).filter(([key]) => !["action", "registry_id", "job_onchain_id"].includes(key)).map(([key, value]) => <div key={key}><dt>{key.replaceAll("_", " ")}</dt><dd>{Array.isArray(value) ? value.join(", ") : typeof value === "object" && value !== null ? JSON.stringify(value) : String(value)}</dd></div>)}</dl>}<p className="field-help">Any token approval is limited to the exact displayed amount and this module. BNB pays network fees.</p>
        {!hash && !uncertain && <button className="primary" disabled={pending || !ready || Date.parse(intent.expires_at) <= Date.now()} onClick={() => void act(sign)}>confirm finance in wallet</button>}
        {uncertain && !hash && <p className="field-help">The wallet interaction may have submitted this action. Check wallet activity and paste the transaction hash. A second submission stays blocked.</p>}
        <label>submitted finance transaction hash<input aria-label="finance transaction hash" value={hash} onChange={event => setHash(event.target.value)} placeholder="0x…" spellCheck={false} /></label><button className="outline" disabled={pending || !validHash(hash)} onClick={() => void act(() => confirm(hash))}>verify finance transaction</button>
        {hash && <><button className="outline" disabled={pending || !validHash(hash)} onClick={() => void act(async () => { await recordSubmitted(api, `/account/wallet-actions/${intent.id}/submitted`, hash); await api(`/account/wallet-actions/${intent.id}/release-failed`, { method: "POST" }); remember(storageKey, null); setIntent(null); setExpected(null); setHash(""); setUncertain(false); setNotice("Failed finance transaction verified. Review a new action when ready."); changedEvent(); await refresh(); })}>check failed finance transaction</button><a className="text-link" href={explorer(hash)} target="_blank" rel="noreferrer">view finance transaction <ArrowUpRight size={13} /></a></>}
        <details><summary>encoded wallet payload</summary><pre className="run-output">{JSON.stringify(intent.transactions, null, 2)}</pre></details>
      </div>}
      <button className="text-link" disabled={pending} onClick={() => void act(refresh)}><RefreshCw size={13} /> refresh finance positions</button>
    </>}
    {notice && <p role="status">{notice}</p>}{error && <p className="form-error" role="alert">{error}</p>}
  </section>;
}

function AdvanceRequestForm({ agent, api, jobs, payments, disabled, run, refreshed }: { agent: RuntimeAgent; api: AccountAPI; jobs: Job[]; payments: PaymentSystem | null; disabled: boolean; run: (operation: () => Promise<void>) => Promise<void>; refreshed: () => Promise<void> }) {
  const [jobId, setJobId] = useState("");
  const [amount, setAmount] = useState("1");
  const [perCall, setPerCall] = useState("1");
  const [daily, setDaily] = useState("1");
  const [signer, setSigner] = useState(agent.wallet || "");
  const [expires, setExpires] = useState("");
  const [tools, setTools] = useState<string[]>([]);
  const [recipients, setRecipients] = useState<string[]>([]);
  const [isPublic, setPublic] = useState(false);
  const job = jobs.find(item => item.id === jobId);
  const merchants = (payments?.merchants || []).filter(merchant => merchant.settlement_enabled && job?.root_services.some(service => (job.services || []).includes(service.id) && sameAddress(service.recipient, merchant.recipient)));
  async function create() {
    const principal = positive(amount), per = positive(perCall), cap = positive(daily);
    const expiresAt = Math.floor(Date.parse(expires) / 1000);
    if (!job || !isAddress(signer, { strict: false }) || !tools.length || !recipients.length || !Number.isFinite(expiresAt) || expiresAt <= Date.now() / 1000 || expiresAt > Date.parse(job.deadline) / 1000 || expiresAt > Date.now() / 1000 + 31 * 86400) throw Error("Choose a funded assigned job, signer, allowed tools/recipients and an expiry before the job deadline.");
    if (per > cap || cap > principal || cap > financeUnits(String(agent.daily_cap)) || per > financeUnits(String(job.max_call)) || principal > financeUnits(String(job.available))) throw Error("Keep principal within the funded job, daily cap within principal and agent policy, and per-call cap within the job's limit.");
    if (recipients.some(address => !merchants.some(merchant => sameAddress(merchant.recipient, address))) || tools.some(tool => !job.tools.includes(tool) || !agent.tools.includes(tool))) throw Error("The chosen recipients and tools do not match the installed providers and job policy.");
    const body: FinanceInput = { action: "advance_request", job_id: job.id, amount, per_call: perCall, daily_cap: daily, expires_at: expiresAt, signer, tools, recipients, public: isPublic };
    await api(`/account/runtime/${agent.id}/finance/requests`, { method: "POST", body: JSON.stringify(body) });
    await refreshed();
  }
  return <details className="finance-advance-request"><summary>request working capital for a job</summary><form className="policy-fields" onSubmit={event => { event.preventDefault(); void run(create); }}><p>Save exact terms for the underwriter to review. The borrower accepts any approved advance separately. Repayment is explicit after the job pays.</p>
    <label>funded assigned job<select aria-label="advance funded job" value={jobId} onChange={event => { setJobId(event.target.value); setRecipients([]); setTools([]); }}><option value="">choose a funded job</option>{jobs.map(item => <option key={item.id} value={item.id}>{item.title} · {item.available} USDT available</option>)}</select></label>{jobs.length === 0 && <p className="field-help">A funded job must be assigned to this agent before requesting an advance.</p>}
    <label>principal requested in USDT<input aria-label="advance principal" inputMode="decimal" value={amount} onChange={event => setAmount(event.target.value)} /></label><label>USDT per call<input inputMode="decimal" value={perCall} onChange={event => setPerCall(event.target.value)} /></label><label>USDT per day<input inputMode="decimal" value={daily} onChange={event => setDaily(event.target.value)} /></label><label>authorized payment signer<input value={signer} onChange={event => setSigner(event.target.value)} spellCheck={false} /></label><label>advance expires at<input type="datetime-local" value={expires} onChange={event => setExpires(event.target.value)} /></label>
    <fieldset><legend>job tools</legend>{job?.tools.filter(tool => agent.tools.includes(tool)).map(tool => <label className="checkbox-row" key={tool}><input type="checkbox" checked={tools.includes(tool)} onChange={event => setTools(previous => event.target.checked ? [...previous, tool] : previous.filter(value => value !== tool))} />{tool}</label>)}</fieldset>
    <fieldset><legend>installed job recipients</legend>{merchants.map(merchant => <label className="checkbox-row" key={merchant.id}><input type="checkbox" checked={recipients.includes(merchant.recipient)} onChange={event => setRecipients(previous => event.target.checked ? [...new Set([...previous, merchant.recipient])] : previous.filter(value => value !== merchant.recipient))} /><span>{merchant.id}<small className="wallet-address">{merchant.recipient}</small></span></label>)}{job && merchants.length === 0 && <p className="field-help">No connected payment provider matches this job's recipient allowlist.</p>}</fieldset>
    <label className="checkbox-row"><input type="checkbox" checked={isPublic} onChange={event => setPublic(event.target.checked)} />share this request publicly</label><p className="field-help">Zero interest · unsecured · unused funds can be released on closure. A saved request transfers no funds.</p><button className="outline" disabled={disabled || !job || merchants.length === 0 || agent.status !== "ready"}>save advance request</button>
  </form></details>;
}
