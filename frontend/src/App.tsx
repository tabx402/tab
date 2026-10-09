import { ProtocolGuide } from "./components/ProtocolGuide";
import {
  useEffect,
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
import { ArrowUpRight, ArrowRight, UserRound } from "lucide-react";
import { request } from "./lib/api";
import { ChainContext, BNB_CHAIN_ID, USDT_ADDRESS } from "./lib/evm";
import type { Config, Provider } from "./lib/api";
import { Bird, Sprout } from "./components/Drawings";
import { LiveAgents, LiveAgentProfile } from "./components/LiveAgents";
import { LiveActivity } from "./components/LiveActivity";
import { WorkRecord } from "./components/WorkRecord";
import { Flock } from "./components/Flock";
import { Garden } from "./components/Garden";
import type { GardenFocus } from "./components/Garden";
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
  providers: Provider[];
  config: Config;
} | null>(null);
function useData() {
  const ctx = useContext(Context);
  if (!ctx) throw new Error("Missing app context");
  return ctx;
}
export function useConfig() {
  return useData().config;
}
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
    <PageIntro title="agent unavailable." description="This agent is not in the public registry."><Link className="text-link" to="/agents">open the garden <ArrowUpRight size={14} /></Link></PageIntro>
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
  const [providers, setProviders] = useState<Provider[]>([]);
  const [config, setConfig] = useState<Config>({
    app_id: null,
    financial_actions_enabled: false,
    holder_access_enabled: false,
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
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let active = true;
    setLoading(true);
    Promise.all([
      request<Provider[]>("/providers"),
      request<Config>("/config"),
    ])
      .then(([p, c]) => {
        if (active) {
          setProviders(p);
          setConfig(c);
          setError("");
        }
      })
      .catch((e) => {
        if (active) {
          setError(e.message);
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);
  return (
    <ChainContext value={config.chain_id}><BrowserRouter>
      <Context
        value={{
          providers,
          config,
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
      </Context>
    </BrowserRouter></ChainContext>
  );
}
