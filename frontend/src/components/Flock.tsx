import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { RotateCcw } from "lucide-react";
import { request } from "../lib/api";
import type { AgentEvent } from "../lib/api";
import type { components } from "../lib/api-schema";
import "./flock.css";

type PublicAgent = components["schemas"]["PublicAgent"];
type Pt = { x: number; y: number };

// The same ink as the SVG drawings: a perched bird, a bird in flight, a blossom petal.
const PERCH = new Path2D("M7 26C3 19 7 10 16 10C25 9 30 15 28 22L34 28L23 27C17 32 10 31 7 26ZM10 23C12 16 21 16 24 23C20 27 14 28 10 23M28 15L34 18L28 20M13 30L12 35M21 29L22 35");
const SLEEP = new Path2D("M6 28C3 21 8 13 17 13C25 13 30 19 27 26L21 28C15 32 9 32 6 28ZM9 25C13 19 21 19 24 25M24 17C27 18 28 21 26 23M13 31L12 35M21 30L22 35");
const BODY = new Path2D("M8 25C5 19 8 11 17 10C25 9 30 15 27 22L35 27L23 26C18 31 11 30 8 25M28 14L35 17L28 19M23 25L33 30L29 25L35 28L29 22");
const WING = new Path2D("M12 23C5 17 2 8 6 3C16 8 20 14 23 22M12 20L7 10M16 20L11 9M19 21L15 12");
const PETAL = new Path2D("M0-4C-18-6-22-24-12-29C-4-32 3-24 1-15C9-27 21-25 20-16C19-7 9-3 0-4Z");

const C = { ink: "#e8eef1", muted: "#94a8b6", blue: "#abd5f1", mint: "#b9dfc6", amber: "#e9cd91", coral: "#efaaa0", line: "#293b49" };
const kindName: Record<string, string> = { run_started: "run started", run_completed: "run finished", run_failed: "run failed", tool_result: "tool call", model_result: "model call", provider_payment: "payment", payment_resolved: "payment", payment_required: "approval needed", registered: "registered", paused: "paused", resumed: "resumed", key_created: "access key", tool_unavailable: "tool unavailable", payment_failed: "payment failed" };

type Flight = { p: [Pt, Pt, Pt, Pt]; t0: number; dur: number; color: string; done: () => void };
type Bird = {
  id: string; name: string; tools: string[]; t: number; pos: Pt; perch: Pt; scale: number;
  hidden: boolean; leaving?: boolean; asleep: boolean; dir: 1 | -1; flight: Flight | null; queue: AgentEvent[];
  busy: number; hop: number; shake: number; blink: number; seed: number; trail: (Pt & { at: number; color: string })[]; runs: number; paid: number;
};
type Bloom = { key: string; pos: Pt; pulse: number; color: string; calls: number };
type Particle = { kind: "ring" | "note" | "feather" | "coin" | "text" | "spark" | "mote"; x: number; y: number; vx: number; vy: number; born: number; life: number; color: string; text?: string; r?: number };

const bez = (p: Pt[], t: number): Pt => {
  const u = 1 - t;
  if (p.length === 3) return { x: u * u * p[0].x + 2 * u * t * p[1].x + t * t * p[2].x, y: u * u * p[0].y + 2 * u * t * p[1].y + t * t * p[2].y };
  return { x: u * u * u * p[0].x + 3 * u * u * t * p[1].x + 3 * u * t * t * p[2].x + t * t * t * p[3].x, y: u * u * u * p[0].y + 3 * u * u * t * p[1].y + 3 * u * t * t * p[2].y + t * t * t * p[3].y };
};
const ease = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2);
const hash = (s: string) => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7) / 4294967295;
const chrono = (evs: AgentEvent[]) => [...evs].sort((a, b) => a.id - b.id);

export function Flock() {
  const wrap = useRef<HTMLDivElement>(null), canvas = useRef<HTMLCanvasElement>(null), tip = useRef<HTMLDivElement>(null);
  const navigate = useNavigate();
  const [ticker, setTicker] = useState<{ ev: AgentEvent; replay: boolean } | null>(null);
  const [mode, setMode] = useState<"loading" | "replay" | "live" | "empty">("loading");
  const [shown, setShown] = useState(0), [total, setTotal] = useState(0);
  const engine = useRef<{ replay: () => void } | null>(null);

  useEffect(() => {
    const cv = canvas.current!, box = wrap.current!, ctx = cv.getContext("2d")!;
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    let W = 0, H = 0, raf = 0, visible = true, alive = true, lastSeen = 0, replayQueue: AgentEvent[] = [], nextAt = 0, replaying = false, drawnAt = 0;
    const birds = new Map<string, Bird>(), blooms = new Map<string, Bloom>(), parts: Particle[] = [], recency = new Map<string, number>();
    let agents: PublicAgent[] = [], history: AgentEvent[] = [];
    const mouse = { x: -1, y: -1 };
    const distant = Array.from({ length: 5 }, (_, i) => ({ x: Math.random(), y: 0.08 + i * 0.09, v: 0.000012 + Math.random() * 0.00001, s: 0.38 + Math.random() * 0.3, ph: Math.random() * 9 }));
    const now = () => performance.now();

    // Geometry: an inked branch from the left, a flowering stem hanging from the top right.
    const branch = (): Pt[] => [{ x: -20, y: H * 0.78 }, { x: W * 0.32, y: H * 0.9 }, { x: W * (W < 640 ? 0.86 : 0.64), y: H * (W < 640 ? 0.74 : 0.66) }];
    const stem = (): Pt[] => (W < 640 ? [{ x: W + 20, y: H * 0.05 }, { x: W * 0.8, y: H * 0.42 }, { x: W * 0.3, y: H * 0.3 }] : [{ x: W + 20, y: -20 }, { x: W * 0.99, y: H * 0.5 }, { x: W * 0.76, y: H * 0.46 }]);
    // A second, higher branch takes every other bird once the flock outgrows one.
    const upper = (): Pt[] => [{ x: -20, y: H * 0.5 }, { x: W * 0.22, y: H * 0.6 }, { x: W * 0.5, y: H * 0.44 }];
    const twoBranches = () => W >= 640 && birds.size > 8;
    const layout = () => {
      const list = [...birds.values()].filter((x) => !x.leaving), two = twoBranches(), per = two ? Math.ceil(list.length / 2) : list.length;
      list.forEach((bird, i) => {
        const onUpper = two && i % 2 === 1, j = two ? Math.floor(i / 2) : i, n = onUpper ? list.length - per : per;
        bird.t = n <= 1 ? 0.55 : 0.2 + (0.72 * j) / Math.max(1, n - 1);
        bird.perch = bez(onUpper ? upper() : branch(), Math.min(0.93, bird.t));
        if (!bird.flight) bird.pos = { ...bird.perch };
        bird.scale = Math.min(2.1, Math.max(1.1, (W < 640 ? 1.3 : 1.75) + bird.runs * 0.05)) * Math.min(1, 6 / Math.max(6, per));
      });
      const keys = [...blooms.keys()], s = stem();
      keys.forEach((k, i) => { blooms.get(k)!.pos = bez(s, 0.42 + (0.58 * (i + 1)) / (keys.length + 0.4)); });
    };
    const resize = () => {
      const r = box.getBoundingClientRect(), dpr = Math.min(2, devicePixelRatio || 1);
      W = r.width; H = r.height; cv.width = W * dpr; cv.height = H * dpr; ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      layout(); if (reduced) draw();
    };

    const ensureBird = (id: string, name: string, tools: string[] = []) => {
      let b = birds.get(id);
      if (!b) {
        b = { id, name, tools, t: 0.5, pos: { x: 0, y: 0 }, perch: { x: 0, y: 0 }, scale: 1.3, hidden: false, asleep: false, dir: 1, flight: null, queue: [], busy: 0, hop: 0, shake: 0, blink: now() + 2000 + hash(id) * 4000, seed: hash(id), trail: [], runs: 0, paid: 0 };
        birds.set(id, b);
      }
      if (tools.length) b.tools = tools;
      tools.forEach((t) => ensureBloom(t));
      return b;
    };
    // Only the most recently active agents get a bird. When another agent acts, the stalest idle bird
    // flies off and the newcomer flies in.
    const maxBirds = () => (W < 640 ? 6 : 16);
    const admit = (id: string, name: string, tools: string[]) => {
      const have = birds.get(id);
      if (have) { have.leaving = false; return have; }
      if (birds.size >= maxBirds()) {
        const t = now(), idle = [...birds.values()].filter((x) => !x.leaving && !x.flight && !x.queue.length && t >= x.busy);
        const out = idle.sort((x, y) => (recency.get(x.id) ?? 0) - (recency.get(y.id) ?? 0))[0];
        if (!out) return null;
        out.leaving = true;
        fly(out, { x: W + 80, y: -40 }, C.muted, 1500, () => {
          out.flight = null;
          if (out.queue.length) { out.leaving = false; out.pos = { x: W + 60, y: H * 0.2 }; home(out, C.blue); } else { birds.delete(out.id); layout(); setShown([...birds.values()].filter((x) => !x.leaving).length); }
        });
      }
      const b = ensureBird(id, name, tools);
      layout(); setShown([...birds.values()].filter((x) => !x.leaving).length);
      if (!reduced && !b.flight) { b.pos = { x: -60, y: H * 0.12 + b.seed * H * 0.25 }; fly(b, { ...b.perch }, C.blue, 1600, () => { b.flight = null; }); b.busy = now() + 1700; }
      return b;
    };
    const ensureBloom = (key: string) => { if (!blooms.has(key)) blooms.set(key, { key, pos: { x: 0, y: 0 }, pulse: 0, color: C.mint, calls: 0 }); return blooms.get(key)!; };

    const emit = (p: Omit<Particle, "born">) => parts.push({ ...p, born: now() });
    const ring = (at: Pt, color: string, r = 30) => emit({ kind: "ring", x: at.x, y: at.y, vx: 0, vy: 0, life: 1100, color, r });
    const label = (at: Pt, text: string, color: string) => emit({ kind: "text", x: at.x, y: at.y, vx: 0, vy: -0.022, life: 2200, color, text });
    const head = (b: Bird): Pt => ({ x: b.pos.x + 6 * b.scale * b.dir, y: b.pos.y - 22 * b.scale });

    const fly = (b: Bird, to: Pt, color: string, dur: number, done: () => void) => {
      const from = { ...b.pos }, lift = Math.min(from.y, to.y) - 60 - b.seed * 50;
      b.flight = { p: [from, { x: from.x + (to.x - from.x) * 0.2, y: lift }, { x: from.x + (to.x - from.x) * 0.8, y: lift - 10 }, to], t0: now(), dur, color, done };
    };
    const home = (b: Bird, color: string) => fly(b, { ...b.perch }, color, 1300, () => { b.flight = null; ring(head(b), color, 18); });

    const play = (b: Bird, ev: AgentEvent) => {
      const t = now(), k = ev.kind, h = head(b);
      b.busy = t + 900;
      const bloom = ev.provider && blooms.has(ev.provider) ? blooms.get(ev.provider)! : null;
      if (k === "registered") {
        b.hidden = false; b.pos = { x: -60, y: H * 0.15 + b.seed * H * 0.2 };
        fly(b, { ...b.perch }, C.mint, 1900, () => { b.flight = null; ring(head(b), C.mint, 40); label(head(b), "registered", C.mint); });
      } else if ((k === "tool_result" || k === "model_result" || k === "provider_payment" || k === "payment_resolved") && bloom) {
        const paid = Number(ev.amount ?? 0) > 0, color = paid ? C.amber : C.mint;
        b.asleep = false;
        fly(b, { x: bloom.pos.x - 10, y: bloom.pos.y + 34 }, color, 1500, () => {
          bloom.pulse = now(); bloom.color = color; bloom.calls++;
          ring(bloom.pos, color, 34);
          if (paid) { b.paid += Number(ev.amount); emit({ kind: "coin", x: bloom.pos.x, y: bloom.pos.y + 6, vx: -0.02, vy: 0.03, life: 1600, color: C.amber }); label(bloom.pos, `${Number(ev.amount).toFixed(2)} USDT`, C.amber); }
          else label({ x: bloom.pos.x, y: bloom.pos.y - 26 }, ev.provider!, C.mint);
          setTimeout(() => alive && home(b, color), 380);
        });
      } else if (k === "payment_required" && bloom) {
        b.asleep = false;
        const mid = { x: (b.perch.x + bloom.pos.x) / 2, y: Math.min(b.perch.y, bloom.pos.y) - 30 };
        fly(b, mid, C.amber, 1200, () => {
          label({ x: mid.x, y: mid.y - 30 }, "wallet approval?", C.amber); ring({ x: mid.x, y: mid.y - 16 }, C.amber, 26);
          setTimeout(() => alive && home(b, C.amber), 700);
        });
      } else if (k === "run_started") {
        b.asleep = false; b.hop = t; ring(h, C.blue, 26);
        for (let i = 0; i < 3; i++) emit({ kind: "spark", x: h.x + (i - 1) * 7, y: h.y - 12, vx: 0, vy: -0.01, life: 900 + i * 200, color: C.blue });
      } else if (k === "run_completed") {
        b.runs++; b.hop = t; layout();
        for (let i = 0; i < 2; i++) emit({ kind: "note", x: h.x + 10 * b.dir, y: h.y, vx: 0.02 * b.dir + i * 0.012, vy: -0.035, life: 1800, color: C.mint });
      } else if (k === "run_failed" || k === "tool_unavailable" || k === "payment_failed" || ev.status === "failed") {
        b.shake = t; label(h, kindName[k] ?? "failed", C.coral);
        emit({ kind: "feather", x: h.x, y: h.y + 8, vx: 0, vy: 0.022, life: 3200, color: C.coral });
      } else if (k === "paused") {
        b.asleep = true; label(h, "paused", C.muted);
      } else if (k === "resumed") {
        b.asleep = false; b.hop = t; ring(h, C.blue, 30); label(h, "resumed", C.blue);
      } else {
        b.hop = t; ring(h, C.blue, 22);
      }
    };

    const dispatch = (ev: AgentEvent, replay: boolean) => {
      const agent = agents.find((a) => a.id === ev.agent_id), id = ev.agent_id ?? ev.agent;
      recency.set(id, Math.max(recency.get(id) ?? 0, ev.id));
      if (ev.provider && !blooms.has(ev.provider)) { ensureBloom(ev.provider); layout(); }
      admit(id, ev.agent, agent?.tools ?? [])?.queue.push(ev);
      setTicker({ ev, replay });
    };

    const startReplay = () => {
      if (!history.length) { setMode(birds.size ? "live" : "empty"); return; }
      const evs = chrono(history).slice(-36);
      parts.length = 0;
      for (const b of birds.values()) {
        const mine = evs.filter((e) => (e.agent_id ?? e.agent) === b.id);
        const firstSleep = mine.find((e) => e.kind === "paused" || e.kind === "resumed");
        const agent = agents.find((a) => a.id === b.id);
        b.hidden = mine.some((e) => e.kind === "registered");
        b.asleep = firstSleep ? firstSleep.kind === "resumed" : agent?.status === "paused";
        b.flight = null; b.queue = []; b.pos = { ...b.perch }; b.runs = 0; b.trail = [];
      }
      if (reduced) {
        for (const b of birds.values()) { b.hidden = false; b.asleep = agents.find((a) => a.id === b.id)?.status === "paused"; }
        setMode("live"); setTicker(history[0] ? { ev: history[0], replay: false } : null); draw(); return;
      }
      replayQueue = evs; nextAt = now() + 700; replaying = true; setMode("replay");
    };

    const load = async (first: boolean) => {
      try {
        const [a, e] = await Promise.all([request<PublicAgent[]>("/agents/live"), request<AgentEvent[]>("/activity?limit=120")]);
        if (!alive) return;
        agents = a;
        for (const x of e) { const id = x.agent_id ?? x.agent; if (!(recency.get(id)! >= x.id)) recency.set(id, x.id); if (x.provider) ensureBloom(x.provider); }
        a.forEach((x) => { if (birds.has(x.id)) birds.get(x.id)!.tools = x.tools; });
        if (first) {
          // the starting flock: the agents active most recently, then the newest
          const names = new Map<string, { name: string; tools: string[] }>(a.map((x) => [x.id, { name: x.name, tools: x.tools }]));
          for (const x of e) { const id = x.agent_id ?? x.agent; if (!names.has(id)) names.set(id, { name: x.agent, tools: [] }); }
          [...names.keys()].sort((p, q) => (recency.get(q) ?? 0) - (recency.get(p) ?? 0)).slice(0, maxBirds()).forEach((id) => ensureBird(id, names.get(id)!.name, names.get(id)!.tools));
        }
        setTotal(new Set([...a.map((x) => x.id), ...e.map((x) => x.agent_id ?? x.agent)]).size); setShown([...birds.values()].filter((x) => !x.leaving).length);
        layout();
        const fresh = chrono(e.filter((x) => x.id > lastSeen));
        history = e; lastSeen = Math.max(lastSeen, ...e.map((x) => x.id), 0);
        if (first) startReplay();
        else if (fresh.length && !reduced) { if (replaying) replayQueue.push(...fresh); else fresh.forEach((x) => dispatch(x, false)); }
        else if (reduced) draw();
      } catch {
        if (first && alive) setMode(birds.size ? "live" : "empty");
      }
    };

    // Rendering
    const inkBranch = (pts: Pt[], reveal: number, width: number) => {
      ctx.save(); ctx.strokeStyle = "#7f97a8"; ctx.lineWidth = width; ctx.lineCap = "round";
      ctx.setLineDash([2000]); ctx.lineDashOffset = 2000 * (1 - reveal);
      ctx.beginPath(); ctx.moveTo(pts[0].x, pts[0].y); ctx.quadraticCurveTo(pts[1].x, pts[1].y, pts[2].x, pts[2].y); ctx.stroke(); ctx.restore();
    };
    const twig = (base: Pt, dx: number, dy: number, alpha: number) => {
      ctx.save(); ctx.globalAlpha = alpha; ctx.strokeStyle = "#7f97a8"; ctx.lineWidth = 1; ctx.beginPath(); ctx.moveTo(base.x, base.y);
      ctx.quadraticCurveTo(base.x + dx * 0.4, base.y + dy * 0.9, base.x + dx, base.y + dy); ctx.stroke();
      ctx.beginPath(); ctx.ellipse(base.x + dx, base.y + dy, 7, 3, Math.atan2(dy, dx), 0, Math.PI * 2); ctx.stroke(); ctx.restore();
    };
    const drawBloom = (bl: Bloom, t: number, sway: number) => {
      const p = (t - bl.pulse) / 1200, glow = p >= 0 && p < 1 ? 1 - p : 0, s = 0.5 + glow * 0.14;
      ctx.save(); ctx.translate(bl.pos.x + sway, bl.pos.y); ctx.rotate(sway * 0.01 + t * 0.00008);
      ctx.strokeStyle = glow ? bl.color : "#c9d6de"; ctx.globalAlpha = 0.55 + glow * 0.45; ctx.lineWidth = 1.1 / s;
      ctx.shadowColor = bl.color; ctx.shadowBlur = glow * 22; ctx.scale(s, s);
      for (let i = 0; i < 5; i++) { ctx.rotate((Math.PI * 2) / 5); ctx.stroke(PETAL); }
      ctx.fillStyle = glow ? bl.color : "#c9d6de"; ctx.beginPath(); ctx.arc(0, 0, 3.4, 0, Math.PI * 2); ctx.fill(); ctx.restore();
      ctx.save(); ctx.fillStyle = C.muted; ctx.globalAlpha = 0.75; ctx.font = "10px 'IBM Plex Mono', monospace"; ctx.textAlign = "center";
      ctx.fillText(bl.key, bl.pos.x + sway, bl.pos.y + 28); ctx.restore();
    };
    const drawBird = (b: Bird, t: number) => {
      if (b.hidden && !b.flight) return;
      let { x, y } = b.pos, flap = 0, rot = 0;
      if (b.flight) {
        const f = b.flight, k = Math.min(1, (t - f.t0) / f.dur), e = ease(k), p = bez(f.p, e), q = bez(f.p, Math.min(1, e + 0.02));
        b.dir = q.x >= p.x ? 1 : -1; rot = Math.max(-0.5, Math.min(0.5, Math.atan2(q.y - p.y, Math.abs(q.x - p.x) + 0.001))) * 0.7;
        b.pos = p; x = p.x; y = p.y; flap = Math.sin(t * 0.03 + b.seed * 9);
        b.trail.push({ x: p.x, y: p.y - 18 * b.scale, at: t, color: f.color });
        if (k >= 1) { b.pos = { ...f.p[3] }; const d = f.done; b.flight = null; d(); }
      } else {
        const hopK = (t - b.hop) / 520; if (hopK >= 0 && hopK < 1) y -= Math.sin(hopK * Math.PI) * 9;
        if (!b.asleep) y += Math.sin(t * 0.002 + b.seed * 20) * 0.8;
        const sk = (t - b.shake) / 700; if (sk >= 0 && sk < 1) x += Math.sin(sk * 40) * 3 * (1 - sk);
        if (mouse.x >= 0 && !b.asleep && Math.abs(mouse.x - x) > 12 && Math.hypot(mouse.x - x, mouse.y - y) < 220) b.dir = mouse.x > x ? 1 : -1;
        else if (Math.sin(t * 0.00035 + b.seed * 50) > 0.92) b.dir = -1; else if (!b.flight && Math.sin(t * 0.00035 + b.seed * 50) < -0.5) b.dir = 1;
      }
      // trail
      b.trail = b.trail.filter((p) => t - p.at < 900);
      if (b.trail.length > 1) {
        ctx.save(); ctx.lineCap = "round"; ctx.shadowBlur = 10;
        for (let i = 1; i < b.trail.length; i++) {
          const a = b.trail[i - 1], c = b.trail[i], life = 1 - (t - c.at) / 900;
          ctx.strokeStyle = c.color; ctx.shadowColor = c.color; ctx.globalAlpha = life * 0.7; ctx.lineWidth = 2.2 * life;
          ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(c.x, c.y); ctx.stroke();
        }
        ctx.restore();
      }
      const s = b.scale, color = b.asleep ? C.muted : C.ink;
      ctx.save(); ctx.translate(x, y); ctx.rotate(rot * b.dir); ctx.scale(s * b.dir, s); ctx.translate(-20, -35);
      ctx.strokeStyle = color; ctx.lineWidth = 1.25 / s; ctx.lineCap = "round"; ctx.lineJoin = "round";
      ctx.globalAlpha = b.asleep ? 0.6 : 0.95;
      if (b.flight) {
        ctx.stroke(BODY);
        ctx.save(); ctx.translate(18, 22); ctx.scale(1, 0.25 + 0.75 * flap); ctx.translate(-18, -22); ctx.stroke(WING); ctx.restore();
        ctx.fillStyle = color; ctx.beginPath(); ctx.arc(24, 15, 0.8, 0, Math.PI * 2); ctx.fill();
      } else if (b.asleep) {
        ctx.stroke(SLEEP);
      } else {
        ctx.stroke(PERCH);
        if (t > b.blink) b.blink = t + 2600 + Math.random() * 4200;
        if (b.blink - t > 130) { ctx.fillStyle = color; ctx.beginPath(); ctx.arc(24, 15, 0.9, 0, Math.PI * 2); ctx.fill(); }
      }
      ctx.restore();
      if (b.asleep && !b.flight && Math.random() < 0.006) emit({ kind: "text", x: x + 14, y: y - 34 * s, vx: 0.012, vy: -0.02, life: 2000, color: C.muted, text: "z" });
    };
    const drawParticles = (t: number) => {
      for (let i = parts.length - 1; i >= 0; i--) {
        const p = parts[i], age = t - p.born, k = age / p.life;
        if (k >= 1) { if (p.kind === "mote") { p.born = t; p.x = Math.random() * W; p.y = H + 10; } else { parts.splice(i, 1); continue; } }
        const dt = 16, a = p.kind === "mote" ? Math.sin(Math.min(1, k) * Math.PI) * 0.35 : 1 - k;
        p.x += p.vx * dt; p.y += p.vy * dt;
        ctx.save(); ctx.globalAlpha = Math.max(0, a); ctx.strokeStyle = p.color; ctx.fillStyle = p.color;
        if (p.kind === "ring") { ctx.lineWidth = 1.4; ctx.shadowColor = p.color; ctx.shadowBlur = 12; ctx.beginPath(); ctx.arc(p.x, p.y, 6 + (p.r ?? 30) * ease(k), 0, Math.PI * 2); ctx.stroke(); }
        else if (p.kind === "note") { const sx = p.x + Math.sin(age * 0.008) * 4; ctx.lineWidth = 1.2; ctx.beginPath(); ctx.ellipse(sx, p.y, 3, 2.2, -0.4, 0, Math.PI * 2); ctx.fill(); ctx.beginPath(); ctx.moveTo(sx + 2.8, p.y); ctx.lineTo(sx + 2.8, p.y - 11); ctx.quadraticCurveTo(sx + 7, p.y - 9, sx + 7, p.y - 5); ctx.stroke(); }
        else if (p.kind === "feather") { const sx = p.x + Math.sin(age * 0.004) * 14; ctx.translate(sx, p.y); ctx.rotate(Math.sin(age * 0.004) * 0.8); ctx.lineWidth = 1.1; ctx.beginPath(); ctx.moveTo(0, -7); ctx.quadraticCurveTo(5, 0, 0, 7); ctx.quadraticCurveTo(-5, 0, 0, -7); ctx.moveTo(0, -9); ctx.lineTo(0, 10); ctx.stroke(); }
        else if (p.kind === "coin") { ctx.shadowColor = p.color; ctx.shadowBlur = 14; ctx.lineWidth = 1.3; ctx.beginPath(); ctx.arc(p.x, p.y, 5, 0, Math.PI * 2); ctx.stroke(); ctx.font = "8px 'IBM Plex Mono', monospace"; ctx.textAlign = "center"; ctx.fillText("$", p.x, p.y + 3); }
        else if (p.kind === "spark" || p.kind === "mote") { ctx.shadowColor = p.color; ctx.shadowBlur = 8; ctx.beginPath(); ctx.arc(p.x, p.y, p.kind === "mote" ? (p.r ?? 1) : 1.8, 0, Math.PI * 2); ctx.fill(); }
        else if (p.kind === "text") { ctx.font = `${p.text === "z" ? 12 : 11}px 'IBM Plex Mono', monospace`; ctx.textAlign = "center"; ctx.globalAlpha = Math.min(1, (1 - k) * 2.2); ctx.fillText(p.text ?? "", p.x, p.y - 8); }
        ctx.restore();
      }
    };
    const draw = () => {
      const t = now();
      ctx.clearRect(0, 0, W, H);
      const reveal = reduced ? 1 : Math.min(1, (t - drawnAt) / 1400), sway = Math.sin(t * 0.0009) * 2;
      inkBranch(branch(), ease(reveal), 1.6);
      if (twoBranches()) inkBranch(upper(), ease(reveal), 1.3);
      inkBranch(stem(), ease(reveal), 1.1);
      if (reveal > 0.6) {
        const b = branch(), a = (reveal - 0.6) / 0.4 * 0.7;
        [[0.12, -16, -18], [0.4, 14, 16], [0.7, -10, 18], [0.86, 18, -14]].forEach(([tt, dx, dy]) => twig(bez(b, tt), dx, dy, a));
        const s = stem(); twig(bez(s, 0.3), -18, 6, a);
      }
      for (const d of distant) {
        const x = ((d.x + t * d.v) % 1.2) * W - 0.1 * W, y = d.y * H + Math.sin(t * 0.0006 + d.ph) * 8;
        ctx.save(); ctx.translate(x, y); ctx.scale(d.s, d.s); ctx.translate(-20, -22); ctx.strokeStyle = C.blue; ctx.globalAlpha = 0.16; ctx.lineWidth = 1.2 / d.s; ctx.lineCap = "round";
        ctx.stroke(BODY); ctx.translate(18, 22); ctx.scale(1, 0.25 + 0.75 * Math.sin(t * 0.012 + d.ph)); ctx.translate(-18, -22); ctx.stroke(WING); ctx.restore();
      }
      for (const bl of blooms.values()) drawBloom(bl, t, sway * (bl.pos.y / Math.max(1, H)));
      drawParticles(t);
      [...birds.values()].sort((a, b) => (a.flight ? 1 : 0) - (b.flight ? 1 : 0)).forEach((b) => drawBird(b, t));
    };
    const tick = () => {
      raf = requestAnimationFrame(tick);
      if (!visible || document.hidden) return;
      const t = now();
      if (replaying && t >= nextAt) {
        const ev = replayQueue.shift();
        if (ev) { dispatch(ev, true); nextAt = t + (["tool_result", "model_result", "provider_payment", "payment_resolved", "payment_required", "registered"].includes(ev.kind) ? 1700 : 900); }
        else { replaying = false; setMode("live"); }
      }
      for (const b of birds.values()) if (!b.leaving && !b.flight && t >= b.busy && b.queue.length) play(b, b.queue.shift()!);
      draw();
    };

    // Hover and click: name, record, link to the agent.
    const hit = (e: PointerEvent) => {
      const r = cv.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top;
      mouse.x = x; mouse.y = y;
      return [...birds.values()].find((b) => !b.hidden && Math.hypot(b.pos.x - x, b.pos.y - 18 * b.scale - y) < 26 * b.scale);
    };
    const onMove = (e: PointerEvent) => {
      const b = hit(e), el = tip.current!;
      cv.style.cursor = b ? "pointer" : "default";
      if (!b) { el.hidden = true; return; }
      const agent = agents.find((a) => a.id === b.id), evs = history.filter((x) => (x.agent_id ?? x.agent) === b.id);
      el.hidden = false;
      el.innerHTML = "";
      const strong = document.createElement("strong"); strong.textContent = b.name;
      const meta = document.createElement("span");
      meta.textContent = `${agent ? `#${agent.registry_id} · ${agent.status} · ${agent.cadence}` : "agent"} · ${evs.filter((x) => x.kind === "run_completed").length} runs`;
      const tools = document.createElement("span"); tools.textContent = b.tools.join(" · ") || "no tools";
      el.append(strong, meta, tools);
      el.style.left = `${Math.min(W - 200, Math.max(8, b.pos.x - 90))}px`; el.style.top = `${Math.max(8, b.pos.y - 48 * b.scale - 70)}px`;
    };
    const onLeave = () => { mouse.x = -1; tip.current!.hidden = true; cv.style.cursor = "default"; };
    const onClick = (e: PointerEvent) => { const b = hit(e); if (b && agents.some((a) => a.id === b.id)) navigate(`/agents/${b.id}`); };

    const ro = new ResizeObserver(resize); ro.observe(box);
    const io = new IntersectionObserver(([en]) => { visible = en.isIntersecting; }); io.observe(box);
    cv.addEventListener("pointermove", onMove); cv.addEventListener("pointerleave", onLeave); cv.addEventListener("click", onClick as EventListener);
    resize(); drawnAt = now();
    if (!reduced) {
      for (let i = 0; i < 26; i++) parts.push({ kind: "mote", x: Math.random() * W, y: Math.random() * H, vx: (Math.random() - 0.5) * 0.008, vy: -0.006 - Math.random() * 0.01, born: now() - Math.random() * 9000, life: 9000 + Math.random() * 6000, color: i % 3 ? C.blue : C.mint, r: 0.6 + Math.random() * 1.1 });
      raf = requestAnimationFrame(tick);
    }
    void load(true);
    const poll = setInterval(() => { if (!document.hidden) void load(false); }, 5000);
    engine.current = { replay: startReplay };
    return () => { alive = false; cancelAnimationFrame(raf); clearInterval(poll); ro.disconnect(); io.disconnect(); cv.removeEventListener("pointermove", onMove); cv.removeEventListener("pointerleave", onLeave); cv.removeEventListener("click", onClick as EventListener); };
  }, [navigate]);

  const ev = ticker?.ev;
  return (
    <section className="panel flock-panel">
      <div className="panel-heading">
        <div>
          <h2>the flock</h2>
          <p className="flock-sub">A bird per agent, a blossom per tool. Calls fly to the tool, payments carry USDT, failures drop a feather.</p>
        </div>
        {total > shown && <span className="flock-count">{shown} of {total} agents · most recently active</span>}
        <button className="icon-button" aria-label="Replay recent activity" title="replay" onClick={() => engine.current?.replay()} disabled={mode === "loading" || mode === "empty"}>
          <RotateCcw size={14} />
        </button>
      </div>
      <div className="flock-stage" ref={wrap}>
        <canvas ref={canvas} aria-label="Animated view of registered agents and their recent runs, tool calls and payments" role="img" />
        <div className="flock-tip" ref={tip} hidden />
        {mode === "empty" && <p className="flock-empty">no agents have landed yet.</p>}
      </div>
      <div className="flock-ticker" aria-live="polite">
        <span className={`flock-mode ${mode}`}><i />{mode === "replay" ? "replaying recent log" : mode === "live" ? "live" : mode === "loading" ? "reading the log" : "waiting"}</span>
        {ev && (
          <span className="flock-event" key={ev.id}>
            <time>{new Date(ev.timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</time>
            <strong>{ev.agent}</strong>
            <span className={`log-kind ${ev.kind}`}>{kindName[ev.kind] ?? ev.kind.replaceAll("_", " ")}</span>
            <em>{ev.message}</em>
          </span>
        )}
      </div>
    </section>
  );
}
