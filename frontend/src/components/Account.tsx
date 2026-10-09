import { useState, useEffect, useCallback, useRef } from "react";
import { PrivyProvider, usePrivy, useWallets } from "@privy-io/react-auth";
import { bsc } from "viem/chains";
import { NoWalletSubmissionError, BNB_CHAIN_ID, bnbWalletClient, explorer, shortId, sameAddress, matchesTransaction, sendBnbTransaction, validateTransaction, waitForTransaction, type EvmTransaction } from "../lib/evm";
import {
  Plus,
  Play,
  Pause,
  Copy,
  LogOut,
  ArrowUpRight,
  RefreshCw,
  ArrowRight,
} from "lucide-react";
import { request, isRecord } from "../lib/api";
import { creationRequest, finishCreation } from "../lib/account";
import type {
  Config,
  Provider,
  RuntimeAgent,
  RuntimeInput,
  AgentRun,
  AgentEvent,
  AgentPlan,
} from "../lib/api";
import { AgentGlyph, Bird } from "./Drawings";
import { AgentJobs } from "./Jobs";
import { RunReceipt } from "./RunReceipt";
import { OnboardingCosts } from "./OnboardingCosts";
import { useSearchParams } from "react-router-dom";
import { isAddress } from "viem";
import { AgentWizard } from "./AgentWizard";
import { AgentAccessKey } from "./AgentAccessKey";
import { AgentPayments } from "./AgentPayments";
import { AgentCredit } from "./AgentCredit";
import { AgentFinance } from "./AgentFinance";
import { AgentWalletActions, walletPromptRejected } from "./AgentWalletActions";
import { signPaymentQuote } from "../lib/payments";
import { RegistrationRecovery } from "./RegistrationRecovery";
import { rememberRegistration, storedRegistration, sponsorStatus, sponsoredResult, startSponsoredRegistration, type DirectRegistration, type PendingRegistration } from "../lib/registration";
function AgentAccount({ config }: { config: Config }) {
  const chainId = config.chain_id;
  const [searchParams] = useSearchParams();
  const starterWatch = searchParams.get("watch");
  const accountSection=searchParams.get("section"), targetAgent=searchParams.get("target")||undefined;
  const initialAsset=searchParams.get("asset")||undefined;
  const requestedAgent=searchParams.get("agent"), appliedAgentLink=useRef<string|null>(null);
  const starter = starterWatch && isAddress(starterWatch, {strict:false}) ? {template:"onchain", name:"wallet monitor", purpose:"Track this wallet and report its BNB and USDT balances.", watch_address:starterWatch, tools:["bnb-rpc"]} : undefined;
  const { ready, authenticated, login, logout, getAccessToken, user } = usePrivy();
  const { wallets, ready: walletsReady } = useWallets();
  const registrationKey=`tab:registration:pending:${user?.id||"signed-out"}`;
  const [agents, setAgents] = useState<RuntimeAgent[]>([]),
    [creating, setCreating] = useState(false),
    [pending, setPending] = useState(false),
    [error, setError] = useState(""),
    [selected, setSelected] = useState<RuntimeAgent | null>(null);
  useEffect(()=>{if(!requestedAgent){appliedAgentLink.current=null;return;}if(appliedAgentLink.current===requestedAgent)return;const agent=agents.find(a=>a.id===requestedAgent);if(agent){setSelected(agent);appliedAgentLink.current=requestedAgent;setCreating(false);}},[agents,requestedAgent]);
  const selectedId = useRef<string | null>(null);
  selectedId.current = selected?.id ?? null;
  useEffect(()=>{if(!selected||!accountSection)return; const frame=requestAnimationFrame(()=>{const node=document.getElementById(`account-${accountSection}`); if(node instanceof HTMLDetailsElement)node.open=true;node?.scrollIntoView({block:"start"});});return()=>cancelAnimationFrame(frame);},[selected?.id,accountSection]);
  const [runs, setRuns] = useState<AgentRun[]>([]),
    [events, setEvents] = useState<AgentEvent[]>([]),
    [balance, setBalance] = useState<{
      wallet: string;
      bnb: string;
      usdt: string;
    } | null>(null),
    [copied, setCopied] = useState(false);
  const [registration, setRegistration] = useState<PendingRegistration | null>(()=>storedRegistration(registrationKey));
  useEffect(()=>setRegistration(storedRegistration(registrationKey)),[registrationKey]);
  const [selfPay, setSelfPay] = useState(false);
  function saveRegistration(value: PendingRegistration | null) { setRegistration(value); rememberRegistration(registrationKey, value); }
  const [prepared, setPrepared] = useState<RuntimeAgent | null>(null);
  const [legacy, setLegacy] = useState<AgentPlan[]>([]),
    [legacySelection, setLegacySelection] = useState<AgentPlan | null>(null);
  const api = useCallback(
    async <T,>(path: string, options: RequestInit = {}) =>
      request<T>(path, options, await getAccessToken()),
    [getAccessToken],
  );
  const reload = useCallback(async () => {
    await api("/account/profile", {method: "POST", body: "{}"});
    const [a, p] = await Promise.all([
      api<RuntimeAgent[]>("/account/runtime"),
      api<AgentPlan[]>("/account/agents"),
    ]);
    setAgents(a);
    setLegacy(p);
    return a;
  }, [api]);
  useEffect(() => { if (authenticated && starterWatch && isAddress(starterWatch,{strict:false})) setCreating(true); }, [authenticated,starterWatch]);
  useEffect(() => {
    if (authenticated) void reload().catch((e) => setError(e.message));
    else setAgents([]);
  }, [authenticated, reload]);
  const detail = useCallback(
    async (agent: RuntimeAgent) => {
      if (!agent.wallet) return;
      const [r, e, b] = await Promise.all([
        api<AgentRun[]>(`/account/runtime/${agent.id}/runs`),
        api<AgentEvent[]>(`/account/runtime/${agent.id}/events`),
        api<{ wallet: string; bnb: string; usdt: string }>(
          `/account/runtime/${agent.id}/balance`,
        ),
      ]);
      if (selectedId.current !== agent.id) return;
      setRuns(r);
      setEvents(e);
      setBalance(b);
    },
    [api],
  );
  useEffect(() => {
    if (!selected) return;
    void detail(selected).catch((e) => setError(e.message));
    const timer = setInterval(
      () => void detail(selected).catch(() => {}),
      15000,
    );
    return () => clearInterval(timer);
  }, [selected, detail]);
  async function refreshSelected(id:string){const values=await reload();const current=values.find(value=>value.id===id);if(current&&selectedId.current===id){setSelected(current);await detail(current);}}
  async function withBusy(action: () => Promise<void>) {
    setPending(true);
    setError("");
    try {
      await action();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setPending(false);
    }
  }
  async function ownerWallet(address?: string) {
    if (chainId !== BNB_CHAIN_ID) throw Error("The app must use BNB Smart Chain (56) before signing.");
    if (!walletsReady) throw Error("Your wallet is still loading. Your setup is saved; try again shortly.");
    const preferred = address || user?.wallet?.address;
    const wallet = preferred ? wallets.find(w => sameAddress(w.address, preferred)) :
      wallets.find(w => w.walletClientType === "privy") || (wallets.length === 1 ? wallets[0] : undefined);
    if (!wallet) throw Error(address ? "Connect the wallet that owns this agent." : "Connect an EVM wallet to continue.");
    return wallet;
  }
  async function sendTransactions(sender: string, transactions: EvmTransaction[]) {
    let wallet: Awaited<ReturnType<typeof ownerWallet>>;
    try {
      if (!transactions.length) throw Error("The wallet action has no transactions.");
      transactions.forEach(validateTransaction);
      wallet = await ownerWallet(sender);
    } catch (cause) {
      throw new NoWalletSubmissionError(cause);
    }
    let hash = "";
    for (let index = 0; index < transactions.length; index++) {
      hash = await sendBnbTransaction(wallet, transactions[index], sender);
      if (index < transactions.length - 1) await waitForTransaction(hash);
    }
    return hash;
  }
  async function acceptRegistration(item: RuntimeAgent) {
    if (user?.id) finishCreation(user.id);
    if (legacySelection) {
      await api(`/account/agents/${legacySelection.id}`, { method: "DELETE" });
      setLegacySelection(null);
    }
    setRegistration(null);
    rememberRegistration(registrationKey,null);
    setPrepared(null);
    setCreating(false);
    setSelected(item);
    await reload();
    return item;
  }
  async function finishRegistration(proof: PendingRegistration) {
    if (proof.mode === "sponsored") {
      const result = await sponsorStatus(api, proof.id);
      const item = sponsoredResult(result, proof.id, proof.wallet, saveRegistration);
      if (item) return acceptRegistration(item);
      throw Error(result.message || (result.retryable ? "No registration is pending. You can review and sign a fresh permission." : "Registration is still pending. Check this request again before signing another."));
    }
    if(!/^0x[0-9a-fA-F]{64}$/.test(proof.tx_hash))throw Error("Enter the submitted registration transaction hash before signing again.");
    const item = await api<RuntimeAgent>(
      `/account/runtime/${proof.id}/register`,
      { method: "POST", body: JSON.stringify({wallet:proof.wallet,signature:proof.signature,tx_hash:proof.tx_hash}) },
    );
    return acceptRegistration(item);
  }
  async function register(agent: RuntimeAgent) {
    if(registration)throw Error("Resolve the pending registration before signing another.");
    const wallet = await ownerWallet(agent.wallet ?? undefined);
    if (!selfPay) {
      const { result, agent: registered } = await startSponsoredRegistration(api, agent, wallet, config.gas_sponsorship_enabled, saveRegistration);
      if (registered) return acceptRegistration(registered);
      if (result.retryable) throw Error(result.message || "Registration was not submitted. Review the saved setup and try again.");
      setCreating(false); setSelected(agent); await reload(); return agent;
    }
    const existing = await sponsorStatus(api, agent.id);
    const recovered = sponsoredResult(existing, agent.id, "", saveRegistration);
    if (recovered) return acceptRegistration(recovered);
    if (!(["not_submitted", "failed"].includes(existing.status) && existing.retryable === true)) throw Error("Resolve the sponsored registration before paying for a separate transaction.");
    const challenge = await api<{ message: string; transaction: EvmTransaction; chain_id: number }>(`/account/runtime/${agent.id}/challenge`, { method: "POST", body: JSON.stringify({ wallet: wallet.address, sponsored: false }) });
    if (challenge.chain_id !== BNB_CHAIN_ID) throw Error("The registration network differs from the app configuration.");
    validateTransaction(challenge.transaction);
    const client = await bnbWalletClient(wallet, agent.wallet ?? undefined);
    const signature = await client.signMessage({ message: challenge.message });
    const unsigned:DirectRegistration={id:agent.id,wallet:wallet.address,signature,tx_hash:"",transaction:challenge.transaction};
    rememberRegistration(registrationKey,unsigned);setRegistration(unsigned);
    let tx_hash:string;
    try{tx_hash=await sendBnbTransaction(wallet,challenge.transaction,wallet.address);}catch(error){if(walletPromptRejected(error)){rememberRegistration(registrationKey,null);setRegistration(null);}throw error;}
    const proof = {...unsigned,tx_hash};
    setRegistration(proof);
    rememberRegistration(registrationKey,proof);
    await waitForTransaction(tx_hash);
    return finishRegistration(proof);
  }
  async function create(input: RuntimeInput) {
    setPending(true);
    try {
      if (registration) return await finishRegistration(registration);
      let agent = prepared;
      if (!agent) {
        if (!user?.id) throw Error("Sign in before creating an agent.");
        agent = await api<RuntimeAgent>("/account/runtime", {
          method: "POST",
          headers: {"Idempotency-Key": creationRequest(user.id, input)},
          body: JSON.stringify(input),
        });
        setPrepared(agent);
      } else if (!agent.registration_tx) {
        agent = await api<RuntimeAgent>(`/account/runtime/${agent.id}`, {
          method: "PATCH",
          body: JSON.stringify(input),
        });
        setPrepared(agent);
      }
      await reload();
      if (config.contracts_status !== "live") { setPrepared(null); setCreating(false); setSelected(agent); return agent; }
      return await register(agent);
    } finally {
      setPending(false);
    }
  }
  function choose(agent: RuntimeAgent) {
    setSelected(agent);
    setRuns([]);
    setEvents([]);
    setBalance(null);
  }
  async function run(agent: RuntimeAgent) {
    await withBusy(async () => {
      await api<AgentRun>(`/account/runtime/${agent.id}/run`, {
        method: "POST",
      });
      await reload();
      await detail(agent);
    });
  }
  async function pause(agent: RuntimeAgent) {
    await withBusy(async () => {
      const a = await api<RuntimeAgent>(`/account/runtime/${agent.id}/pause`, {
        method: "POST",
        body: JSON.stringify({ paused: agent.status !== "paused" }),
      });
      setSelected(a);
      await reload();
    });
  }
  async function copy(value: string) {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    setTimeout(() => setCopied(false), 1800);
  }
  return (
    <div className="account-flow">
      <section className="page-intro account-heading">
        <div>
          <h1>{creating ? "create your agent." : "your agents."}</h1>
        </div>
        {authenticated && (
          <button className="text-link" onClick={logout}>
            <LogOut size={14} />
            sign out
          </button>
        )}
      </section>
      {error && (
        <p role="alert" className="error-banner">
          {error}
        </p>
      )}
      {!authenticated ? (
        <section className="account-signin panel">
          <Bird small />
          <h2>your agent starts here.</h2>
          <p>Choose its job, connect its tools, and follow each run.</p>
          <OnboardingCosts sponsored={config.gas_sponsorship_enabled}/>
          <button className="primary" disabled={!ready} onClick={() => login({ loginMethods: ["wallet"] })}>
            connect wallet
            <ArrowUpRight size={15} />
          </button>
          <button className="text-link" disabled={!ready} onClick={() => login({ loginMethods: ["email"] })}>continue with email</button>
        </section>
      ) : (
        <>
          {registration && <RegistrationRecovery registration={registration} busy={pending} onChange={saveRegistration}
            onCheck={() => void withBusy(async () => { await finishRegistration(registration); })}
            onFailed={() => void withBusy(async () => {
              if (registration.mode === "sponsored") return;
              const state = await api<{status:string;transaction?:Record<string,unknown>}>(`/bnb/transactions/${registration.tx_hash}`);
              if (state.status !== "failed" || !state.transaction || !matchesTransaction(state.transaction, registration.transaction, registration.wallet)) throw Error("This exact registration has not been verified as reverted. Keep its pending record.");
              saveRegistration(null);
            })} />}
          {creating ? (
            <AgentWizard
              wallet={user?.wallet?.address || (wallets.length === 1 ? wallets[0].address : undefined)}
              initial={
                legacySelection
                  ? {
                      name: legacySelection.name,
                      purpose: legacySelection.purpose,
                      daily_cap: legacySelection.daily_cap,
                    }
                  : starter
              }
              onCreate={create}
              sponsorshipAvailable={config.gas_sponsorship_enabled}
              sponsorshipMessage={config.gas_sponsorship_message}
              selfPay={selfPay}
              onSelfPayChange={setSelfPay}
              onCancel={() => setCreating(false)}
              pending={pending}
            />
          ) : (
            <>
              <div className="workspace-heading">
                <span>{agents.length} agents</span>
                <button
                  className="primary"
                  onClick={() => {
                    setCreating(true);
                    setSelected(null);
                    setLegacySelection(null);
                    setPrepared(null);
                    setSelfPay(false);
                  }}
                >
                  <Plus size={15} />
                  create agent
                </button>
              </div>
              {legacy.length > 0 && (
                <div className="legacy-setups">
                  {legacy.map((p) => (
                    <div key={p.id}>
                      <span>
                        {p.name}
                        <small>saved setup</small>
                      </span>
                      <button
                        className="text-link"
                        onClick={() => {
                          setLegacySelection(p);
                          setSelfPay(false);
                          setCreating(true);
                        }}
                      >
                        continue setup
                        <ArrowRight size={13} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <div className="owned-agents">
                {agents.map((a, i) => (
                  <button
                    key={a.id}
                    className={`panel owned-agent ${selected?.id === a.id ? "selected" : ""}`}
                    onClick={() => choose(a)}
                  >
                    <AgentGlyph index={i} />
                    <div>
                      <strong>{a.name}</strong>
                      <small>
                        {a.registry_id
                          ? `#${shortId(a.registry_id)} · ${a.cadence}`
                          : "finish registration"}
                      </small>
                    </div>
                    <span className={`status-pill ${a.status}`}>
                      {a.status === "awaiting_registration"
                        ? "not registered"
                        : a.status === "paused" ? "runs paused" : a.status}
                    </span>
                  </button>
                ))}
              </div>
              {!agents.length && (
                <div className="panel empty">
                  create your first agent to get started.
                </div>
              )}
              {selected && (
                <section className="panel agent-controls">
                  <div className="panel-heading">
                    <div>
                      <h2>{selected.name}</h2>
                      <p className="muted">{selected.purpose}</p>
                    </div>
                    {selected.wallet && (
                      <button
                        className="icon-button"
                        aria-label="Refresh agent"
                        onClick={() => detail(selected)}
                      >
                        <RefreshCw size={15} />
                      </button>
                    )}
                  </div>
                  {!selected.registry_id ? (
                    <>
                    <p className="field-help">{config.gas_sponsorship_enabled ? "Sign a registration permission. Tab covers its BNB gas fee." : `${config.gas_sponsorship_message || "Registration sponsorship is currently unavailable."} Your setup is saved.`}</p>
                    <label className="checkbox-row"><input type="checkbox" checked={selfPay} disabled={pending || !!registration} onChange={event => setSelfPay(event.target.checked)} />pay registration BNB gas from my wallet</label>
                    <button
                      className="primary"
                      disabled={pending || config.contracts_status !== "live"}
                      onClick={() =>
                        withBusy(async () => {
                          await register(selected);
                        })
                      }
                    >
                      register agent
                      <ArrowUpRight size={14} />
                    </button>
                    </>
                  ) : (
                    <>
                      <div className="agent-action-row">
                        <button
                          className="primary"
                          disabled={pending || selected.status !== "ready"}
                          onClick={() => run(selected)}
                        >
                          <Play size={14} />
                          run now
                        </button>
                        <button
                          className="outline"
                          disabled={pending}
                          onClick={() => pause(selected)}
                        >
                          {selected.status === "paused" ? (
                            <Play size={14} />
                          ) : (
                            <Pause size={14} />
                          )}{" "}
                          {selected.status === "paused" ? "resume scheduled runs" : "pause scheduled runs"}
                        </button>
                        {selected.registration_tx && <a
                          className="outline"
                          href={explorer(selected.registration_tx, "tx", chainId)}
                          target="_blank"
                          rel="noreferrer"
                        >
                          registration
                          <ArrowUpRight size={13} />
                        </a>}
                      </div>
                      <p className="field-help">Pausing stops this agent's scheduled and manual runs in Tab. Existing onchain jobs and spending sessions keep their permissions until changed with a separate wallet signature.</p>
                      <div className="agent-funding">
                        <div>
                          <span className="eyebrow">connected wallet</span>
                          <strong>{balance?.usdt ?? "…"} USDT</strong>
                          <small>{balance?.bnb ?? "…"} BNB for fees</small>
                        </div>
                        <div>
                          <h3>add USDT</h3>
                          <p>
                            Send USDT on BNB Smart Chain to your connected
                            wallet. Agents using this wallet share its balance.
                          </p>
                          <button
                            className="wallet-copy"
                            onClick={() => copy(selected.wallet!)}
                          >
                            <span>{selected.wallet}</span>
                            <Copy size={14} />
                          </button>
                          {copied && <small>copied</small>}
                        </div>
                      </div>
                      <div className="agent-config-summary">
                        <span>{selected.daily_cap} USDT / day</span>
                        <span>{String(selected.max_call)} USDT / call</span>
                        <span>
                          {selected.cadence === "manual"
                            ? "manual runs"
                            : selected.cadence === "hourly"
                              ? "every hour"
                              : "once a day"}
                        </span>
                      </div>
                      <div className="agent-results">
                        <h3>latest run</h3>
                        {runs.length ? (
                          <RunReceipt run={runs[0]} agent={selected}/>
                        ) : (
                          <p className="muted">no runs yet</p>
                        )}
                      </div>
                      {selected.registry_id && selected.wallet && <>
                        <AgentPayments key={`payments:${selected.id}`} agent={selected} api={api} live={config.contracts_status === "live"} send={sendTransactions} sign={async (quote, merchant) => signPaymentQuote(await ownerWallet(selected.wallet!),quote,merchant,selected.wallet!,String(selected.max_call),String(selected.daily_cap))} changed={()=>refreshSelected(selected.id)} />
                        <details className="account-disclosure" id="account-finance"><summary>pooled lending, job advances and stock loans</summary><AgentFinance key={`finance:${selected.id}`} agent={selected} api={api} send={sendTransactions} changed={()=>refreshSelected(selected.id)} /></details>
                        <details className="account-disclosure" id="account-credit"><summary>secured USDT credit</summary><AgentCredit initialTarget={targetAgent} key={`credit:${selected.id}`} agent={selected} owned={agents} api={api} live={config.contracts_status === "live"} send={sendTransactions} changed={()=>refreshSelected(selected.id)} /></details>
                        <details className="account-disclosure" id="account-backing"><summary>backing, tokens and wallet actions</summary><AgentWalletActions initialTarget={targetAgent} initialAsset={initialAsset === "native" ? undefined : initialAsset} initialAction={accountSection === "backing" ? initialAsset === "native" ? "back_bnb" : "back" : undefined} key={`wallet:${selected.id}`} agent={selected} owned={agents} api={api} live={config.contracts_status === "live"} send={sendTransactions} changed={()=>refreshSelected(selected.id)} /></details>
                      </>}
                      {selected.registry_id && selected.wallet && <div id="account-jobs"><AgentJobs agent={selected} owned={agents} api={api} sendTransaction={async (intent) => {
                        if (intent.chain_id !== BNB_CHAIN_ID) throw Error("The transaction network differs from the app configuration.");
                        return sendTransactions(intent.sender, intent.transactions?.length ? intent.transactions : [{ to: intent.to, data: intent.data, value: intent.value, chainId: intent.chain_id }]);
                      }} /></div>}
                      <AgentAccessKey key={selected.id} agentId={selected.id} api={api} disabled={pending} />
                      <div className="personal-events">
                        <h3>activity</h3>
                        {events.slice(0, 20).map((e) => (
                          <div key={e.id}>
                            <time>
                              {new Date(e.timestamp).toLocaleTimeString()}
                            </time>
                            <span>{e.message}</span>
                            <small>{e.status}</small>
                          </div>
                        ))}
                      </div>
                    </>
                  )}
                </section>
              )}
            </>
          )}
        </>
      )}
    </div>
  );
}
export default function Account({
  config,
}: {
  providers: Provider[];
  config: Config;
}) {
  if (!config.app_id)
    return (
      <div className="account-flow panel empty">
        sign-in is temporarily unavailable.
      </div>
    );
  return (
    <PrivyProvider
      appId={config.app_id}
      config={{
        appearance: {
          theme: "dark",
          accentColor: "#a9d5ed",
          logo: "/mark.svg",
          walletChainType: "ethereum-only",
          walletList: ["metamask", "binance", "wallet_connect", "detected_ethereum_wallets"],
        },
        loginMethods: ["email", "wallet"],
        embeddedWallets: {
          ethereum: { createOnLogin: "users-without-wallets" },
        },
        defaultChain: bsc,
        supportedChains: [bsc],
      }}
    >
      <AgentAccount config={config} />
    </PrivyProvider>
  );
}
