import { useEffect, useId, useRef, useState } from "react";
import { usdtUnits, USDT_SCALE } from "../lib/amounts";

export function DailyCapField({ value, onChange, label = "daily limit · USDT", active = false }: {
  value: string;
  onChange?: (value: string) => void;
  label?: string;
  active?: boolean;
}) {
  const id = useId();
  const input = useRef<HTMLInputElement>(null);
  const [raw, setRaw] = useState(String(value));
  const [custom, setCustom] = useState(!["1", "5", "25"].includes(value));
  useEffect(() => {
    setRaw(value);
  }, [value]);
  return <div className={`daily-cap-field ${active ? "field-active" : ""}`}>
    <label htmlFor={id}>{label}</label>
    <div className="cap-suggestions" role="group" aria-label="Suggested daily USDT caps">
      {["1", "5", "25"].map(cap => <button key={cap} type="button" aria-pressed={!custom && value === cap} onClick={() => { setCustom(false); setRaw(cap); input.current?.setCustomValidity(""); onChange?.(cap); }}>{cap} USDT</button>)}
      <button type="button" aria-pressed={custom} onClick={() => { setCustom(true); input.current?.focus(); input.current?.select(); }}>custom</button>
    </div>
    <input id={id} ref={input} aria-label={label} required type="text" inputMode="decimal" pattern="[0-9]+(\.[0-9]{1,18})?" value={raw} onChange={event => {
      setCustom(true); setRaw(event.target.value);
      const units = usdtUnits(event.target.value);
      event.target.setCustomValidity(units === null || units < USDT_SCALE || units > 10000n * USDT_SCALE ? "Enter 1–10,000 USDT with up to 18 decimal places." : "");
      onChange?.(event.target.value);
    }} />
    <small className="field-help">1–10,000 USDT per day · up to 18 decimal places</small>
  </div>;
}
