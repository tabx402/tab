import { useState } from "react";
import { createRoot } from "react-dom/client";
import type { ConnectedWallet } from "@privy-io/react-auth";
import "@fontsource/dm-sans/400.css";
import "@fontsource/dm-sans/500.css";
import "../src/styles.css";
import "../src/components/action-colors.css";
import { request, type RuntimeAgent } from "../src/lib/api";
import { AgentWizard } from "../src/components/AgentWizard";
import { RegistrationRecovery } from "../src/components/RegistrationRecovery";
import { startSponsoredRegistration, sponsorStatus, sponsoredResult, storedRegistration, rememberRegistration, type PendingRegistration } from "../src/lib/registration";
const owner = "0x1111111111111111111111111111111111111111", storageKey = "tab:registration:pending:qa";
const wallet = { address: owner, switchChain: async () => {}, getEthereumProvider: async () => ({ request: async ({method,params}: {method:string;params?:unknown[]}) => {
  if (method === "eth_chainId") return "0x38";
  if (method === "eth_accounts") return [owner];
  if (method === "eth_getBalance") return "0x0";
  if (method === "eth_signTypedData_v4") { document.body.dataset.signed = String(params?.[1]); if (localStorage.getItem("qa:registration:reject")) throw Object.assign(Error("Wallet request rejected."), {code:4001}); return "0x"+"a".repeat(130); }
  if (method === "eth_sendTransaction") { document.body.dataset.sent = "unexpected"; throw Error("Zero-BNB registration must never send from this wallet."); }
  throw Error(`Unexpected wallet method ${method}`);
} }) } as unknown as ConnectedWallet;
function Fixture() {
  const [pending,setPending]=useState(false),[record,setRecord]=useState<PendingRegistration|null>(()=>storedRegistration(storageKey)),[error,setError]=useState(""),[done,setDone]=useState<RuntimeAgent|null>(null),[selfPay,setSelfPay]=useState(false);
  function save(value:PendingRegistration|null) { rememberRegistration(storageKey,value);setRecord(value); }
  async function check() { if (!record)return;setPending(true);setError("");try { const result=await sponsorStatus(request,record.id);const agent=sponsoredResult(result,record.id,record.wallet,save);if(agent)setDone(agent);else if(result.retryable)setError(result.message||"Review a fresh registration."); }catch(e){setError((e as Error).message);}finally{setPending(false);} }
  return <main className="account-flow"><h1>registration QA · no funds</h1>{record&&<RegistrationRecovery registration={record} busy={pending} onChange={save} onCheck={()=>void check()} onFailed={()=>{}}/>}{done?<p role="status">registered {done.name}</p>:<AgentWizard pending={pending} sponsorshipAvailable={!new URLSearchParams(location.search).has("unavailable")} sponsorshipMessage={new URLSearchParams(location.search).has("unavailable")?"The registration sponsor needs a BNB top-up.":undefined} selfPay={selfPay} onSelfPayChange={setSelfPay} onCancel={()=>{}} onCreate={async input=>{setPending(true);try{if(record)throw Error("Check the pending request before signing again.");if(selfPay)throw Error("Explicit wallet-paid path selected; QA does not send funds.");const agent=await request<RuntimeAgent>("/account/runtime",{method:"POST",body:JSON.stringify(input)});const result=await startSponsoredRegistration(request,agent,wallet,!new URLSearchParams(location.search).has("unavailable"),save);if(result.agent)setDone(result.agent);return result.agent||agent;}finally{setPending(false);}}}/>}{error&&<p role="alert">{error}</p>}</main>;
}
createRoot(document.getElementById("root")!).render(<Fixture/>);
