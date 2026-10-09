import { useEffect, useRef, useState } from "react";
import { isAddress } from "viem";
import { request } from "../lib/api";
import type { components } from "../lib/api-schema";
import { formatAmount, usdtUnits } from "../lib/amounts";

type Snapshot = components["schemas"]["StarterObservation"];

export function AgentWalletPreview({ address }: { address?: string | null }) {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const active = useRef<AbortController | null>(null);
  useEffect(() => {
    active.current?.abort(); active.current = null;
    setSnapshot(null); setError(""); setPending(false);
    return () => { active.current?.abort(); active.current = null; };
  }, [address]);

  async function refresh() {
    if (!address || !isAddress(address, { strict: false })) return;
    active.current?.abort();
    const controller = new AbortController(); active.current = controller;
    setPending(true); setError(""); setSnapshot(null);
    const timeout = window.setTimeout(() => controller.abort(), 20000);
    try {
      const data = await request<Snapshot>(`/starter/wallet?address=${encodeURIComponent(address)}`, { signal: controller.signal, cache: "no-store" });
      const validBlock = typeof data.block === "string" && /^\d{1,20}$/.test(data.block) && BigInt(data.block) <= 2n ** 64n - 1n;
      if (data.chain_id !== 56 || data.source !== "public_rpc" || typeof data.wallet !== "string" || data.wallet.toLowerCase() !== address.toLowerCase() || data.settled_usdt !== false || typeof data.bnb !== "string" || typeof data.usdt !== "string" || usdtUnits(data.bnb) === null || usdtUnits(data.usdt) === null || !validBlock || typeof data.observed_at !== "string" || !Number.isFinite(Date.parse(data.observed_at))) {
        throw Error("The wallet snapshot could not be verified. Try again.");
      }
      if (active.current === controller) setSnapshot(data);
    } catch (reason) {
      if (active.current === controller) setError(controller.signal.aborted ? "The balance read timed out. Try again." : (reason as Error).message);
    } finally {
      window.clearTimeout(timeout);
      if (active.current === controller) { active.current = null; setPending(false); }
    }
  }

  if (!address || !isAddress(address, { strict: false })) return null;
  return <div className="paid-tool agent-wallet-preview" data-testid="agent-wallet-preview">
    <button type="button" className="text-link" disabled={pending} onClick={() => void refresh()}>{pending ? "reading balances…" : "read wallet balances"}</button>
    {snapshot && <div aria-live="polite"><p>{formatAmount(snapshot.bnb)} BNB · {formatAmount(snapshot.usdt)} USDT</p><small className="field-help">BNB mainnet · block {BigInt(snapshot.block).toLocaleString("en-US")} · {new Date(snapshot.observed_at).toLocaleTimeString()} · no funds spent</small></div>}
    {error && <p className="form-error" role="alert">{error}</p>}
  </div>;
}
