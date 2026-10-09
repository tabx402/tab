import { isAddress } from "viem";
import { useState, useEffect } from "react";
import { ArrowLeft, ArrowRight, Check } from "lucide-react";
import type { RuntimeInput, RuntimeAgent, ModelOption } from "../lib/api";
import { request } from "../lib/api";
import { AgentGlyph } from "./Drawings";
import { DailyCapField } from "./DailyCapField";
import { AgentWalletPreview } from "./AgentWalletPreview";
import { usdtUnits, USDT_SCALE } from "../lib/amounts";
const templates = [
  {
    id: "onchain",
    name: "onchain monitor",
    purpose: "Track a wallet and summarize its activity on BNB Smart Chain.",
    tools: ["bnb-rpc"],
  },
  {
    id: "research",
    name: "research agent",
    purpose: "Research a topic using primary sources and return a short report with links.",
    tools: ["bnb-rpc"],
  },
  {
    id: "custom",
    name: "bring your own",
    purpose: "Connect my existing agent through the Tab API.",
    tools: ["bnb-rpc"],
  },
] as const;
export function AgentWizard({
  onCreate,
  onCancel,
  pending,
  initial,
  sponsorshipAvailable = false,
  sponsorshipMessage,
  selfPay = false,
  onSelfPayChange,
  wallet,
}: {
  wallet?: string;
  initial?: Partial<RuntimeInput>;
  sponsorshipAvailable?: boolean;
  sponsorshipMessage?: string;
  selfPay?: boolean;
  onSelfPayChange?: (value: boolean) => void;
  onCreate: (input: RuntimeInput) => Promise<RuntimeAgent>;
  onCancel: () => void;
  pending: boolean;
}) {
  const [step, setStep] = useState(0);
  const [connected, setConnected] = useState<Record<string, boolean>>({});
  const [models, setModels] = useState<ModelOption[]>([]);
  useEffect(() => {
    void request<Record<string, boolean>>("/tools")
      .then(setConnected)
      .catch(() => {});
    void request<ModelOption[]>("/models").then(setModels).catch(() => {});
  }, []);
  const [input, setInput] = useState<RuntimeInput>({
    name: "",
    purpose: templates[0].purpose,
    template: "onchain",
    tools: ["bnb-rpc"],
    cadence: "manual",
    daily_cap: "5",
    max_call: "0.10",
    model: "openai/gpt-4.1-mini",
    watch_address: null,
    report_agent: 437,
    public_activity: true,
    ...initial,
  });
  const [error, setError] = useState("");
  useEffect(() => {
    if (step > 0)
      window.scrollTo({
        top: 0,
        behavior: matchMedia("(prefers-reduced-motion: reduce)").matches
          ? "instant"
          : "smooth",
      });
  }, [step]);
  const patch = (v: Partial<RuntimeInput>) => setInput((p) => ({ ...p, ...v }));
  const patchCap = (daily_cap: string) => setInput(current => {
    const cap = usdtUnits(daily_cap), perCall = usdtUnits(String(current.max_call));
    return { ...current, daily_cap, max_call: cap !== null && perCall !== null && perCall > cap ? daily_cap : current.max_call };
  });
  function next() {
    setError("");
    if (
      step === 0 &&
      (input.name.trim().length < 2 || input.purpose.trim().length < 5)
    ) {
      setError("Add a name and a short description.");
      return;
    }
    if (
      step === 1 &&
      (!input.tools.length ||
        usdtUnits(String(input.max_call)) === null ||
        (usdtUnits(String(input.max_call)) ?? 0n) <= 0n ||
        (usdtUnits(String(input.max_call)) ?? 0n) > 10n * USDT_SCALE ||
        (usdtUnits(String(input.max_call)) ?? 0n) > (usdtUnits(input.daily_cap) ?? 0n) ||
        (usdtUnits(input.daily_cap) ?? 0n) < USDT_SCALE ||
        (usdtUnits(input.daily_cap) ?? 0n) > 10000n * USDT_SCALE)
    ) {
      setError("Choose a tool and set your spending limits.");
      return;
    }
    if (
      step === 1 &&
      input.watch_address &&
      !isAddress(input.watch_address, { strict: false })
    ) {
      setError("Use a valid EVM wallet address (0x…).");
      return;
    }
    setStep((s) => s + 1);
  }
  return (
    <section className="agent-builder panel">
      <div className="wizard-progress" aria-label="Creation steps">
        {["agent", "tools", "create"].map((s, i) => (
          <span
            key={s}
            className={i === step ? "current" : i < step ? "done" : ""}
          >
            <i>{i < step ? <Check size={12} /> : i + 1}</i>
            {s}
          </span>
        ))}
      </div>
      <div key={step} className="wizard-stage">
        <h2>
          {
            [
              "what should it do?",
              "give it the right tools.",
              "ready to create.",
            ][step]
          }
        </h2>
        {step === 0 && (
          <>
            <div className="template-options">
              {templates.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  className={input.template === t.id ? "selected" : ""}
                  onClick={() =>
                    patch({
                      template: t.id,
                      purpose: t.purpose,
                      tools: t.id === "research" && connected["web-search"] ? ["web-search"] : [...t.tools],
                    })
                  }
                >
                  <AgentGlyph index={templates.indexOf(t)} />
                  {t.name}
                </button>
              ))}
            </div>
            <label>
              agent name
              <input
                autoFocus
                value={input.name}
                maxLength={48}
                placeholder="e.g. wren"
                onChange={(e) => patch({ name: e.target.value })}
              />
            </label>
            <label>
              its job
              <textarea
                value={input.purpose}
                maxLength={600}
                onChange={(e) => patch({ purpose: e.target.value })}
              />
            </label>
          </>
        )}
        {step === 1 && (
          <>
            <fieldset className="tool-options">
              <legend>data and services</legend>
              {[
                {
                  id: "bnb-rpc",
                  name: "BNB Smart Chain",
                  detail: "wallet balances + chain snapshots",
                  ready: true,
                },
                {
                  id: "openrouter",
                  name: "OpenRouter",
                  detail: connected.openrouter
                    ? "model summaries · connected provider"
                    : "model API connection needed",
                  ready: !!connected.openrouter,
                },
                {
                  id: "web-search",
                  name: "web research",
                  detail: connected["web-search"] ? "source-linked reports · operator provider credits" : "research provider connection needed",
                  ready: !!connected["web-search"],
                },
                {
                  id: "tavily",
                  name: "Tavily",
                  detail: connected.tavily ? "web research and source links" : "search API connection needed",
                  ready: !!connected.tavily,
                },
                {
                  id: "x402",
                  name: "x402",
                  detail: connected.x402 ? "USDT requests approved in your wallet" : "USDT payment provider connection needed",
                  ready: !!connected.x402,
                },
              ].map((t) => (
                <label
                  key={t.id}
                  className={`tool-choice ${!t.ready ? "unavailable" : ""}`}
                >
                  <input
                    type="checkbox"
                    checked={input.tools.includes(
                      t.id as RuntimeInput["tools"][number],
                    )}
                    disabled={!t.ready}
                    onChange={(e) =>
                      patch({
                        tools: e.target.checked
                          ? [
                              ...input.tools,
                              t.id as RuntimeInput["tools"][number],
                            ]
                          : input.tools.filter((x) => x !== t.id),
                      })
                    }
                  />
                  <span>
                    <strong>{t.name}</strong>
                    <small>{t.detail}</small>
                  </span>
                  <span className="tool-state">
                    {t.ready ? "available" : "not connected"}
                  </span>
                </label>
              ))}
            </fieldset>
            {input.tools.includes("openrouter") && <label>
              language model
              <select value={input.model} onChange={event => patch({ model: event.target.value })} aria-label="Language model">
                {models.length ? models.map(model => <option key={model.id} value={model.id}>{model.name} · {model.provider}{model.available ? "" : " · connection needed"}</option>) : <option value={input.model}>{input.model} · model directory unavailable</option>}
              </select>
              <small className="field-help">{models.find(model => model.id === input.model)?.billing ?? "Paid model access needs a connected provider."}</small>
            </label>}
            {input.tools.includes("bnb-rpc") && (
              <><label>
                wallet to watch <small>optional</small>
                <input
                  value={input.watch_address ?? ""}
                  placeholder="0x… · defaults to your connected wallet"
                  onChange={(e) =>
                    patch({ watch_address: e.target.value || null })
                  }
                />
              </label><AgentWalletPreview address={input.watch_address || wallet} /></>
            )}
            <div className="wizard-pair">
              <label>
                run schedule
                <select
                  value={input.cadence}
                  onChange={(e) =>
                    patch({
                      cadence: e.target.value as RuntimeInput["cadence"],
                    })
                  }
                >
                  <option value="manual">when I run it</option>
                  <option value="hourly">every hour</option>
                  <option value="daily">once a day</option>
                </select>
              </label>
              <DailyCapField value={input.daily_cap} onChange={patchCap} />
            </div>
            <label>
              maximum per paid call · USDT
              <input
                type="text"
                inputMode="decimal"
                pattern="[0-9]+(\.[0-9]{1,18})?"
                value={String(input.max_call)}
                onChange={(e) => patch({ max_call: e.target.value })}
              />
            </label>
            <label className="public-toggle">
              <input
                type="checkbox"
                checked={input.public_activity}
                onChange={(e) => patch({ public_activity: e.target.checked })}
              />
              show activity in the public log
            </label>
            <p className="field-help">
              Paid calls ask for wallet approval. The schedule runs connected
              read-only tools.
            </p>
          </>
        )}
        {step === 2 && (
          <>
            <div className="creation-summary">
              <AgentGlyph />
              <h3>{input.name}</h3>
              <p>{input.purpose}</p>
              <dl>
                <div>
                  <dt>tools</dt>
                  <dd>{input.tools.join(" · ")}</dd>
                </div>
                <div>
                  <dt>schedule</dt>
                  <dd>{input.cadence}</dd>
                </div>
                <div>
                  <dt>spending limit</dt>
                  <dd>{input.daily_cap} USDT / day</dd>
                </div>
                <div>
                  <dt>per paid call</dt>
                  <dd>{String(input.max_call)} USDT max</dd>
                </div>
                {input.tools.includes("openrouter") && <div><dt>model</dt><dd>{models.find(model => model.id === input.model)?.name ?? input.model}</dd></div>}
              </dl>
            </div>
            <p className="field-help">
              {selfPay ? "Your wallet will send the registration and pay its BNB gas fee." : sponsorshipAvailable ? "Sign a registration permission with your wallet. When sponsorship is available, Tab pays the BNB gas fee; your wallet needs no BNB to register." : `${sponsorshipMessage || "Registration sponsorship is currently unavailable."} Your setup will be saved; you can retry later or choose to pay BNB gas yourself.`}
              {" "}USDT funds supported paid work. Saving a setup does not move funds.
            </p>
            {onSelfPayChange && <label className="public-toggle"><input type="checkbox" checked={selfPay} disabled={pending} onChange={event => onSelfPayChange(event.target.checked)} />pay registration BNB gas from my wallet</label>}
          </>
        )}
      </div>
      {error && (
        <p role="alert" className="form-error">
          {error}
        </p>
      )}
      <div className="wizard-actions">
        <button
          className="text-link"
          disabled={pending}
          onClick={() => (step ? setStep((s) => s - 1) : onCancel())}
        >
          <ArrowLeft size={14} />
          {step ? "back" : "cancel"}
        </button>
        {step < 2 ? (
          <button className="primary" onClick={next}>
            continue
            <ArrowRight size={15} />
          </button>
        ) : (
          <button
            className="primary"
            disabled={pending}
            onClick={async () => {
              try {
                setError("");
                await onCreate(input);
              } catch (e) {
                setError((e as Error).message);
              }
            }}
          >
            {pending ? "confirming…" : "create agent"}
            <ArrowRight size={15} />
          </button>
        )}
      </div>
    </section>
  );
}
