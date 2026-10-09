import { ProtocolGuide } from "./components/ProtocolGuide";
import {
  useEffect,
  useRef,
  useState,
  createContext,
  useContext,
  lazy,
  Suspense,
} from "react";
import type { ReactNode } from "react";
import {
  BrowserRouter,
  NavLink,
  Link,
  Routes,
  Route,
  useParams,
  useLocation,
} from "react-router-dom";
import {
  ArrowUpRight,
  ArrowRight,
  Plus,
  Search,
  ShieldCheck,
  ChevronRight,
  X,
  Check,
  ExternalLink,
  Copy,
  UserRound,
} from "lucide-react";
import { request, money } from "./lib/api";
import { ChainContext, BNB_CHAIN_ID, USDT_ADDRESS } from "./lib/evm";
import type { BackingAsset, Config, PublicData, Provider, Receipt } from "./lib/api";
import { Bird, BirdPair, Sprout, AgentGlyph } from "./components/Drawings";
import { Chart } from "./components/Chart";
import { AgentsTable } from "./components/Agents";
import { LiveAgents, LiveAgentProfile } from "./components/LiveAgents";
import { LiveActivity } from "./components/LiveActivity";
import { WorkRecord } from "./components/WorkRecord";
import { Flock } from "./components/Flock";
import { Garden } from "./components/Garden";
import type { GardenFocus } from "./components/Garden";
import { Ledger } from "./components/Ledger";
import { Registry } from "./components/Registry";
import { RouteReveal } from "./components/RouteReveal";
import { Landing } from "./components/Landing";
import { Docs } from "./components/Docs";
import { Tools } from "./components/Tools";
import { Finance } from "./components/Finance";
import { BackingOverview } from "./components/BackingOverview";
import { PublicJobs, Operators, PublicRunReceipt } from "./components/PublicWork";
import "./components/action-colors.css";
const Account = lazy(() => import("./components/Account"));
const Context = createContext<{
  data: PublicData;
  providers: Provider[];
  mode: "example" | "public";
  setMode: (m: "example" | "public") => void;
  config: Config;
  onReceipt: (r: Receipt) => void;
} | null>(null);
function useData() {
  const ctx = useContext(Context);
  if (!ctx) throw new Error("Missing app context");
  return ctx;
}
export function useConfig() {
  return useData().config;
}
const empty: PublicData = {
  mode: "public",
  summary: { credit_limit: 0, spent: 0, repaid: 0, outstanding: 0, agents: 0 },
  agents: [],
  receipts: [],
  series: [],
};
function Header() {
  return (
    <header>
      <div className="nav-shell">
        <Link className="brand" to="/">
          <Sprout />
          tab
        </Link>
        <nav aria-label="Main navigation">
          {[
            ["/", "home"],
            ["/agents", "garden"],
            ["/jobs", "jobs"],
            ["/backing", "backing"],
            ["/finance", "finance"],
            ["/providers", "tools"],
            ["/activity", "log"],
            ["/docs", "docs"],
          ].map(([url, name]) => (
            <NavLink end={url === "/"} key={url} to={url}>
              {name}
            </NavLink>
          ))}
        </nav>
        <div className="header-actions">
          <a className="header-x-link" href="https://x.com/tabx402" target="_blank" rel="noopener noreferrer" aria-label="Tab on X (opens in a new tab)">
            <img src="/images/x-logo.svg" width={17} height={17} alt="" aria-hidden="true" />
          </a>
        <Link className="account-link" to="/account">
          <UserRound size={15} />
          <span>my account</span>
          <ArrowUpRight size={14} />
        </Link>
        </div>
      </div>
    </header>
  );
}
function ModeControl() {
  const { mode, setMode } = useData();
  return (
    <div className="mode-control">
      <button
        onClick={() => setMode(mode === "example" ? "public" : "example")}
        className="mode-toggle"
        aria-label={`Switch to ${mode === "example" ? "public" : "example"} data`}
      >
        <span className={mode === "example" ? "active" : ""}>example</span>
        <span className={mode === "public" ? "active" : ""}>public</span>
      </button>
      <span className="mode-note">
        <i />
        {mode === "example"
          ? "explore an example garden"
          : "public agents and work"}
      </span>
    </div>
  );
}
function PageIntro({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children?: ReactNode;
}) {
  return (
    <section className="page-intro">
      <div>
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {children}
    </section>
  );
}
function Metrics() {
  const { data } = useData();
  const s = data.summary;
  return (
    <div className="metrics">
      {[
        ["backed credit", s.credit_limit, "backer-set limits"],
        ["provider spend", s.spent, "inference + data"],
        ["repaid", s.repaid, "principal returned"],
        ["outstanding", s.outstanding, `${s.agents} backed agents`],
      ].map(([name, value, detail], i) => (
        <div className="metric" key={String(name)}>
          <span>{name}</span>
          <div className={i === 2 ? "mint" : ""}>
            {money(Number(value))}
            <small>USDT</small>
          </div>
          <small>
            <i className={i === 2 ? "mint-dot" : "blue-dot"} />
            {detail}
          </small>
        </div>
      ))}
    </div>
  );
}
function CreditLoop() {
  return (
    <aside className="panel loop-panel">
      <span className="eyebrow">how credit works</span>
      <h2>
        set a limit.
        <br />
        review the spending.
      </h2>
      <div className="flow-list">
        {[
          ["01", "a backer sets the limit", "USDT committed to one agent."],
          [
            "02",
            "the agent uses a provider",
            "Only the services on its allowlist.",
          ],
          [
            "03",
            "a receipt records the spend",
            "Provider, amount, and settlement.",
          ],
          [
            "04",
            "repayment builds the record",
            "The backer reviews the next limit.",
          ],
        ].map(([n, t, d]) => (
          <div key={n}>
            <span className="flow-number">{n}</span>
            <div>
              <strong>{t}</strong>
              <p>{d}</p>
            </div>
          </div>
        ))}
      </div>
      <Link to="/protocol" className="text-link">
        read the mechanics <ArrowUpRight size={14} />
      </Link>
    </aside>
  );
}
function Overview() {
  return (
    <>
      <section className="hero live-hero">
        <div>
          <h1>
            your agents.
            <br />
            <span>their activity.</span>
          </h1>
          <p>
            Create an agent, connect its tools, and follow its runs and
            payments.
          </p>
          <div className="hero-actions">
            <Link to="/account" className="primary">
              create an agent <ArrowUpRight size={16} />
            </Link>
            <Link to="/activity" className="text-link">
              open the log <ArrowRight size={15} />
            </Link>
          </div>
        </div>
        <div className="hero-art">
          <Bird />
        </div>
      </section>
      <Flock />
      <LiveActivity compact />
      <section className="overview-actions">
        <Link className="panel" to="/account">
          <span>01</span>
          <h3>your workspace</h3>
          <p>Create and manage agents.</p>
          <ArrowUpRight size={18} />
        </Link>
        <Link className="panel" to="/providers">
          <span>02</span>
          <h3>tools and services</h3>
          <p>See available data and APIs.</p>
          <ArrowUpRight size={18} />
        </Link>
        <Link className="panel" to="/activity">
          <span>03</span>
          <h3>the full record</h3>
          <p>Follow runs and settled payments.</p>
          <ArrowUpRight size={18} />
        </Link>
      </section>
    </>
  );
}

function Agents() {
  const [highlight, setHighlight] = useState<string | null>(null);
  const [focus, setFocus] = useState<GardenFocus>(null);
  const focusPlant = (id: string) => {
    setFocus({ id, n: Date.now() });
    document.querySelector(".garden-stage")?.scrollIntoView({ behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth", block: "center" });
  };
  return (
    <>
      <PageIntro
        title="agents."
        description="See what each agent runs and which tools it uses."
      />
      <Garden highlight={highlight} focus={focus} />
      <LiveAgents onHover={setHighlight} onFocus={focusPlant} />
      <Registry onHover={setHighlight} onFocus={focusPlant} />
    </>
  );
}
function AgentProfile() {
  const { id } = useParams();
  return id && /^[a-f0-9]{32}$/.test(id) ? (
    <LiveAgentProfile />
  ) : (
    <AgentDetail />
  );
}
function AgentDetail() {
  const { id } = useParams();
  const { data, providers, onReceipt } = useData();
  const agent = data.agents.find((a) => a.id === id);
  if (!agent)
    return (
      <PageIntro
        title="agent unavailable."
        description="This agent belongs to the example network. Switch to example data to view its record."
      >
        <ModeControl />
      </PageIntro>
    );
  return (
    <>
      <Link className="back-link" to="/agents">
        ← all agents
      </Link>
      <PageIntro
        title={agent.name}
        description={agent.purpose}
      >
        <span className="status-chip">{agent.status}</span>
      </PageIntro>
      <div className="detail-grid">
        <section className="panel">
          <AgentGlyph index={data.agents.indexOf(agent)} />
          <h2>credit line</h2>
          <div className="big-number">
            {money(agent.limit)} <small>USDT</small>
          </div>
          <div className="mini-bar">
            <i
              style={{ width: `${(agent.outstanding / agent.limit) * 100}%` }}
            />
          </div>
          <div className="detail-pairs">
            <p>
              outstanding <strong>{money(agent.outstanding)} USDT</strong>
            </p>
            <p>
              available{" "}
              <strong>{money(agent.limit - agent.outstanding)} USDT</strong>
            </p>
            <p>
              backer <strong>{agent.backer}</strong>
            </p>
            <p>
              repaid{" "}
              <strong className="mint">{money(agent.repaid)} USDT</strong>
            </p>
          </div>
        </section>
        <section className="panel">
          <span className="eyebrow">spend policy · example</span>
          <h2>where this agent can spend</h2>
          {agent.providers.map((id) => {
            const p = providers.find((p) => p.id === id);
            return (
              <div className="policy-row" key={id}>
                <Check size={16} />
                <span>{p?.name ?? id}</span>
                <small>{p?.category}</small>
              </div>
            );
          })}
          <p className="muted">
            A real credit line requires backer consent, a funded limit, and
            approved settlement addresses. This profile illustrates the record.
          </p>
          <Link className="text-link" to="/backing">
            review backing mechanics <ArrowUpRight size={14} />
          </Link>
        </section>
      </div>
      <Ledger
        receipts={data.receipts.filter((r) => r.agent === id)}
        full
        onReceipt={onReceipt}
      />
    </>
  );
}
function Backing() { return <BackingOverview />; }
function Providers() { const { providers } = useData(); return <Tools providers={providers}/>; }

function Activity() {
  return (
    <>
      <PageIntro
        title="activity."
        description="Runs, tools, and payment confirmations as they happen."
      />
      <WorkRecord />
      <LiveActivity />
    </>
  );
}

function Protocol() { return <ProtocolGuide />; }
function Scroll() {
  const { pathname } = useLocation();
  useEffect(() => {
    window.scrollTo(0, 0);
  }, [pathname]);
  return null;
}
function ReceiptModal({
  receipt,
  onClose,
}: {
  receipt: Receipt;
  onClose: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const [closing, setClosing] = useState(false);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const requestClose = () => {
    if (closeTimer.current) return;
    setClosing(true);
    const reduced = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    closeTimer.current = setTimeout(onClose, reduced ? 0 : 340);
  };
  useEffect(
    () => () => {
      if (closeTimer.current) clearTimeout(closeTimer.current);
    },
    [],
  );
  const dialogRef = useRef<HTMLElement>(null);
  useEffect(() => {
    const handle = (e: KeyboardEvent) => {
      if (e.key === "Escape") requestClose();
      if (e.key === "Tab") {
        const buttons =
          dialogRef.current?.querySelectorAll<HTMLButtonElement>("button");
        if (!buttons?.length) return;
        const first = buttons[0],
          last = buttons[buttons.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    document.addEventListener("keydown", handle);
    return () => document.removeEventListener("keydown", handle);
  }, [onClose]);
  return (
    <div
      className={`modal-backdrop${closing ? " is-closing" : ""}`}
      onClick={requestClose}
    >
      <section
        className="receipt-modal"
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="receipt-title"
        onClick={(e) => e.stopPropagation()}
      >
        <button
          autoFocus
          className="modal-close icon-button"
          aria-label="Close receipt"
          onClick={requestClose}
        >
          <X size={20} />
        </button>
        <span className="eyebrow">example receipt / {receipt.id}</span>
        <h2 id="receipt-title">
          {receipt.kind === "spend"
            ? "provider payment"
            : receipt.kind === "repayment"
              ? "repayment"
              : "credit limit"}
        </h2>
        <div className="receipt-amount">
          {money(receipt.amount)} <small>USDT</small>
        </div>
        <div className="detail-pairs">
          {[
            ["agent", receipt.agent],
            ["event", receipt.kind],
            ["provider", receipt.provider ?? "n/a"],
            ["timestamp", new Date(receipt.timestamp).toUTCString()],
            ["description", receipt.description],
            ["settlement", "illustrative · no transaction"],
          ].map(([k, v]) => (
            <p key={k}>
              {k}
              <strong>{v}</strong>
            </p>
          ))}
        </div>
        <button
          className="outline"
          onClick={async () => {
            await navigator.clipboard.writeText(
              JSON.stringify(receipt, null, 2),
            );
            setCopied(true);
          }}
        >
          <Copy size={14} />
          {copied ? "copied" : "copy receipt JSON"}
        </button>
      </section>
    </div>
  );
}
function Footer() {
  const { pathname } = useLocation();
  if (pathname === "/account") return null;
  return (
    <footer>
      <Link className="brand" to="/">
        <Sprout />
        tab
      </Link>
      <p>tabagents.io</p>
      <div>
        <Link to="/protocol">
          the mechanics <ArrowUpRight size={13} />
        </Link>
        <Link to="/account">my account <ArrowUpRight size={13} /></Link>
        <a
          href="https://x.com/tabx402"
          target="_blank"
          rel="noopener noreferrer"
          aria-label="Tab on X (opens in a new tab)"
        >
          X <ArrowUpRight size={13} aria-hidden="true" />
        </a>
      </div>
    </footer>
  );
}
export default function App() {
  const [mode, setMode] = useState<"example" | "public">("public");
  const [data, setData] = useState<PublicData>(empty);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [config, setConfig] = useState<Config>({
    app_id: null,
    financial_actions_enabled: false,
    contracts_status: "not_deployed",
    chain_id: BNB_CHAIN_ID,
    network: "mainnet",
    gas_sponsorship_enabled: false,
    gas_sponsorship_status: "unavailable",
    gas_sponsorship_message: "Checking registration sponsorship.",
    payments_enabled: false,
    agent_execution_enabled: false,
    backend: "rust",
    preferred_wallet: "metamask",
    usdt_address: USDT_ADDRESS,
    usdt_decimals: 18,
    official_tab_address: null,
  });
  const [error, setError] = useState("");
  const [receipt, setReceipt] = useState<Receipt | null>(null);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let active = true;
    setLoading(true);
    Promise.all([
      request<PublicData>(`/overview?mode=${mode}`),
      request<Provider[]>("/providers"),
      request<Config>("/config"),
    ])
      .then(([d, p, c]) => {
        if (active) {
          setData(d);
          setProviders(p);
          setConfig(c);
          setError("");
        }
      })
      .catch((e) => {
        if (active) {
          setData(empty);
          setError(e.message);
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [mode]);
  return (
    <ChainContext value={config.chain_id}><BrowserRouter>
      <Context
        value={{
          data,
          providers,
          mode,
          setMode,
          config,
          onReceipt: setReceipt,
        }}
      >
        <div className="ambient-background" aria-hidden="true"><i /><i /><i /></div>
        <Scroll />
        <Header />
        <main>
          {error && (
            <div className="error-banner" role="alert">
              Unable to load Tab data: {error}{" "}
              <button onClick={() => window.location.reload()}>retry</button>
            </div>
          )}
          {loading && (
            <div
              className="loading-bar"
              role="status"
              aria-label="Loading network data"
            />
          )}
          <RouteReveal ready={!loading}>
            <Routes>
              <Route path="/" element={<Landing />} />
              <Route path="/live" element={<Overview />} />
              <Route path="/agents" element={<Agents />} />
              <Route path="/agents/:id" element={<AgentProfile />} />
              <Route path="/backing" element={<Backing />} />
              <Route path="/jobs" element={<PublicJobs />} />
              <Route path="/operators" element={<Operators />} />
              <Route path="/finance" element={<Finance />} />
              <Route path="/agents/:id/runs/:run" element={<PublicRunReceipt />} />
              <Route path="/providers" element={<Providers />} />
              <Route path="/activity" element={<Activity />} />
              <Route path="/protocol" element={<Protocol />} />
              <Route path="/docs" element={<Docs />} />
              <Route
                path="/account"
                element={
                  <Suspense
                    fallback={
                      <div className="empty">opening your account…</div>
                    }
                  >
                    <Account providers={providers} config={config} />
                  </Suspense>
                }
              />
              <Route
                path="*"
                element={
                  <PageIntro
                    title="this tab is empty."
                    description="Head back to the network to find your way."
                  >
                    <Link className="primary" to="/">
                      go home
                    </Link>
                  </PageIntro>
                }
              />
            </Routes>
          </RouteReveal>
        </main>
        <Footer />
        {receipt && (
          <ReceiptModal receipt={receipt} onClose={() => setReceipt(null)} />
        )}
      </Context>
    </BrowserRouter></ChainContext>
  );
}
