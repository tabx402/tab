import { useState } from "react";
import { Copy } from "lucide-react";
import type { AccountAPI } from "../lib/jobs";

export function AgentAccessKey({ agentId, api, disabled = false }: { agentId: string; api: AccountAPI; disabled?: boolean }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [error, setError] = useState("");
  async function change(action: "generate" | "revoke") {
    setBusy(true); setFeedback(""); setError("");
    try {
      if (action === "revoke") {
        await api<void>(`/account/runtime/${encodeURIComponent(agentId)}/key`, { method: "DELETE" });
        setKey("");
        setFeedback("access key revoked. it can no longer run this agent.");
      } else {
        const result = await api<{ key: string }>(`/account/runtime/${encodeURIComponent(agentId)}/key`, { method: "POST" });
        if (typeof result.key !== "string" || !result.key) throw Error("The API did not return an access key.");
        setKey(result.key);
        setFeedback("new access key ready. copy it now.");
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Access key update failed.");
    } finally {
      setBusy(false);
    }
  }
  async function copyKey() {
    try {
      await navigator.clipboard.writeText(key);
      setFeedback("access key copied.");
    } catch {
      setError("Clipboard access is unavailable. Try again with clipboard permission enabled.");
    }
  }
  return <details className="builder-tools">
    <summary>connect your own agent</summary>
    <p>Generate an access key for this agent. It can run its configured tools and cannot sign wallet transactions.</p>
    <button className="outline" disabled={disabled || busy} onClick={() => void change("generate")}>generate access key</button>{" "}
    <button className="outline" disabled={disabled || busy} onClick={() => void change("revoke")}>revoke access key</button>
    {feedback && <p className="field-help" role="status">{feedback}</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
    {key && <>
      <button className="wallet-copy" disabled={busy || disabled} onClick={() => void copyKey()}>copy key <Copy size={14} /></button>
      <p className="field-help">Shown once. Generating another key replaces this one.</p>
      <pre className="run-output">{"POST https://tabagents.io/api/agent/run\nAuthorization: Bearer <your-agent-key>"}</pre>
    </>}
  </details>;
}
