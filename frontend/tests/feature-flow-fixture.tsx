import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import "@fontsource/dm-sans/400.css";
import "@fontsource/dm-sans/500.css";
import "@fontsource/ibm-plex-mono/400.css";
import { AgentWalletPreview } from "../src/components/AgentWalletPreview";
import { useState } from "react";
import { OnboardingCosts } from "../src/components/OnboardingCosts";
import { RunReceipt } from "../src/components/RunReceipt";
import { Tools } from "../src/components/Tools";
import { Finance } from "../src/components/Finance";
import type { RuntimeAgent } from "../src/lib/api";
import "../src/styles.css";
import "../src/components/action-colors.css";
const agent = { purpose: "Check the wallet and summarize changes." } as RuntimeAgent;
const partial = { id: "local-partial", agent_id: "qa", status: "partial", started_at: "2026-10-08T12:00:00Z", output: { research: { status: "completed", cost_usd: "0.003", sources: [] }, chain: { wallet: "0x1111111111111111111111111111111111111111", bnb: "0.5", usdt: "0.000000000000000001", block: 123456 }, openrouter: { status: "completed", summary: "The balance is unchanged.", cost_usd: "0.002", settled_usdt: false }, x402: { status: "requires_authorization", message: "Review a market-data request.", settled_usdt: false, amount: "2", tx_hash: "0x" + "a".repeat(64) } } };
const providers = [{ id: "bnb-rpc", name: "BNB chain data", category: "data", status: "live", description: "Public wallet balances and blocks.", website: "https://www.bnbchain.org" }, { id: "openrouter", name: "OpenRouter", category: "inference", status: "not_connected", description: "Summarize a tool result.", website: "https://openrouter.ai" }];
function WalletFixture(){const [address,setAddress]=useState("");return <><label>wallet address<input value={address} onChange={e=>setAddress(e.target.value)}/></label><AgentWalletPreview address={address}/></>;}
createRoot(document.getElementById("root")!).render(<BrowserRouter><main><p>local QA fixture · no wallet transactions</p><WalletFixture /><OnboardingCosts /><div style={{ margin: "30px 0" }}><RunReceipt run={partial} agent={agent} /></div><Tools providers={providers} /><Finance /></main></BrowserRouter>);
