import type { PendingRegistration } from "../lib/registration";
import { explorer } from "../lib/evm";

export function RegistrationRecovery({ registration, busy, onChange, onCheck, onFailed }: {
  registration: PendingRegistration; busy: boolean;
  onChange: (value: PendingRegistration) => void; onCheck: () => void; onFailed: () => void;
}) {
  const sponsored = registration.mode === "sponsored";
  return <div className="panel registration-pending">
    <p>{sponsored ? registration.message || "Checking the sponsor's record for this registration. Check the existing request before signing another permission." : "Check this registration before signing another. If the wallet response was lost, find its transaction in your wallet activity."}</p>
    {!sponsored && <label>registration transaction hash<input value={registration.tx_hash} onChange={event => onChange({ ...registration, tx_hash: event.target.value })} placeholder="0x…" spellCheck={false} /></label>}
    {registration.tx_hash && <a href={explorer(registration.tx_hash)} target="_blank" rel="noreferrer">view transaction</a>}
    <button className="outline" disabled={busy || (!sponsored && !/^0x[0-9a-fA-F]{64}$/.test(registration.tx_hash))} onClick={onCheck}>{sponsored ? "check registration" : "check confirmation"}</button>
    {!sponsored && <button className="outline" disabled={busy || !/^0x[0-9a-fA-F]{64}$/.test(registration.tx_hash)} onClick={onFailed}>check failed registration</button>}
  </div>;
}
