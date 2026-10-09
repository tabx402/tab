import type { ConnectedWallet } from "@privy-io/react-auth";
import { isAddress, type Hex } from "viem";
import type { RuntimeAgent, SponsorshipResult, SponsoredChallenge as ApiSponsoredChallenge } from "./api";
import type { AccountAPI } from "./jobs";
import { usdtUnits } from "./amounts";
import { BNB_CHAIN_ID, bnbWalletClient, sameAddress, type EvmTransaction } from "./evm";

export type DirectRegistration = { mode?: "direct"; id: string; wallet: string; signature: string; tx_hash: string; transaction: EvmTransaction };
export type SponsoredRegistration = { mode: "sponsored"; id: string; wallet: string; request_id: string; tx_hash: string; status: string; message?: string };
export type PendingRegistration = DirectRegistration | SponsoredRegistration;
export type SponsorResult = SponsorshipResult;
export type SponsoredChallenge = Omit<ApiSponsoredChallenge, "typed_data"> & {
  typed_data: {
    domain: { name: string; version: string; chainId: number; verifyingContract: string };
    primaryType: string; types: Record<string, { name: string; type: string }[]>;
    message: { id: string; owner: string; name: string; dailyCap: string; policyHash: string; nonce: string; deadline: string };
  };
};
const registerTypes = { Register: [
  { name: "id", type: "bytes32" }, { name: "owner", type: "address" }, { name: "name", type: "string" },
  { name: "dailyCap", type: "uint256" }, { name: "policyHash", type: "bytes32" },
  { name: "nonce", type: "uint256" }, { name: "deadline", type: "uint256" },
] } as const;
const domainTypes = [{ name: "name", type: "string" }, { name: "version", type: "string" }, { name: "chainId", type: "uint256" }, { name: "verifyingContract", type: "address" }];
export function storedRegistration(key: string): PendingRegistration | null {
  try { const value = JSON.parse(localStorage.getItem(key) || "null"); return value && typeof value.id === "string" && typeof value.tx_hash === "string" ? value as PendingRegistration : null; } catch { return null; }
}
export function rememberRegistration(key: string, value: PendingRegistration | null) {
  try { if (value) localStorage.setItem(key, JSON.stringify(value)); else localStorage.removeItem(key); } catch { /* Authenticated server status also retains sponsored submissions. */ }
}
export function validateSponsoredChallenge(challenge: SponsoredChallenge, agent: RuntimeAgent, owner: string) {
  const typed = challenge.typed_data, domain = typed?.domain, message = typed?.message;
  const localId = agent.id.replace(/^0x/, "");
  const expectedId = `${owner.toLowerCase()}${localId.slice(8)}`;
  if (!challenge.sponsored || !challenge.request_id || challenge.chain_id !== BNB_CHAIN_ID || !/^[0-9a-fA-F]{32}$/.test(localId) || !isAddress(owner, { strict: false })) throw Error("The sponsored registration request is invalid.");
  if (!domain || domain.name !== "Tab Protocol" || domain.version !== "1" || Number(domain.chainId) !== BNB_CHAIN_ID || !sameAddress(domain.verifyingContract, agent.registry_address) || !sameAddress(challenge.registry, agent.registry_address)) throw Error("The registration contract or network differs from this agent's setup.");
  if (typed.primaryType !== "Register" || JSON.stringify(typed.types.Register) !== JSON.stringify(registerTypes.Register) || Object.keys(typed.types).some(key => !["Register", "EIP712Domain"].includes(key)) || (typed.types.EIP712Domain && JSON.stringify(typed.types.EIP712Domain) !== JSON.stringify(domainTypes))) throw Error("The wallet request is not the expected registration permission.");
  if (!message || !sameAddress(challenge.wallet, owner) || !sameAddress(message.owner, owner) || message.id.toLowerCase() !== expectedId || challenge.registry_id.toLowerCase() !== expectedId || message.name !== agent.name || String(message.dailyCap) !== usdtUnits(agent.daily_cap)?.toString() || !/^0x[0-9a-fA-F]{64}$/.test(message.policyHash) || /^0x0{64}$/.test(message.policyHash) || message.policyHash.toLowerCase() !== challenge.policy_hash.toLowerCase()) throw Error("The registration permission differs from your wallet or agent setup.");
  const deadline = Number(message.deadline), expiry = Math.floor(Date.parse(challenge.expires_at) / 1000), now = Math.floor(Date.now() / 1000);
  if (!/^\d+$/.test(String(message.nonce)) || BigInt(message.nonce) >= 2n ** 256n || !Number.isSafeInteger(deadline) || deadline !== expiry || deadline <= now || deadline > now + 900) throw Error("The registration permission has expired or has an invalid nonce.");
  return { domain: { name: domain.name, version: domain.version, chainId: BNB_CHAIN_ID, verifyingContract: domain.verifyingContract as Hex }, types: registerTypes, primaryType: "Register" as const, message: { id: message.id as Hex, owner: owner as Hex, name: message.name, dailyCap: BigInt(message.dailyCap), policyHash: message.policyHash as Hex, nonce: BigInt(message.nonce), deadline: BigInt(message.deadline) } };
}
export function sponsoredResult(result: SponsorResult, id: string, wallet: string, pending: (value: PendingRegistration | null) => void): RuntimeAgent | null {
  if (!["not_submitted", "pending", "confirmed", "failed"].includes(result.status)) throw Error("Registration status is unavailable. Check again before starting another registration.");
  if (result.status === "confirmed") {
    if (!result.agent || result.agent.id !== id || !result.agent.registry_id || !result.agent.registration_tx || (wallet && !sameAddress(result.agent.wallet, wallet))) throw Error("The confirmed registration does not match this agent.");
    pending(null); return result.agent;
  }
  if ((result.status === "not_submitted" || result.status === "failed") && result.retryable === true) { pending(null); return null; }
  if (!result.request_id) throw Error("Registration recovery is missing its request reference. Check again before signing.");
  pending({ mode: "sponsored", id, wallet, request_id: result.request_id, tx_hash: result.tx_hash || "", status: result.status, message: result.message || undefined });
  return null;
}
export async function sponsorStatus(api: AccountAPI, id: string) {
  return api<SponsorResult>(`/account/runtime/${id}/sponsor/status`, { method: "POST" });
}
export async function startSponsoredRegistration(api: AccountAPI, agent: RuntimeAgent, wallet: ConnectedWallet, available: boolean, pending: (value: PendingRegistration | null) => void) {
  const previous = await sponsorStatus(api, agent.id);
  const completed = sponsoredResult(previous, agent.id, "", pending);
  if (completed || !(["not_submitted", "failed"].includes(previous.status) && previous.retryable === true)) return { result: previous, agent: completed };
  if (!available) {
    const current = await api<{status:string;message?:string}>("/sponsorship");
    if (current.status !== "ready") throw Error(`${current.message || "Registration sponsorship is unavailable right now."} Your setup is saved. Try again later, or choose to pay BNB gas from your wallet.`);
  }
  const challenge = await api<SponsoredChallenge>(`/account/runtime/${agent.id}/challenge`, { method: "POST", body: JSON.stringify({ wallet: wallet.address, sponsored: true }) });
  validateSponsoredChallenge(challenge, agent, wallet.address);
  const client = await bnbWalletClient(wallet, agent.wallet || wallet.address);
  const signature = await client.signTypedData(validateSponsoredChallenge(challenge, agent, wallet.address));
  // Persist recovery before handing the permission to the relayer. Never store the signature.
  pending({ mode: "sponsored", id: agent.id, wallet: wallet.address, request_id: challenge.request_id, tx_hash: "", status: "pending" });
  const result = await api<SponsorResult>(`/account/runtime/${agent.id}/sponsor`, { method: "POST", body: JSON.stringify({ wallet: wallet.address, signature, request_id: challenge.request_id }) });
  return { result, agent: sponsoredResult(result, agent.id, wallet.address, pending) };
}
