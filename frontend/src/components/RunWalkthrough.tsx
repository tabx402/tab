import { useState } from "react";
import { ArrowRight, Check, FileText, ShieldCheck, Sprout, Telescope } from "lucide-react";

const steps = [
  { label: "task", title: "start with one clear question.", detail: "Ask an agent to check a wallet’s BNB and USDT balances. A specific task keeps the output easy to review.", icon: Telescope },
  { label: "tools & limits", title: "give it just the access it needs.", detail: "This example uses a read-only chain connection. Paid tools need a separate approval or a spending permission with limits.", icon: ShieldCheck },
  { label: "result", title: "read what came back.", detail: "A chain check returns balances and the block used. Review the evidence before deciding what to do next.", icon: Sprout },
  { label: "receipt", title: "keep the work and cost together.", detail: "The record shows the tool, outcome and spending. Partial runs stay visible, and a wallet payment has its own transaction receipt.", icon: FileText },
];

export function RunWalkthrough() {
  const [step, setStep] = useState(0);
  const current = steps[step];
  const Icon = current.icon;
  return <div className="run-walkthrough">
    <div className="walkthrough-top"><span className="eyebrow">a run, from start to finish</span><span className="example-label">illustrative example</span></div>
    <div className="walkthrough-tabs" role="tablist" aria-label="Explore an example run">{steps.map((item, index) => <button key={item.label} role="tab" id={`walkthrough-tab-${index}`} aria-controls="walkthrough-panel" aria-selected={step === index} tabIndex={step === index ? 0 : -1} onClick={() => setStep(index)} onKeyDown={event => { let next = index; if (event.key === "ArrowRight") next = (index + 1) % steps.length; else if (event.key === "ArrowLeft") next = (index + steps.length - 1) % steps.length; else if (event.key === "Home") next = 0; else if (event.key === "End") next = steps.length - 1; else return; event.preventDefault(); setStep(next); document.getElementById(`walkthrough-tab-${next}`)?.focus(); }}><span>0{index + 1}</span>{item.label}</button>)}</div>
    <div className="walkthrough-panel" id="walkthrough-panel" role="tabpanel" aria-labelledby={`walkthrough-tab-${step}`} tabIndex={0}>
      <div className="walkthrough-copy" key={step}><Icon size={27} strokeWidth={1.25} /><h3>{current.title}</h3><p>{current.detail}</p></div>
      <div className="walkthrough-example" aria-label="Example data">
        {step === 0 && <><span className="eyebrow">your task</span><p>“Check my BNB and USDT balances and include the block number.”</p><div className="walkthrough-rule"><span>schedule</span><strong>run when needed</strong></div></>}
        {step === 1 && <><span className="eyebrow">allowed tool</span><p>BNB Smart Chain <Check size={16} /></p><div className="walkthrough-rule"><span>access</span><strong>read only</strong></div><div className="walkthrough-rule"><span>USDT payment</span><strong>none required</strong></div></>}
        {step === 2 && <><span className="eyebrow">example output</span><div className="walkthrough-rule"><span>BNB balance</span><strong>0.025 BNB</strong></div><div className="walkthrough-rule"><span>USDT balance</span><strong>12.00 USDT</strong></div><div className="walkthrough-rule"><span>evidence</span><strong>confirmed block</strong></div><small>Sample balances for this walkthrough.</small></>}
        {step === 3 && <><span className="eyebrow">example record</span><div className="walkthrough-rule"><span>tool</span><strong>chain check</strong></div><div className="walkthrough-rule"><span>USDT spent</span><strong>0 USDT</strong></div><div className="walkthrough-rule"><span>payment transaction</span><strong>none</strong></div><small>This walkthrough does not execute a run.</small></>}
      </div>
    </div><div className="walkthrough-foot"><span>0{step + 1} / 04</span><button className="text-link" onClick={() => setStep((step + 1) % steps.length)}>{step === 3 ? "back to the task" : "next step"}<ArrowRight size={15} /></button></div>
  </div>;
}
