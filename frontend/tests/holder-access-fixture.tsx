import { useState } from "react";
import { createRoot } from "react-dom/client";
import type { ConnectedWallet } from "@privy-io/react-auth";
import { HolderAccess } from "../src/components/HolderAccess";
import { AgentAccessKey } from "../src/components/AgentAccessKey";
import { AgentPayments } from "../src/components/AgentPayments";
import { HolderAccessContext, useHolderAccess } from "../src/lib/holder-access";
import { request, type RuntimeAgent } from "../src/lib/api";
import "@fontsource/dm-sans/400.css";
import "@fontsource/dm-sans/500.css";
import "../src/styles.css";
const addresses = ["0x1111111111111111111111111111111111111111", "0x2222222222222222222222222222222222222222"];
function Fixture() {
  const [wallet, setWallet] = useState(addresses[0]);
  const [notice, setNotice] = useState("");
  const access = useHolderAccess(request, "did:privy:holder-qa", wallet);
  const connected = { address: wallet, switchChain: async () => {}, getEthereumProvider: async () => ({ request: async ({ method, params }: { method: string; params?: unknown[] }) => {
    if (method === "eth_chainId") return "0x38";
    if (method === "eth_accounts") return [wallet];
    if (method === "personal_sign") { if (sessionStorage.getItem("qa:reject")) throw Object.assign(Error("Wallet proof rejected."), { code: 4001 }); document.body.dataset.signed = JSON.stringify(params); return "0x" + "a".repeat(130); }
    if (method === "eth_sendTransaction") document.body.dataset.sent = "true";
    throw Error("Unexpected wallet request " + method);
  } }) } as unknown as ConnectedWallet;
  async function act(action: string) { try { await access.require(action); setNotice(action + " allowed"); } catch (cause) { setNotice((cause as Error).message); } }
  const agent = { id: "holder-agent", name: "wren", wallet, registry_id: "0x" + "1".repeat(64), daily_cap: "5", max_call: "1", status: "ready", tools: ["x402"] } as RuntimeAgent;
  return <HolderAccessContext.Provider value={access}><main className="account-flow"><h1>your agents.</h1><p>local access QA · no real funds</p><label>wallet<select aria-label="selected wallet" value={wallet} onChange={event => { setWallet(event.target.value); setNotice(""); }}><option value={addresses[0]}>wallet one</option><option value={addresses[1]}>wallet two</option></select></label><HolderAccess access={access} connect={async () => connected} />
    <div className="agent-action-row"><button className="primary" disabled={!access.allows("create_agent")} onClick={() => void act("create_agent")}>create agent</button><button className="outline" onClick={() => void act("run")}>review new action</button><button className="outline" disabled={!access.allows("repay_credit")} onClick={() => void act("repay_credit")}>repay existing credit</button><button className="outline" disabled={!access.allows("withdraw_backing")} onClick={() => void act("withdraw_backing")}>withdraw existing backing</button></div><p role="status">{notice}</p>
    <AgentAccessKey agentId={agent.id} api={request} /><AgentPayments agent={agent} api={request} live send={async () => { throw Error("QA cannot send transactions."); }} sign={async () => { throw Error("QA cannot sign payments."); }} changed={async () => {}} /><a href="/garden">browse the public garden</a>
  </main></HolderAccessContext.Provider>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);
