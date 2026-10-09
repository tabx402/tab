import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { ArrowUpRight, Check, Pause, Play, RotateCcw } from "lucide-react";
import type { Provider } from "../lib/api";
import { AgentPolicyFields } from "./AgentPolicyFields";
import { BirdPair, AgentGlyph } from "./Drawings";

const duration = 12500;
const steps = ["name your agent", "set its daily cap", "choose its providers", "save a draft plan"];

export function AgentCreationPreview({ providers }: { providers: Provider[] }) {
  const root = useRef<HTMLElement>(null);
  const [elapsed, setElapsed] = useState(0);
  const [playing, setPlaying] = useState(true);
  const [visible, setVisible] = useState(false);
  const [reduced, setReduced] = useState(false);
  useEffect(() => {
    const media = matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => {
      setReduced(media.matches);
      if (media.matches) { setPlaying(false); setElapsed(duration); }
    };
    update(); media.addEventListener("change", update);
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting), { threshold: 0.2 });
    if (root.current) observer.observe(root.current);
    return () => { observer.disconnect(); media.removeEventListener("change", update); };
  }, []);
  useEffect(() => {
    if (!playing || !visible || reduced || !providers.length || elapsed >= duration) return;
    const timer = window.setInterval(() => setElapsed((time) => Math.min(duration, time + 100)), 100);
    return () => clearInterval(timer);
  }, [playing, visible, reduced, providers.length, elapsed >= duration]);
  const done = elapsed >= 11500;
  const active = elapsed < 2100 ? "name" : elapsed < 4900 ? "purpose" : elapsed < 6900 ? "cap" : elapsed < 9700 ? "providers" : "save";
  const phase = elapsed < 4900 ? 0 : elapsed < 6900 ? 1 : elapsed < 9700 ? 2 : 3;
  const demoProviders = providers.filter((p) => ["OpenRouter", "Tavily"].includes(p.name)).slice(0, 2);
  const selectedProviders = demoProviders.length ? demoProviders : providers.slice(0, 2);
  const purpose = "onchain research and daily summaries";
  const value = {
    name: "wren".slice(0, Math.max(0, Math.floor((elapsed - 450) / 280))),
    purpose: purpose.slice(0, Math.max(0, Math.floor((elapsed - 2350) / 60))),
    daily_cap: elapsed >= 5800 ? "25" : "1",
    providers: selectedProviders.slice(0, elapsed >= 8700 ? 2 : elapsed >= 7600 ? 1 : 0).map((p) => p.id),
  };
  function toggle() {
    if (elapsed >= duration) { setElapsed(0); setPlaying(true); }
    else setPlaying((current) => !current);
  }
  return <section className="creation-preview" id="agent-preview" ref={root} aria-labelledby="creation-title">
    <div className="creation-copy">
      <span className="eyebrow">a small start</span>
      <h2 id="creation-title">set up<br /><span>your agent.</span></h2>
      <p>A name, a daily cap, and the providers it can use. Start with a plan you can review.</p>
      <ol className="creation-steps">
        {steps.map((step, index) => <li key={step} className={phase === index ? "current" : phase > index || done ? "complete" : ""}>
          <span>{phase > index || done ? <Check size={13} /> : `0${index + 1}`}</span>{step}
        </li>)}
      </ol>
      <div className="creation-copy-footer"><BirdPair /><Link to="/account" className="text-link">create your agent <ArrowUpRight size={14} /></Link></div>
    </div>
    <div className="creation-stage panel">
      <div className="creation-toolbar"><span className="eyebrow">example walkthrough</span>
        <div className="inline-controls">
          {!reduced && <button className="icon-button" onClick={toggle} aria-label={elapsed >= duration ? "Replay agent walkthrough" : playing ? "Pause agent walkthrough" : "Play agent walkthrough"}>{elapsed >= duration ? <RotateCcw size={14} /> : playing ? <Pause size={14} /> : <Play size={14} />}</button>}
          {!reduced && <button className="icon-button" aria-label="Restart agent walkthrough" onClick={() => { setElapsed(0); setPlaying(true); }}><RotateCcw size={14} /></button>}
        </div>
      </div>
      <div className="creation-form policy-form" aria-label="Example agent plan">
        <h3>agent spending policy</h3>
        <AgentPolicyFields providers={selectedProviders} value={value} preview active={done || reduced ? "" : active} />
        <div className={`creation-save ${done ? "saved" : ""}`}>
          {done ? <Check size={15} /> : <AgentGlyph />}{done ? "draft plan saved" : elapsed >= 9700 ? "saving draft…" : "save agent plan"}
        </div>
        <p className="form-foot">Example only. Your plan saves after sign-in; it doesn't open credit or move funds.</p>
      </div>
      <div className="creation-progress" aria-hidden="true"><i style={{ transform: `scaleX(${elapsed / duration})` }} /></div>
    </div>
  </section>;
}
