import type { AgentPlanInput, Provider } from "../lib/api";
import { DailyCapField } from "./DailyCapField";

export function AgentPolicyFields({ providers, value, onChange, preview = false, active = "" }: {
  providers: Provider[];
  value: AgentPlanInput;
  onChange?: (value: Partial<AgentPlanInput>) => void;
  preview?: boolean;
  active?: string;
}) {
  return <div className="policy-fields">
    <label className={active === "name" ? "field-active" : ""}>
      agent name
      <input aria-label={`${preview ? "Example " : ""}agent name`} required minLength={2} maxLength={48} placeholder="e.g. wren"
        value={value.name} readOnly={preview} tabIndex={preview ? -1 : undefined} onChange={(e) => onChange?.({ name: e.target.value })} />
    </label>
    <label className={active === "purpose" ? "field-active" : ""}>
      what does it do?
      <textarea aria-label={`${preview ? "Example " : ""}agent purpose`} required minLength={5} maxLength={240} placeholder="a research agent that tracks onchain activity…"
        value={value.purpose} readOnly={preview} tabIndex={preview ? -1 : undefined} onChange={(e) => onChange?.({ purpose: e.target.value })} />
    </label>
    <DailyCapField label="daily spending cap · USDT" value={value.daily_cap} preview={preview} active={active === "cap"} onChange={daily_cap => onChange?.({ daily_cap })} />
    <fieldset className={active === "providers" ? "field-active" : ""}>
      <legend>provider allowlist</legend>
      <div className="policy-provider-grid">
        {providers.map((p) => <label className="checkbox-row" key={p.id}>
          <input type="checkbox" disabled={preview} checked={value.providers.includes(p.id)}
            onChange={(e) => onChange?.({ providers: e.target.checked ? [...value.providers, p.id] : value.providers.filter((id) => id !== p.id) })} />
          <span>{p.name}</span><small>{p.category}</small>
        </label>)}
      </div>
    </fieldset>
  </div>;
}
