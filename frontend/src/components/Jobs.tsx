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
import type { AccountAPI } from "../lib/jobs";
import { minimumAmount, usdtUnits } from "../lib/amounts";
import "./jobs.css";


type AgentChoice = { id: string; name: string; tools: string[]; status: string };
const label = (value: string) => value.replaceAll("_", " ");
const shortDate = (value: string) => new Date(value).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

export function JobBud({ accepted = false }: { accepted?: boolean }) {
  return <svg className={`job-bud${accepted ? " accepted" : ""}`} viewBox="0 0 24 30" fill="none" aria-hidden="true"><path d="M12 28V15M12 20C5 20 4 13 6 9C8 5 11 6 12 10C13 6 16 5 18 9C20 13 19 20 12 20Z" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" /><path d="M12 26C6 26 5 22 5 22M12 23C18 23 19 19 19 19" stroke="currentColor" strokeWidth="1.2" />{accepted && <path d="m8 12 3 3 5-6" stroke="currentColor" strokeWidth="1.3" />}</svg>;
}

function PublicJobRow({ job, jobs }: { job: PublicJob; jobs: PublicJob[] }) {
  const chainId = useChainId();
  const children = jobs.filter((j) => j.parent_id === job.id);
  return <details className="garden-job" id={`job-${job.id}`}>
    <summary><JobBud accepted={job.state === "accepted"} /><span><strong>{job.title}</strong><small>{job.executor_name} · {label(job.state)}</small></span><span className="job-status" data-funding={job.funding}>{job.funding === "unfunded" ? "unfunded" : `${jobMoney(job.budget)} USDT`}</span></summary>
    <div className="job-public-detail">
      <div className="job-pairs"><span>acceptance<strong>{job.acceptance === "block_observation" ? "recent block observation" : "buyer reviews evidence"}</strong></span><span>deadline<strong>{shortDate(job.deadline)}</strong></span><span>available<strong>{jobMoney(job.available)} USDT{job.funding === "unfunded" ? " · planned" : ""}</strong></span><span>branches<strong>{children.length}</strong></span><span>reward paid<strong>{jobMoney(job.reward_paid ?? 0)} USDT</strong></span><span>services paid<strong>{jobMoney(job.provider_paid ?? 0)} USDT</strong></span></div>
      {job.paused && <p className="field-help">This job's escrow actions are paused.</p>}
      {children.map((child) => <div className="job-branch-row" key={child.id}><GitBranch size={14} /><Link to={`/agents/${child.executor_id}#job-${child.id}`}>{child.executor_name}</Link><span>{jobMoney(child.budget)} USDT · {child.funding === "unfunded" ? "planned" : label(child.state)}</span></div>)}
      <div className="job-public-links"><Link className="text-link" to={`/agents/${job.executor_id}`}>open agent <ArrowUpRight size={13} /></Link>{job.chain_tx && <a className="text-link" href={explorer(job.chain_tx, "tx", chainId)} target="_blank" rel="noreferrer">transaction <ArrowUpRight size={13} /></a>}</div>
      <details className="job-proof"><summary>agreement record</summary><p>terms hash <code>{job.terms_hash}</code></p>{job.evidence_hash && <p>evidence hash <code>{job.evidence_hash}</code></p>}</details>
    </div>
  </details>;
}

export function GardenJobs({ jobs, error }: { jobs: PublicJob[]; error?: string }) {
  const roots = jobs.filter((j) => !j.parent_id && j.state !== "cancelled");
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

function JobForm({ agent, choices, parent, merchants, onSave, onCancel, busy }: { agent: RuntimeAgent; choices: AgentChoice[]; parent: Job | null; merchants: JobMerchant[]; onSave: (input: JobInput) => Promise<void>; onCancel: () => void; busy: boolean }) {
  const [title, setTitle] = useState(""), [description, setDescription] = useState("");
  const [executor, setExecutor] = useState(choices.find((a) => a.status === "ready" && a.id !== agent.id)?.id ?? agent.id);
  const [budget, setBudget] = useState(minimumAmount(parent?.available ?? String(agent.daily_cap), parent ? "1" : "5"));
  const [maxCall, setMaxCall] = useState(minimumAmount(agent.max_call, parent?.max_call ?? "0.1"));
  const [deadline, setDeadline] = useState(() => { const value = new Date(Math.min(Date.now() + 3600e3, parent ? new Date(parent.deadline).getTime() - 60e3 : Infinity)); return new Date(value.getTime() - value.getTimezoneOffset() * 60e3).toISOString().slice(0, 16); });
  const [acceptance] = useState<JobInput["acceptance"]>("buyer_review"), [minimumBlock] = useState(parent?.minimum_block ?? 1);
  const [tools, setTools] = useState<JobInput["tools"]>(["bnb-rpc"]), [services, setServices] = useState<string[]>([]), [error, setError] = useState("");
  const target = choices.find((a) => a.id === executor);
  const allowed = agent.tools.filter((tool) => (!parent || parent.tools.includes(tool as JobInput["tools"][number])) && target?.tools.includes(tool));
  const budgetLimit = parent?.available ?? String(agent.daily_cap);
  const callLimit = minimumAmount(budget, agent.max_call, parent?.max_call ?? "10");
  async function submit(event: FormEvent) {
    event.preventDefault(); setError("");
    const budgetUnits = usdtUnits(budget), callUnits = usdtUnits(maxCall);
    if (budgetUnits === null || callUnits === null || budgetUnits <= 0n || callUnits <= 0n || budgetUnits > (usdtUnits(budgetLimit) ?? 0n) || callUnits > (usdtUnits(callLimit) ?? 0n)) { setError("Enter USDT amounts within the available budget and per-call limit, with up to 18 decimal places."); return; }
    if (!tools.length || tools.some((t) => !allowed.includes(t))) { setError("Choose tools enabled for both agents."); return; }
    try { await onSave({ title, description, executor_id: executor, budget, max_call: maxCall, deadline: new Date(deadline).toISOString(), tools, acceptance, minimum_block: minimumBlock, public_activity: parent?.public_activity ?? agent.public_activity, services }); }
    catch (e) { setError((e as Error).message); }
  }
  return <form className="job-form" onSubmit={submit}>
    <div className="panel-heading"><div><span className="eyebrow">{parent ? "a branch of the job" : "an agreement for the work"}</span><h3>{parent ? "delegate a smaller job" : "give it a job"}</h3></div><button type="button" className="text-link" onClick={onCancel} disabled={busy}>cancel</button></div>
    {parent && <p className="field-help">From {parent.title}. {jobMoney(parent.available)} USDT available{parent.funding === "unfunded" ? " in the planned budget" : ""}. A branch inherits narrower permissions and an earlier deadline.</p>}
    <label>job name<input required minLength={3} maxLength={120} value={title} onChange={(e) => setTitle(e.target.value)} placeholder="e.g. observe the next confirmed block" /></label>
    <label>what counts as done?<textarea required minLength={5} maxLength={1200} value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Describe the result you want the agent to deliver." /></label>
    <label>assigned agent<select value={executor} onChange={(e) => { setExecutor(e.target.value); setTools([]); setServices([]); }}>{choices.filter((a) => a.status === "ready").map((a) => <option key={a.id} value={a.id}>{a.name}{a.id === agent.id ? " · your agent" : ""}</option>)}</select></label>
    <div className="job-form-grid"><label>total budget · USDT<input required type="text" inputMode="decimal" pattern="[0-9]+(\.[0-9]{1,18})?" max={budgetLimit} value={budget} onChange={(e) => setBudget(e.target.value)} /></label><label>per-call cap · USDT<input required type="text" inputMode="decimal" pattern="[0-9]+(\.[0-9]{1,18})?" max={callLimit} value={maxCall} onChange={(e) => setMaxCall(e.target.value)} /></label></div>
    <label>deadline<input required type="datetime-local" value={deadline} min={new Date(Date.now() - new Date().getTimezoneOffset() * 60e3 + 5 * 60e3).toISOString().slice(0, 16)} max={parent ? new Date(new Date(parent.deadline).getTime() - new Date().getTimezoneOffset() * 60e3).toISOString().slice(0, 16) : undefined} onChange={(e) => setDeadline(e.target.value)} /></label>
    <p className="field-help">The buyer reviews the exact submitted evidence, including finalized BNB Smart Chain block observations.</p>
    <fieldset><legend>tools for this job</legend>{allowed.length ? allowed.map((tool) => <label className="checkbox-row" key={tool}><input type="checkbox" checked={tools.includes(tool as JobInput["tools"][number])} onChange={(e) => { setTools(e.target.checked ? [...tools, tool as JobInput["tools"][number]] : tools.filter((t) => t !== tool)); setServices([]); }} /><span>{tool === "bnb-rpc" ? "chain data" : tool}</span></label>) : <p className="field-help">These agents have no shared tools.</p>}</fieldset>
    {merchants.filter((m) => tools.includes(m.tool) && (!parent || parent.services?.includes(m.id))).map((m) => <label className="checkbox-row" key={m.id}><input type="checkbox" checked={services.includes(m.id)} onChange={(e) => setServices(e.target.checked ? [...services, m.id] : services.filter((s) => s !== m.id))} />{m.name}<small>escrow service</small></label>)}
    {error && <p className="form-error" role="alert">{error}</p>}
    <button type="submit" className="primary" disabled={busy || !allowed.length}>{busy ? "saving…" : parent ? "reserve draft branch" : "save job terms"}<Plus size={14} /></button>
    <p className="field-help">Saving terms reserves a planned budget. Funding and onchain allocations need a separate wallet approval.</p>
  </form>;
}

export function AgentJobs({ agent, owned, api, sendTransaction }: { agent: RuntimeAgent; owned: RuntimeAgent[]; api: AccountAPI; sendTransaction: (intent: JobIntent) => Promise<string> }) {
  const chainId = useChainId();
  const [jobs, setJobs] = useState<Job[]>([]), [choices, setChoices] = useState<AgentChoice[]>([]), [system, setSystem] = useState<JobSystem | null>(null), [merchants, setMerchants] = useState<JobMerchant[]>([]);
  const [creating, setCreating] = useState(false), [parent, setParent] = useState<Job | null>(null), [selected, setSelected] = useState<string | null>(null), [busy, setBusy] = useState(false), [error, setError] = useState("");
  const [evidenceText, setEvidenceText] = useState("");
  const [intent, setIntent] = useState<JobIntent | null>(null), [txHash, setTxHash] = useState<string | null>(null), [uncertain, setUncertain] = useState(false);
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
  const rootBuyer = job && jobs.find((item) => item.id === (job.root_id || job.id))?.requester_id;
  async function work(action: () => Promise<void>) { setBusy(true); setError(""); try { await action(); } catch (e) { setError((e as Error).message); } finally { setBusy(false); } }
  async function prepare(item: Job, action: string) { await work(async () => { if(intent)throw Error("Resolve the existing wallet action before preparing another.");const value = await jobAPI.prepare(api, item.id, action); if (value.chain_id !== chainId) throw Error("The transaction network differs from the app configuration."); setIntent(value); setTxHash(null);setUncertain(false); }); }
  async function confirmAction() {
    if (!intent || !txHash) return;
    await work(async () => { await recordSubmitted(api,`/account/job-actions/${intent.id}/submitted`,txHash);await jobAPI.confirm(api, intent.id, txHash); localStorage.removeItem(`tab-job-action:${intent.id}`); setIntent(null); setTxHash(null); setUncertain(false); await reload(); });
  }
  async function send() {
    if (!intent || txHash || uncertain) return;
    await work(async () => {
      if (intent.chain_id !== chainId) throw Error("The transaction network differs from the app configuration.");
      if (Date.parse(intent.expires_at) <= Date.now()) throw Error("This unsigned transaction has expired. Prepare a fresh wallet action before signing.");
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
    <div className="panel-heading"><div><h3>jobs and branches</h3><p className="field-help">Give work clear terms. Keep each branch inside its parent’s budget.</p></div><button className="outline" disabled={busy || agent.status !== "ready"} onClick={() => { setCreating(true); setParent(null); }}><Plus size={14} />give it a job</button></div>
    {error && <p className="form-error" role="alert">{error}</p>}
    {!live && <p className="job-availability">Prepare job terms and branches now. Funded jobs are not open yet.</p>}
    {intent && <div className="job-wallet-review"><h4>review wallet action · {label(intent.action)}</h4><p>{jobs.find((j) => j.id === intent.job_id)?.title}</p><p className="field-help">Your BNB Smart Chain wallet signs this action and pays its BNB fees. USDT is held by the job vault.</p><details><summary>transaction details</summary><p>fee payer <code>{intent.sender}</code></p><p>contract <code>{intent.to}</code></p><p>network {intent.chain_id}</p><p>expires {shortDate(intent.expires_at)}</p></details>{txHash || uncertain ? <>{!txHash&&<p className="field-help">The wallet may have submitted this action. Check wallet activity and enter the final transaction hash before signing again.</p>}<label>submitted job transaction hash<input value={txHash||""} onChange={event=>setTxHash(event.target.value)} placeholder="0x…" spellCheck={false}/></label><a className="text-link" target="_blank" rel="noreferrer" href={explorer(txHash||"", "tx", chainId)}>view submitted transaction <ArrowUpRight size={13} /></a><button className="outline" disabled={busy||!/^0x[0-9a-fA-F]{64}$/.test(txHash||"")} onClick={() => void confirmAction()}><RefreshCw size={13} />check confirmation</button><button className="outline" disabled={busy||!/^0x[0-9a-fA-F]{64}$/.test(txHash||"")} onClick={()=>void work(async()=>{await recordSubmitted(api,`/account/job-actions/${intent.id}/submitted`,txHash!);await api(`/account/job-actions/${intent.id}/release-failed`,{method:"POST"});localStorage.removeItem(`tab-job-action:${intent.id}`);setIntent(null);setTxHash(null);setUncertain(false);await reload();})}>check failed transaction</button></> : <><button className="primary" disabled={busy} onClick={() => void send()}>sign {label(intent.action)}</button>{Date.parse(intent.expires_at) <= Date.now() && <button className="outline" disabled={busy} onClick={() => void work(async () => { const value = await jobAPI.prepare(api, intent.job_id, intent.action); if (value.chain_id !== chainId) throw Error("The transaction network differs from the app configuration."); setIntent(value); })}>prepare a fresh transaction</button>}</>}</div>}
    {creating && <JobForm agent={parent ? owned.find((a) => a.id === parent.executor_id) ?? agent : agent} choices={choices} parent={parent} merchants={merchants} busy={busy} onCancel={() => setCreating(false)} onSave={async (input) => { setBusy(true); try { const created = parent ? await jobAPI.branch(api, parent.id, input) : await jobAPI.create(api, agent.id, input); setCreating(false); setParent(null); setSelected(created.id); await reload(); window.dispatchEvent(new Event("tab:jobs-changed")); } finally { setBusy(false); } }} />}
    {!jobs.length && !creating && <p className="field-help">No agreements yet. Start with a small deliverable and a budget you can review.</p>}
    <div className="owned-job-list">{jobs.map((item) => <button type="button" key={item.id} className={`owned-job${selected === item.id ? " selected" : ""}`} onClick={() => setSelected(item.id)}><JobBud accepted={item.state === "accepted"} /><span><strong>{item.title}</strong><small>{item.parent_id ? "branch" : "job"} · {item.executor_name}</small></span><span className="job-status" data-funding={item.funding}>{item.funding === "unfunded" ? "unfunded" : label(item.state)}</span></button>)}</div>
    {job && <div className="owned-job-detail" id={`job-${job.id}`}>
      <div className="panel-heading"><h4>{job.title}</h4><span className="job-status">{label(job.state)} · {job.funding}</span></div><p className="job-description">{job.description}</p>
      <div className="job-pairs"><span>budget<strong>{jobMoney(job.budget)} USDT</strong></span><span>available<strong>{jobMoney(job.available)} USDT{job.funding === "unfunded" ? " · planned" : ""}</strong></span><span>per-call cap<strong>{jobMoney(job.max_call)} USDT</strong></span><span>deadline<strong>{shortDate(job.deadline)}</strong></span><span>acceptance<strong>{"buyer review"}</strong></span><span>tools<strong>{job.tools.join(" · ")}</strong></span></div>
      {job.parent_id && <p className="field-help">Branch of {jobs.find((j) => j.id === job.parent_id)?.title ?? "its parent job"}. Unused funds return to that parent.</p>}
      {job.evidence != null && <div className="job-evidence"><h4>evidence to review</h4>{isRecord(job.evidence) && "block_number" in job.evidence ? <p>BNB Smart Chain · block {String(job.evidence.block_number)}<br /><code>{String(job.evidence.block_hash)}</code></p> : <pre>{JSON.stringify(job.evidence, null, 2)}</pre>}<p className="field-help">{job.funding === "unfunded" ? "Collected for this draft. No reward has been paid." : job.state === "accepted" ? "Accepted onchain." : "Review before submission or acceptance."}</p></div>}
      {own(job.executor_id) && job.acceptance === "buyer_review" && ["draft", "open"].includes(job.state) && !job.paused && <form className="job-evidence-input" onSubmit={(event) => { event.preventDefault(); void work(async () => { await jobAPI.attach(api, job.id, { result: evidenceText }); setEvidenceText(""); await reload(); }); }}><label>deliverable or result<textarea required minLength={5} maxLength={12000} value={evidenceText} onChange={(event) => setEvidenceText(event.target.value)} placeholder="Add the result for the buyer to review." /></label><button className="outline" disabled={busy}>save evidence</button><p className="field-help">Saving evidence records its hash. Submit it separately for onchain acceptance.</p></form>}
      <div className="job-actions">
        {job.state === "draft" && (own(job.requester_id) || own(job.executor_id)) && <button className="outline" disabled={busy} onClick={() => void work(async () => { await jobAPI.cancelDraft(api, job.id); await reload(); window.dispatchEvent(new Event("tab:jobs-changed")); })}>cancel draft</button>}
        {own(job.executor_id) && ["draft", "open"].includes(job.state) && !job.paused && <><button className="outline" disabled={busy || Number(job.available) <= 0 || job.depth >= 8} onClick={() => { setParent(job); setCreating(true); }}><GitBranch size={13} />delegate a branch</button>{job.tools.includes("bnb-rpc") && <button className="outline" disabled={busy} onClick={() => void work(async () => { await jobAPI.evidence(api, job.id); await reload(); })}>collect observation</button>}</>}
        {live && !intent && <>
          {job.state === "draft" && !job.parent_id && own(job.requester_id) && <button className="primary" disabled={busy} onClick={() => void prepare(job, "fund")}>review funding</button>}
          {job.state === "draft" && job.parent_id && own(job.requester_id) && <button className="primary" disabled={busy} onClick={() => void prepare(job, "delegate")}>allocate branch onchain</button>}
          {job.funding === "funded" && <>
            {job.state === "open" && own(job.executor_id) && job.evidence_hash && <button className="primary" disabled={busy || job.paused} onClick={() => void prepare(job, "submit")}>submit {"evidence"}</button>}
            {job.state === "submitted" && rootBuyer && own(rootBuyer) && <><button className="primary" disabled={busy || job.paused} onClick={() => void prepare(job, "accept")}><Check size={13} />accept evidence and pay</button><button className="outline" disabled={busy} onClick={() => void prepare(job, "reject")}>request revision</button></>}
            {!job.parent_id && own(job.requester_id) && ["open", "submitted"].includes(job.state) && <button className="outline" disabled={busy} onClick={() => void prepare(job, job.paused ? "resume" : "pause")}><Pause size={13} />{job.paused ? "resume job" : "pause job tree"}</button>}
            {["open", "submitted"].includes(job.state) && (own(job.requester_id) || own(job.executor_id)) && <button className="outline" disabled={busy} onClick={() => void prepare(job, "cancel")}>review cancellation</button>}
            <button className="text-link" disabled={busy} onClick={() => void work(async () => { await jobAPI.refresh(api, job.id); await reload(); })}><RefreshCw size={13} />refresh chain record</button>
          </>}
        </>}
      </div>
      <details className="job-proof"><summary>agreement record</summary><p>terms <code>{job.terms_hash}</code></p>{job.evidence_hash && <p>evidence <code>{job.evidence_hash}</code></p>}{job.chain_tx && <a className="text-link" href={explorer(job.chain_tx, "tx", chainId)} target="_blank" rel="noreferrer">transaction <ArrowUpRight size={13} /></a>}</details>
    </div>}
  </section>;
}
