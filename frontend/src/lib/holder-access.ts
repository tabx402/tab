import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import type { ConnectedWallet } from "@privy-io/react-auth";
import { isAddress } from "viem";
import type { AccountAPI } from "./jobs";
import { BNB_CHAIN_ID, bnbWalletClient, sameAddress } from "./evm";

export type HolderStatus = {
  status: "disabled" | "eligible" | "not_holder" | "wallet_required" | "wallet_unverified" | "not_configured" | "unavailable";
  enforced: boolean; eligible: boolean; chain_id: number; token_address: string | null;
  wallet: string | null; balance_units: string | null; minimum_units: string;
  decimals: number | null; checked_at: string | null; message: string; recovery_allowed: boolean;
};
export type HolderChallenge = { id: string; wallet: string; message: string; expires_at: string; chain_id: number };
export type HolderGate = {
  allows: (action: string, details?: Record<string, unknown>) => boolean;
  require: (action: string, details?: Record<string, unknown>, wallet?: string) => Promise<void>;
};
export type HolderAccessController = HolderGate & {
  status: HolderStatus | null; wallet: string | null; busy: boolean; error: string;
  refresh: () => Promise<HolderStatus>; verify: (wallet: ConnectedWallet) => Promise<void>;
};

export function holderCreationOptions(path: string, options: RequestInit, wallet: string | null): RequestInit {
  if (options.method?.toUpperCase() !== "POST" || !["/account/runtime", "/account/agents", "/account/bounties"].includes(path)) return options;
  const headers = new Headers(options.headers);
  headers.delete("X-Tab-Holder-Wallet");
  if (wallet && isAddress(wallet, { strict: false })) headers.set("X-Tab-Holder-Wallet", wallet.toLowerCase());
  // The shared client merges a plain header record with its auth headers.
  return { ...options, headers: Object.fromEntries(headers.entries()) };
}

// These paths reduce or reconcile existing obligations. Unknown actions stay gated.
const recovery = new Set([
  "reconcile", "revoke_key", "pause_runs", "cancel_draft",
  "withdraw_bnb", "withdraw_backing", "withdraw_spending", "unstake", "settle_bond",
  "resolve_outcome", "claim_outcome", "revoke_session", "repay_credit", "withdraw_credit", "close_credit",
  "withdraw_collateral", "pledge_collateral", "pool_redeem", "stock_redeem", "advance_repay", "advance_close",
  "advance_pledge", "advance_withdraw_collateral", "advance_liquidate",
  "stock_repay", "stock_withdraw", "stock_add_collateral", "stock_liquidate", "liquidate_credit",
  "job_pause", "job_cancel", "job_submit", "job_accept", "job_reject", "job_close_branch", "job_evidence",
]);
export function holderRecoveryAction(action: string, details: Record<string, unknown> = {}) {
  if (action === "pause_agent") return details.paused === true;
  return recovery.has(action.replace(/^finance_/, ""));
}
export function holderPermitted(status: HolderStatus | null) {
  return status !== null && ((!status.enforced && status.status === "disabled") || (status.enforced && status.eligible && status.status === "eligible"));
}
export function validateHolderStatus(status: HolderStatus, wallet: string | null): HolderStatus {
  if (!status || status.chain_id !== BNB_CHAIN_ID || typeof status.enforced !== "boolean" || typeof status.eligible !== "boolean" || !["disabled", "eligible", "not_holder", "wallet_required", "wallet_unverified", "not_configured", "unavailable"].includes(status.status)) throw Error("Holder access status is unavailable for this network.");
  if (status.wallet && (!wallet || !sameAddress(status.wallet, wallet))) throw Error("Holder access belongs to another wallet. Check the selected wallet again.");
  if (!status.enforced && (status.status !== "disabled" || status.eligible)) throw Error("The holder access policy could not be verified.");
  if (status.enforced && status.status === "disabled") throw Error("The holder access policy could not be verified.");
  if (status.eligible) {
    if (status.status !== "eligible" || !wallet || !sameAddress(status.wallet, wallet) || !isAddress(status.token_address || "", { strict: false }) || !/^\d+$/.test(status.balance_units || "") || !/^\d+$/.test(status.minimum_units) || BigInt(status.minimum_units) < 1n || BigInt(status.balance_units!) < BigInt(status.minimum_units) || status.decimals === null || !Number.isInteger(status.decimals) || status.decimals < 0 || status.decimals > 255 || !status.checked_at || !Number.isFinite(Date.parse(status.checked_at))) throw Error("The TAB balance has not been verified for this wallet.");
  } else if (status.status === "eligible") throw Error("The TAB balance has not been verified for this wallet.");
  return status;
}
export function validateHolderChallenge(challenge: HolderChallenge, account: string, wallet: string) {
  if (!account || /[\r\n]/.test(account) || !isAddress(wallet, { strict: false }) || !challenge || challenge.chain_id !== BNB_CHAIN_ID || !sameAddress(challenge.wallet, wallet) || !/^[a-zA-Z0-9-]+$/.test(challenge.id)) throw Error("The wallet access proof does not match this account or network.");
  const expires = Date.parse(challenge.expires_at);
  if (!Number.isFinite(expires) || expires <= Date.now() || expires > Date.now() + 6 * 60000) throw Error("The wallet access proof expired. Check access again.");
  const expected = `tabagents.io\nBNB Smart Chain mainnet 56\nVerify TAB holder access\nAccount: ${account}\nWallet: ${wallet.toLowerCase()}\nNonce: ${challenge.id}\nExpires: ${challenge.expires_at}\nThis signature proves wallet control for TAB holder access. It does not authorize a transaction.`;
  if (challenge.message !== expected) throw Error("The wallet access message differs from the expected proof. Nothing was signed.");
  return challenge.message;
}

const missingGate: HolderGate = {
  allows: holderRecoveryAction,
  require: async (action, details) => { if (!holderRecoveryAction(action, details)) throw Error("Check TAB holder access before starting a new action."); },
};
export const HolderAccessContext = createContext<HolderGate>(missingGate);
export const useHolderGate = () => useContext(HolderAccessContext);

export function useHolderAccess(api: AccountAPI, account: string | null, wallet: string | null): HolderAccessController {
  const key = `${account || ""}:${wallet?.toLowerCase() || ""}`;
  const current = useRef(key); current.current = key;
  const serial = useRef(0);
  const [snapshot, setSnapshot] = useState<{ key: string; status: HolderStatus | null; error: string; busy: boolean }>({ key, status: null, error: "", busy: true });
  const status = snapshot.key === key ? snapshot.status : null;
  const refresh = useCallback(async () => {
    if (current.current !== key) throw Error("The selected wallet changed. Check its access again.");
    const request = ++serial.current;
    setSnapshot(previous => ({ key, status: previous.key === key ? previous.status : null, error: "", busy: true }));
    try {
      if (!account) throw Error("Sign in to check TAB holder access.");
      const value = validateHolderStatus(await api<HolderStatus>(`/account/holder-access${wallet ? `?wallet=${encodeURIComponent(wallet)}` : ""}`), wallet);
      if (current.current !== key) throw Error("The selected wallet changed. Check its access again.");
      if (serial.current !== request) throw Error("A newer access check started. Try the action again.");
      if (serial.current === request) setSnapshot({ key, status: value, error: "", busy: false });
      return value;
    } catch (cause) {
      if (current.current === key && serial.current === request) setSnapshot({ key, status: null, error: (cause as Error).message, busy: false });
      throw cause;
    }
  }, [api, account, wallet, key]);
  useEffect(() => { if (account) void refresh().catch(() => {}); }, [account, refresh]);
  const require = useCallback(async (action: string, details?: Record<string, unknown>, target?: string) => {
    if (holderRecoveryAction(action, details)) return;
    const value = target && !sameAddress(target, wallet)
      ? validateHolderStatus(await api<HolderStatus>(`/account/holder-access?wallet=${encodeURIComponent(target)}`), target)
      : await refresh();
    if (current.current !== key) throw Error("The selected wallet changed. Review the action again.");
    if (!holderPermitted(value)) throw Error(value.message || "A verified TAB holding is required for new app actions.");
  }, [api, key, wallet, refresh]);
  const verify = useCallback(async (connected: ConnectedWallet) => {
    if (!account || !wallet || !sameAddress(connected.address, wallet)) throw Error("Connect the selected wallet to verify TAB access.");
    // A previous verification may have succeeded despite a lost HTTP response.
    const before = await refresh();
    if (holderPermitted(before)) return;
    if (before.status !== "wallet_unverified") throw Error(before.message || "Wallet verification is currently unavailable.");
    setSnapshot(previous => ({ ...previous, busy: true, error: "" }));
    try {
      const challenge = await api<HolderChallenge>("/account/holder-access/challenge", { method: "POST", body: JSON.stringify({ wallet }) });
      const message = validateHolderChallenge(challenge, account, wallet);
      const client = await bnbWalletClient(connected, wallet);
      if (current.current !== key) throw Error("The selected wallet changed. Nothing was signed.");
      const signature = await client.signMessage({ message });
      if (current.current !== key) throw Error("The selected wallet changed. Check its access again.");
      validateHolderStatus(await api<HolderStatus>("/account/holder-access/verify", { method: "POST", body: JSON.stringify({ id: challenge.id, signature }) }), wallet);
      await refresh();
    } catch (cause) {
      if (current.current === key) setSnapshot(previous => ({ ...previous, busy: false, error: (cause as Error).message }));
      throw cause;
    }
  }, [account, api, key, refresh, wallet]);
  return { status, wallet, busy: snapshot.key !== key || snapshot.busy, error: snapshot.key === key ? snapshot.error : "", refresh, verify,
    allows: (action, details) => holderRecoveryAction(action, details) || holderPermitted(status), require };
}
