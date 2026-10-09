import { useHolderGate } from "../lib/holder-access";
import { walletPromptRejected } from "./AgentWalletActions";
import { recordSubmitted } from "../lib/transactions";
import { useCallback, useEffect, useState } from "react";
import type { FormEvent } from "react";
import { explorer, useChainId, waitForTransaction } from "../lib/evm";
import { Link } from "react-router-dom";
import { ArrowUpRight, Plus, GitBranch, Check, Pause, RefreshCw } from "lucide-react";
import { request, isRecord } from "../lib/api";
import type { Job, JobInput, JobIntent, JobActionRecord, PublicJob, RuntimeAgent, JobSystem, JobMerchant } from "../lib/api";
import { jobAPI, jobMoney } from "../lib/jobs";
import type { AccountAPI, JobRun } from "../lib/jobs";
import { RunReceipt } from "./RunReceipt";
import { minimumAmount, usdtUnits } from "../lib/amounts";
import "./jobs.css";


type AgentChoice = Pick<RuntimeAgent, "id" | "name" | "tools" | "status" | "max_call" | "daily_cap" | "wallet" | "registry_id" | "registry_address">;
const localJobTime = (milliseconds: number) => { const value = new Date(milliseconds); return new Date(value.getTime() - value.getTimezoneOffset() * 60e3).toISOString().slice(0, 19); };
const label = (value: string) => value.replaceAll("_", " ");
const shortDate = (value: string) => new Date(value).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
const REVIEW_SECONDS = 86400;
const paidWork = (state: string) => state === "accepted" || state === "closed";
const jobWindow = (job: Pick<Job, "deadline">, current = Date.now()) => {
  const deadline = Math.floor(Date.parse(job.deadline) / 1000);
  const reviewEnd = deadline + REVIEW_SECONDS;
  const seconds = Math.floor(current / 1000);
  return { deadline, reviewEnd, submissionOpen: seconds <= deadline, reviewOpen: seconds <= reviewEnd, refundOpen: seconds > reviewEnd };
};
function ExactJobTime({ seconds }: { seconds: number }) {
  if (!Number.isFinite(seconds)) return <>unavailable</>;
  const value = new Date(seconds * 1000);
  return <time dateTime={value.toISOString()}>{value.toLocaleString([], { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit", timeZoneName: "short" })}</time>;
}

export function JobBud({ accepted = false }: { accepted?: boolean }) {
  return <svg className={`job-bud${accepted ? " accepted" : ""}`} viewBox="0 0 24 30" fill="none" aria-hidden="true"><path d="M12 28V15M12 20C5 20 4 13 6 9C8 5 11 6 12 10C13 6 16 5 18 9C20 13 19 20 12 20Z" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" /><path d="M12 26C6 26 5 22 5 22M12 23C18 23 19 19 19 19" stroke="currentColor" strokeWidth="1.2" />{accepted && <path d="m8 12 3 3 5-6" stroke="currentColor" strokeWidth="1.3" />}</svg>;
}

function PublicJobRow({ job, jobs }: { job: PublicJob; jobs: PublicJob[] }) {
  const chainId = useChainId();
  const children = jobs.filter((j) => j.parent_id === job.id);
  return <details className="garden-job" id={`job-${job.id}`}>
    <summary><JobBud accepted={paidWork(job.state)} /><span><strong>{job.title}</strong><small>{job.executor_name} · {label(job.state)}</small></span><span className="job-status" data-funding={job.funding}>{job.funding === "unfunded" ? "unfunded" : `${jobMoney(job.budget)} USDT`}</span></summary>
    <div className="job-public-detail">
      <div className="job-pairs"><span>acceptance<strong>buyer reviews evidence</strong></span><span>deadline<strong><ExactJobTime seconds={jobWindow(job).deadline} /></strong></span><span>review ends<strong><ExactJobTime seconds={jobWindow(job).reviewEnd} /></strong></span><span>available<strong>{jobMoney(job.available)} USDT{job.funding === "unfunded" ? " · planned" : ""}</strong></span><span>branches<strong>{children.length}</strong></span><span>reward paid<strong>{jobMoney(job.reward_paid ?? 0)} USDT</strong></span><span>services paid<strong>{jobMoney(job.provider_paid ?? 0)} USDT</strong></span></div>
      {job.paused && <p className="field-help">This job's escrow actions are paused.</p>}
      {children.map((child) => <div className="job-branch-row" key={child.id}><GitBranch size={14} /><Link to={`/agents/${child.executor_id}#job-${child.id}`}>{child.executor_name}</Link><span>{jobMoney(child.budget)} USDT · {child.funding === "unfunded" ? "planned" : label(child.state)}</span></div>)}
      <div className="job-public-links"><Link className="text-link" to={`/agents/${job.executor_id}`}>open agent <ArrowUpRight size={13} /></Link>{job.chain_tx && <a className="text-link" href={explorer(job.chain_tx, "tx", chainId)} target="_blank" rel="noreferrer">transaction <ArrowUpRight size={13} /></a>}</div>
      <details className="job-proof"><summary>agreement record</summary><p>terms hash <code>{job.terms_hash}</code></p>{job.evidence_hash && <p>evidence hash <code>{job.evidence_hash}</code></p>}</details>
    </div>
  </details>;
}

export function GardenJobs({ jobs, error, visibleJobs }: { jobs: PublicJob[]; error?: string; visibleJobs?: PublicJob[] }) {
  const roots = visibleJobs ?? jobs.filter((j) => !j.parent_id && j.state !== "cancelled");
  return <details className="garden-jobs" open={roots.length > 0}>
    <summary><span><JobBud /> jobs in the garden</span><span className="muted">{roots.length} agreements</span></summary>
    <div className="garden-jobs-body">
      {error ? <p className="field-help" role="status">{error}</p> : roots.length ? roots.map((job) => <PublicJobRow key={job.id} job={job} jobs={jobs} />) : <p className="field-help">A job gives an agent a deliverable, deadline and bounded budget. Its branches connect the agents doing the work.</p>}
      <Link className="text-link" to="/account">give an agent a job <ArrowUpRight size={13} /></Link>
    </div>
  </details>;
}

export function AgentPublicJobs({ id }: { id: string }) {
  const [jobs, setJobs] = useState<PublicJob[]>([]), [error, setError] = useState("");
  useEffect(() => { let active = true; void jobAPI.public().then((value) => { if (active) setJobs(value); }).catch(() => { if (active) setError("Job records are temporarily unavailable."); }); return () => { active = false; }; }, [id]);
  const assigned = jobs.filter((j) => j.executor_id === id || j.requester_id === id);
  return <section className="panel agent-public-jobs"><div className="panel-heading"><h2>jobs</h2><Link to="/account" className="text-link">manage yours <ArrowUpRight size={13} /></Link></div>{error ? <p className="field-help">{error}</p> : assigned.length ? assigned.map((job) => <PublicJobRow key={job.id} job={job} jobs={jobs} />) : <p className="field-help">No public job agreements for this agent yet.</p>}</section>;
}

function JobForm({ agent, choices, parent, merchants, system, onSave, onCancel, busy }: { agent: RuntimeAgent; choices: AgentChoice[]; parent: Job | null; merchants: JobMerchant[]; system: JobSystem | null; onSave: (input: JobInput) => Promise<void>; onCancel: () => void; busy: boolean }) {
  const sharedTools = (candidate: AgentChoice) => agent.tools.filter((tool) => (!parent || parent.tools.includes(tool)) && candidate.tools.includes(tool));
  const candidates = choices.filter((candidate) => candidate.status === "ready" && candidate.wallet && candidate.registry_id && (!system?.escrow || candidate.registry_address.toLowerCase() === system.escrow.toLowerCase()) && sharedTools(candidate).length && (usdtUnits(candidate.max_call) ?? 0n) > 0n);
  const initialExecutor = candidates.find((candidate) => candidate.id !== agent.id) ?? candidates[0];
  const [title, setTitle] = useState(""), [description, setDescription] = useState("");
  const [executor, setExecutor] = useState(initialExecutor?.id ?? "");
  const budgetLimit = parent ? minimumAmount(parent.available, agent.daily_cap) : agent.daily_cap;
  const [budget, setBudget] = useState(minimumAmount(budgetLimit, parent ? "1" : "5"));
  const [maxCall, setMaxCall] = useState(minimumAmount(budget, agent.max_call, initialExecutor?.max_call ?? "0", parent?.max_call ?? "0.1"));
  const [deadline, setDeadline] = useState(() => localJobTime(Math.min(Date.now() + 3600e3, parent ? Date.parse(parent.deadline) : Infinity)));
  const [acceptance] = useState<JobInput["acceptance"]>("buyer_review"), [minimumBlock] = useState(parent?.minimum_block ?? 1);
  const initialTools = initialExecutor ? sharedTools(initialExecutor) : [];
  const [tools, setTools] = useState<JobInput["tools"]>(initialTools.includes("bnb-rpc") ? ["bnb-rpc"] : initialTools.slice(0, 1));
  const [services, setServices] = useState<string[]>([]), [error, setError] = useState("");
  const target = candidates.find((candidate) => candidate.id === executor);
  const allowed = target ? sharedTools(target) : [];
  const callLimit = minimumAmount(budget, agent.max_call, target?.max_call ?? "0", parent?.max_call ?? agent.max_call);
  const depthLimit = system?.max_depth ?? 8;
  const depthBlocked = Boolean(parent && parent.depth >= depthLimit);
  const deadlineLimit = Math.min(Date.now() + 30 * 86400e3, parent ? Date.parse(parent.deadline) : Infinity);
  // Branch recipients come from the saved root terms, even when the current catalog has changed.
  const connectedServices = parent ? parent.root_services.filter((merchant) => parent.services?.includes(merchant.id)) : merchants;
  const availableServices = connectedServices.filter((merchant) => tools.includes(merchant.tool));
  const recipientBlocked = (merchant: JobMerchant) => [agent.wallet, target?.wallet].some((wallet) => wallet?.toLowerCase() === merchant.recipient.toLowerCase());
  async function submit(event: FormEvent) {
    event.preventDefault(); setError("");
    if (!target) { setError("Choose a registered, ready agent with shared tools."); return; }
    if (depthBlocked) { setError(`This job has reached the ${depthLimit}-level branch limit.`); return; }
    const budgetUnits = usdtUnits(budget), callUnits = usdtUnits(maxCall);
    if (budgetUnits === null || callUnits === null || budgetUnits <= 0n || callUnits <= 0n || budgetUnits > (usdtUnits(budgetLimit) ?? 0n) || callUnits > (usdtUnits(callLimit) ?? 0n)) { setError("Enter USDT amounts within the available budget and both agents' per-call limits, with up to 18 decimal places."); return; }
    const deadlineMilliseconds = Date.parse(deadline);
    if (!Number.isFinite(deadlineMilliseconds) || deadlineMilliseconds < Date.now() + 5 * 60e3 || deadlineMilliseconds > deadlineLimit) { setError(parent ? "Choose a deadline at least five minutes away and no later than the parent deadline." : "Choose a deadline between five minutes and thirty days away."); return; }
    if (!tools.length || tools.some((tool) => !allowed.includes(tool))) { setError("Choose shared tools within the parent permissions."); return; }
    if (services.some((id) => !availableServices.some((merchant) => merchant.id === id && !recipientBlocked(merchant)))) { setError("Choose service recipients within the inherited permissions."); return; }
    try { await onSave({ title, description, executor_id: executor, budget, max_call: maxCall, deadline: new Date(deadlineMilliseconds).toISOString(), tools, acceptance, minimum_block: minimumBlock, public_activity: parent?.public_activity ?? agent.public_activity, services }); }
    catch (reason) { setError((reason as Error).message); }
  }
  return <form className="job-form" onSubmit={submit}>
    <div className="panel-heading"><div><span className="eyebrow">{parent ? "a branch of the job" : "an agreement for the work"}</span><h3>{parent ? "delegate a smaller job" : "give it a job"}</h3></div><button type="button" className="text-link" onClick={onCancel} disabled={busy}>cancel</button></div>
    {parent && <p className="field-help">From {parent.title}. {jobMoney(parent.available)} USDT available{parent.funding === "unfunded" ? " in the planned budget" : ""}. Branch depth {parent.depth + 1} of {depthLimit}. Tools, recipients and per-call limits stay the same or narrower; the deadline cannot be later.</p>}
    <label>job name<input required minLength={3} maxLength={120} value={title} onChange={(event) => setTitle(event.target.value)} placeholder="e.g. research changes in pool liquidity" /></label>
    <label>what counts as done?<textarea required minLength={5} maxLength={1200} value={description} onChange={(event) => setDescription(event.target.value)} placeholder="Describe the result you want the agent to deliver." /></label>
    <label>assigned agent<select required aria-label="assigned agent" value={executor} onChange={(event) => { setExecutor(event.target.value); setTools([]); setServices([]); }}>{!candidates.length && <option value="">no eligible agents</option>}{candidates.map((candidate) => <option key={candidate.id} value={candidate.id}>{candidate.name}{candidate.id === agent.id ? " · your agent" : ""}</option>)}</select></label>
    <p className="field-help">Only registered, ready agents with shared tools are listed.{target && <> {target.name}'s per-call limit is {jobMoney(target.max_call)} USDT.</>}</p>
    <div className="job-form-grid"><label>total budget · USDT<input required type="text" inputMode="decimal" pattern="[0-9]+(\.[0-9]{1,18})?" max={budgetLimit} value={budget} onChange={(event) => setBudget(event.target.value)} /></label><label>per-call cap · USDT<input required type="text" inputMode="decimal" pattern="[0-9]+(\.[0-9]{1,18})?" max={callLimit} value={maxCall} onChange={(event) => setMaxCall(event.target.value)} /></label></div>
    <p className="field-help">Budget limit: {jobMoney(budgetLimit)} USDT. Per-call limit: {jobMoney(callLimit)} USDT, bounded by the budget, both agents{parent && " and the parent job"}.</p>
    <label>deadline<input required type="datetime-local" step="1" value={deadline} min={localJobTime(Date.now() + 5 * 60e3 + 1000)} max={localJobTime(deadlineLimit)} onChange={(event) => setDeadline(event.target.value)} /></label>
    {parent && <p className="field-help">Parent deadline: <ExactJobTime seconds={jobWindow(parent).deadline} />. The root buyer reviews every branch's evidence and signs acceptance.</p>}
    {!parent && <p className="field-help">The buyer reviews the exact submitted evidence, including finalized BNB Smart Chain block observations.</p>}
    <fieldset><legend>tools for this job</legend>{allowed.length ? allowed.map((tool) => <label className="checkbox-row" key={tool}><input type="checkbox" checked={tools.includes(tool)} onChange={(event) => { setTools(event.target.checked ? [...tools, tool] : tools.filter((value) => value !== tool)); setServices([]); }} /><span>{tool === "bnb-rpc" ? "chain data" : label(tool)}</span></label>) : <p className="field-help">No eligible agent shares these permissions. Enable shared tools and register or resume an agent first.</p>}</fieldset>
    {availableServices.length > 0 && <fieldset className="job-service-choices"><legend>approved service recipients</legend>{availableServices.map((merchant) => <label className="checkbox-row" key={merchant.id}><input type="checkbox" checked={services.includes(merchant.id)} disabled={recipientBlocked(merchant)} onChange={(event) => setServices(event.target.checked ? [...services, merchant.id] : services.filter((value) => value !== merchant.id))} /><span>{merchant.name}<code>{merchant.recipient}</code>{recipientBlocked(merchant) && <small>A service recipient cannot be either assigned wallet.</small>}</span></label>)}</fieldset>}
    {parent?.funding === "unfunded" && <p className="field-help">Fund the parent before allocating this branch onchain.</p>}
    {parent && !connectedServices.length && <p className="field-help">This parent allows no escrow service recipients. A branch cannot add one.</p>}
    {system?.service_payments_enabled !== true && <p className="field-help">Direct payments from job escrow to services are disabled. Connected model and research tools use operator credits separately.</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
    <button type="submit" className="primary" disabled={busy || !target || !allowed.length || depthBlocked || (usdtUnits(budgetLimit) ?? 0n) <= 0n}>{busy ? "saving…" : parent ? "reserve draft branch" : "save job terms"}<Plus size={14} /></button>
    <p className="field-help">{parent ? "Saving reserves a draft allocation within the parent budget. The parent executor's wallet signs the onchain allocation; it uses the funded parent escrow and needs BNB for gas. No new USDT deposit is required." : "Saving records the terms without transferring funds. The buyer separately approves and funds the USDT escrow with their wallet."}</p>
  </form>;
}

export function AgentJobs({ agent, owned, api, sendTransaction }: { agent: RuntimeAgent; owned: RuntimeAgent[]; api: AccountAPI; sendTransaction: (intent: JobIntent) => Promise<string> }) {
  const access = useHolderGate();
  const chainId = useChainId();
  const [jobs, setJobs] = useState<Job[]>([]), [choices, setChoices] = useState<AgentChoice[]>([]), [system, setSystem] = useState<JobSystem | null>(null), [merchants, setMerchants] = useState<JobMerchant[]>([]);
  const [creating, setCreating] = useState(false), [parent, setParent] = useState<Job | null>(null), [selected, setSelected] = useState<string | null>(null), [busy, setBusy] = useState(false), [error, setError] = useState("");
  const [evidenceText, setEvidenceText] = useState("");
  const [runs, setRuns] = useState<JobRun[]>([]), [runsLoading, setRunsLoading] = useState(false), [runsError, setRunsError] = useState("");
  const [intent, setIntent] = useState<JobIntent | null>(null), [txHash, setTxHash] = useState<string | null>(null), [uncertain, setUncertain] = useState(false);
  const [currentTime, setCurrentTime] = useState(Date.now);
  useEffect(() => { const timer = window.setInterval(() => setCurrentTime(Date.now()), 1000); return () => window.clearInterval(timer); }, []);
  const reload = useCallback(async () => {
    const [records, state, publicAgents, services, actions] = await Promise.all([jobAPI.list(api, agent.id), jobAPI.system(), request<AgentChoice[]>("/agents/live"), jobAPI.merchants(), jobAPI.actions(api)]);
    setJobs(records); setSystem(state); setMerchants(services); setChoices([...new Map([...publicAgents, ...owned.filter((a) => a.registry_id)].map((a) => [a.id, a])).values()]);
    const pending = actions.find((r: JobActionRecord) => r.confirmed === 0 && owned.some((a) => a.wallet?.toLowerCase() === r.intent.sender.toLowerCase()) && records.some((j) => j.id === r.intent.job_id));
    let localPending: {intent:JobIntent;txHash:string;uncertain:boolean}|null=null;
    if (!pending) try {
      for (const key of Object.keys(localStorage).filter(key=>key.startsWith("tab-job-action:"))) {
        const value=JSON.parse(localStorage.getItem(key)||"null") as {intent?:JobIntent;txHash?:string;uncertain?:boolean}|null;
        if(value?.intent&&(value.txHash||value.uncertain)&&owned.some(a=>a.wallet?.toLowerCase()===value.intent!.sender.toLowerCase())&&records.some(j=>j.id===value.intent!.job_id)) {localPending={intent:value.intent,txHash:value.txHash||"",uncertain:!!value.uncertain};break;}
      }
    } catch { /* The server's submitted receipt remains authoritative. */ }
    if(localPending){setIntent(localPending.intent);setTxHash(localPending.txHash);setUncertain(localPending.uncertain);}

    if (pending) {
      setIntent(pending.intent);
      let cached: { txHash?: string; uncertain?: boolean } | null = null;
      try { cached = JSON.parse(localStorage.getItem(`tab-job-action:${pending.intent.id}`) || "null"); } catch { /* retain the server record */ }
      setTxHash(pending.tx_hash ?? cached?.txHash ?? null);setUncertain(!!pending.tx_hash||!!cached?.uncertain);
    }
  }, [agent.id, api, owned]);
  useEffect(() => { setSelected(null); setCreating(false); setParent(null); setIntent(null); setTxHash(null); setUncertain(false); void reload().catch((e) => setError(e.message)); }, [reload]);
  const own = (id: string) => owned.some((a) => a.id === id);
  const job = jobs.find((j) => j.id === selected);
  useEffect(() => {
    const controller = new AbortController();
    setRuns([]); setRunsError(""); setRunsLoading(!!selected);
    if (selected) void jobAPI.runs(api, selected, { signal: controller.signal }).then(records => {
      if (!controller.signal.aborted) setRuns(records);
    }).catch(reason => { if (!controller.signal.aborted) setRunsError((reason as Error).message); }).finally(() => { if (!controller.signal.aborted) setRunsLoading(false); });
    return () => controller.abort();
  }, [selected, api]);
  const rootJob = job && jobs.find((item) => item.id === (job.root_id || job.id));
  const rootBuyer = rootJob?.requester_id;
  const windowState = job && jobWindow(job, currentTime);
  const treePaused = Boolean(job?.paused || rootJob?.paused);
  const unsettledChildren = (item: Job) => jobs.filter(child => child.parent_id === item.id && child.funding === "funded" && !["closed", "cancelled"].includes(child.state));
  const childCount = job ? unsettledChildren(job).length : 0;
  const ownsRootBuyer = Boolean(rootBuyer && own(rootBuyer));
  const ownsParentExecutor = Boolean(job?.parent_id && own(jobs.find(item => item.id === job.parent_id)?.executor_id ?? job.requester_id));
  const cancellationActor = Boolean(job && (own(job.executor_id) || own(job.requester_id) || (job.parent_id && ownsRootBuyer)));
  const cancellationAllowed = Boolean(job && windowState && childCount === 0 && (own(job.executor_id) || (windowState.refundOpen && (own(job.requester_id) || (job.parent_id && ownsRootBuyer)))));
  function delegationIssue(item: Job, current = Date.now()): string | null {
    const parentJob = jobs.find((value) => value.id === item.parent_id);
    const root = jobs.find((value) => value.id === (item.root_id || item.id));
    if (item.paused || parentJob?.paused || root?.paused) return "Resume the job tree before allocating this branch.";
    if (!jobWindow(item, current).submissionOpen) return "The branch deadline has passed. Save new terms before allocating work.";
    // A missing ancestor is validated by the API, which refreshes its chain state.
    if (!parentJob) return null;
    if (parentJob.funding !== "funded") return "Fund the parent before allocating this branch onchain.";
    if (parentJob.state !== "open") return "Allocate branches only while the funded parent job is open.";
    if (!jobWindow(parentJob, current).submissionOpen) return "The parent deadline has passed. Save new terms before allocating work.";
    return null;
  }
  const branchAllocationIssue = job?.parent_id && job.state === "draft" ? delegationIssue(job, currentTime) : null;
  function actionIssue(item: Job, action: string): string | null {
    const timing = jobWindow(item);
    const root = jobs.find(value => value.id === (item.root_id || item.id));
    const paused = item.paused || root?.paused;
    const children = unsettledChildren(item).length;
    if (action === "submit" && (item.state !== "open" || !timing.submissionOpen)) return "Evidence must be submitted by the original job deadline.";
    if (["accept", "reject"].includes(action)) {
      if (item.state !== "submitted" || !timing.reviewOpen) return "The review period has ended. Acceptance and rejection are no longer available.";
      if (paused) return "Resume the job tree before accepting or rejecting evidence.";
      if (action === "accept" && children > 0) return "Close or cancel each funded child branch before accepting this job.";
    }
    if (["cancel", "close_branch"].includes(action) && children > 0) return "Close or cancel each funded child branch first.";
    if (action === "cancel" && !own(item.executor_id) && !timing.refundOpen) return "Buyer refunds become available after the deadline plus 24 hours.";
    if (action === "close_branch" && (!item.parent_id || item.state !== "accepted")) return "Only an accepted branch that has not already closed can be closed.";
    if (["fund", "delegate"].includes(action) && !timing.submissionOpen) return "The job deadline has passed. Save new terms before funding work.";
    if (action === "delegate") return delegationIssue(item);
    return null;
  }
  async function work(action: () => Promise<void>) { setBusy(true); setError(""); try { await action(); } catch (e) { setError((e as Error).message); } finally { setBusy(false); } }
  async function prepare(item: Job, action: string) { await work(async () => { const issue = actionIssue(item, action); if (issue) throw Error(issue); await access.require(`job_${action}`); if(intent)throw Error("Resolve the existing wallet action before preparing another.");const value = await jobAPI.prepare(api, item.id, action); if (value.chain_id !== chainId) throw Error("The transaction network differs from the app configuration."); setIntent(value); setTxHash(null);setUncertain(false); }); }
  async function confirmAction() {
    if (!intent || !txHash) return;
    await work(async () => { await recordSubmitted(api,`/account/job-actions/${intent.id}/submitted`,txHash);await jobAPI.confirm(api, intent.id, txHash); localStorage.removeItem(`tab-job-action:${intent.id}`); setIntent(null); setTxHash(null); setUncertain(false); await reload(); });
  }
  async function send() {
    if (!intent || txHash || uncertain) return;
    await work(async () => {
      await access.require(`job_${intent.action}`, undefined, intent.sender);
      if (intent.chain_id !== chainId) throw Error("The transaction network differs from the app configuration.");
      if (Date.parse(intent.expires_at) <= Date.now()) throw Error("This unsigned transaction has expired. Prepare a fresh wallet action before signing.");
      const current = jobs.find(item => item.id === intent.job_id);
      if (current) { const issue = actionIssue(current, intent.action); if (issue) throw Error(issue); }
      setUncertain(true);
      try { localStorage.setItem(`tab-job-action:${intent.id}`,JSON.stringify({intent,txHash:"",uncertain:true})); } catch {}
      let hash:string;
      try { hash=await sendTransaction(intent); } catch(error) {if(walletPromptRejected(error)){setUncertain(false);try{localStorage.removeItem(`tab-job-action:${intent.id}`);}catch{}}throw error;}
      setTxHash(hash);
      try { localStorage.setItem(`tab-job-action:${intent.id}`, JSON.stringify({ txHash: hash, intent, uncertain:true })); } catch { /* continue receipt reconciliation even when browser storage is unavailable */ }
      await recordSubmitted(api,`/account/job-actions/${intent.id}/submitted`,hash);
      await waitForTransaction(hash);
      await jobAPI.confirm(api, intent.id, hash);
      localStorage.removeItem(`tab-job-action:${intent.id}`);
      setIntent(null); setTxHash(null); setUncertain(false); await reload();
    });
  }
  const live = system?.status === "live";
  return <section className="agent-jobs" aria-label="Agent job agreements">
    <div className="panel-heading"><div><h3>jobs and branches</h3><p className="field-help">Give work clear terms. Keep each branch inside its parent’s budget.</p></div><button className="outline" disabled={busy || agent.status !== "ready" || !access.allows("create_job")} onClick={() => { setCreating(true); setParent(null); }}><Plus size={14} />give it a job</button></div>
    {error && <p className="form-error" role="alert">{error}</p>}
    {!live && <p className="job-availability">Prepare job terms and branches now. Funded jobs are not open yet.</p>}
    {intent && <div className="job-wallet-review"><h4>review wallet action · {label(intent.action)}</h4><p>{jobs.find((j) => j.id === intent.job_id)?.title}</p><p className="field-help">Your BNB Smart Chain wallet signs this action and pays its BNB fees. USDT is held by the job vault.</p><details><summary>transaction details</summary><p>fee payer <code>{intent.sender}</code></p><p>contract <code>{intent.to}</code></p><p>network {intent.chain_id}</p><p>expires {shortDate(intent.expires_at)}</p></details>{txHash || uncertain ? <>{!txHash&&<p className="field-help">The wallet may have submitted this action. Check wallet activity and enter the final transaction hash before signing again.</p>}<label>submitted job transaction hash<input value={txHash||""} onChange={event=>setTxHash(event.target.value)} placeholder="0x…" spellCheck={false}/></label><a className="text-link" target="_blank" rel="noreferrer" href={explorer(txHash||"", "tx", chainId)}>view submitted transaction <ArrowUpRight size={13} /></a><button className="outline" disabled={busy||!/^0x[0-9a-fA-F]{64}$/.test(txHash||"")} onClick={() => void confirmAction()}><RefreshCw size={13} />check confirmation</button><button className="outline" disabled={busy||!/^0x[0-9a-fA-F]{64}$/.test(txHash||"")} onClick={()=>void work(async()=>{await recordSubmitted(api,`/account/job-actions/${intent.id}/submitted`,txHash!);await api(`/account/job-actions/${intent.id}/release-failed`,{method:"POST"});localStorage.removeItem(`tab-job-action:${intent.id}`);setIntent(null);setTxHash(null);setUncertain(false);await reload();})}>check failed transaction</button></> : <><button className="primary" disabled={busy || !access.allows(`job_${intent.action}`)} onClick={() => void send()}>sign {label(intent.action)}</button>{Date.parse(intent.expires_at) <= Date.now() && <button className="outline" disabled={busy} onClick={() => void work(async () => { await access.require(`job_${intent.action}`, undefined, intent.sender); const value = await jobAPI.prepare(api, intent.job_id, intent.action); if (value.chain_id !== chainId) throw Error("The transaction network differs from the app configuration."); setIntent(value); })}>prepare a fresh transaction</button>}</>}</div>}
    {creating && <JobForm agent={parent ? owned.find((a) => a.id === parent.executor_id) ?? agent : agent} choices={choices} parent={parent} merchants={merchants} system={system} busy={busy} onCancel={() => setCreating(false)} onSave={async (input) => { setBusy(true); try { await access.require("create_job"); const created = parent ? await jobAPI.branch(api, parent.id, input) : await jobAPI.create(api, agent.id, input); setCreating(false); setParent(null); setSelected(created.id); await reload(); window.dispatchEvent(new Event("tab:jobs-changed")); } finally { setBusy(false); } }} />}
    {!jobs.length && !creating && <p className="field-help">No agreements yet. Start with a small deliverable and a budget you can review.</p>}
    <div className="owned-job-list">{jobs.map((item) => <button type="button" key={item.id} disabled={busy} className={`owned-job${selected === item.id ? " selected" : ""}`} onClick={() => setSelected(item.id)}><JobBud accepted={paidWork(item.state)} /><span><strong>{item.title}</strong><small>{item.parent_id ? "branch" : "job"} · {item.executor_name}</small></span><span className="job-status" data-funding={item.funding}>{item.funding === "unfunded" ? "unfunded" : label(item.state)}</span></button>)}</div>
    {job && <div className="owned-job-detail" id={`job-${job.id}`}>
      <div className="panel-heading"><h4>{job.title}</h4><span className="job-status">{label(job.state)} · {job.funding}</span></div><p className="job-description">{job.description}</p>
      <div className="job-pairs"><span>budget<strong>{jobMoney(job.budget)} USDT</strong></span><span>available<strong>{jobMoney(job.available)} USDT{job.funding === "unfunded" ? " · planned" : ""}</strong></span><span>per-call cap<strong>{jobMoney(job.max_call)} USDT</strong></span><span>deadline<strong><ExactJobTime seconds={windowState!.deadline} /></strong></span><span>review ends<strong><ExactJobTime seconds={windowState!.reviewEnd} /></strong></span><span>acceptance<strong>buyer review</strong></span><span>tools<strong>{job.tools.join(" · ")}</strong></span></div>
      {job.funding === "funded" && ["open", "submitted"].includes(job.state) && <p className="field-help">{windowState!.reviewOpen ? "The root buyer signs acceptance to pay the executor. Review ends at the deadline plus 24 hours." : "The review period has ended. No automatic payment or refund was sent; an eligible wallet must review cancellation."}</p>}
      {job.funding === "funded" && job.state === "accepted" && <p className="field-help">Confirmed acceptance paid {jobMoney(job.reward_paid)} USDT to the executor.{job.parent_id && " This accepted branch still needs its separate closure."}</p>}
      {treePaused && ["open", "submitted"].includes(job.state) && <p className="field-help">The job tree is paused. Timely evidence submission and permitted cancellation remain available; new execution and acceptance wait for a resume.</p>}
      {job.state === "submitted" && !windowState!.submissionOpen && windowState!.reviewOpen && <p className="field-help">The delivery deadline has passed. Rejecting this submission cannot be followed by revised evidence under the same deadline.</p>}
      {childCount > 0 && <p className="field-help">{childCount} funded child {childCount === 1 ? "branch still needs" : "branches still need"} closure or cancellation before this job can be accepted or refunded. An accepted branch still needs its separate close action.</p>}
      {job.state === "closed" && <p className="field-help">Branch closure is confirmed onchain. Its accepted work and payment remain recorded.</p>}
      {job.funding === "funded" && childCount === 0 && ["open", "submitted", "accepted"].includes(job.state) && <p className="field-help">The contract checks the latest branch state before a wallet action is prepared.</p>}
      {job.parent_id && <p className="field-help">Branch of {jobs.find((j) => j.id === job.parent_id)?.title ?? "its parent job"}. Unused funds return to that parent.</p>}
      {job.evidence != null && <div className="job-evidence"><h4>evidence to review</h4>{isRecord(job.evidence) && "block_number" in job.evidence ? <p>BNB Smart Chain · block {String(job.evidence.block_number)}<br /><code>{String(job.evidence.block_hash)}</code></p> : <pre>{JSON.stringify(job.evidence, null, 2)}</pre>}<p className="field-help">{job.funding === "unfunded" ? "Collected for this draft. No reward has been paid." : paidWork(job.state) ? "Accepted onchain." : "Review before submission or acceptance."}</p></div>}
      <section className="job-runs" aria-label="Private job runs"><h4>work delivered by the agent</h4>{runsLoading?<p className="field-help" role="status">reading job runs…</p>:runsError?<p className="form-error" role="alert">Job runs unavailable: {runsError}</p>:runs.length?runs.slice(0,5).map(run=><div key={run.id} className="job-run" data-job-run-status={run.status}><RunReceipt run={{id:run.id,agent_id:run.agent_id,status:run.status,started_at:run.started_at,finished_at:run.finished_at,output:{...run.output.tools,...(run.output.error?{error:run.output.error}:{})}}} task={run.output.task} /><p className="field-help">{run.status==="completed"&&run.output.evidence_status==="attached"?"Result saved as job evidence. Submit it onchain for the buyer to review; payment follows acceptance.":"This run did not attach completed evidence. Resolve the unavailable tool or required authorization before running again."}</p></div>):<p className="field-help">No agent execution recorded for this job yet.</p>}</section>
      {own(job.executor_id) && job.acceptance === "buyer_review" && ["draft", "open"].includes(job.state) && windowState!.submissionOpen && <form className="job-evidence-input" onSubmit={(event) => { event.preventDefault(); void work(async () => { await jobAPI.attach(api, job.id, { result: evidenceText }); setEvidenceText(""); await reload(); }); }}><label>deliverable or result<textarea required minLength={5} maxLength={12000} value={evidenceText} onChange={(event) => setEvidenceText(event.target.value)} placeholder="Add the result for the buyer to review." /></label><button className="outline" disabled={busy}>save evidence</button><p className="field-help">Saving evidence records its hash. Submit it separately for onchain acceptance.</p></form>}
      {branchAllocationIssue && own(job.requester_id) && <p className="field-help">{branchAllocationIssue}</p>}
      <div className="job-actions">
        {job.state === "draft" && (own(job.requester_id) || own(job.executor_id)) && <button className="outline" disabled={busy} onClick={() => void work(async () => { await jobAPI.cancelDraft(api, job.id); await reload(); window.dispatchEvent(new Event("tab:jobs-changed")); })}>cancel draft</button>}
        {own(job.executor_id) && ["draft", "open"].includes(job.state) && !treePaused && <button className="outline" disabled={busy || (usdtUnits(job.available) ?? 0n) <= 0n || job.depth >= (system?.max_depth ?? 8) || owned.find((value) => value.id === job.executor_id)?.status !== "ready" || !windowState!.submissionOpen || !access.allows("job_delegate")} onClick={() => { setParent(job); setCreating(true); }}><GitBranch size={13} />delegate a branch</button>}
        {own(job.executor_id)&&job.state==="open"&&job.funding==="funded"&&<button className="primary" disabled={busy||!access.allows("run")||!live||!!intent||treePaused||Date.parse(job.deadline)<=currentTime||owned.find(value=>value.id===job.executor_id)?.status!=="ready"} onClick={()=>void work(async()=>{await access.require("run", undefined, owned.find(value=>value.id===job.executor_id)?.wallet || undefined);await jobAPI.run(api,job.id);const records=await jobAPI.runs(api,job.id);setRuns(records);setRunsError("");await reload();window.dispatchEvent(new Event("tab:jobs-changed"));})}>run job</button>}
        {live && !intent && <>
          {job.state === "draft" && !job.parent_id && own(job.requester_id) && <button className="primary" disabled={busy || !access.allows("job_fund")} onClick={() => void prepare(job, "fund")}>review funding</button>}
          {job.state === "draft" && job.parent_id && own(job.requester_id) && <button className="primary" disabled={busy || Boolean(branchAllocationIssue) || !access.allows("job_delegate")} onClick={() => void prepare(job, "delegate")}>allocate branch onchain</button>}
          {job.funding === "funded" && <>
            {job.state === "open" && own(job.executor_id) && job.evidence_hash && <button className="primary" disabled={busy || !windowState!.submissionOpen} onClick={() => void prepare(job, "submit")}>submit evidence</button>}
            {job.state === "submitted" && ownsRootBuyer && <><button className="primary" disabled={busy || treePaused || !windowState!.reviewOpen || childCount > 0} onClick={() => void prepare(job, "accept")}><Check size={13} />accept evidence and pay</button><button className="outline" disabled={busy || treePaused || !windowState!.reviewOpen} onClick={() => void prepare(job, "reject")}>{windowState!.submissionOpen ? "request revision" : "reject evidence"}</button></>}
            {job.state === "accepted" && job.parent_id && (ownsRootBuyer || ownsParentExecutor) && <button className="outline" disabled={busy || childCount > 0} onClick={() => void prepare(job, "close_branch")}>review branch closure</button>}
            {!job.parent_id && own(job.requester_id) && ["open", "submitted"].includes(job.state) && <button className="outline" disabled={busy || !access.allows(job.paused ? "job_resume" : "job_pause")} onClick={() => void prepare(job, job.paused ? "resume" : "pause")}><Pause size={13} />{job.paused ? "resume job" : "pause job tree"}</button>}
            {["open", "submitted"].includes(job.state) && cancellationActor && <button className="outline" disabled={busy || !cancellationAllowed} onClick={() => void prepare(job, "cancel")}>review cancellation</button>}
            {["open", "submitted"].includes(job.state) && cancellationActor && !own(job.executor_id) && !windowState!.refundOpen && <p className="field-help">Buyer cancellation becomes available after the review cutoff. The assigned executor can cancel earlier.</p>}
            <button className="text-link" disabled={busy} onClick={() => void work(async () => { await jobAPI.refresh(api, job.id); await reload(); })}><RefreshCw size={13} />refresh chain record</button>
          </>}
        </>}
      </div>
      <details className="job-proof"><summary>agreement record</summary><p>terms <code>{job.terms_hash}</code></p>{job.evidence_hash && <p>evidence <code>{job.evidence_hash}</code></p>}{job.chain_tx && <a className="text-link" href={explorer(job.chain_tx, "tx", chainId)} target="_blank" rel="noreferrer">transaction <ArrowUpRight size={13} /></a>}</details>
    </div>}
  </section>;
}
