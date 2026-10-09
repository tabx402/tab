import { FixtureHolderAccess } from "./holder-access-fixture-policy";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import "@fontsource/dm-sans/400.css";
import "@fontsource/dm-sans/500.css";
import "@fontsource/ibm-plex-mono/400.css";
import { AgentFinance } from "../src/components/AgentFinance";
import { request, type RuntimeAgent, type WalletActionIntent } from "../src/lib/api";
import { validateFinanceIntent, type FinanceData, type FinanceInput } from "../src/lib/finance";
import "../src/styles.css";
import "../src/components/action-colors.css";
const agent = { id: "11111111111111111111111111111111", name: "QA agent", purpose: "local finance fixture", wallet: "0x1111111111111111111111111111111111111111", registry_id: "0x" + "1".repeat(64), status: "ready", daily_cap: "5", max_call: "1", tools: ["x402", "bnb-rpc"], cadence: "manual" } as RuntimeAgent;
const send = async () => {
  document.body.dataset.sends = String(Number(document.body.dataset.sends || "0") + 1);
  if (document.body.dataset.walletMode === "rejected") throw Object.assign(Error("Wallet request rejected by user."), { code: 4001 });
  throw Error("Wallet response lost after submission.");
};
(window as Window & { checkFinancePayload?: (intent: WalletActionIntent, data: FinanceData, expected?: FinanceInput) => string }).checkFinancePayload = (intent, data, expected) => {
  try { validateFinanceIntent(intent, agent, data, expected); return "accepted"; } catch (reason) { return (reason as Error).message; }
};
createRoot(document.getElementById("root")!).render(<FixtureHolderAccess><BrowserRouter><main><section className="panel"><p>local finance QA · no real funds or transactions</p><AgentFinance agent={agent} api={request} send={send} changed={async () => {}} /></section></main></BrowserRouter></FixtureHolderAccess>);
