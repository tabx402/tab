import { useState } from "react";
import type { ConnectedWallet } from "@privy-io/react-auth";
import { formatUnits } from "viem";
import { RefreshCw } from "lucide-react";
import { explorer, shortId } from "../lib/evm";
import type { HolderAccessController } from "../lib/holder-access";

export function HolderAccess({ access, connect }: { access: HolderAccessController; connect: () => Promise<ConnectedWallet> }) {
  const [error, setError] = useState("");
  const status = access.status;
  const verified = status?.enforced && status.eligible;
  async function verify() { setError(""); try { await access.verify(await connect()); } catch (cause) { setError((cause as Error).message); } }
  return <section className="panel" aria-label="TAB holder access">
    <div className="panel-heading"><div><h3>TAB holder access</h3><p className="field-help">{access.busy ? "Checking this wallet’s access…" : status?.message || "Holder access could not be checked. Try again before starting a new action."}</p></div><button className="icon-button" aria-label="Refresh TAB holder access" disabled={access.busy} onClick={() => { setError(""); void access.refresh().catch(() => {}); }}><RefreshCw size={15} /></button></div>
    {access.wallet && <p className="field-help">wallet {shortId(access.wallet)}{verified && status.balance_units !== null && status.decimals !== null ? ` · ${formatUnits(BigInt(status.balance_units), status.decimals)} TAB` : ""}</p>}
    {status?.enforced && <p className="field-help">A positive TAB balance unlocks new app actions. Repayment, withdrawals and pending transaction recovery stay available.</p>}
    {status?.token_address && <a className="text-link" href={explorer(status.token_address, "address")} target="_blank" rel="noreferrer">view TAB contract ↗</a>}
    {status?.enforced && status.status === "wallet_unverified" && <button className="outline" disabled={access.busy} onClick={() => void verify()}>verify wallet access</button>}
    {(error || access.error) && <p className="form-error" role="alert">{error || access.error}</p>}
  </section>;
}
