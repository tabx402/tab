import { shortId } from "../lib/evm";
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { request } from "../lib/api";
import type { AgentEvent, RegistryData, PublicJob } from "../lib/api";
import type { components } from "../lib/api-schema";
import "./garden.css";
import { GardenJobs } from "./Jobs";
import { jobAPI } from "../lib/jobs";

type PublicAgent = components["schemas"]["PublicAgent"];
type AgentRecord = components["schemas"]["AgentRecord"];
type Pt = { x: number; y: number };

/* The garden. One plant per agent, grown from its record and nothing else: a leaf (or seed) per completed
   run, a blossom per tool, an amber berry per settled payment, a fallen coral leaf per failure. The species and
   shape come from the agent's id, so a refresh grows the same plant; new growth unfurls in place. */

const C = { ink: "#d9e3e9", stem: "#9fb3c1", muted: "#7d91a0", blue: "#abd5f1", mint: "#b9dfc6", amber: "#e9cd91", coral: "#efaaa0", bg: "#0c1926" };
const TOOL_COLORS = ["#abd5f1", "#b9dfc6", "#c9b8ef", "#f2c6d6", "#9fd6d2", "#dfe3a6"];
const PETAL = new Path2D("M0-4C-18-6-22-24-12-29C-4-32 3-24 1-15C9-27 21-25 20-16C19-7 9-3 0-4Z");
const PERCH = new Path2D("M7 26C3 19 7 10 16 10C25 9 30 15 28 22L34 28L23 27C17 32 10 31 7 26ZM10 23C12 16 21 16 24 23C20 27 14 28 10 23M28 15L34 18L28 20M13 30L12 35M21 29L22 35");
const SPECIES = ["spray", "fern", "reeds", "allium", "shrub", "willow"] as const;
type Species = (typeof SPECIES)[number] | "registry";
const SPECIES_NAME: Record<Species, string> = { spray: "a flowering spray", fern: "a fern", reeds: "tulip reeds", allium: "an allium", shrub: "a shrub", willow: "a willow", registry: "the registry tree" };
// Each row back is a quarter smaller, receding toward the horizon however many rows there are.
const depth = (row: number) => Math.max(0.09, 0.76 ** row);
const MAX_PLANTS = 800, MAX_BRANCHES = 18;
const GROW = 2600;

type El =
  | { key: string; type: "curve"; pts: Pt[]; w: number; color: string; order: number }
  | { key: string; type: "leaf"; at: Pt; ang: number; size: number; color: string; order: number }
  | { key: string; type: "bloom"; at: Pt; ang: number; size: number; color: string; order: number; shape: "petal" | "tulip" | "globe" | "curl"; tool: string }
  | { key: string; type: "berry"; at: Pt; r: number; color: string; order: number }
  | { key: string; type: "seed"; at: Pt; ang: number; size: number; color: string; order: number }
  | { key: string; type: "bud"; at: Pt; ang: number; size: number; color: string; order: number };
type Record_ = { jobs: PublicJob[]; runs: number; fails: number; pays: number; paid: number; tools: string[]; paused: boolean; young: boolean; recent: boolean };
type Plant = {
  key: string; name: string; meta: string; species: Species; seed: number; rec: Record_; href?: string;
  els: El[]; height: number; bounds: { x0: number; x1: number; y0: number }; perch: Pt | null;
  base: Pt; scale: number; row: number; start: number; settle?: number;
};
type Spark = { x: number; y: number; vx: number; vy: number; at: number; life: number; color: string };

// A soft glow drawn from one cached gradient per colour: far cheaper than shadowBlur on every blossom, every frame.
const halos = new Map<string, HTMLCanvasElement>();
const halo = (color: string) => {
  let c = halos.get(color);
  if (!c) {
    c = document.createElement("canvas"); c.width = c.height = 64;
    const g = c.getContext("2d")!, grad = g.createRadialGradient(32, 32, 0, 32, 32, 32);
    grad.addColorStop(0, `${color}88`); grad.addColorStop(0.45, `${color}2e`); grad.addColorStop(1, `${color}00`);
    g.fillStyle = grad; g.fillRect(0, 0, 64, 64); halos.set(color, c);
  }
  return c;
};
const hash = (s: string) => { let h = 2166136261; for (const c of s) h = Math.imul(h ^ c.charCodeAt(0), 16777619); return h >>> 0; };
const rng = (seed: number) => { let a = seed >>> 0; return () => { a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; };
const toolColor = (tool: string) => TOOL_COLORS[hash(tool) % TOOL_COLORS.length];
const clamp = (v: number, a = 0, b = 1) => Math.max(a, Math.min(b, v));
const easeOut = (t: number) => 1 - (1 - t) ** 3;
const easeBack = (t: number) => 1 + 2.2 * (t - 1) ** 3 + 1.2 * (t - 1) ** 2;

// A stroke that grows from (x, y) at an angle from vertical, bending by `curl` radians along its length.
function shoot(x: number, y: number, ang: number, len: number, curl: number, steps = 22): Pt[] {
  const pts = [{ x, y }]; let a = ang; const seg = len / steps;
  for (let i = 0; i < steps; i++) { a += curl / steps; x += Math.sin(a) * seg; y -= Math.cos(a) * seg; pts.push({ x, y }); }
  return pts;
}
const at = (pts: Pt[], t: number) => pts[Math.round(clamp(t) * (pts.length - 1))];
const angleAt = (pts: Pt[], t: number) => { const i = Math.min(pts.length - 2, Math.round(clamp(t) * (pts.length - 1))); return Math.atan2(pts[i + 1].x - pts[i].x, -(pts[i + 1].y - pts[i].y)); };

function grow(p: Omit<Plant, "els" | "height" | "bounds" | "perch" | "base" | "scale" | "row" | "start">, extra?: { agents: { key: string; color: string }[]; txs: number }): Pick<Plant, "els" | "height" | "bounds" | "perch"> {
  const r = rng(p.seed), rec = p.rec, els: El[] = [];
  const ink = rec.paused ? C.muted : C.stem;
  const tools = rec.tools.length ? rec.tools : [""];
  // the newest 28 runs get leaves; the plant keeps growing taller with the lifetime count
  const runs = Math.min(28, rec.runs);
  const H0 = (118 + r() * 46) * (0.72 + 0.42 * Math.min(1, Math.log2(1 + rec.runs) / 5)) * (rec.young ? 0.74 : 1);
  const spots: { pts: Pt[]; t0: number; t1: number; side?: number }[] = [];
  const blooms: El[] = [];
  let top: Pt = { x: 0, y: -H0 };
  const curve = (key: string, pts: Pt[], w: number, order: number, color = ink) => els.push({ key, type: "curve", pts, w, color, order });
  const bloom = (i: number, at_: Pt, ang: number, size: number, shape: "petal" | "tulip" | "globe" | "curl", order: number) => {
    const tool = tools[i] ?? "";
    const el: El = { key: `bloom:${tool || i}`, type: "bloom", at: at_, ang, size: rec.paused ? size * 0.7 : size, color: tool ? toolColor(tool) : C.ink, order, shape, tool };
    blooms.push(el); els.push(el);
  };

  if (p.species === "registry" && extra) {
    const trunk = shoot(0, 0, (r() - 0.5) * 0.1, 250, 0.18, 30);
    curve("trunk", trunk, 4.2, 0, C.stem); curve("trunk2", trunk.map((q, i) => ({ x: q.x + 3.5 * (1 - i / trunk.length), y: q.y })), 1.2, 0.05, C.stem); top = trunk[trunk.length - 1];
    for (let i = 0; i < 4; i++) curve(`root:${i}`, shoot((i - 1.5) * 6, 0, (i < 2 ? -1 : 1) * (1.7 + r() * 0.3), 30 + r() * 26, (i < 2 ? -1 : 1) * 0.5, 10), 1.6, 0.05);
    const n = Math.max(1, extra.agents.length);
    extra.agents.forEach((a, i) => {
      const t = 0.38 + (0.56 * (i + 0.5)) / n, side = i % 2 ? 1 : -1, o = at(trunk, t);
      const br = shoot(o.x, o.y, side * (0.75 + r() * 0.35), 70 + r() * 40, -side * (0.5 + r() * 0.4), 16);
      curve(`branch:${a.key}`, br, 1.8, 0.2 + t * 0.3);
      const tip = br[br.length - 1];
      for (let j = 0; j < 5; j++) els.push({ key: `bud:${a.key}:${j}`, type: "leaf", at: { x: tip.x + (r() - 0.5) * 18, y: tip.y + (r() - 0.5) * 16 }, ang: side * (0.4 + r() * 1.2), size: 9 + r() * 5, color: a.color, order: 0.6 + j * 0.05 });
      spots.push({ pts: br, t0: 0.3, t1: 0.9, side });
    });
    for (let i = 0; i < Math.min(30, extra.txs * 2); i++) { const s = spots[i % Math.max(1, spots.length)] ?? { pts: trunk, t0: 0.5, t1: 0.95 }; const t = s.t0 + r() * (s.t1 - s.t0), q = at(s.pts, t); els.push({ key: `tx:${i}`, type: "leaf", at: q, ang: angleAt(s.pts, t) + (i % 2 ? 1 : -1) * (0.7 + r() * 0.4), size: 7 + r() * 4, color: C.ink, order: 0.5 + r() * 0.4 }); }
    const crown = { x: top.x, y: top.y - 8 };
    for (let j = 0; j < 7; j++) els.push({ key: `crown:${j}`, type: "leaf", at: crown, ang: -1.5 + j * 0.5, size: 13 + r() * 5, color: C.blue, order: 0.8 + j * 0.02 });
  } else if (p.species === "spray") {
    const stem = shoot(0, 0, (r() - 0.5) * 0.3, H0, (r() - 0.5) * 0.6);
    curve("stem", stem, 1.6, 0); top = stem[stem.length - 1];
    spots.push({ pts: stem, t0: 0.15, t1: 0.88 });
    tools.forEach((_, i) => {
      const t = 0.3 + (0.55 * (i + 1)) / (tools.length + 1), side = i % 2 ? 1 : -1, o = at(stem, t);
      const br = shoot(o.x, o.y, angleAt(stem, t) + side * (0.55 + r() * 0.4), H0 * (0.3 + r() * 0.16), -side * (0.3 + r() * 0.4), 14);
      curve(`branch:${i}`, br, 1.2, 0.25 + t * 0.2);
      bloom(i, br[br.length - 1], angleAt(br, 1), 13 + r() * 5, "petal", 0.75);
      spots.push({ pts: br, t0: 0.35, t1: 0.85, side });
    });
    if (rec.paused) els.push({ key: "bud:top", type: "bud", at: top, ang: angleAt(stem, 1), size: 9, color: C.muted, order: 0.7 });
    else els.push({ key: "bloom:top", type: "bloom", at: top, ang: 0, size: 16, color: C.ink, order: 0.85, shape: "petal", tool: "" });
  } else if (p.species === "fern") {
    const k = Math.min(4, Math.max(2, tools.length + 1));
    for (let i = 0; i < k; i++) {
      const a = (i - (k - 1) / 2) * 0.42 + (r() - 0.5) * 0.18, side = a >= 0 ? 1 : -1;
      const fr = shoot((i - (k - 1) / 2) * 3, 0, a, H0 * (0.78 + r() * 0.34), side * (1.05 + r() * 0.5), 28);
      curve(`frond:${i}`, fr, 1.3, i * 0.06);
      spots.push({ pts: fr, t0: 0.12, t1: 0.82 });
      if (i < tools.length && tools[i]) bloom(i, fr[fr.length - 1], angleAt(fr, 1), 9 + r() * 3, "curl", 0.8);
      if (fr[fr.length - 1].y < top.y) top = fr[Math.round(fr.length * 0.55)];
    }
  } else if (p.species === "reeds") {
    tools.forEach((_, i) => {
      const off = i - (tools.length - 1) / 2;
      const st = shoot(off * 9, 0, off * 0.13 + (r() - 0.5) * 0.15, H0 * (0.82 + r() * 0.36), (r() - 0.5) * 0.35, 20);
      curve(`stem:${i}`, st, 1.3, i * 0.08);
      bloom(i, st[st.length - 1], angleAt(st, 1), 15 + r() * 4, "tulip", 0.7 + i * 0.05);
      if (st[st.length - 1].y < top.y) top = st[st.length - 1];
    });
    // a blade per run, two at least
    const blades = Math.max(4, runs);
    for (let i = 0; i < blades; i++) {
      const side = i % 2 ? 1 : -1, key = i < runs ? `run:${i}` : `blade:${i}`;
      curve(key, shoot(side * (2 + r() * 7), 0, side * (0.15 + r() * 0.6), H0 * (0.3 + r() * 0.32), side * (0.4 + r() * 0.6), 12), 1.1, 0.3 + (0.5 * i) / blades, i < runs && !rec.paused ? C.mint : ink);
    }
  } else if (p.species === "allium") {
    const stem = shoot(0, 0, (r() - 0.5) * 0.2, H0 * 1.05, (r() - 0.5) * 0.4, 24);
    curve("stem", stem, 1.4, 0); top = stem[stem.length - 1];
    const seeds = 8 + runs;
    for (let i = 0; i < seeds; i++) {
      const run = i >= 8;
      els.push({ key: run ? `run:${i - 8}` : `seed:${i}`, type: "seed", at: top, ang: (i / seeds) * Math.PI * 2 + r() * 0.2, size: 13 + r() * 6, color: run && !rec.paused ? C.mint : ink, order: 0.55 + (0.4 * i) / seeds });
    }
    tools.forEach((tool, i) => {
      if (!tool) return;
      const t = 0.3 + 0.18 * i, side = i % 2 ? 1 : -1, o = at(stem, t);
      const br = shoot(o.x, o.y, side * (0.35 + r() * 0.3), H0 * (0.34 + r() * 0.12), -side * 0.3, 12);
      curve(`branch:${i}`, br, 1.1, 0.3);
      bloom(i, br[br.length - 1], 0, 10 + r() * 3, "globe", 0.7);
    });
    for (let i = 0; i < 3; i++) els.push({ key: `rosette:${i}`, type: "leaf", at: { x: 0, y: 0 }, ang: (i - 1) * 0.95, size: 20 + r() * 8, color: ink, order: 0.1 });
  } else if (p.species === "shrub") {
    const trunk = shoot(0, 0, (r() - 0.5) * 0.5, H0 * 0.56, (r() - 0.5) * 1.3, 18);
    curve("trunk", trunk, 2.3, 0); top = trunk[trunk.length - 1];
    const centers: Pt[] = [{ x: top.x, y: top.y - 10 }];
    const limbs = 2 + Math.floor(r() * 2);
    for (let i = 0; i < limbs; i++) {
      const t = 0.5 + 0.42 * r(), side = i % 2 ? 1 : -1, o = at(trunk, t);
      const lb = shoot(o.x, o.y, side * (0.7 + r() * 0.5), H0 * (0.28 + r() * 0.12), -side * (0.6 + r() * 0.6), 12);
      curve(`limb:${i}`, lb, 1.6, 0.15 + i * 0.05);
      centers.push(lb[lb.length - 1]);
    }
    centers.forEach((c, i) => curve(`cloud:${i}`, Array.from({ length: 19 }, (_, j) => { const a = Math.PI * (0.95 + (j / 18) * 1.1); return { x: c.x + Math.cos(a) * 24, y: c.y + Math.sin(a) * 15 + 4 }; }), 0.9, 0.4 + i * 0.05));
    const leaves = Math.max(9, runs);
    for (let i = 0; i < leaves; i++) {
      const c = centers[i % centers.length], a = r() * Math.PI * 2, d = 6 + r() * 14, run = i < runs;
      els.push({ key: run ? `run:${i}` : `leaf:${i}`, type: "leaf", at: { x: c.x + Math.cos(a) * d, y: c.y + Math.sin(a) * d * 0.6 }, ang: Math.cos(a) * 1.2, size: 9 + r() * 4, color: run && !rec.paused ? C.mint : ink, order: 0.5 + (0.4 * i) / leaves });
    }
    tools.forEach((tool, i) => { if (tool) { const c = centers[(i + 1) % centers.length]; bloom(i, { x: c.x + (r() - 0.5) * 20, y: c.y - 6 - r() * 8 }, 0, 10 + r() * 3, "petal", 0.85); } });
  } else {
    const trunk = shoot(0, 0, (r() - 0.5) * 0.3, H0 * 0.72, (r() - 0.5) * 0.6, 18);
    curve("trunk", trunk, 2.2, 0); top = trunk[trunk.length - 1];
    const strands = Math.max(5, tools.length + 4);
    for (let i = 0; i < strands; i++) {
      const side = i % 2 ? 1 : -1, spread = 0.35 + (1.25 * Math.floor(i / 2)) / Math.ceil(strands / 2);
      const st = shoot(top.x, top.y, side * spread, H0 * (0.48 + r() * 0.26), side * (2.3 + r() * 0.6), 18);
      curve(`strand:${i}`, st, 1, 0.2 + i * 0.03);
      spots.push({ pts: st, t0: 0.25, t1: 0.95 });
      if (i < tools.length && tools[i]) bloom(i, st[st.length - 1], Math.PI, 9 + r() * 2, "petal", 0.8);
    }
  }

  // Leaves on the spots for species that carry their runs as leaves.
  if (p.species === "spray" || p.species === "fern" || p.species === "willow") {
    const n = Math.max(p.species === "fern" ? 10 : 6, runs);
    for (let i = 0; i < n; i++) {
      const s = spots[i % spots.length], t = s.t0 + ((s.t1 - s.t0) * (Math.floor(i / spots.length) + 0.5 + r() * 0.3)) / Math.ceil(n / spots.length);
      const q = at(s.pts, t), run = i < runs;
      const size = p.species === "fern" ? 8 + 6 * (1 - t) : p.species === "willow" ? 9 + r() * 3 : 11 + r() * 5;
      els.push({ key: run ? `run:${i}` : `leaf:${i}`, type: "leaf", at: q, ang: angleAt(s.pts, t) + (i % 2 ? 1 : -1) * (0.75 + r() * 0.4) * (p.species === "willow" ? 0.5 : 1), size, color: run && !rec.paused ? C.mint : ink, order: 0.35 + (0.5 * i) / n });
    }
  }
  // An amber berry per settled payment (up to ten), hung under the tool blossoms in turn.
  const toolBlooms = blooms.filter((x) => x.type === "bloom" && x.tool);
  for (let i = 0; i < Math.min(10, rec.pays); i++) {
    const b = toolBlooms[i % Math.max(1, toolBlooms.length)];
    const c = b && "at" in b ? b.at : top;
    els.push({ key: `pay:${i}`, type: "berry", at: { x: c.x + (r() - 0.5) * 22, y: c.y + 10 + r() * 12 }, r: 2.6 + r() * 1.2, color: C.amber, order: 0.92 + i * 0.01 });
  }
  // A coral leaf on the ground per failure (up to eight).
  for (let i = 0; i < Math.min(8, rec.fails); i++) {
    const side = i % 2 ? 1 : -1;
    els.push({ key: `fail:${i}`, type: "leaf", at: { x: side * (14 + r() * 26), y: -1 }, ang: side * (1.35 + r() * 0.3), size: 10 + r() * 3, color: C.coral, order: 0.95 });
  }

  // Append job shoots without changing the existing plant's species or geometry.
  for (const job of rec.jobs.filter((j) => j.state !== "cancelled").slice(0, 8)) {
    const jr = rng(hash(job.id)), side = jr() < .5 ? -1 : 1;
    const at = { x: top.x + side * (10 + jr() * 18), y: top.y + 18 + jr() * 22 };
    const color = job.state === "accepted" ? C.mint : job.funding === "unfunded" ? C.muted : C.blue;
    els.push({ key: `job-stem:${job.id}`, type: "curve", pts: [top, { x: at.x, y: at.y + 12 }, at], w: .8, color, order: .97 });
    if (job.funding === "funded" && Number(job.reward_paid) > 0) els.push({ key: `job-payment:${job.id}`, type: "berry", at: { x: at.x + 6, y: at.y + 7 }, r: 2.8, color: C.amber, order: 1 });
    els.push({ key: `job:${job.id}:${job.state}`, type: job.state === "accepted" ? "leaf" : "bud", at, ang: side * .35, size: 9, color, order: .99 });
  }
  let x0 = 0, x1 = 0, y0 = 0;
  for (const e of els) for (const q of e.type === "curve" ? e.pts : [e.at]) { x0 = Math.min(x0, q.x - 12); x1 = Math.max(x1, q.x + 12); y0 = Math.min(y0, q.y - 14); }
  return { els, height: -y0, bounds: { x0, x1, y0 }, perch: rec.recent && !rec.paused ? top : null };
}

export type GardenFocus = { id: string; n: number } | null;

export function Garden({ highlight, focus }: { highlight: string | null; focus: GardenFocus }) {
  const wrap = useRef<HTMLDivElement>(null), canvas = useRef<HTMLCanvasElement>(null), tip = useRef<HTMLDivElement>(null);
  const navigate = useNavigate();
  const props = useRef({ highlight, focus });
  const [summary, setSummary] = useState("");
  const [publicJobs, setPublicJobs] = useState<PublicJob[]>([]), [jobsError, setJobsError] = useState("");
  const api = useRef<{ focus: (id: string | null) => void; redraw: () => void } | null>(null);

  useEffect(() => { props.current.highlight = highlight; api.current?.redraw(); }, [highlight]);
  useEffect(() => { props.current.focus = focus; if (focus) api.current?.focus(focus.id); }, [focus]);

  useEffect(() => {
    const cv = canvas.current!, box = wrap.current!, main = cv.getContext("2d")!;
    let ctx = main, focused: string | null = null;
    // Rows from FAR back are drawn once into a cached layer after they finish growing: no sway, no glow.
    const FAR = 3;
    let far: HTMLCanvasElement | null = null, farAt = Infinity, farSig = "", glowOn = true;
    // Grown plants are drawn once into a sprite (at 1x, 2x or 4x for zoom) and swayed as a shear of that image.
    const sprites = new Map<string, { sig: string; c: HTMLCanvasElement; x0: number; y0: number; w: number; h: number }>();
    // ink: line weight relative to the plant's drawn size, so strokes stay fine when the camera zooms in
    let swayOff = false, spriteBudget = 0, ink = 1;
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    let W = 0, H = 0, raf = 0, visible = true, alive = true, first = true, hover: Plant | null = null, lastSeen = 0;
    let plants: Plant[] = [], decor: { x: number; y: number; s: number; kind: number; seed: number }[] = [];
    let jobs: PublicJob[] = [];
    let live: PublicAgent[] = [], records = new Map<string, AgentRecord>(), registry: RegistryData | null = null;
    const born = new Map<string, number>(), pulses = new Map<string, number>(), sparks: Spark[] = [];
    const cam = { x: 0, y: 0, z: 1, tx: 0, ty: 0, tz: 1 };
    const flies = Array.from({ length: 22 }, () => ({ x: Math.random(), y: 0.35 + Math.random() * 0.6, ph: Math.random() * 9, sp: 0.4 + Math.random() }));
    const now = () => performance.now();
    const horizon = () => H * 0.3, front = () => H * 0.86;
    const rowY = (k: number) => horizon() + (front() - horizon()) * depth(k) ** 1.25;
    const baseScale = () => (W < 640 ? 0.78 : Math.min(1.5, H / 420));

    const build = () => {
      const t = now();
      const reg = registry?.agents ?? [], regAddr = registry?.address ?? "";
      const liveKeys = new Set(live.map((a) => `${a.registry_address ?? ""}:${a.registry_id}`));
      const recent = Date.now() - 24 * 3600e3;
      type Seedling = Omit<Plant, "els" | "height" | "bounds" | "perch" | "base" | "scale" | "row" | "start" | "species"> & { activity: number };
      const seeds: Seedling[] = [];
      live.forEach((a) => {
        const r = records.get(a.id);
        seeds.push({
          key: a.id, name: a.name, href: `/agents/${a.id}`, seed: hash(a.id), activity: r?.last_event_id ?? 0,
          meta: `#${shortId(a.registry_id)} · ${a.status} · ${a.cadence}`,
          rec: {
            jobs: jobs.filter((j) => j.executor_id === a.id), runs: r?.runs ?? 0, fails: r?.failures ?? 0, pays: r?.payments ?? 0, paid: Number(r?.paid ?? 0),
            tools: a.tools, paused: a.status === "paused", young: !r,
            recent: !!a.last_run && new Date(a.last_run).getTime() > recent,
          },
        });
      });
      reg.filter((a) => !liveKeys.has(`${regAddr}:${a.id}`)).forEach((a) => seeds.push({
        key: `reg:${regAddr}:${a.id}`, name: a.name, seed: hash(`${regAddr}:${a.id}`), activity: -a.id,
        meta: `#${shortId(a.id)} · ${a.paused ? "paused" : "registered"} · ${a.purpose}`,
        rec: { jobs: [], runs: 0, fails: 0, pays: 0, paid: 0, tools: a.providers, paused: a.paused, young: true, recent: false },
      }));
      // the most recently active agents stand in front; the rest recede toward the horizon
      seeds.sort((a, b) => b.activity - a.activity || a.key.localeCompare(b.key));
      const hidden = Math.max(0, seeds.length - MAX_PLANTS);
      seeds.length = Math.min(seeds.length, MAX_PLANTS);
      // species: from the id, but no two neighbours alike while there are species to spare
      const used = new Set<string>();
      const withSpecies = seeds.map((s) => {
        let i = s.seed % SPECIES.length;
        for (let k = 0; k < SPECIES.length && used.has(SPECIES[i]); k++) i = (i + 1) % SPECIES.length;
        used.add(SPECIES[i]); if (used.size === SPECIES.length) used.clear();
        return { ...s, species: SPECIES[i] as Species };
      });
      // layout: the registry tree in the middle of the second row, agents filling the front row out from the centre
      const bs = baseScale(), cap0 = Math.max(2, Math.floor((W * 0.9) / (W < 640 ? 105 : 165)));
      const rows: (typeof withSpecies)[] = [];
      let rest = [...withSpecies], k = 0;
      while (rest.length) { const cap = Math.max(2, Math.floor(cap0 / depth(k))) - (k === 1 ? 1 : 0); rows.push(rest.slice(0, cap)); rest = rest.slice(cap); k++; }
      // an odd front row would put an agent in front of the tree: send its middle plant back beside it
      if (rows[0]?.length >= 3 && rows[0].length % 2 === 1) { const [mid] = rows[0].splice((rows[0].length - 1) / 2, 1); (rows[1] ??= []).unshift(mid); }
      const next: Plant[] = [];
      const placeRow = (list: typeof withSpecies, row: number) => {
        const n = list.length, y = rowY(row), sc = bs * depth(row);
        const span = Math.min(W * (W < 640 ? 0.48 : 0.8), (n + 1) * 190 * sc * 1.15);
        const order = list.map((_, i) => i).sort((a, b) => Math.abs(a - (n - 1) / 2) - Math.abs(b - (n - 1) / 2));
        order.forEach((idx, j) => {
          const s = list[idx], rr = rng(s.seed + 7);
          // the second row flanks the registry tree; other rows spread evenly
          let x = row === 1 ? W / 2 + (j % 2 ? -1 : 1) * ((W < 640 ? W * 0.36 : 160 * sc) + 190 * Math.floor(j / 2) * sc) : W / 2 + (n === 1 ? -W * 0.2 : (idx / (n - 1) - 0.5) * span);
          x += (rr() - 0.5) * 30 * sc;
          // the whole garden grows in within a few seconds, however many agents there are
          next.push({ ...s, ...grow(s), base: { x, y: y + (rr() - 0.5) * 10 * sc }, scale: sc * (0.92 + rr() * 0.16), row, start: t + 500 + Math.min(row, 6) * 300 + j * Math.min(260, 1600 / n) });
        });
      };
      rows.forEach(placeRow);
      const treeSeed = hash(regAddr || "registry");
      const tree: Plant = {
        key: "registry", name: "tab registry", species: "registry", seed: treeSeed,
        meta: registry?.address ? `${registry.address.slice(0, 6)}…${registry.address.slice(-4)} · ${withSpecies.length + hidden} agents · ${registry.transactions.length} transactions` : `${withSpecies.length + hidden} agents`,
        rec: { jobs: [], runs: 0, fails: 0, pays: 0, paid: 0, tools: [], paused: false, young: false, recent: false },
        els: [], height: 0, bounds: { x0: 0, x1: 0, y0: 0 }, perch: null,
        base: { x: W / 2, y: rowY(1) }, scale: bs * depth(1) * 1.3, row: 1, start: t + 150,
      };
      Object.assign(tree, grow(tree, { agents: withSpecies.slice(0, MAX_BRANCHES).map((s) => ({ key: s.key, color: s.rec.paused ? C.muted : s.rec.young ? C.blue : C.mint })), txs: registry?.transactions.length ?? 0 }));
      tree.scale = Math.min(tree.scale, (tree.base.y - 16) / tree.height);
      next.push(tree);
      next.sort((a, b) => a.base.y - b.base.y);
      // growth: unseen pieces unfurl; after the first load they spark as they arrive
      for (const p of next) {
        p.els.forEach((e, i) => {
          const id = `${p.key}|${e.key}`;
          if (born.has(id)) return;
          const when = first ? p.start + e.order * GROW : t + 200 + i * 40;
          born.set(id, reduced ? 0 : when);
          if (!first && !reduced) {
            const q = e.type === "curve" ? e.pts[e.pts.length - 1] : e.at, wx = p.base.x + q.x * p.scale, wy = p.base.y + q.y * p.scale;
            for (let j = 0; j < 7; j++) { const a = Math.random() * Math.PI * 2, v = 0.03 + Math.random() * 0.06; sparks.push({ x: wx, y: wy, vx: Math.cos(a) * v, vy: Math.sin(a) * v - 0.02, at: when, life: 900, color: e.color }); }
          }
        });
      }
      plants = next;
      // distant growth for depth: tufts and far shrubs, decoration only
      const dr = rng(42); decor = [];
      for (let i = 0; i < Math.round(W / 22); i++) { const row = Math.floor(dr() * 5) + (dr() < 0.35 ? 0 : 1); decor.push({ x: dr() * W, y: rowY(row) + (dr() - 0.5) * 18 * depth(row), s: bs * depth(row), kind: dr() < 0.8 ? 0 : 1, seed: Math.floor(dr() * 1e9) }); }
      decor.sort((a, b) => a.y - b.y);
      const total = withSpecies.length + hidden, runs = withSpecies.reduce((n, s) => n + s.rec.runs, 0);
      setSummary(`${total} agent${total === 1 ? "" : "s"} · ${runs} run${runs === 1 ? "" : "s"}${hidden ? ` · ${MAX_PLANTS} most active shown` : ""}`);
      // rebuild the far layer only when its plants moved or grew something new
      const farPlants = next.filter((p) => p.row >= FAR);
      const sig = farPlants.map((p) => `${p.key}:${Math.round(p.base.x)}:${Math.round(p.base.y)}:${p.els.length}:${p.rec.paused}`).join("|") + `|${W}x${H}`;
      if (sig !== farSig) {
        farSig = sig; far = null;
        farAt = farPlants.reduce((m, p) => p.els.reduce((mm, e) => Math.max(mm, born.get(`${p.key}|${e.key}`) ?? 0), m), 0) + 900;
      }
      first = false;
      if (reduced) draw();
    };

    const resize = () => {
      const r = box.getBoundingClientRect(), dpr = Math.min(2, devicePixelRatio || 1);
      W = r.width; H = r.height; cv.width = W * dpr; cv.height = H * dpr; ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      cam.x = cam.tx = W / 2; cam.y = cam.ty = H / 2;
      if (live.length || registry) build();
    };

    const load = async () => {
      try {
        // totals per agent, plus only the events since the last poll (for blossom pulses)
        const [a, r, e, g, j] = await Promise.all([
          request<PublicAgent[]>("/agents/live"),
          request<AgentRecord[]>("/agents/records").catch(() => [] as AgentRecord[]),
          request<AgentEvent[]>(`/activity?after=${lastSeen}&limit=100`),
          registry && !first ? Promise.resolve(registry) : request<RegistryData>("/registry").catch(() => null),
          jobAPI.public().catch(() => null),
        ]);
        if (!alive) return;
        if (j) { jobs = j; setPublicJobs(j); setJobsError(""); } else setJobsError("Job records are temporarily unavailable.");
        const fresh = lastSeen ? e : [];
        live = a; records = new Map(r.map((x) => [x.id, x])); registry = g; lastSeen = Math.max(lastSeen, ...r.map((x) => x.last_event_id), ...e.map((x) => x.id));
        build();
        for (const ev of fresh) if (ev.provider && ev.agent_id) pulses.set(`${ev.agent_id}|bloom:${ev.provider}`, now());
      } catch { /* keep the last garden */ }
    };

    // ---- drawing ----
    const sway = (p: Plant, t: number, y: number) => reduced || swayOff || p.row >= FAR ? 0 : Math.sin(t * 0.0011 + (p.seed % 1000)) * (p.rec.paused ? 1 : 3 + p.height / 70) * Math.max(0, -y / Math.max(60, p.height)) ** 1.5;
    const leafPath = (s: number) => { ctx.beginPath(); ctx.moveTo(0, 0); ctx.quadraticCurveTo(s * 0.42, -s * 0.45, 0, -s); ctx.quadraticCurveTo(-s * 0.42, -s * 0.45, 0, 0); };
    const drawEl = (p: Plant, e: El, t: number, dim: number) => {
      const b = born.get(`${p.key}|${e.key}`) ?? 0, g = clamp((t - b) / (e.type === "curve" ? 750 : 520));
      if (g <= 0) return;
      ctx.globalAlpha = dim;
      if (e.type === "curve") {
        const n = e.pts.length, upto = easeOut(g) * (n - 1);
        ctx.strokeStyle = e.color; ctx.lineWidth = e.w * ink; ctx.beginPath();
        for (let i = 0; i <= Math.ceil(upto); i++) {
          const q = e.pts[Math.min(i, n - 1)], f = i > upto ? upto - Math.floor(upto) : 1, pr = e.pts[Math.max(0, i - 1)];
          const x = pr.x + (q.x - pr.x) * f, y = pr.y + (q.y - pr.y) * f, xx = x + sway(p, t, y);
          if (i === 0) ctx.moveTo(xx, y); else ctx.lineTo(xx, y);
        }
        ctx.stroke(); return;
      }
      const s = e.type === "berry" ? easeOut(g) : easeBack(g);
      ctx.save(); ctx.translate(e.at.x + sway(p, t, e.at.y), e.at.y);
      if (e.type === "leaf") {
        ctx.rotate(e.ang); leafPath(e.size * s);
        ctx.fillStyle = e.color; ctx.globalAlpha = dim * 0.14; ctx.fill(); ctx.globalAlpha = dim;
        ctx.strokeStyle = e.color; ctx.lineWidth = ink; ctx.stroke();
        ctx.beginPath(); ctx.moveTo(0, 0); ctx.lineTo(0, -e.size * s * 0.8); ctx.globalAlpha = dim * 0.6; ctx.stroke();
      } else if (e.type === "seed") {
        ctx.rotate(e.ang); ctx.strokeStyle = e.color; ctx.lineWidth = 0.8 * ink; ctx.beginPath(); ctx.moveTo(0, 0); ctx.lineTo(0, -e.size * s); ctx.stroke();
        ctx.fillStyle = e.color; ctx.beginPath(); ctx.arc(0, -e.size * s, 1.5, 0, Math.PI * 2); ctx.fill();
      } else if (e.type === "berry") {
        if (glowOn) { const h = e.r * s * 4; ctx.drawImage(halo(e.color), -h, -h, h * 2, h * 2); }
        ctx.fillStyle = e.color; ctx.beginPath(); ctx.arc(0, 0, e.r * s, 0, Math.PI * 2); ctx.fill();
      } else if (e.type === "bud") {
        ctx.rotate(e.ang); ctx.strokeStyle = e.color; ctx.lineWidth = ink; ctx.beginPath(); ctx.ellipse(0, -e.size * 0.5 * s, e.size * 0.35 * s, e.size * 0.55 * s, 0, 0, Math.PI * 2); ctx.stroke();
      } else if (e.type === "bloom") {
        const pt = pulses.get(`${p.key}|${e.key}`), glow = pt ? clamp(1 - (t - pt) / 1600) : 0;
        if (glow <= 0 && pt) pulses.delete(`${p.key}|${e.key}`);
        ctx.strokeStyle = e.color; ctx.fillStyle = e.color; ctx.lineWidth = 1.1 * ink;
        const sz = e.size * s * (1 + glow * 0.25);
        if (glowOn || glow) { const h = sz * (1.25 + glow * 1.4); ctx.globalAlpha = dim * (0.75 + glow * 0.25); ctx.drawImage(halo(e.color), -h, -h - (e.shape === "tulip" ? sz * 0.5 : 0), h * 2, h * 2); ctx.globalAlpha = dim; }
        if (e.shape === "petal") {
          ctx.rotate(e.ang + (reduced ? 0 : t * 0.00006)); ctx.scale(sz / 30, sz / 30); ctx.lineWidth = (1.1 * ink) / (sz / 30);
          for (let i = 0; i < 5; i++) { ctx.rotate((Math.PI * 2) / 5); ctx.stroke(PETAL); }
          ctx.beginPath(); ctx.arc(0, 0, 3.6, 0, Math.PI * 2); ctx.fill();
        } else if (e.shape === "tulip") {
          ctx.rotate(e.ang); ctx.beginPath(); ctx.moveTo(-sz * 0.45, -sz * 0.15);
          ctx.quadraticCurveTo(-sz * 0.6, -sz * 0.95, -sz * 0.18, -sz * 1.05); ctx.lineTo(0, -sz * 0.7); ctx.lineTo(sz * 0.18, -sz * 1.05);
          ctx.quadraticCurveTo(sz * 0.6, -sz * 0.95, sz * 0.45, -sz * 0.15); ctx.quadraticCurveTo(0, sz * 0.22, -sz * 0.45, -sz * 0.15);
          ctx.globalAlpha = dim * 0.18; ctx.fill(); ctx.globalAlpha = dim; ctx.stroke();
          ctx.beginPath(); ctx.moveTo(0, 0); ctx.quadraticCurveTo(sz * 0.1, -sz * 0.5, 0, -sz * 0.7); ctx.stroke();
        } else if (e.shape === "globe") {
          for (let i = 0; i < 16; i++) { const a = i * 2.39996, d = Math.sqrt(i / 16) * sz * 0.75; ctx.beginPath(); ctx.arc(Math.cos(a) * d, Math.sin(a) * d - sz * 0.2, 1.4, 0, Math.PI * 2); ctx.fill(); }
        } else {
          ctx.rotate(e.ang); ctx.beginPath();
          for (let i = 0; i <= 40; i++) { const a = i * 0.32, rr = sz * 0.7 * (1 - i / 46); const x = Math.cos(a) * rr - sz * 0.7, y = Math.sin(a) * rr; if (i) ctx.lineTo(x, y); else ctx.moveTo(x, y); }
          ctx.stroke();
        }
      }
      ctx.restore();
    };
    const drawPlant = (p: Plant, t: number, caching = false) => {
      const hl = props.current.highlight, lit = !caching && (hl === p.key || hover === p || focused === p.key), dimmed = !caching && (hl || hover || focused) && !lit;
      const fog = p.row === 0 ? 1 : Math.max(0.28, 0.86 - p.row * 0.1);
      glowOn = p.row < FAR || lit; ink = Math.min(1, 1.8 / (p.scale * cam.z));
      // while a plant is focused, whatever stands in front of it fades further so it reads clearly
      const fp = focused ? plants.find((x) => x.key === focused) : null, veil = dimmed ? (fp && p.base.y > fp.base.y ? 0.16 : 0.5) : 1;
      const dim = fog * veil;
      ctx.save(); ctx.translate(p.base.x, p.base.y); ctx.scale(p.scale, p.scale); ctx.lineCap = "round"; ctx.lineJoin = "round";
      const sprite = caching || lit || reduced ? null : spriteFor(p, t, fog);
      if (sprite) {
        ctx.save(); ctx.transform(1, 0, -sway(p, t, -p.height) / Math.max(60, p.height), 1, 0, 0);
        ctx.globalAlpha = veil; ctx.drawImage(sprite.c, sprite.x0, sprite.y0, sprite.w, sprite.h); ctx.restore();
      } else {
        if (lit) {
          const g = 0.6 + 0.4 * Math.sin(t * 0.005);
          ctx.save(); ctx.strokeStyle = C.mint; ctx.shadowColor = C.mint; ctx.shadowBlur = 16; ctx.globalAlpha = 0.7 * g; ctx.lineWidth = 1.4;
          ctx.beginPath(); ctx.ellipse(0, 2, Math.max(40, (p.bounds.x1 - p.bounds.x0) * 0.42), 9, 0, 0, Math.PI * 2); ctx.stroke(); ctx.restore();
        }
        // ground shadow
        ctx.save(); ctx.globalAlpha = 0.35 * fog; ctx.fillStyle = "#06101a"; ctx.beginPath(); ctx.ellipse(0, 2, 34, 6, 0, 0, Math.PI * 2); ctx.fill(); ctx.restore();
        for (const e of p.els) drawEl(p, e, t, dim);
      }
      if (p.perch) {
        const b = born.get(`${p.key}|perch`) ?? (born.set(`${p.key}|perch`, p.start + GROW + 400), p.start + GROW + 400), g = clamp((t - b) / 600);
        if (g > 0) {
          ctx.save(); ctx.globalAlpha = dim * g; ctx.translate(p.perch.x + sway(p, t, p.perch.y), p.perch.y - (1 - easeOut(g)) * 30);
          const s = 0.85 / Math.max(0.6, p.scale) * 0.9; ctx.scale(s * (Math.sin(t * 0.0003 + p.seed) > 0.6 ? -1 : 1), s); ctx.translate(-20, -35);
          ctx.strokeStyle = C.ink; ctx.lineWidth = (1.2 * ink) / s; ctx.stroke(PERCH);
          if ((t + p.seed) % 4200 > 140) { ctx.fillStyle = C.ink; ctx.beginPath(); ctx.arc(24, 15, 0.9, 0, Math.PI * 2); ctx.fill(); }
          ctx.restore();
        }
      }
      if (p.row <= 1) {
        ctx.globalAlpha = (lit ? 1 : 0.7) * fog; ctx.fillStyle = lit ? C.ink : C.muted; ctx.textAlign = "center";
        ctx.font = `${11 / p.scale}px 'IBM Plex Mono', monospace`; ctx.fillText(p.name, 0, (p.species === "registry" ? 34 : 22) / p.scale + 6);
      }
      ctx.restore();
    };
    const drawDecor = (d: (typeof decor)[number], t: number) => {
      const r = rng(d.seed), fog = Math.max(0.12, 0.5 * d.s);
      ctx.save(); ctx.translate(d.x, d.y); ctx.scale(d.s, d.s); ctx.strokeStyle = C.stem; ctx.globalAlpha = fog; ctx.lineWidth = 1 / Math.max(0.5, d.s); ctx.lineCap = "round";
      const blades = d.kind ? 1 : 3 + Math.floor(r() * 3), sw = reduced ? 0 : Math.sin(t * 0.0012 + d.seed) * 2;
      for (let i = 0; i < blades; i++) {
        const pts = d.kind ? shoot(0, 0, (r() - 0.5) * 0.3, 50 + r() * 30, (r() - 0.5) * 0.8, 10) : shoot((i - blades / 2) * 2, 0, (r() - 0.5) * 1.1, 12 + r() * 14, (r() - 0.5) * 1.2, 6);
        ctx.beginPath(); pts.forEach((q, j) => { const x = q.x + sw * (-q.y / 60); if (j) ctx.lineTo(x, q.y); else ctx.moveTo(x, q.y); }); ctx.stroke();
        if (d.kind) for (let j = 0; j < 4; j++) { const q = pts[3 + j * 2]; ctx.save(); ctx.translate(q.x + sw * (-q.y / 60), q.y); ctx.rotate((j % 2 ? 1 : -1) * 0.9); leafPath(8); ctx.stroke(); ctx.restore(); }
      }
      ctx.restore();
    };
    const draw = () => {
      const t = reduced ? 1e12 : now();
      spriteBudget = 16;
      if (!reduced) { cam.x += (cam.tx - cam.x) * 0.06; cam.y += (cam.ty - cam.y) * 0.06; cam.z += (cam.tz - cam.z) * 0.06; }
      else { cam.x = cam.tx; cam.y = cam.ty; cam.z = cam.tz; }
      ctx.setTransform(1, 0, 0, 1, 0, 0); const dpr = cv.width / Math.max(1, W); ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, W, H);
      // sky and mist
      const hz = horizon();
      const sky = ctx.createLinearGradient(0, 0, 0, H); sky.addColorStop(0, "#0a1520"); sky.addColorStop(hz / H, "#10233300"); sky.addColorStop(Math.min(1, hz / H + 0.02), "#1324330d"); sky.addColorStop(1, "#08121c");
      ctx.fillStyle = sky; ctx.fillRect(0, 0, W, H);
      const sr = rng(7);
      for (let i = 0; i < Math.round(W / 9); i++) {
        const x = sr() * W, y = sr() * hz * 0.95, tw = reduced ? 0.5 : 0.35 + 0.65 * Math.max(0, Math.sin(t * 0.0009 * (0.4 + sr()) + i));
        ctx.globalAlpha = (0.15 + sr() * 0.35) * tw; ctx.fillStyle = sr() < 0.2 ? C.mint : C.blue; ctx.fillRect(x, y, sr() < 0.12 ? 1.6 : 1, sr() < 0.12 ? 1.6 : 1);
      }
      ctx.save(); ctx.globalAlpha = 0.55; ctx.strokeStyle = C.ink; ctx.lineWidth = 1; ctx.shadowColor = C.blue; ctx.shadowBlur = 18;
      const mx = W * 0.82, my = hz * 0.42, mr = Math.min(22, H * 0.04);
      ctx.beginPath(); ctx.arc(mx, my, mr, Math.PI * 0.35, Math.PI * 1.65); ctx.quadraticCurveTo(mx - mr * 0.2, my, mx + mr * Math.cos(Math.PI * 0.35), my + mr * Math.sin(Math.PI * 0.35)); ctx.stroke(); ctx.restore();
      ctx.globalAlpha = 1;
      ctx.save(); ctx.translate(W / 2, H / 2); ctx.scale(cam.z, cam.z); ctx.translate(-cam.x, -cam.y);
      // far hills
      ctx.strokeStyle = C.stem; ctx.lineWidth = 1; ctx.globalAlpha = 0.18; ctx.beginPath();
      for (let x = -W; x <= W * 2; x += 12) { const y = hz - 10 - Math.sin(x * 0.006) * 14 - Math.sin(x * 0.017 + 1) * 6; if (x === -W) ctx.moveTo(x, y); else ctx.lineTo(x, y); }
      ctx.stroke();
      ctx.globalAlpha = 0.1; ctx.beginPath(); ctx.moveTo(-W, hz + 4); ctx.lineTo(W * 2, hz + 4); ctx.stroke(); ctx.globalAlpha = 1;
      // Delegation roots connect the existing plants in their original positions.
      const byPlant = new Map(plants.map((p) => [p.key, p]));
      const byJob = new Map(jobs.map((j) => [j.id, j]));
      for (const job of jobs.filter((j) => j.parent_id && j.state !== "cancelled").slice(0, 80)) {
        const parent = byJob.get(job.parent_id!);
        const source = parent && byPlant.get(parent.executor_id), target = byPlant.get(job.executor_id);
        if (!source || !target || source === target) continue;
        ctx.save(); ctx.globalAlpha = .3; ctx.strokeStyle = job.funding === "unfunded" ? C.muted : C.mint; ctx.lineWidth = .8;
        if (job.funding === "unfunded") ctx.setLineDash([3, 5]);
        ctx.beginPath(); ctx.moveTo(source.base.x, source.base.y + 4);
        ctx.bezierCurveTo(source.base.x, source.base.y + 32, target.base.x, target.base.y + 32, target.base.x, target.base.y + 4);
        ctx.stroke(); ctx.restore();
      }
      // the far layer, once everything in it has grown; skipped while the camera is away from home
      const split = rowY(FAR - 0.5), home = Math.abs(cam.z - 1) < 0.002 && Math.abs(cam.x - W / 2) < 0.5 && Math.abs(cam.y - H / 2) < 0.5;
      if (!far && !reduced && t > farAt) far = renderFar(t, split);
      const cached = !!far && home;
      if (cached) { ctx.globalAlpha = props.current.highlight || hover || focused ? 0.5 : 1; ctx.drawImage(far!, 0, 0, W, H); ctx.globalAlpha = 1; }
      const vx0 = cam.x - W / 2 / cam.z, vx1 = cam.x + W / 2 / cam.z, vy0 = cam.y - H / 2 / cam.z, vy1 = cam.y + H / 2 / cam.z;
      // a highlighted or focused plant is drawn last, over whatever stands in front of it
      const raised = (p: Plant) => props.current.highlight === p.key || hover === p || focused === p.key;
      const onTop: Plant[] = [];
      let di = 0;
      for (const p of plants) {
        while (di < decor.length && decor[di].y < p.base.y) { const d = decor[di++]; if (!(cached && d.y < split)) drawDecor(d, t); }
        if (raised(p)) { onTop.push(p); continue; }
        if (cached && p.row >= FAR) continue;
        if (p.base.x + p.bounds.x1 * p.scale < vx0 || p.base.x + p.bounds.x0 * p.scale > vx1 || p.base.y + p.bounds.y0 * p.scale > vy1 || p.base.y + 24 * p.scale < vy0) continue;
        drawPlant(p, t);
      }
      while (di < decor.length) { const d = decor[di++]; if (!(cached && d.y < split)) drawDecor(d, t); }
      for (const p of onTop) drawPlant(p, t);
      // sparks
      for (let i = sparks.length - 1; i >= 0; i--) {
        const s = sparks[i], age = t - s.at; if (age < 0) continue; if (age > s.life) { sparks.splice(i, 1); continue; }
        const k = age / s.life; ctx.globalAlpha = 1 - k; ctx.fillStyle = s.color; ctx.shadowColor = s.color; ctx.shadowBlur = 8;
        ctx.beginPath(); ctx.arc(s.x + s.vx * age, s.y + s.vy * age + 0.00003 * age * age, 1.6, 0, Math.PI * 2); ctx.fill();
      }
      ctx.shadowBlur = 0;
      // fireflies
      if (!reduced) for (const f of flies) {
        const x = ((f.x + Math.sin(t * 0.00008 * f.sp + f.ph) * 0.08) % 1) * W, y = f.y * H + Math.sin(t * 0.0005 * f.sp + f.ph) * 14;
        const a = Math.max(0, Math.sin(t * 0.0013 * f.sp + f.ph * 3)) ** 3;
        ctx.globalAlpha = a * 0.75; ctx.drawImage(halo(C.mint), x - 8, y - 8, 16, 16); ctx.fillStyle = C.mint; ctx.beginPath(); ctx.arc(x, y, 1.4, 0, Math.PI * 2); ctx.fill();
      }
      ctx.restore(); ctx.globalAlpha = 1; ctx.shadowBlur = 0;
    };
    const spriteFor = (p: Plant, t: number, fog: number) => {
      if (p.settle === undefined) p.settle = p.els.reduce((m, e) => Math.max(m, born.get(`${p.key}|${e.key}`) ?? 0), 0) + 800;
      if (t < p.settle || [...pulses.keys()].some((k) => k.startsWith(`${p.key}|`))) return null;
      const bucket = cam.tz > 4.5 ? 8 : cam.tz > 2.5 ? 4 : cam.tz > 1.2 ? 2 : 1, id = `${p.key}|${bucket}`, sig = `${p.els.length}:${p.scale.toFixed(4)}:${fog}:${p.rec.paused}`;
      const have = sprites.get(id);
      if (have && have.sig === sig) return have;
      if (spriteBudget-- <= 0) return null;
      const x0 = Math.min(p.bounds.x0, -40), x1 = Math.max(p.bounds.x1, 40), y0 = p.bounds.y0 - 6, y1 = 14, res = (cv.width / Math.max(1, W)) * bucket * p.scale;
      const c = document.createElement("canvas"); c.width = Math.ceil((x1 - x0) * res); c.height = Math.ceil((y1 - y0) * res);
      ctx = c.getContext("2d")!; ctx.setTransform(res, 0, 0, res, -x0 * res, -y0 * res); ctx.lineCap = "round"; ctx.lineJoin = "round"; swayOff = true;
      try {
        ctx.save(); ctx.globalAlpha = 0.35 * fog; ctx.fillStyle = "#06101a"; ctx.beginPath(); ctx.ellipse(0, 2, 34, 6, 0, 0, Math.PI * 2); ctx.fill(); ctx.restore();
        glowOn = p.row < FAR; ink = Math.min(1, 1.8 / (p.scale * bucket));
        for (const e of p.els) drawEl(p, e, t, fog);
      } finally { ctx = main; swayOff = false; ink = Math.min(1, 1.8 / (p.scale * cam.z)); }
      if (sprites.size > 3000) sprites.clear();
      const sp = { sig, c, x0, y0, w: x1 - x0, h: y1 - y0 };
      sprites.set(id, sp);
      return sp;
    };
    const renderFar = (t: number, split: number) => {
      const layer = document.createElement("canvas"), dpr = cv.width / Math.max(1, W);
      layer.width = cv.width; layer.height = cv.height;
      ctx = layer.getContext("2d")!; ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      try {
        let di = 0;
        for (const p of plants) {
          if (p.row < FAR) continue;
          while (di < decor.length && decor[di].y < p.base.y && decor[di].y < split) drawDecor(decor[di++], t);
          drawPlant(p, t, true);
        }
        while (di < decor.length && decor[di].y < split) drawDecor(decor[di++], t);
      } finally { ctx = main; }
      return layer;
    };
    const tick = () => { raf = requestAnimationFrame(tick); if (visible && !document.hidden) draw(); };

    // ---- interaction ----
    const toWorld = (sx: number, sy: number) => ({ x: (sx - W / 2) / cam.z + cam.x, y: (sy - H / 2) / cam.z + cam.y });
    const pick = (e: PointerEvent | MouseEvent) => {
      const r = cv.getBoundingClientRect(), w = toWorld(e.clientX - r.left, e.clientY - r.top);
      let best: Plant | null = null;
      for (const p of plants) {
        const x0 = p.base.x + p.bounds.x0 * p.scale, x1 = p.base.x + p.bounds.x1 * p.scale, y0 = p.base.y + p.bounds.y0 * p.scale;
        if (w.x >= x0 && w.x <= x1 && w.y >= y0 && w.y <= p.base.y + 24 * p.scale) if (!best || p.base.y > best.base.y) best = p;
      }
      return best;
    };
    const showTip = (p: Plant | null, sx = 0, sy = 0) => {
      const el = tip.current!;
      if (!p) { el.hidden = true; return; }
      el.hidden = false; el.innerHTML = "";
      const add = (tag: string, text: string) => { const n = document.createElement(tag); n.textContent = text; el.append(n); };
      add("strong", p.name); add("span", p.meta);
      if (p.species === "registry") add("span", `a branch for each of the ${MAX_BRANCHES} most active agents`);
      else {
        const { paid, fails } = p.rec;
        add("span", `${p.rec.runs} run${p.rec.runs === 1 ? "" : "s"} · ${p.rec.tools.join(" · ") || "no tools"}`);
        if (paid || fails) add("span", `${paid ? `${paid.toFixed(2)} USDT paid` : ""}${paid && fails ? " · " : ""}${fails ? `${fails} failure${fails === 1 ? "" : "s"}` : ""}`);
        if (p.rec.jobs.length) add("span", `${p.rec.jobs.filter((j) => j.state !== "accepted" && j.state !== "cancelled").length} active job agreements · ${p.rec.jobs.filter((j) => j.state === "accepted").length} accepted`);
        add("em", `grown as ${SPECIES_NAME[p.species]}${p.rec.young ? ", still young" : ""}`);
      }
      el.style.left = `${Math.min(W - 230, Math.max(8, sx + 14))}px`; el.style.top = `${Math.max(8, sy - 20)}px`;
    };
    const onMove = (e: PointerEvent) => {
      const p = pick(e), r = cv.getBoundingClientRect();
      if (p !== hover) { hover = p; if (reduced) draw(); }
      cv.style.cursor = p ? (p.href ? "pointer" : "zoom-in") : cam.tz > 1 ? "zoom-out" : "default";
      showTip(p, e.clientX - r.left, e.clientY - r.top);
    };
    const onLeave = () => { hover = null; showTip(null); if (reduced) draw(); };
    const focusOn = (key: string | null) => {
      const p = key ? plants.find((x) => x.key === key) : null;
      focused = p?.key ?? null;
      if (!p) { cam.tx = W / 2; cam.ty = H / 2; cam.tz = 1; }
      else { cam.tz = clamp((H * 0.5) / (p.height * p.scale), 1.5, 8); cam.tx = p.base.x; cam.ty = p.base.y - (p.height * p.scale) / 2; }
      if (reduced) draw();
    };
    const onClick = (e: MouseEvent) => {
      const p = pick(e);
      if (!p) { focusOn(null); return; }
      if (cam.tz > 1 && p.href && Math.abs(cam.tx - p.base.x) < 2) { navigate(p.href); return; }
      focusOn(p.key);
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") focusOn(null); };

    api.current = { focus: focusOn, redraw: () => { if (reduced) draw(); } };
    const ro = new ResizeObserver(resize); ro.observe(box);
    const io = new IntersectionObserver(([en]) => { visible = en.isIntersecting; }); io.observe(box);
    cv.addEventListener("pointermove", onMove); cv.addEventListener("pointerleave", onLeave); cv.addEventListener("click", onClick); addEventListener("keydown", onKey);
    resize();
    void load();
    const poll = setInterval(() => { if (!document.hidden) void load(); }, 6000);
    const regPoll = setInterval(() => { if (!document.hidden) void request<RegistryData>("/registry").then((g) => { registry = g; }).catch(() => {}); }, 60000);
    if (!reduced) raf = requestAnimationFrame(tick);
    return () => { alive = false; cancelAnimationFrame(raf); clearInterval(poll); clearInterval(regPoll); ro.disconnect(); io.disconnect(); cv.removeEventListener("pointermove", onMove); cv.removeEventListener("pointerleave", onLeave); cv.removeEventListener("click", onClick); removeEventListener("keydown", onKey); };
  }, [navigate]);

  return (
    <section className="panel garden-panel">
      <div className="panel-heading">
        <div>
          <h2>the garden</h2>
          <p className="garden-sub">A plant per agent, grown from its record. The registry tree stands in the middle.</p>
        </div>
        <span className="garden-count">{summary || "—"}</span>
      </div>
      <div className="garden-stage" ref={wrap}>
        <canvas ref={canvas} role="img" aria-label="A garden of plants, one per agent, grown from each agent's runs, tools, payments, failures, job agreements and delegation roots" />
        <div className="garden-tip" ref={tip} hidden />
      </div>
      <div className="garden-legend" aria-hidden="true">
        <span><i className="leaf" />leaf or seed · completed run</span>
        <span><i className="bloom" />blossom · tool</span>
        <span><i className="berry" />berry · settled payment</span>
        <span><i className="fail" />fallen leaf · failure</span>
        <span><i className="bird" />bird · ran in the last day</span>
        <span><i className="job" />bud · job agreement</span>
        <span><i className="accepted" />mint shoot · accepted job</span>
        <span><i className="root" />root · delegation (dashed when unfunded)</span>
        <span className="garden-hint">click a plant to look closer · esc to step back</span>
      </div>
      <GardenJobs jobs={publicJobs} error={jobsError} />
    </section>
  );
}
