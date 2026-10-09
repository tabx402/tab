import { useEffect, useRef, useState } from "react";
import { Link, NavLink, useLocation, useNavigate } from "react-router-dom";
import { ArrowUpRight, ArrowRight, ChevronDown, Menu, Search, UserRound, X } from "lucide-react";
import { Sprout } from "./Drawings";
import { primaryLinks, secondaryLinks, searchEntries } from "../lib/navigation";

export function SearchButton({ docs = false }: { docs?: boolean }) {
  return <button className={docs ? "docs-search" : "search-trigger icon-button"} aria-label="Search Tab" onClick={() => window.dispatchEvent(new Event("tab:search"))}>
    <Search size={17} />{docs && <><span>search docs and pages</span><kbd>{/Mac|iPhone|iPad/.test(navigator.platform) ? "⌘ K" : "Ctrl K"}</kbd></>}
  </button>;
}

function AppSearch() {
  const dialog = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const navigate = useNavigate();
  const results = searchEntries.filter(item => query.toLowerCase().trim().split(/\s+/).every(word => `${item.title} ${item.keywords} ${item.section}`.toLowerCase().includes(word))).slice(0, 10);
  useEffect(() => {
    const open = () => { setQuery(""); setSelected(0); if (!dialog.current?.open) dialog.current?.showModal(); input.current?.focus(); };
    const shortcut = (event: KeyboardEvent) => { if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") { event.preventDefault(); if (dialog.current?.open) dialog.current.close(); else open(); } };
    window.addEventListener("tab:search", open);
    window.addEventListener("keydown", shortcut);
    return () => { window.removeEventListener("tab:search", open); window.removeEventListener("keydown", shortcut); };
  }, []);
  useEffect(() => { dialog.current?.querySelector(`#tab-search-result-${selected}`)?.scrollIntoView({ block: "nearest" }); }, [selected]);
  const visit = (href: string) => { dialog.current?.close(); navigate(href); };
  return <dialog className="app-search" ref={dialog} aria-label="Search Tab" onClick={event => { if (event.target === event.currentTarget) { const rect = event.currentTarget.getBoundingClientRect(); if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) event.currentTarget.close(); } }}>
    <div className="app-search-input"><Search size={19} /><input ref={input} autoFocus role="combobox" aria-label="Search docs and pages" aria-autocomplete="list" aria-controls="tab-search-results" aria-expanded="true" aria-activedescendant={results.length ? `tab-search-result-${selected}` : undefined} placeholder="find a page or a topic…" value={query} onChange={event => { setQuery(event.target.value); setSelected(0); }} onKeyDown={event => {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); setSelected(index => results.length ? (index + (event.key === "ArrowDown" ? 1 : results.length - 1)) % results.length : 0); }
      if (event.key === "Enter" && results[selected]) { event.preventDefault(); visit(results[selected].href); }
    }} /><button className="icon-button" aria-label="Close search" onClick={() => dialog.current?.close()}><X size={18} /></button></div>
    <div className="app-search-results" id="tab-search-results" role="listbox" aria-label="Search results">{results.map((item, index) => <button key={item.href} id={`tab-search-result-${index}`} role="option" aria-selected={selected === index} onClick={() => visit(item.href)} onPointerMove={() => setSelected(index)}><span><small>{item.section}</small>{item.title}</span><ArrowRight size={16} /></button>)}</div>
    {!results.length && <p className="search-empty" role="status">No matching pages. Try “payments”, “jobs” or “API”.</p>}
    <div className="search-hint"><span>↑ ↓ to move · enter to open</span><span>esc to close</span></div>
  </dialog>;
}

export function Header() {
  const [menu, setMenu] = useState<"more" | "mobile" | null>(null);
  const header = useRef<HTMLElement>(null);
  const more = useRef<HTMLButtonElement>(null);
  const mobile = useRef<HTMLButtonElement>(null);
  const { pathname, hash } = useLocation();
  useEffect(() => { setMenu(null); }, [pathname, hash]);
  useEffect(() => {
    const outside = (event: PointerEvent) => { if (!header.current?.contains(event.target as Node)) setMenu(null); };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape" && menu) { (menu === "more" ? more : mobile).current?.focus(); setMenu(null); } };
    document.addEventListener("pointerdown", outside); document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("pointerdown", outside); document.removeEventListener("keydown", escape); };
  }, [menu]);
  return <><header ref={header} className="app-header"><div className="nav-shell">
    <Link className="brand" to="/" aria-label="Tab home"><Sprout />tab</Link>
    <nav className="primary-nav" aria-label="Main navigation">{primaryLinks.map(([url, name]) => <NavLink key={url} to={url}>{name}</NavLink>)}
      <div className="nav-more"><button ref={more} aria-expanded={menu === "more"} aria-controls="more-navigation" onClick={() => setMenu(menu === "more" ? null : "more")}>more <ChevronDown size={13} /></button>{menu === "more" && <div id="more-navigation" className="nav-dropdown">{secondaryLinks.map(([url, name]) => <NavLink key={url} to={url}>{name}<ArrowUpRight size={13} /></NavLink>)}<a href="https://github.com/tabx402/tab" target="_blank" rel="noopener noreferrer">GitHub<ArrowUpRight size={13} /></a></div>}</div>
    </nav>
    <div className="header-actions"><SearchButton /><a className="header-x-link" href="https://x.com/tabx402" target="_blank" rel="noopener noreferrer" aria-label="Tab on X (opens in a new tab)"><img src="/images/x-logo.svg" width={16} height={16} alt="" /></a><Link className="account-link" to="/account"><UserRound size={15} /><span>my account</span><ArrowUpRight size={14} /></Link><button ref={mobile} className="mobile-menu-button icon-button" aria-label={menu === "mobile" ? "Close navigation" : "Open navigation"} aria-expanded={menu === "mobile"} aria-controls="mobile-navigation" onClick={() => setMenu(menu === "mobile" ? null : "mobile")}>{menu === "mobile" ? <X size={20} /> : <Menu size={20} />}</button></div>
    {menu === "mobile" && <nav id="mobile-navigation" className="mobile-navigation" aria-label="Mobile navigation">{[...primaryLinks, ...secondaryLinks].map(([url, name]) => <NavLink key={url} to={url}>{name}<ArrowUpRight size={14} /></NavLink>)}<a href="https://github.com/tabx402/tab" target="_blank" rel="noopener noreferrer">GitHub<ArrowUpRight size={14} /></a></nav>}
  </div></header><AppSearch /></>;
}
